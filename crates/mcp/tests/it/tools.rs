//! Each tool is one engine command; calls run side by side.

use serde_json::json;
use sshcraft_mcp::tools::{Call, call_for, tools};

use crate::{start, text, world};

#[test]
fn a_tool_runs_its_command() {
    let table = [
        ("sshcraft_list_hosts", json!({}), "host.list", json!({})),
        ("sshcraft_list_groups", json!({}), "group.list", json!({})),
        ("sshcraft_list_keys", json!({}), "key.list", json!({})),
        ("sshcraft_host_info", json!({"host": "web"}), "host.info", json!({"host": "web"})),
        ("sshcraft_exec", json!({"host": "web", "command": "ls"}), "host.exec", json!({"host": "web", "command": "ls"})),
        ("sshcraft_session_open", json!({"host": "web"}), "session.open", json!({"host": "web"})),
        ("sshcraft_session_write", json!({"session": "s", "data": "ls\n"}), "session.send", json!({"id": "s", "data": "ls\n"})),
        ("sshcraft_session_read", json!({"session": "s", "wait_ms": 5}), "session.read", json!({"id": "s", "wait_ms": 5})),
        ("sshcraft_session_close", json!({"session": "s"}), "session.close", json!({"id": "s"})),
        ("sshcraft_save_host", json!({"host": {"name": "db"}}), "host.save", json!({"host": {"name": "db"}})),
        ("sshcraft_save_group", json!({"group": {"name": "g"}}), "group.save", json!({"group": {"name": "g"}})),
        ("sshcraft_delete_host", json!({"host": "web"}), "host.delete", json!({"host": "web"})),
        ("sshcraft_delete_group", json!({"group": "g"}), "group.delete", json!({"group": "g"})),
        ("sshcraft_delete_key", json!({"key": "k"}), "key.delete", json!({"key": "k"})),
        ("sshcraft_generate_key", json!({"name": "k", "type": "ed25519"}), "key.generate", json!({"name": "k", "key_type": "ed25519"})),
        ("sshcraft_import_key", json!({"name": "k", "text": "t"}), "key.import", json!({"name": "k", "text": "t"})),
        ("execute_command", json!({"id": "forward.status", "params": {"x": 1}}), "forward.status", json!({"x": 1})),
    ];
    for (tool, arguments, command, params) in table {
        assert_eq!(call_for(tool, &arguments).unwrap(), Call::Execute { command: command.into(), params }, "{tool}");
    }
    assert_eq!(call_for("list_commands", &json!({})).unwrap(), Call::Commands);
    // Every listed tool has a call.
    for t in tools() {
        let name = t["name"].as_str().unwrap();
        assert!(call_for(name, &json!({"id": "x"})).is_ok(), "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_listed_host_comes_through_and_commands_list_what_agents_reach() {
    let w = world().await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    assert!(text(&mcp.call(2, "sshcraft_list_hosts", json!({})).await).contains("web"));
    let commands = text(&mcp.call(3, "list_commands", json!({})).await);
    assert!(commands.contains("host.exec") && !commands.contains("agent.set_level"), "{commands}");
    let run = mcp.call(4, "execute_command", json!({"id": "host.list"})).await;
    assert!(text(&run).contains("web"));
}

#[tokio::test(flavor = "multi_thread")]
async fn calls_run_side_by_side() {
    let w = world().await;
    let _window = w.session.subscribe();
    w.set_connect("ask").await;
    let mut mcp = start(w.backend(), None);
    mcp.initialize("claude").await;
    // This one waits for the user's answer in the window.
    mcp.send(json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "sshcraft_session_open", "arguments": {"host": "web"}}}))
        .await;
    w.until_questions(1).await;
    assert_eq!(mcp.request(10, "ping", json!({})).await["result"], json!({}), "the waiting call holds up nothing");
    assert!(text(&mcp.call(11, "sshcraft_list_hosts", json!({})).await).contains("web"));
}
