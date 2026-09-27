use std::process::{Command, Output};

fn areitu(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_areitu"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn ingest_calendar_then_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("areitu.db");
    let cal = dir.path().join("events.json");
    std::fs::write(
        &cal,
        r#"{"items":[{"id":"a1","summary":"ランチ",
            "start":{"dateTime":"2026-09-01T12:00:00+09:00"},
            "end":{"dateTime":"2026-09-01T13:00:00+09:00"}}]}"#,
    )
    .unwrap();

    let out = areitu(&db, &["ingest-calendar", cal.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 件"));

    let out = areitu(&db, &["list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("場所はまだありません"));
}

#[test]
fn rename_unknown_place_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = areitu(&dir.path().join("areitu.db"), &["rename", "999", "X"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("place 999 not found"));
}

#[test]
fn ingest_missing_folder_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = areitu(&dir.path().join("areitu.db"), &["ingest-photos", "/no/such/dir/areitu"]);
    assert!(!out.status.success());
}
