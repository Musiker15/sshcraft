//! SSHCraft's MCP server (`sshcraft-cli mcp`): the Model Context Protocol over stdio, one
//! JSON-RPC message per line. Every tool is a thin wrapper around one engine command of the
//! running app ([`backend::Remote`]); the app's agent gate decides what runs and asks the user
//! where its settings say so. Calls run side by side, so one waiting for the user holds up
//! nothing else; a cancelled call is dropped, and with it its question in SSHCraft. When stdin
//! ends, the connection to the app ends, and with it the sessions the agent opened.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod backend;
pub mod tools;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use backend::{Backend, BackendError, Remote};
use tools::{Call, call_for, render, tools};

/// The protocol version this server speaks; a client asking for one of [`OLDER_VERSIONS`] gets
/// that one.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const OLDER_VERSIONS: [&str; 2] = ["2025-03-26", "2024-11-05"];
/// The longest message line read; a longer one gets an error and is skipped.
pub const MAX_LINE: usize = 1 << 20;
/// Calls running at once.
pub const MAX_CALLS: usize = 64;

const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const PARSE_ERROR: i64 = -32700;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

/// An agent name as SSHCraft takes it: 1 to 64 printable characters.
pub fn valid_agent_name(name: &str) -> bool {
    let count = name.chars().count();
    (1..=64).contains(&count)
        && !name.chars().any(|c| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}

/// Who the agent is, and what to do with one message.
struct Server {
    agent: String,
    /// `--agent` given on the command line wins over the client's own name.
    fixed: bool,
}

enum Step {
    Answer(Value),
    Call { id: Value, call: Call },
    Cancel(Value),
    Nothing,
}

impl Server {
    fn step(&mut self, message: &Value) -> Step {
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
                    && let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str).filter(|n| valid_agent_name(n))
                {
                    self.agent = name.to_owned();
                }
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if OLDER_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSION };
                reply(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name": "sshcraft", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": "SSHCraft runs SSH commands and sessions on the user's hosts. Private keys never leave SSHCraft. Some actions need the user's approval in the SSHCraft window; a denied action is the user's decision. Sessions end when this server stops.",
                    }),
                )
            }
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({"tools": tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                match call_for(name, &arguments) {
                    Ok(call) => return Step::Call { id, call },
                    Err(problem) => failure(&id, INVALID_PARAMS, &problem),
                }
            }
            _ => failure(&id, METHOD_NOT_FOUND, "method not found"),
        })
    }
}

async fn run_call(backend: &dyn Backend, agent: &str, id: &Value, call: Call) -> Value {
    let result = match call {
        Call::Execute { command, params } => backend.execute(agent, &command, params).await,
        Call::Commands => backend.commands(agent).await,
        Call::Windows => Ok(json!([{ "id": "main", "title": "SSHCraft" }])),
    };
    reply(
        id,
        match result {
            Ok(value) => text_result(render(&value), false),
            Err(error) => text_result(error.to_string(), true),
        },
    )
}

/// Reads one line of at most [`MAX_LINE`] bytes; `Ok(None)` at the end, `Err(())` (skipped
/// to its end) when it is longer.
async fn next_line<R: AsyncBufRead + Unpin>(input: &mut R) -> std::io::Result<Option<Result<String, ()>>> {
    let mut buf = Vec::new();
    let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX).saturating_add(1);
    let n = (&mut *input).take(limit).read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > MAX_LINE {
        // Skip the rest of the line.
        let mut rest = Vec::new();
        loop {
            rest.clear();
            let n = (&mut *input).take(64 * 1024).read_until(b'\n', &mut rest).await?;
            if n == 0 || rest.last() == Some(&b'\n') {
                break;
            }
        }
        return Ok(Some(Err(())));
    }
    Ok(Some(String::from_utf8(buf).map_err(|_| ())))
}

/// Serves MCP on `input` and `output` until the input ends. `agent` (from `--agent`) wins over
/// the name the client gives.
pub async fn serve<R, W>(backend: Arc<dyn Backend>, agent: Option<String>, mut input: R, mut output: W) -> Result<(), McpError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut server = Server { fixed: agent.is_some(), agent: agent.unwrap_or_else(|| "mcp".into()) };
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
    while let Some(line) = next_line(&mut input).await? {
        calls.retain(|_, task| !task.is_finished());
        let Ok(line) = line else {
            let _ = answers.send(failure(&Value::Null, PARSE_ERROR, "message too long or not UTF-8"));
            continue;
        };
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
            Step::Call { id, .. } if calls.len() >= MAX_CALLS => {
                let _ = answers.send(failure(&id, INVALID_REQUEST, "too many calls at once"));
            }
            Step::Call { id, call } => {
                let backend = backend.clone();
                let agent = server.agent.clone();
                let answers = answers.clone();
                let key = id.to_string();
                let task = tokio::spawn(async move {
                    let answer = run_call(backend.as_ref(), &agent, &id, call).await;
                    let _ = answers.send(answer);
                });
                calls.insert(key, task);
            }
            Step::Cancel(request) => {
                // Dropping the call withdraws its question in SSHCraft; no answer follows.
                if let Some(task) = calls.remove(&request.to_string()) {
                    task.abort();
                }
            }
            Step::Nothing => {}
        }
    }
    // The input ended: running calls stop, then the answers still queued go out.
    for task in calls.into_values() {
        task.abort();
    }
    drop(answers);
    let _ = writer.await;
    Ok(())
}
