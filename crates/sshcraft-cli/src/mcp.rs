//! `sshcraft mcp`: a Model Context Protocol server over stdio (JSON-RPC 2.0, one message per
//! line). Every SSHCraft action is a tool; the agent's name is the client name it sends in
//! `initialize`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use sshcraft_agent::{Action, valid_agent_name};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::client::Caller;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

fn host_arg() -> Value {
    json!({"type": "string", "description": "Host id, or a name only one host has"})
}

fn session_arg() -> Value {
    json!({"type": "string", "description": "Session id from sshcraft_session_open"})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

/// The tools: name (`sshcraft_<action>`), description and input schema.
pub fn tools() -> Vec<Value> {
    let tool = |action: &str, description: &str, schema: Value| json!({"name": format!("sshcraft_{action}"), "description": description, "inputSchema": schema});
    let host_input = json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "Leave out to create a host"},
            "name": {"type": "string"},
            "group": {"type": "string", "description": "Group id"},
            "address": {"type": "string"},
            "port": {"type": "integer"},
            "username": {"type": "string"},
            "key": {"type": "string", "description": "Id of a stored key"},
            "tags": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["name"]
    });
    let group_input = json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "Leave out to create a group"},
            "name": {"type": "string"},
            "parent": {"type": "string", "description": "Parent group id"}
        },
        "required": ["name"]
    });
    vec![
        tool(
            "list_hosts",
            "List the hosts in SSHCraft (no passwords, no keys).",
            object(json!({}), &[]),
        ),
        tool(
            "list_groups",
            "List the host groups in SSHCraft.",
            object(json!({}), &[]),
        ),
        tool(
            "list_keys",
            "List stored SSH keys by name, type and fingerprint. Key material is never available.",
            object(json!({}), &[]),
        ),
        tool(
            "host_info",
            "Show one host.",
            object(json!({"host": host_arg()}), &["host"]),
        ),
        tool(
            "exec",
            "Run a command on a host over SSH and return its output (at most 1 MiB). SSHCraft may ask the user first.",
            object(
                json!({"host": host_arg(), "command": {"type": "string"}, "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 600}}),
                &["host", "command"],
            ),
        ),
        tool(
            "session_open",
            "Open an interactive shell on a host; returns a session id. SSHCraft may ask the user first.",
            object(json!({"host": host_arg()}), &["host"]),
        ),
        tool(
            "session_write",
            "Send text to a session (end lines with \\n to run them).",
            object(
                json!({"session": session_arg(), "data": {"type": "string"}}),
                &["session", "data"],
            ),
        ),
        tool(
            "session_read",
            "Read new output of a session, waiting up to wait_ms (at most 10000) if there is none yet.",
            object(
                json!({"session": session_arg(), "wait_ms": {"type": "integer", "minimum": 0, "maximum": 10000}}),
                &["session"],
            ),
        ),
        tool(
            "session_close",
            "Close a session.",
            object(json!({"session": session_arg()}), &["session"]),
        ),
        tool(
            "save_host",
            "Create or change a host.",
            object(json!({"host": host_input}), &["host"]),
        ),
        tool(
            "save_group",
            "Create or change a group.",
            object(json!({"group": group_input}), &["group"]),
        ),
        tool(
            "delete_host",
            "Delete a host.",
            object(json!({"host": host_arg()}), &["host"]),
        ),
        tool(
            "delete_group",
            "Delete a group and its hosts.",
            object(json!({"group": {"type": "string"}}), &["group"]),
        ),
        tool(
            "delete_key",
            "Delete a stored key.",
            object(json!({"key": {"type": "string"}}), &["key"]),
        ),
        tool(
            "generate_key",
            "Generate a new SSH key in SSHCraft; returns its name, type and fingerprint.",
            object(
                json!({"name": {"type": "string"}, "type": {"type": "string", "enum": ["ed25519", "ecdsa", "rsa"]}}),
                &["name", "type"],
            ),
        ),
        tool(
            "import_key",
            "Import a private key (OpenSSH or PEM) into SSHCraft. It goes in, never out.",
            object(
                json!({"name": {"type": "string"}, "text": {"type": "string"}, "passphrase": {"type": "string"}}),
                &["name", "text"],
            ),
        ),
    ]
}

/// The action a tool call stands for.
pub fn action_for(tool: &str, arguments: &Value) -> Result<Action, String> {
    let name = tool
        .strip_prefix("sshcraft_")
        .filter(|name| *name != "cancel")
        .ok_or_else(|| format!("unknown tool {tool}"))?;
    let mut message = match arguments {
        Value::Object(map) => map.clone(),
        Value::Null => serde_json::Map::new(),
        _ => return Err("arguments must be an object".into()),
    };
    message.insert("action".into(), Value::String(name.to_owned()));
    serde_json::from_value(Value::Object(message)).map_err(|e| format!("invalid arguments: {e}"))
}

/// JSON-RPC error codes.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const PARSE_ERROR: i64 = -32700;

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

/// The MCP side: who the agent is, and the answers that need no call to the app.
pub struct McpServer {
    agent: String,
    /// `--agent` given on the command line wins over the client's own name.
    fixed: bool,
}

/// What to do with one incoming message.
pub enum Step {
    /// Answer at once.
    Answer(Value),
    /// A tool call: run it (it may wait for the user) and answer when done.
    Call { id: Value, action: Action },
    /// Stop waiting for the call with this id.
    Cancel(Value),
    /// A notification: nothing to answer.
    Nothing,
}

impl McpServer {
    pub fn new(agent: Option<String>) -> Self {
        Self {
            fixed: agent.is_some(),
            agent: agent.unwrap_or_else(|| "mcp".into()),
        }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    pub fn step(&mut self, message: &Value) -> Step {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            if method == "notifications/cancelled"
                && let Some(request) = params.get("requestId")
            {
                return Step::Cancel(request.clone());
            }
            return Step::Nothing;
        };
        // MCP request ids are strings or numbers, never null.
        if !(id.is_string() || id.is_number()) {
            return Step::Answer(failure(&Value::Null, INVALID_REQUEST, "invalid request id"));
        }
        Step::Answer(match method {
            "initialize" => {
                if !self.fixed
                    && let Some(name) = params
                        .pointer("/clientInfo/name")
                        .and_then(Value::as_str)
                        .filter(|n| valid_agent_name(n))
                {
                    self.agent = name.to_owned();
                }
                reply(
                    &id,
                    json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name": "sshcraft", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": "SSHCraft runs SSH commands and sessions on the user's hosts. Private keys never leave SSHCraft. Some actions need the user's approval in the SSHCraft window; a denied action is the user's decision. Sessions end when this server stops.",
                    }),
                )
            }
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({"tools": tools()})),
            "tools/call" => {
                let tool = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                match action_for(tool, &arguments) {
                    Ok(action) => return Step::Call { id, action },
                    Err(problem) => failure(&id, INVALID_PARAMS, &problem),
                }
            }
            _ => failure(&id, METHOD_NOT_FOUND, "method not found"),
        })
    }
}

