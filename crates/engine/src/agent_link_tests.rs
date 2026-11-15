//! The agent link: agents reach engine commands through the gate, nothing of the window, and
//! what a connection opened ends with it.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sshcraft_link::{CONNECTION_FILE, Client, ConnectionFile, LinkError, Token};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentPolicy};

use crate::testing::test_host;
use crate::{Caller, Session, SessionConfig};

struct World {
    session: Session,
    link: tempfile::TempDir,
    _data: tempfile::TempDir,
    _server: TestServer,
}

async fn world() -> World {
    let data = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(data.path())).unwrap();
    let (server, _host) = test_host(&session, "web", TestAuth::default(), true).await;
    let link = tempfile::tempdir().unwrap();
    session.start_agent_link(link.path(), 0).await.unwrap();
    World { session, link, _data: data, _server: server }
}

async fn client(w: &World) -> Client {
    let file = ConnectionFile::read(w.link.path()).expect("the connection file");
    let token = Token::load_or_create(&file.token_file).unwrap();
    Client::connect(SocketAddr::from(([127, 0, 0, 1], file.port)), &token).await.unwrap()
}

async fn hello(c: &Client, agent: &str, via: &str) {
    c.call("hello", json!({ "agent": agent, "via": via })).await.unwrap();
}

async fn run(c: &Client, command: &str, params: Value) -> Result<Value, LinkError> {
    c.call("engine.execute", json!({ "command": command, "params": params })).await
}

async fn agent_log(w: &World) -> Vec<Value> {
    w.session.execute(Caller::User, "agent.log", json!({})).await.unwrap().as_array().cloned().unwrap_or_default()
}

async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_lists_hosts_through_the_link() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "claude", "mcp").await;
    let hosts = run(&c, "host.list", json!({})).await.unwrap();
    assert!(hosts.to_string().contains("web"), "{hosts}");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_request_runs_as_that_agent() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "codex", "cli").await;
    let _ = run(&c, "host.delete", json!({ "id": uuid::Uuid::new_v4() })).await;
    let log = agent_log(&w).await;
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0]["agent"], "codex", "the name the connection said hello with");
    assert!(w.session.journal().iter().all(|e| e.caller != "user"), "nothing ran as the user");
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_get_no_ui_methods() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "claude", "mcp").await;
    for method in ["ui.click", "ui.screenshot", "ui.text", "app.quit", "engine.journal", "terminal.read"] {
        match c.call(method, json!({ "x": 1, "y": 1 })).await {
            Err(LinkError::Remote(message)) => assert!(message.starts_with("unknown method"), "{method}: {message}"),
            other => panic!("{method}: {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_before_hello() {
    let w = world().await;
    let c = client(&w).await;
    assert!(matches!(run(&c, "host.list", json!({})).await, Err(LinkError::Remote(m)) if m.contains("hello")));
    assert!(matches!(c.call("engine.commands", json!({})).await, Err(LinkError::Remote(_))));
    assert!(matches!(c.call("hello", json!({ "agent": "x", "via": "window" })).await, Err(LinkError::Remote(_))), "an unknown way in");
    assert!(matches!(c.call("hello", json!({ "agent": "\u{202E}x", "via": "mcp" })).await, Err(LinkError::Remote(_))), "a hidden character");
    hello(&c, "claude", "mcp").await;
    assert!(c.call("hello", json!({ "agent": "other", "via": "mcp" })).await.is_err(), "a connection has one name");
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_list_only_what_agents_reach() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "claude", "mcp").await;
    let list = c.call("engine.commands", json!({})).await.unwrap();
    let ids: Vec<String> = list.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&"host.list".to_string()));
    for id in &ids {
        assert!(!id.starts_with("agent.") && !id.starts_with("settings.") && !id.starts_with("prompt."), "{id}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_link_ends_sessions_and_withdraws_questions() {
    let w = world().await;
    // A window listens: a question waits for it instead of being no at once.
    let _window = w.session.subscribe();
    AgentPolicy::set_level(&w.session.store(), AgentCategory::Connect, AgentLevel::Allow).unwrap();
    let c = client(&w).await;
    hello(&c, "claude", "mcp").await;
    run(&c, "session.open", json!({ "host": "web" })).await.unwrap();
    assert_eq!(w.session.sessions().ids().len(), 1);
    AgentPolicy::set_level(&w.session.store(), AgentCategory::Connect, AgentLevel::Ask).unwrap();
    let asking = c.clone();
    let waiting = tokio::spawn(async move { run(&asking, "session.open", json!({ "host": "web" })).await });
    eventually("the question", || !w.session.prompts().open_prompts().is_empty()).await;
    // The agent's program goes away with the question open.
    waiting.abort();
    let _ = waiting.await;
    drop(c);
    eventually("the question goes", || w.session.prompts().open_prompts().is_empty()).await;
    eventually("the sessions end", || w.session.sessions().ids().is_empty()).await;
    let log = agent_log(&w).await;
    assert!(log.iter().any(|e| e["outcome"] == "cancelled"), "{log:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_removes_the_file_and_drops_connections() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "claude", "mcp").await;
    assert_eq!(w.session.agent_link_status().connections, 1);
    w.session.stop_agent_link();
    assert!(ConnectionFile::read(w.link.path()).is_none(), "the file went");
    assert_eq!(w.session.agent_link_status().port, None);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(c.call("ping", json!({})).await.is_err(), "the connection was cut");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_file_names_the_port_and_holds_no_secret() {
    let w = world().await;
    let file = ConnectionFile::read(w.link.path()).unwrap();
    assert_eq!(Some(file.port), w.session.agent_link_status().port);
    let token = Token::load_or_create(&file.token_file).unwrap();
    let text = std::fs::read_to_string(w.link.path().join(CONNECTION_FILE)).unwrap();
    assert!(!text.contains(token.hex()), "{text}");
}

/// Spec 10 (T15): an agent calling as fast as it can through the link cannot flood the log, and
/// the app keeps answering.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_cannot_flood_the_log_through_the_link() {
    let w = world().await;
    let c = client(&w).await;
    hello(&c, "flooder", "mcp").await;
    for _ in 0..50 {
        let calls: Vec<_> = (0..40).map(|_| run(&c, "host.delete", json!({ "id": uuid::Uuid::new_v4() }))).collect();
        for result in futures::future::join_all(calls).await {
            assert!(result.is_err(), "nothing was deleted");
        }
    }
    let log = agent_log(&w).await;
    assert!(log.len() <= sshcraft_store::agent::MAX_LOG, "{} entries", log.len());
    assert!(log.len() >= 900, "the newest stay: {}", log.len());
    assert_eq!(c.call("ping", json!({})).await.unwrap(), json!("pong"), "the app still answers");
}

/// A token per start (AGENTS.md): one read before is no good after the link starts again.
#[tokio::test(flavor = "multi_thread")]
async fn every_start_has_a_new_token() {
    let w = world().await;
    let file = ConnectionFile::read(w.link.path()).unwrap();
    let old = Token::load(&file.token_file).unwrap();
    w.session.stop_agent_link();
    w.session.start_agent_link(w.link.path(), 0).await.unwrap();
    let file = ConnectionFile::read(w.link.path()).unwrap();
    let new = Token::load(&file.token_file).unwrap();
    assert_ne!(old.hex(), new.hex());
    let addr = SocketAddr::from(([127, 0, 0, 1], file.port));
    assert!(Client::connect(addr, &old).await.is_err(), "the old token is no good");
    assert!(Client::connect(addr, &new).await.is_ok());
}
