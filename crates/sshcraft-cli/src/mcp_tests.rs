use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::*;
use sshcraft_agent::{AgentError, ErrorKind};
use tokio::io::{AsyncWriteExt, BufReader};

/// Notes calls; `exec` waits until dropped (as a question would), others answer at once.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(String, Action)>>,
    error: Option<AgentError>,
    dropped: Arc<AtomicBool>,
}

struct Flag(Arc<AtomicBool>);
impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl Caller for Fake {
    async fn call(&self, agent: &str, action: Action) -> Result<Value, AgentError> {
        self.calls
            .lock()
            .unwrap()
            .push((agent.to_owned(), action.clone()));
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if matches!(action, Action::Exec { .. }) {
            let _flag = Flag(self.dropped.clone());
            std::future::pending::<()>().await;
        }
        Ok(json!({"output": "hello\n", "truncated": false, "exit_status": 0}))
    }
}

fn call(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn answer(step: Step) -> Value {
    match step {
        Step::Answer(a) => a,
        _ => panic!("expected an answer"),
    }
}

#[test]
fn initialize_names_the_agent_unless_the_command_line_did() {
    let mut server = McpServer::new(None);
    let a = answer(server.step(&call(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "claude-code", "version": "2"}, "capabilities": {}}),
    )));
    assert_eq!(a["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(a["result"]["capabilities"]["tools"].is_object());
    assert_eq!(server.agent(), "claude-code");
    assert!(matches!(
        server.step(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        Step::Nothing
    ));
    let mut fixed = McpServer::new(Some("nightly-job".into()));
    fixed.step(&call(
        1,
        "initialize",
        json!({"clientInfo": {"name": "codex"}}),
    ));
    assert_eq!(fixed.agent(), "nightly-job");
}

#[test]
fn every_action_is_a_tool_with_a_schema() {
    let mut server = McpServer::new(None);
    let a = answer(server.step(&call(2, "tools/list", json!({}))));
    let tools = a["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16);
    for tool in tools {
        assert!(tool["name"].as_str().unwrap().starts_with("sshcraft_"));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[test]
fn unknown_tools_bad_arguments_and_bad_ids_are_protocol_errors() {
    let mut server = McpServer::new(None);
    let bad = answer(server.step(&call(
        4,
        "tools/call",
        json!({"name": "sshcraft_exec", "arguments": {"host": "web"}}),
    )));
    assert_eq!(bad["error"]["code"], -32602);
    let unknown = answer(server.step(&call(5, "tools/call", json!({"name": "rm_rf"}))));
    assert_eq!(unknown["error"]["code"], -32602);
    let cancel_is_no_tool = answer(server.step(&call(
        6,
        "tools/call",
        json!({"name": "sshcraft_cancel", "arguments": {"request": 1}}),
    )));
    assert_eq!(cancel_is_no_tool["error"]["code"], -32602);
    let null_id = answer(server.step(&json!({"jsonrpc": "2.0", "id": null, "method": "ping"})));
    assert_eq!(null_id["error"]["code"], -32600);
    let missing = answer(server.step(&call(7, "resources/list", json!({}))));
    assert_eq!(missing["error"]["code"], -32601);
}

#[tokio::test]
async fn a_tool_call_becomes_an_action_and_its_result_text() {
    let fake = Fake::default();
    let a = call_tool(&fake, "codex", &json!(3), Action::ListHosts).await;
    assert_eq!(a["result"]["isError"], false);
    assert_eq!(
        a["result"]["content"][0]["text"],
        "hello\n\n[exit status 0]"
    );
    assert_eq!(fake.calls.lock().unwrap()[0].0, "codex");
    let refused = Fake {
        error: Some(AgentError::new(ErrorKind::Denied, "declined in SSHCraft")),
        ..Fake::default()
    };
    let a = call_tool(&refused, "codex", &json!(4), Action::ListHosts).await;
    assert_eq!(a["result"]["isError"], true);
    assert!(
        a["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("declined")
    );
}

async fn next_line(read: &mut tokio::io::Lines<BufReader<tokio::io::DuplexStream>>) -> Value {
    let line = tokio::time::timeout(Duration::from_secs(5), read.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn a_waiting_call_holds_up_nothing_and_cancelling_drops_it() {
    let dropped = Arc::new(AtomicBool::new(false));
    let fake = Fake {
        dropped: dropped.clone(),
        ..Fake::default()
    };
    let (mut to_server, server_in) = tokio::io::duplex(64 * 1024);
    let (server_out, from_server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(serve(fake, None, BufReader::new(server_in), server_out));
    let mut read = BufReader::new(from_server).lines();
    let send = |v: Value| format!("{v}\n");
    to_server
        .write_all(
            send(call(
                1,
                "tools/call",
                json!({"name": "sshcraft_exec", "arguments": {"host": "web", "command": "sleep"}}),
            ))
            .as_bytes(),
        )
        .await
        .unwrap();
    to_server
        .write_all(send(call(2, "ping", json!({}))).as_bytes())
        .await
        .unwrap();
    assert_eq!(next_line(&mut read).await["id"], 2);
    to_server
        .write_all(
            send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}}))
                .as_bytes(),
        )
        .await
        .unwrap();
    to_server.write_all(b"not json\n").await.unwrap();
    assert_eq!(next_line(&mut read).await["error"]["code"], -32700);
    assert!(dropped.load(Ordering::SeqCst));
    drop(to_server);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
