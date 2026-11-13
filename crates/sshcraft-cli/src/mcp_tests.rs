use super::*;
use sshcraft_agent::{AgentError, ErrorKind};

/// Notes calls and answers with a fixed value or error.
#[derive(Default)]
struct Fake {
    calls: Vec<(String, Action)>,
    error: Option<AgentError>,
}

#[async_trait::async_trait]
impl Caller for &mut Fake {
    async fn call(&mut self, agent: &str, action: Action) -> Result<Value, AgentError> {
        self.calls.push((agent.to_owned(), action));
        match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok(json!({"output": "hello\n", "truncated": false})),
        }
    }
}

fn call(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[tokio::test]
async fn initialize_names_the_agent_and_the_protocol() {
    let mut fake = Fake::default();
    let mut server = McpServer::new(&mut fake);
    let answer = server
        .handle(call(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "claude-code", "version": "2"}, "capabilities": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(answer["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(answer["result"]["capabilities"]["tools"].is_object());
    assert_eq!(server.agent(), "claude-code");
    // The notification that follows gets no answer.
    assert!(
        server
            .handle(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await
            .is_none()
    );
}

#[tokio::test]
async fn every_action_is_a_tool_with_a_schema() {
    let mut fake = Fake::default();
    let mut server = McpServer::new(&mut fake);
    let answer = server
        .handle(call(2, "tools/list", json!({})))
        .await
        .unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16);
    for tool in tools {
        assert!(tool["name"].as_str().unwrap().starts_with("sshcraft_"));
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[tokio::test]
async fn a_tool_call_becomes_an_action_and_its_result_text() {
    let mut fake = Fake::default();
    {
        let mut server = McpServer::new(&mut fake);
        server
            .handle(call(
                1,
                "initialize",
                json!({"clientInfo": {"name": "codex"}}),
            ))
            .await;
        let answer = server
            .handle(call(
                3,
                "tools/call",
                json!({"name": "sshcraft_exec", "arguments": {"host": "web", "command": "uptime"}}),
            ))
            .await
            .unwrap();
        assert_eq!(answer["result"]["isError"], false);
        assert_eq!(answer["result"]["content"][0]["text"], "hello\n");
    }
    assert_eq!(
        fake.calls,
        vec![(
            "codex".to_owned(),
            Action::Exec {
                host: "web".into(),
                command: "uptime".into(),
                timeout_secs: None
            }
        )]
    );
}

#[tokio::test]
async fn wrong_arguments_and_refusals_are_tool_errors() {
    let mut fake = Fake {
        error: Some(AgentError::new(ErrorKind::Denied, "declined in SSHCraft")),
        ..Fake::default()
    };
    let mut server = McpServer::new(&mut fake);
    let bad = server
        .handle(call(
            4,
            "tools/call",
            json!({"name": "sshcraft_exec", "arguments": {"host": "web"}}),
        ))
        .await
        .unwrap();
    assert_eq!(bad["result"]["isError"], true);
    let unknown = server
        .handle(call(
            5,
            "tools/call",
            json!({"name": "rm_rf", "arguments": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(unknown["result"]["isError"], true);
    let denied = server
        .handle(call(
            6,
            "tools/call",
            json!({"name": "sshcraft_list_hosts"}),
        ))
        .await
        .unwrap();
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("declined")
    );
}

#[tokio::test]
async fn unknown_methods_get_the_json_rpc_error() {
    let mut fake = Fake::default();
    let mut server = McpServer::new(&mut fake);
    let answer = server
        .handle(call(7, "resources/list", json!({})))
        .await
        .unwrap();
    assert_eq!(answer["error"]["code"], -32601);
    let pong = server.handle(call(8, "ping", json!({}))).await.unwrap();
    assert!(pong["result"].is_object());
}

#[tokio::test]
async fn serve_answers_line_by_line_and_reports_broken_json() {
    let mut fake = Fake::default();
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\nnot json\n".to_vec();
    let mut output = Vec::new();
    serve(&mut fake, input.as_slice(), &mut output)
        .await
        .unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["error"]["code"], -32700);
}
