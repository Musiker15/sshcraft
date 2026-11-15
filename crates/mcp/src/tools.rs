//! The tools: the three of every Craft app (`execute_command`, `list_commands`, `list_windows`)
//! and SSHCraft's own under the names the Tauri version gave them. Each one is a thin wrapper
//! around one engine command; the engine's agent gate decides what runs.

use serde_json::{Map, Value, json};

fn host_arg() -> Value {
    json!({"type": "string", "description": "Host id, or a name only one host has"})
}

fn session_arg() -> Value {
    json!({"type": "string", "description": "Session id from sshcraft_session_open"})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({"name": name, "description": description, "inputSchema": schema})
}

/// Every tool with its description and input schema.
pub fn tools() -> Vec<Value> {
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
            "execute_command",
            "Run any SSHCraft engine command an agent may use, by id (see list_commands). SSHCraft may ask the user first.",
            object(json!({"id": {"type": "string"}, "params": {"type": "object"}}), &["id"]),
        ),
        tool("list_commands", "List the engine commands agents may use, with their parameters.", object(json!({}), &[])),
        tool("list_windows", "List SSHCraft's windows.", object(json!({}), &[])),
        tool("sshcraft_list_hosts", "List the hosts in SSHCraft (no passwords, no keys).", object(json!({}), &[])),
        tool("sshcraft_list_groups", "List the host groups in SSHCraft.", object(json!({}), &[])),
        tool("sshcraft_list_keys", "List stored SSH keys by name, type and fingerprint. Key material is never available.", object(json!({}), &[])),
        tool("sshcraft_host_info", "Show one host.", object(json!({"host": host_arg()}), &["host"])),
        tool(
            "sshcraft_exec",
            "Run a command on a host over SSH and return its output (at most 1 MiB). SSHCraft may ask the user first.",
            object(
                json!({"host": host_arg(), "command": {"type": "string"}, "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 600}}),
                &["host", "command"],
            ),
        ),
        tool(
            "sshcraft_session_open",
            "Open an interactive shell on a host; returns a session id. SSHCraft may ask the user first.",
            object(json!({"host": host_arg()}), &["host"]),
        ),
        tool(
            "sshcraft_session_write",
            "Send text to a session (end lines with \\n to run them).",
            object(json!({"session": session_arg(), "data": {"type": "string"}}), &["session", "data"]),
        ),
        tool(
            "sshcraft_session_read",
            "Read new output of a session, waiting up to wait_ms (at most 10000) if there is none yet.",
            object(json!({"session": session_arg(), "wait_ms": {"type": "integer", "minimum": 0, "maximum": 10000}}), &["session"]),
        ),
        tool("sshcraft_session_close", "Close a session.", object(json!({"session": session_arg()}), &["session"])),
        tool("sshcraft_save_host", "Create or change a host.", object(json!({"host": host_input}), &["host"])),
        tool("sshcraft_save_group", "Create or change a group.", object(json!({"group": group_input}), &["group"])),
        tool("sshcraft_delete_host", "Delete a host.", object(json!({"host": host_arg()}), &["host"])),
        tool("sshcraft_delete_group", "Delete a group and its hosts.", object(json!({"group": {"type": "string"}}), &["group"])),
        tool("sshcraft_delete_key", "Delete a stored key.", object(json!({"key": {"type": "string"}}), &["key"])),
        tool(
            "sshcraft_generate_key",
            "Generate a new SSH key in SSHCraft; returns its name, type and fingerprint.",
            object(json!({"name": {"type": "string"}, "type": {"type": "string", "enum": ["ed25519", "ecdsa", "rsa"]}}), &["name", "type"]),
        ),
        tool(
            "sshcraft_import_key",
            "Import a private key (OpenSSH or PEM) into SSHCraft. It goes in, never out.",
            object(json!({"name": {"type": "string"}, "text": {"type": "string"}, "passphrase": {"type": "string"}}), &["name", "text"]),
        ),
    ]
}

/// What a tool call does.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// Run engine command `command` with `params`.
    Execute {
        command: String,
        params: Value,
    },
    Commands,
    Windows,
}

/// Renames `from` to `to` in `args`.
fn rename(mut args: Map<String, Value>, from: &str, to: &str) -> Map<String, Value> {
    if let Some(value) = args.remove(from) {
        args.insert(to.to_string(), value);
    }
    args
}

/// The call tool `name` with `arguments` stands for.
pub fn call_for(name: &str, arguments: &Value) -> Result<Call, String> {
    let args = match arguments {
        Value::Object(map) => map.clone(),
        Value::Null => Map::new(),
        _ => return Err("arguments must be an object".into()),
    };
    let execute = |command: &str, args: Map<String, Value>| Ok(Call::Execute { command: command.to_string(), params: Value::Object(args) });
    match name {
        "execute_command" => {
            let command = args.get("id").and_then(Value::as_str).ok_or("execute_command needs id")?.to_string();
            let params = match args.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(Value::Object(p)) => Value::Object(p.clone()),
                Some(_) => return Err("params must be an object".into()),
            };
            Ok(Call::Execute { command, params })
        }
        "list_commands" => Ok(Call::Commands),
        "list_windows" => Ok(Call::Windows),
        "sshcraft_list_hosts" => execute("host.list", args),
        "sshcraft_list_groups" => execute("group.list", args),
        "sshcraft_list_keys" => execute("key.list", args),
        "sshcraft_host_info" => execute("host.info", args),
        "sshcraft_exec" => execute("host.exec", args),
        "sshcraft_session_open" => execute("session.open", args),
        "sshcraft_session_write" => execute("session.send", rename(args, "session", "id")),
        "sshcraft_session_read" => execute("session.read", rename(args, "session", "id")),
        "sshcraft_session_close" => execute("session.close", rename(args, "session", "id")),
        "sshcraft_save_host" => execute("host.save", args),
        "sshcraft_save_group" => execute("group.save", args),
        "sshcraft_delete_host" => execute("host.delete", args),
        "sshcraft_delete_group" => execute("group.delete", args),
        "sshcraft_delete_key" => execute("key.delete", args),
        "sshcraft_generate_key" => execute("key.generate", rename(args, "type", "key_type")),
        "sshcraft_import_key" => execute("key.import", args),
        other => Err(format!("unknown tool {}", other.chars().filter(|c| !c.is_control()).take(64).collect::<String>())),
    }
}

/// A result as text: plain output for command output, JSON otherwise.
pub fn render(value: &Value) -> String {
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
