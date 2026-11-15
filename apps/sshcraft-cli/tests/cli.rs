//! `sshcraft-cli` as a program: against no app, a stale connection file, a running engine, and
//! an app it has to start first.

// Tests report by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session, SessionConfig};
use sshcraft_link::{ConnectionFile, TOKEN_FILE, Token};
use sshcraft_ssh::testing::TestAuth;

const CLI: &str = env!("CARGO_BIN_EXE_sshcraft-cli");

fn cli(link: &std::path::Path, args: &[&str]) -> Output {
    Command::new(CLI).args(args).env("SSHCRAFT_LINK_DIR", link).output().unwrap()
}

#[test]
fn without_the_app_the_exit_code_is_3() {
    let link = tempfile::tempdir().unwrap();
    let out = cli(link.path(), &["hosts"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not running"));
}

#[test]
fn a_stale_connection_file_counts_as_not_running() {
    let link = tempfile::tempdir().unwrap();
    let token = Token::load_or_create(&link.path().join(TOKEN_FILE)).unwrap();
    drop(token);
    // A port nothing listens on any more.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    ConnectionFile::new(port, link.path().join(TOKEN_FILE), "0".into()).write(link.path()).unwrap();
    let out = cli(link.path(), &["hosts"]);
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
}

struct App {
    session: Session,
    _data: tempfile::TempDir,
    _server: sshcraft_ssh::testing::TestServer,
}

async fn app(link: &std::path::Path) -> App {
    let data = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(data.path())).unwrap();
    let (server, _) = test_host(&session, "web", TestAuth::default(), true).await;
    session.start_agent_link(link, 0).await.unwrap();
    App { session, _data: data, _server: server }
}

async fn run(link: std::path::PathBuf, args: Vec<&'static str>) -> Output {
    tokio::task::spawn_blocking(move || cli(&link, &args)).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_calls_run_as_the_cli_agent() {
    let link = tempfile::tempdir().unwrap();
    let app = app(link.path()).await;
    let out = run(link.path().to_path_buf(), vec!["hosts"]).await;
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("web"));
    let out = run(link.path().to_path_buf(), vec!["--agent", "nightly", "delete", "host", "nowhere"]).await;
    assert_eq!(out.status.code(), Some(1));
    let log = app.session.execute(Caller::User, "agent.log", json!({})).await.unwrap();
    assert_eq!(log[0]["agent"], "nightly", "{log}");
    let out = run(link.path().to_path_buf(), vec!["run", "--cmd", "agent.set_level={\"category\":\"connect\",\"level\":\"allow\"}"]).await;
    assert_eq!(out.status.code(), Some(1), "agents never reach their own rights");
}

/// A program that starts, does nothing and ends: in place of the app, which the test starts.
fn harmless() -> String {
    if cfg!(windows) {
        format!(r"{}\System32\whoami.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
    } else {
        "/bin/true".into()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn launch_starts_the_app_and_waits_for_it() {
    let link = tempfile::tempdir().unwrap();
    let mut child = Command::new(CLI)
        .args(["mcp", "--when-closed", "launch"])
        .env("SSHCRAFT_LINK_DIR", link.path())
        .env("SSHCRAFT_APP_PATH", harmless())
        // Never the user's own data folder (an SSHCraft may run on this computer).
        .env("SSHCRAFT_DATA_DIR", link.path().join("data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "sshcraft_list_hosts", "arguments": {}}});
    writeln!(stdin, "{call}").unwrap();
    // The "app" comes up a moment later, as a started one does.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let _app = app(link.path()).await;
    let stdout = child.stdout.take().unwrap();
    let line = tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut line).unwrap();
        line
    });
    let line = tokio::time::timeout(Duration::from_secs(30), line).await.expect("an answer").unwrap();
    drop(stdin);
    let _ = child.wait();
    let answer: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    assert!(answer["result"]["content"][0]["text"].as_str().unwrap().contains("web"));
}
