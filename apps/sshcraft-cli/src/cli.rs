//! The command line: the same engine commands as the MCP tools, for scripts and agents without
//! MCP, plus `ui …` for a `--control` app.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "sshcraft-cli",
    version,
    about = "Talk to the running SSHCraft app: list hosts, run commands, edit hosts. Private keys stay in SSHCraft. Interactive sessions are for MCP (`sshcraft-cli mcp`)."
)]
pub struct Cli {
    /// Who asks; shown in SSHCraft's questions and log.
    #[arg(long, global = true, default_value = "cli")]
    pub agent: String,
    /// Print results as JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KeyTypeArg {
    Ed25519,
    Ecdsa,
    Rsa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum WhenClosed {
    /// Fail with "SSHCraft is not running" (exit code 3).
    #[default]
    Error,
    /// Start the app as a double click would, and wait for it.
    Launch,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run as an MCP server on stdin and stdout (for Claude Code, Codex and others).
    Mcp {
        /// What to do while SSHCraft is not running.
        #[arg(long, value_enum, default_value_t = WhenClosed::Error)]
        when_closed: WhenClosed,
        /// Work against an app started with `--control <port>` instead of the open window.
        #[arg(long)]
        bridge: Option<u16>,
        /// The token file of that app (with --bridge).
        #[arg(long, requires = "bridge")]
        control_token_file: Option<PathBuf>,
    },
    /// List the engine commands agents may use.
    Commands,
    /// Run one engine command: `--cmd 'host.list'` or `--cmd 'host.info={"host": "web"}'`.
    Run {
        #[arg(long)]
        cmd: String,
    },
    /// List hosts.
    Hosts,
    /// List groups.
    Groups,
    /// List keys (name, type, fingerprint).
    Keys,
    /// Show one host.
    Host { host: String },
    /// Run a command on a host: `sshcraft-cli exec web -- uptime`.
    Exec {
        host: String,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Create or change a host from JSON (`{"name": …, "address": …}`).
    SaveHost {
        #[arg(value_name = "JSON")]
        host: String,
    },
    /// Create or change a group from JSON (`{"name": …, "parent": …}`).
    SaveGroup {
        #[arg(value_name = "JSON")]
        group: String,
    },
    /// Delete a host, a group or a key.
    #[command(subcommand)]
    Delete(DeleteCommand),
    /// Generate a key.
    GenerateKey {
        name: String,
        #[arg(long = "type", value_enum, default_value = "ed25519")]
        key_type: KeyTypeArg,
    },
    /// Import a private key from a file.
    ImportKey { name: String, file: PathBuf },
    /// Drive the window of an app started with `--control <port>` (for tests and development).
    Ui {
        #[arg(long)]
        bridge: u16,
        #[arg(long)]
        control_token_file: PathBuf,
        #[command(subcommand)]
        action: UiCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeleteCommand {
    Host { host: String },
    Group { group: String },
    Key { key: String },
}

#[derive(Debug, Subcommand)]
pub enum UiCommand {
    /// The window's state: page, tabs with their panes, questions, notices, window size.
    Inspect,
    /// Click at a point in screen points.
    Click { x: f32, y: f32 },
    /// Press a key: `Enter`, `Tab`, `A`, with `--ctrl`, `--shift`, `--alt`.
    Key {
        key: String,
        #[arg(long)]
        ctrl: bool,
        #[arg(long)]
        shift: bool,
        #[arg(long)]
        alt: bool,
    },
    /// Type text.
    Text { text: String },
    /// Save a picture of the window as PNG.
    Screenshot { file: PathBuf },
}

/// An engine command and its params.
#[derive(Debug, Clone, PartialEq)]
pub struct Engine {
    pub command: String,
    pub params: Value,
}

fn engine(command: &str, params: Value) -> Option<Engine> {
    Some(Engine { command: command.into(), params })
}

/// `id` or `id={json object}`.
pub fn parse_run(cmd: &str) -> Result<Engine, String> {
    let (id, params) = match cmd.split_once('=') {
        Some((id, json)) => (id.trim(), serde_json::from_str::<Value>(json).map_err(|e| format!("--cmd: the params are not JSON: {e}"))?),
        None => (cmd.trim(), json!({})),
    };
    if id.is_empty() {
        return Err("--cmd: no command id".into());
    }
    if !params.is_object() {
        return Err("--cmd: the params must be a JSON object".into());
    }
    Ok(Engine { command: id.into(), params })
}

/// The engine command a subcommand runs; `Ok(None)` for `mcp`, `commands` and `ui`.
pub fn engine_call(command: &Command) -> Result<Option<Engine>, String> {
    let parse = |what: &str, text: &str| serde_json::from_str::<Value>(text).map_err(|e| format!("{what} JSON: {e}"));
    Ok(match command {
        Command::Mcp { .. } | Command::Commands | Command::Ui { .. } => None,
        Command::Run { cmd } => Some(parse_run(cmd)?),
        Command::Hosts => engine("host.list", json!({})),
        Command::Groups => engine("group.list", json!({})),
        Command::Keys => engine("key.list", json!({})),
        Command::Host { host } => engine("host.info", json!({ "host": host })),
        Command::Exec { host, timeout, command } => {
            let mut params = json!({ "host": host, "command": command.join(" ") });
            if let Some(secs) = timeout {
                params["timeout_secs"] = json!(secs);
            }
            engine("host.exec", params)
        }
        Command::SaveHost { host } => engine("host.save", json!({ "host": parse("host", host)? })),
        Command::SaveGroup { group } => engine("group.save", json!({ "group": parse("group", group)? })),
        Command::Delete(DeleteCommand::Host { host }) => engine("host.delete", json!({ "host": host })),
        Command::Delete(DeleteCommand::Group { group }) => engine("group.delete", json!({ "group": group })),
        Command::Delete(DeleteCommand::Key { key }) => engine("key.delete", json!({ "key": key })),
        Command::GenerateKey { name, key_type } => {
            let key_type = match key_type {
                KeyTypeArg::Ed25519 => "ed25519",
                KeyTypeArg::Ecdsa => "ecdsa",
                KeyTypeArg::Rsa => "rsa",
            };
            engine("key.generate", json!({ "name": name, "key_type": key_type }))
        }
        Command::ImportKey { name, file } => {
            // A key file is a small regular file; a pipe or device could be read without end.
            let meta = std::fs::metadata(file).map_err(|e| format!("{}: {e}", file.display()))?;
            if !meta.is_file() {
                return Err(format!("{}: not a file", file.display()));
            }
            if meta.len() > 1024 * 1024 {
                return Err(format!("{}: larger than 1 MiB, not a key file", file.display()));
            }
            let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
            engine("key.import", json!({ "name": name, "text": text }))
        }
    })
}

/// A result for people: output as it is, lists as lines, the rest as JSON.
pub fn readable(value: &Value) -> String {
    if let Some(output) = value.get("output").and_then(Value::as_str) {
        return output.to_owned();
    }
    if let Some(session) = value.get("session").and_then(Value::as_str) {
        return session.to_owned();
    }
    if let Some(items) = value.as_array() {
        // Names and addresses come from imports and other devices: no escape sequence of theirs
        // reaches the terminal.
        return items
            .iter()
            .map(|item| {
                let field = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or("").replace(|c: char| c.is_control(), "");
                let detail = ["address", "fingerprint", "parent", "label"].iter().map(|f| field(f)).find(|v| !v.is_empty()).unwrap_or_default();
                let (name, id) = (field("name"), field("id"));
                if name.is_empty() { format!("{id}\t{detail}\t") } else { format!("{name}\t{detail}\t{id}") }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if value.is_null() {
        return String::new();
    }
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("sshcraft-cli").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn exec_takes_everything_after_the_dashes() {
        let cli = parse(&["--agent", "script", "exec", "web", "--", "ls", "-la", "/tmp"]);
        assert_eq!(cli.agent, "script");
        assert_eq!(engine_call(&cli.command).unwrap(), engine("host.exec", json!({ "host": "web", "command": "ls -la /tmp" })));
    }

    #[test]
    fn run_parses_id_and_json() {
        assert_eq!(parse_run("host.list").unwrap(), Engine { command: "host.list".into(), params: json!({}) });
        assert_eq!(parse_run(r#"host.info={"host": "web"}"#).unwrap(), Engine { command: "host.info".into(), params: json!({ "host": "web" }) });
        assert!(parse_run("host.info=[1]").is_err(), "params are an object");
        assert!(parse_run("={}").is_err());
    }

    #[test]
    fn bad_json_is_a_usage_error() {
        let cli = parse(&["save-host", "{"]);
        assert!(engine_call(&cli.command).is_err());
        assert!(Cli::try_parse_from(["sshcraft-cli", "exec", "web"]).is_err(), "exec needs a command");
        assert!(Cli::try_parse_from(["sshcraft-cli", "mcp", "--control-token-file", "x"]).is_err(), "a token file needs --bridge");
    }

    #[test]
    fn mcp_is_no_engine_call_and_the_default_agent_is_cli() {
        let cli = parse(&["mcp"]);
        assert_eq!(cli.agent, "cli");
        assert!(matches!(cli.command, Command::Mcp { when_closed: WhenClosed::Error, bridge: None, .. }));
        assert_eq!(engine_call(&cli.command).unwrap(), None);
        assert!(matches!(parse(&["mcp", "--when-closed", "launch"]).command, Command::Mcp { when_closed: WhenClosed::Launch, .. }));
    }

    #[test]
    fn list_fields_lose_control_characters() {
        let hosts = json!([{"id": "1", "name": "web\u{1b}]0;owned\u{7}", "address": "10.0.0.1\u{1b}[2J"}]);
        let text = readable(&hosts);
        assert!(!text.chars().any(|c| c.is_control() && c != '\t'), "{text:?}");
        assert!(text.contains("web") && text.contains("10.0.0.1"));
    }

    #[test]
    fn import_key_takes_only_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let cli = parse(&["import-key", "k", dir.path().to_str().unwrap()]);
        assert!(engine_call(&cli.command).is_err(), "a folder is no key file");
    }

    #[test]
    fn lists_read_as_lines() {
        let hosts = json!([{"id": "1", "name": "web", "address": "10.0.0.1"}]);
        assert_eq!(readable(&hosts), "web\t10.0.0.1\t1");
        assert_eq!(readable(&json!({"output": "up\n", "truncated": false})), "up\n");
        assert_eq!(readable(&json!([{"id": "host.list", "label": "List hosts"}])), "host.list\tList hosts\t");
    }
}