/// Runs one tool call against the app and turns the result into the tool result.
pub async fn call_tool<C: Caller>(caller: &C, agent: &str, id: &Value, action: Action) -> Value {
    let result = match caller.call(agent, action).await {
        Ok(value) => text_result(render(&value), false),
        Err(error) => text_result(format!("{:?}: {}", error.kind, error.message), true),
    };
    reply(id, result)
}

/// A result as text: plain output for command output, JSON otherwise.
fn render(value: &Value) -> String {
    match value.get("output").and_then(Value::as_str) {
        Some(output) if value.get("truncated").is_some() || value.get("ended").is_some() => {
            let mut text = output.to_owned();
            if value.get("truncated") == Some(&Value::Bool(true)) {
                text.push_str("\n[output cut at 1 MiB]");
            }
            if let Some(status) = value.get("exit_status").and_then(Value::as_u64) {
                text.push_str(&format!("\n[exit status {status}]"));
            }
            if value.get("ended") == Some(&Value::Bool(true)) {
                text.push_str("\n[session ended]");
            }
            text
        }
        _ => serde_json::to_string_pretty(value).unwrap_or_default(),
    }
}

fn key_of(id: &Value) -> String {
    id.to_string()
}

/// Serves MCP on `input`/`output` until the input ends. Tool calls run side by side, so a call
/// waiting for the user's answer holds up neither `ping` nor other calls; a cancelled call is
/// dropped (and with it the question in SSHCraft).
pub async fn serve<C: Caller, R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin + Send + 'static>(
    caller: C,
    agent: Option<String>,
    input: R,
    mut output: W,
) -> std::io::Result<()> {
    let caller = Arc::new(caller);
    let mut server = McpServer::new(agent);
    let (answers, mut outgoing) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(answer) = outgoing.recv().await {
            let mut text = serde_json::to_string(&answer).unwrap_or_default();
            text.push('\n');
            if output.write_all(text.as_bytes()).await.is_err() || output.flush().await.is_err() {
                return;
            }
        }
    });
    let mut calls: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut lines = input.lines();
    while let Some(line) = lines.next_line().await? {
        calls.retain(|_, task| !task.is_finished());
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            let _ = answers.send(failure(&Value::Null, PARSE_ERROR, "parse error"));
            continue;
        };
        match server.step(&message) {
            Step::Answer(answer) => {
                let _ = answers.send(answer);
            }
            Step::Call { id, action } => {
                let caller = caller.clone();
                let agent = server.agent().to_owned();
                let answers = answers.clone();
                let key = key_of(&id);
                let task = tokio::spawn(async move {
                    let answer = call_tool(caller.as_ref(), &agent, &id, action).await;
                    let _ = answers.send(answer);
                });
                calls.insert(key, task);
            }
            Step::Cancel(id) => {
                if let Some(task) = calls.remove(&key_of(&id)) {
                    task.abort();
                }
            }
            Step::Nothing => {}
        }
    }
    // Input ended: running calls finish and answer, then the output closes.
    for (_, task) in calls {
        let _ = task.await;
    }
    drop(answers);
    let _ = writer.await;
    Ok(())
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
