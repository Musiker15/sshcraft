//! The command line: the same actions as the MCP tools, for scripts and agents without MCP.

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;
use sshcraft_agent::{Action, GroupInput, HostInput, KeyType};

#[derive(Debug, Parser)]
#[command(
    name = "sshcraft",
    version,
    about = "Talk to the running SSHCraft app: list hosts, run commands, keep sessions. Private keys stay in SSHCraft."
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum KeyTypeArg {
    Ed25519,
    Ecdsa,
    Rsa,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run as an MCP server on stdin and stdout (for Claude Code, Codex and others).
    Mcp,
    /// List hosts.
    Hosts,
    /// List groups.
    Groups,
    /// List keys (name, type, fingerprint).
    Keys,
    /// Show one host.
    Host { host: String },
    /// Run a command on a host: `sshcraft exec web -- uptime`.
    Exec {
        host: String,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Interactive sessions.
    #[command(subcommand)]
    Session(SessionCommand),
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
    ImportKey {
        name: String,
        file: std::path::PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Open a shell; prints the session id.
    Open {
        host: String,
    },
    /// Send text; `--enter` adds a line end.
    Write {
        session: String,
        data: String,
        #[arg(long)]
        enter: bool,
    },
    /// Print new output, waiting up to `--wait` milliseconds.
    Read {
        session: String,
        #[arg(long, default_value_t = 1000)]
        wait: u64,
    },
    Close {
        session: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeleteCommand {
    Host { host: String },
    Group { group: String },
    Key { key: String },
}

/// The action for a command; `Ok(None)` for `mcp`.
pub fn action(command: &Command) -> Result<Option<Action>, String> {
    Ok(Some(match command {
        Command::Mcp => return Ok(None),
        Command::Hosts => Action::ListHosts,
        Command::Groups => Action::ListGroups,
        Command::Keys => Action::ListKeys,
        Command::Host { host } => Action::HostInfo { host: host.clone() },
        Command::Exec {
            host,
            timeout,
            command,
        } => Action::Exec {
            host: host.clone(),
            command: command.join(" "),
            timeout_secs: *timeout,
        },
        Command::Session(SessionCommand::Open { host }) => {
            Action::SessionOpen { host: host.clone() }
        }
        Command::Session(SessionCommand::Write {
            session,
            data,
            enter,
        }) => Action::SessionWrite {
            session: session.clone(),
            data: if *enter {
                format!("{data}\n")
            } else {
                data.clone()
            },
        },
        Command::Session(SessionCommand::Read { session, wait }) => Action::SessionRead {
            session: session.clone(),
            wait_ms: Some(*wait),
        },
        Command::Session(SessionCommand::Close { session }) => Action::SessionClose {
            session: session.clone(),
        },
        Command::SaveHost { host } => Action::SaveHost {
            host: serde_json::from_str::<HostInput>(host).map_err(|e| format!("host JSON: {e}"))?,
        },
        Command::SaveGroup { group } => Action::SaveGroup {
            group: serde_json::from_str::<GroupInput>(group)
                .map_err(|e| format!("group JSON: {e}"))?,
        },
        Command::Delete(DeleteCommand::Host { host }) => Action::DeleteHost { host: host.clone() },
        Command::Delete(DeleteCommand::Group { group }) => Action::DeleteGroup {
            group: group.clone(),
        },
        Command::Delete(DeleteCommand::Key { key }) => Action::DeleteKey { key: key.clone() },
        Command::GenerateKey { name, key_type } => Action::GenerateKey {
            name: name.clone(),
            key_type: match key_type {
                KeyTypeArg::Ed25519 => KeyType::Ed25519,
                KeyTypeArg::Ecdsa => KeyType::Ecdsa,
                KeyTypeArg::Rsa => KeyType::Rsa,
            },
        },
        Command::ImportKey { name, file } => Action::ImportKey {
            name: name.clone(),
            text: std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?,
            passphrase: None,
        },
    }))
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
        return items
            .iter()
            .map(|item| {
                let field = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or("");
                let detail = ["address", "fingerprint", "parent"]
                    .iter()
                    .map(|f| field(f))
                    .find(|v| !v.is_empty())
                    .unwrap_or("");
                format!("{}\t{}\t{}", field("name"), detail, field("id"))
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
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("sshcraft").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn exec_takes_everything_after_the_dashes() {
        let cli = parse(&[
            "--agent", "script", "exec", "web", "--", "ls", "-la", "/tmp",
        ]);
        assert_eq!(cli.agent, "script");
        assert_eq!(
            action(&cli.command).unwrap(),
            Some(Action::Exec {
                host: "web".into(),
                command: "ls -la /tmp".into(),
                timeout_secs: None
            })
        );
    }

    #[test]
    fn session_write_can_add_the_line_end() {
        let cli = parse(&["session", "write", "abc", "uptime", "--enter"]);
        assert_eq!(
            action(&cli.command).unwrap(),
            Some(Action::SessionWrite {
                session: "abc".into(),
                data: "uptime\n".into()
            })
        );
    }

    #[test]
    fn hosts_from_json_and_bad_json() {
        let cli = parse(&["save-host", r#"{"name": "db", "address": "10.0.0.5"}"#]);
        match action(&cli.command).unwrap() {
            Some(Action::SaveHost { host }) => {
                assert_eq!(host.address.as_deref(), Some("10.0.0.5"))
            }
            other => panic!("{other:?}"),
        }
        let cli = parse(&["save-host", "{"]);
        assert!(action(&cli.command).is_err());
    }

    #[test]
    fn mcp_is_no_action_and_the_default_agent_is_cli() {
        let cli = parse(&["mcp"]);
        assert_eq!(cli.agent, "cli");
        assert_eq!(action(&cli.command).unwrap(), None);
    }

    #[test]
    fn lists_read_as_lines() {
        let hosts = serde_json::json!([{"id": "1", "name": "web", "address": "10.0.0.1"}]);
        assert_eq!(readable(&hosts), "web\t10.0.0.1\t1");
        assert_eq!(
            readable(&serde_json::json!({"output": "up\n", "truncated": false})),
            "up\n"
        );
    }
}
