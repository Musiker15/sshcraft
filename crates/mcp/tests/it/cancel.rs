//! A cancelled call withdraws its question; an agent program that goes away takes its questions
//! and sessions with it.

use serde_json::{Value, json};

use crate::{start, world};

fn cancelled(log: &[Value]) -> bool {
    log.iter().any(|e| e["outcome"] == "cancelled")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_call_withdraws_its_question() {
    let w = world().await;
    let _window = w.session.subscribe();
    w.set_connect("ask").await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    mcp.send(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "sshcraft_session_open", "arguments": {"host": "web"}}}))
        .await;
    w.until_questions(1).await;
    mcp.send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 5, "reason": "the user stopped"}})).await;
    w.until_questions(0).await;
    assert!(cancelled(&w.agent_log().await), "the cancellation is logged");
    assert_eq!(mcp.request(6, "ping", json!({})).await["result"], json!({}), "no answer came for the cancelled call, the server goes on");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gone_mcp_process_ends_its_sessions_and_questions() {
    let w = world().await;
    let _window = w.session.subscribe();
    w.set_connect("allow").await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    let opened = mcp.call(2, "sshcraft_session_open", json!({"host": "web"})).await;
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    assert_eq!(w.session.agent_link_status().connections, 1);
    w.set_connect("ask").await;
    mcp.send(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "sshcraft_session_open", "arguments": {"host": "web"}}}))
        .await;
    w.until_questions(1).await;
    // The agent's program ends: stdin closes. Its connection ends, and with it (engine test
    // closing_the_link_ends_sessions_and_withdraws_questions) its sessions.
    mcp.close().await;
    w.until_questions(0).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while w.session.agent_link_status().connections != 0 {
        assert!(std::time::Instant::now() < deadline, "the agent's connection is still open");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // The log entry is written as the connection winds down: it may come a moment later.
    while !cancelled(&w.agent_log().await) {
        assert!(std::time::Instant::now() < deadline, "no cancelled call in the agent log");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_id_still_running_is_refused_and_cancel_hits_the_right_call() {
    let w = world().await;
    let _window = w.session.subscribe();
    w.set_connect("ask").await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    let open = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "sshcraft_session_open", "arguments": {"host": "web"}}});
    mcp.send(open.clone()).await;
    w.until_questions(1).await;
    mcp.send(open).await;
    let answer = mcp.recv().await;
    assert_eq!(answer["id"], 9);
    assert_eq!(answer["error"]["code"], -32600, "{answer}");
    assert_eq!(w.questions().await, 1, "the second call never ran");
    mcp.send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 9}})).await;
    w.until_questions(0).await;
}
