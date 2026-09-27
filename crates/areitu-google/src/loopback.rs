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

pub fn await_callback(listener: TcpListener, expected_state: &str, timeout: Duration) -> Result<CallbackResult> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return handle_connection(stream, expected_state),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(Error::OAuth("timed out waiting for the OAuth redirect".into()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
}

fn handle_connection(mut stream: TcpStream, expected_state: &str) -> Result<CallbackResult> {
    stream.set_nonblocking(false)?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
    let first_line = request.lines().next().unwrap_or("").to_owned();
    let path_and_query = first_line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let query = path_and_query.split_once('?').map(|(_, q)| q.to_owned()).unwrap_or_default();
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let outcome = match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) if state == expected_state => {
            Ok(CallbackResult { code: code.clone(), state: state.clone() })
        }
        (Some(_), Some(_)) => Err(Error::OAuth("state mismatch on OAuth redirect".into())),
        _ => Err(Error::OAuth("missing code or state on OAuth redirect".into())),
    };

    let body = match &outcome {
        Ok(_) => "<html><body>AREITU: sign-in complete. You can close this tab.</body></html>",
        Err(_) => "<html><body>AREITU: sign-in failed. You can close this tab.</body></html>",
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    outcome
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
}
