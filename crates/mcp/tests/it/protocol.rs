//! The protocol: who the agent is, the versions, and bad input that gets an error, never a crash.

use serde_json::json;

use crate::{start, text, world};

#[tokio::test(flavor = "multi_thread")]
async fn initialize_names_the_agent() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    let init = mcp.initialize("claude-code").await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "sshcraft");
    mcp.call(2, "sshcraft_delete_host", json!({"host": "nowhere"})).await;
    assert_eq!(w.agent_log().await[0]["agent"], "claude-code", "the client's own name");
    mcp.close().await;
    // --agent wins over the client's name.
    let w = world().await;
    let mut mcp = start(w.backend(), Some("ops-bot"));
    mcp.initialize("claude-code").await;
    mcp.call(2, "sshcraft_delete_host", json!({"host": "nowhere"})).await;
    assert_eq!(w.agent_log().await[0]["agent"], "ops-bot");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_client_gets_its_version() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    let init = mcp.request(1, "initialize", json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "old"}})).await;
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    let init = mcp.request(2, "initialize", json!({"protocolVersion": "1999-01-01"})).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "an unknown one gets ours");
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_has_the_old_names() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    let list = mcp.request(1, "tools/list", json!({})).await;
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in
        ["execute_command", "list_commands", "list_windows", "sshcraft_list_hosts", "sshcraft_exec", "sshcraft_session_open", "sshcraft_import_key"]
    {
        assert!(names.contains(&name), "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_gets_an_error_not_a_crash() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    mcp.send_raw(b"this is not json\n").await;
    let answer = mcp.recv().await;
    assert_eq!(answer["error"]["code"], -32700);
    mcp.send_raw(&vec![b'x'; (1 << 20) + 100]).await;
    mcp.send_raw(b"\n").await;
    assert_eq!(mcp.recv().await["error"]["code"], -32700, "a line too long");
    mcp.send(json!({"jsonrpc": "2.0", "id": null, "method": "ping"})).await;
    assert_eq!(mcp.recv().await["error"]["code"], -32600, "a null id");
    let answer = mcp.call(5, "sshcraft_exec", json!("not an object")).await;
    assert_eq!(answer["error"]["code"], -32602);
    let answer = mcp.call(6, "no_such_tool", json!({})).await;
    assert_eq!(answer["error"]["code"], -32602);
    assert_eq!(mcp.request(7, "ping", json!({})).await["result"], json!({}), "it keeps serving");
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_are_tool_errors() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    let answer = mcp.call(2, "sshcraft_delete_host", json!({"host": "nowhere"})).await;
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    assert!(!text(&answer).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_app_every_tool_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let backend = std::sync::Arc::new(sshcraft_mcp::Remote::window(dir.path().to_path_buf(), "mcp"));
    let mut mcp = start(backend, None);
    let answer = mcp.call(1, "sshcraft_list_hosts", json!({})).await;
    assert_eq!(answer["result"]["isError"], true);
    assert!(text(&answer).contains("SSHCraft is not running"), "{answer}");
}
