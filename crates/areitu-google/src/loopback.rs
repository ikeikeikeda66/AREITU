use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::{Error, Result};

pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

// Manual (redacted) Debug impl instead of #[derive(Debug)]: `code` and `state`
// are secrets (the OAuth authorization code and CSRF state), and Result::unwrap_err
// requires the Ok side to be Debug. A derive would print both in full on panic.
impl std::fmt::Debug for CallbackResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackResult")
            .field("code", &"[redacted]")
            .field("state", &"[redacted]")
            .finish()
    }
}

pub fn bind_loopback() -> Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

const CALLBACK_PATH: &str = "/callback";

/// 1本の接続を処理した結果。`Callback` はループを終了させる最終結果
/// （成功・state 不一致・Google 側のエラー応答）。`Ignore` はポートプローブ、
/// ブラウザの preconnect、favicon リクエストなど、本物のリダイレクトではない
/// 接続を表し、呼び出し側はデッドラインまで待ち続ける。
enum ConnectionOutcome {
    Callback(Result<CallbackResult>),
    Ignore,
}

pub fn await_callback(listener: TcpListener, expected_state: &str, timeout: Duration) -> Result<CallbackResult> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::OAuth("timed out waiting for the OAuth redirect".into()));
        }
        match listener.accept() {
            Ok((stream, _)) => match handle_connection(stream, expected_state, remaining) {
                ConnectionOutcome::Callback(outcome) => return outcome,
                ConnectionOutcome::Ignore => continue,
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20).min(remaining));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
}

fn handle_connection(mut stream: TcpStream, expected_state: &str, remaining: Duration) -> ConnectionOutcome {
    // 何も送らずに接続だけする（あるいはゆっくり送る）ストレイ接続がデッドラインを
    // 超えてループをブロックしないよう、読み取りタイムアウトを残り時間で上限にする。
    let read_timeout = remaining.min(Duration::from_secs(10));
    if stream.set_nonblocking(false).is_err() {
        return ConnectionOutcome::Ignore;
    }
    if stream.set_read_timeout(Some(read_timeout)).is_err() {
        return ConnectionOutcome::Ignore;
    }

    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(0) => return ConnectionOutcome::Ignore, // 何も送らずに閉じた接続
        Ok(n) => n,
        Err(_) => return ConnectionOutcome::Ignore, // 読み取りタイムアウト・その他の読み取りエラー
    };
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
    let first_line = request.lines().next().unwrap_or("").to_owned();
    let path_and_query = first_line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query.as_str(), ""));
    let path = path.to_owned();
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if path != CALLBACK_PATH {
        respond(&mut stream, 404, "not found");
        return ConnectionOutcome::Ignore;
    }

    if let Some(error) = params.get("error") {
        respond(&mut stream, 400, "sign-in was denied");
        return ConnectionOutcome::Callback(Err(Error::OAuth(format!("OAuth redirect reported an error: {error}"))));
    }

    let outcome = match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) if state == expected_state => {
            Some(Ok(CallbackResult { code: code.clone(), state: state.clone() }))
        }
        (Some(_), Some(_)) => Some(Err(Error::OAuth("state mismatch on OAuth redirect".into()))),
        _ => None,
    };

    match outcome {
        Some(outcome) => {
            let (status, body) = match &outcome {
                Ok(_) => (200, "<html><body>AREITU: sign-in complete. You can close this tab.</body></html>"),
                Err(_) => (400, "<html><body>AREITU: sign-in failed. You can close this tab.</body></html>"),
            };
            respond(&mut stream, status, body);
            ConnectionOutcome::Callback(outcome)
        }
        None => {
            // /callback へのリクエストだが code/state を欠く（ブラウザの preconnect や
            // 不完全なリクエストなど） — 本物のリダイレクトを待ち続ける。
            respond(&mut stream, 400, "missing code or state");
            ConnectionOutcome::Ignore
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_get(port: u16, path_and_query: &str) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!("GET {path_and_query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut discard = [0u8; 512];
        let _ = stream.read(&mut discard);
    }

    #[test]
    fn accepts_matching_code_and_state() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=expected-state");
        let result = handle.join().unwrap().unwrap();
        assert_eq!(result.code, "auth-code-1");
        assert_eq!(result.state, "expected-state");
    }

    #[test]
    fn rejects_state_mismatch() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=wrong-state");
        let err = handle.join().unwrap().unwrap_err();
        assert!(matches!(err, Error::OAuth(_)));
    }

    #[test]
    fn times_out_when_nothing_connects() {
        let (listener, _port) = bind_loopback().unwrap();
        let started = Instant::now();
        let err = await_callback(listener, "expected-state", Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, Error::OAuth(_)));
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    /// ポートプローブやブラウザの preconnect が本物のリダイレクトより先に届いても、
    /// 待ち続けて本物のコールバックを拾えることを確認する。
    #[test]
    fn survives_a_stray_connection_before_the_real_callback() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/favicon.ico");
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=expected-state");
        let result = handle.join().unwrap().unwrap();
        assert_eq!(result.code, "auth-code-1");
        assert_eq!(result.state, "expected-state");
    }

    /// 何も送らずに接続して閉じるだけのコネクション（ブラウザの preconnect 等）が
    /// あっても、デッドラインを超えて止まらず、後続の本物のコールバックを拾える。
    #[test]
    fn survives_a_connection_that_sends_nothing_and_closes() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        // 接続だけして何も送らずに閉じる。
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        drop(stream);
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=expected-state");
        let result = handle.join().unwrap().unwrap();
        assert_eq!(result.code, "auth-code-1");
        assert_eq!(result.state, "expected-state");
    }
}
