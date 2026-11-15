#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! `sshcraft-cli`: the command line and MCP server for AI agents. It holds no keys, opens no SSH
//! connection and never opens SSHCraft's data: everything goes to the running app, which asks
//! the user where its settings say so.

mod cli;
mod launch;

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use serde_json::{Value, json};
use sshcraft_link::{Client, Token};
use sshcraft_mcp::{Backend, BackendError, Remote, valid_agent_name};

use crate::cli::{Command, UiCommand, WhenClosed};

/// The app is not running (or does not let agents connect).
const EXIT_NOT_RUNNING: u8 = 3;
const EXIT_USAGE: u8 = 2;

fn print(value: &Value, as_json: bool) {
    let text = if as_json { serde_json::to_string_pretty(value).unwrap_or_default() } else { cli::readable(value) };
    if !text.is_empty() {
        println!("{}", text.trim_end_matches('\n'));
    }
}

fn failed(error: &BackendError) -> ExitCode {
    eprintln!("{error}");
    if *error == BackendError::NotRunning { ExitCode::from(EXIT_NOT_RUNNING) } else { ExitCode::FAILURE }
}

fn token_of(path: &Path) -> Result<Token, String> {
    Token::load(path).ok_or_else(|| format!("{}: no token there", path.display()))
}

/// `ui …` against a `--control` app.
async fn ui(port: u16, token_file: &Path, action: &UiCommand, as_json: bool) -> ExitCode {
    let token = match token_of(token_file) {
        Ok(token) => token,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let client = match Client::connect(SocketAddr::from(([127, 0, 0, 1], port)), &token).await {
        Ok(client) => client,
        Err(e) => return failed(&e.into()),
    };
    let (method, params) = match action {
        UiCommand::Inspect => ("ui.inspect", json!({})),
        UiCommand::Click { x, y } => ("ui.click", json!({ "x": x, "y": y })),
        UiCommand::Key { key, ctrl, shift, alt } => ("ui.key", json!({ "key": key, "ctrl": ctrl, "shift": shift, "alt": alt })),
        UiCommand::Text { text } => ("ui.text", json!({ "text": text })),
        UiCommand::Screenshot { .. } => ("ui.screenshot", json!({})),
    };
    match client.call(method, params).await {
        Ok(value) => {
            if let UiCommand::Screenshot { file } = action {
                let png = value.get("png_base64").and_then(Value::as_str).and_then(|b| data_encoding::BASE64.decode(b.as_bytes()).ok());
                let Some(png) = png else {
                    eprintln!("the app sent no picture");
                    return ExitCode::FAILURE;
                };
                if let Err(e) = std::fs::write(file, png) {
                    eprintln!("{}: {e}", file.display());
                    return ExitCode::FAILURE;
                }
                return ExitCode::SUCCESS;
            }
            print(&value, as_json);
            ExitCode::SUCCESS
        }
        Err(e) => failed(&e.into()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = cli::Cli::parse();
    if !valid_agent_name(&args.agent) {
        eprintln!("--agent: 1 to 64 printable characters");
        return ExitCode::from(EXIT_USAGE);
    }
    if let Command::Ui { bridge, control_token_file, action } = &args.command {
        return ui(*bridge, control_token_file, action, args.json).await;
    }
    let call = match cli::engine_call(&args.command) {
        Ok(call) => call,
        Err(problem) => {
            eprintln!("{problem}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let link_dir = match sshcraft_platform::config_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Command::Mcp { when_closed, bridge, control_token_file } = &args.command {
        let remote = match (bridge, control_token_file) {
            (Some(port), Some(file)) => match token_of(file) {
                Ok(token) => Remote::bridge(SocketAddr::from(([127, 0, 0, 1], *port)), token),
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(EXIT_USAGE);
                }
            },
            (Some(_), None) => {
                eprintln!("--bridge needs --control-token-file");
                return ExitCode::from(EXIT_USAGE);
            }
            _ => Remote::window(link_dir, "mcp"),
        };
        let backend: Arc<dyn Backend> = match when_closed {
            WhenClosed::Launch if bridge.is_none() => Arc::new(launch::Launching::new(remote, launch::app_path(), launch::LAUNCH_WAIT)),
            _ => Arc::new(remote),
        };
        // `--agent` given explicitly names the agent; else the MCP client's own name does.
        let agent = (args.agent != "cli").then(|| args.agent.clone());
        let input = tokio::io::BufReader::new(tokio::io::stdin());
        return match sshcraft_mcp::serve(backend, agent, input, tokio::io::stdout()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let remote = Remote::window(link_dir, "cli");
    let result = match call {
        Some(call) => remote.execute(&args.agent, &call.command, call.params).await,
        None => remote.commands(&args.agent).await,
    };
    match result {
        Ok(value) => {
            print(&value, args.json);
            ExitCode::SUCCESS
        }
        Err(error) => failed(&error),
    }
}
