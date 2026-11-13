//! `sshcraft`: the command line and MCP server for AI agents. It holds no keys and opens no SSH
//! connection itself; everything goes to the running SSHCraft app, which asks the user where
//! its settings say so.

mod cli;
mod client;
mod mcp;

use std::process::ExitCode;

use clap::Parser;
use sshcraft_agent::ErrorKind;

use crate::client::{AppClient, Caller};

/// The app is not running (or not this user's).
const EXIT_NOT_RUNNING: u8 = 3;

#[tokio::main]
async fn main() -> ExitCode {
    let args = cli::Cli::parse();
    if !sshcraft_agent::valid_agent_name(&args.agent) {
        eprintln!("--agent: 1 to 64 printable characters");
        return ExitCode::from(2);
    }
    let action = match cli::action(&args.command) {
        Ok(action) => action,
        Err(problem) => {
            eprintln!("{problem}");
            return ExitCode::from(2);
        }
    };
    let Some(action) = action else {
        let input = tokio::io::BufReader::new(tokio::io::stdin());
        // `--agent` given explicitly names the agent; else the MCP client's own name does.
        let agent = (args.agent != "cli").then(|| args.agent.clone());
        return match mcp::serve(AppClient::default(), agent, input, tokio::io::stdout()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    };
    match AppClient::default().call(&args.agent, action).await {
        Ok(value) => {
            let text = if args.json {
                serde_json::to_string_pretty(&value).unwrap_or_default()
            } else {
                cli::readable(&value)
            };
            if !text.is_empty() {
                println!("{}", text.trim_end_matches('\n'));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.message);
            if error.kind == ErrorKind::NotRunning {
                ExitCode::from(EXIT_NOT_RUNNING)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
