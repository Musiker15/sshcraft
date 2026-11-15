//! `--control <port>`: the control channel, a developer tool, never on by default. Whoever holds
//! its token clicks and types like the user (docs/control-protocol.md); engine commands through
//! it still go through the agent gate as the control channel.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use sshcraft_engine::{AgentConnection, Session};
use sshcraft_link::{ConnId, Handler, Server, Token};
use sshcraft_ui_egui::control::{ControlQueue, ControlRequest};

/// How long a request waits for the window.
const WINDOW_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlArgs {
    pub port: u16,
    /// The token's file; without one, a fresh token for this start in the settings folder.
    pub token_file: Option<PathBuf>,
}

/// The control options among the command line `args` (without the program name).
pub fn parse(args: &[String]) -> Result<Option<ControlArgs>, String> {
    let mut port = None;
    let mut token_file = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--control" => {
                let value = rest.next().ok_or("--control needs a port")?;
                port = Some(value.parse::<u16>().ok().filter(|p| *p != 0).ok_or("--control: a port from 1 to 65535")?);
            }
            "--control-token-file" => token_file = Some(PathBuf::from(rest.next().ok_or("--control-token-file needs a path")?)),
            other => return Err(format!("unknown option {}", other.chars().filter(|c| !c.is_control()).take(64).collect::<String>())),
        }
    }
    match (port, token_file) {
        (Some(port), token_file) => Ok(Some(ControlArgs { port, token_file })),
        (None, Some(_)) => Err("--control-token-file goes with --control".into()),
        (None, None) => Ok(None),
    }
}

/// Engine requests go to the engine as the control channel; the rest waits for the window.
pub struct ControlHandler {
    session: Session,
    queue: ControlQueue,
    conns: Mutex<HashMap<ConnId, Arc<AgentConnection>>>,
}

impl ControlHandler {
    pub fn new(session: Session, queue: ControlQueue) -> Self {
        Self { session, queue, conns: Mutex::new(HashMap::new()) }
    }

    fn connection(&self, conn: ConnId) -> Arc<AgentConnection> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner).entry(conn).or_insert_with(|| Arc::new(self.session.control_connection())).clone()
    }
}

impl Handler for ControlHandler {
    fn call(&self, conn: ConnId, method: String, params: Value) -> BoxFuture<'static, Result<Value, String>> {
        match method.as_str() {
            "ping" => futures::future::ready(Ok(json!("pong"))).boxed(),
            "engine.execute" => {
                let session = self.session.clone();
                let agent = self.connection(conn);
                let command = params.get("command").and_then(Value::as_str).unwrap_or_default().to_string();
                let args = match params.get("params") {
                    None | Some(Value::Null) => json!({}),
                    Some(v) => v.clone(),
                };
                async move { session.execute(agent.caller(), &command, args).await.map_err(|e| e.to_string()) }.boxed()
            }
            "engine.commands" => {
                let list: Vec<Value> = self
                    .session
                    .commands()
                    .into_iter()
                    .map(|c| json!({ "id": c.id, "label": c.label, "params": c.params, "enabled": c.enabled, "agents": c.agents }))
                    .collect();
                futures::future::ready(Ok(Value::Array(list))).boxed()
            }
            "engine.journal" => futures::future::ready(serde_json::to_value(self.session.journal()).map_err(|e| e.to_string())).boxed(),
            _ => {
                let (reply, answer) = tokio::sync::oneshot::channel();
                self.queue.push(ControlRequest { method, params, reply });
                async move {
                    match tokio::time::timeout(WINDOW_WAIT, answer).await {
                        Ok(Ok(result)) => result,
                        Ok(Err(_)) => Err("the window closed".into()),
                        Err(_) => Err("timeout: the window did not answer in 60 s".into()),
                    }
                }
                .boxed()
            }
        }
    }

    fn closed(&self, conn: ConnId) {
        let gone = self.conns.lock().unwrap_or_else(PoisonError::into_inner).remove(&conn);
        drop(gone);
    }
}

/// Starts the control channel on `127.0.0.1:args.port`.
pub async fn start(args: &ControlArgs, session: Session, queue: ControlQueue) -> Result<Server, String> {
    let token = match &args.token_file {
        Some(path) => Token::load_or_create(path),
        None => {
            let dir = sshcraft_platform::config_dir().map_err(|e| e.to_string())?;
            Token::create_fresh(&dir.join("control.token"))
        }
    }
    .map_err(|e| e.to_string())?;
    Server::bind(args.port, token, Arc::new(ControlHandler::new(session, queue))).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn control_is_off_without_the_flag() {
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(parse(&args(&["--control", "7878"])).unwrap(), Some(ControlArgs { port: 7878, token_file: None }));
        let with_file = parse(&args(&["--control", "7878", "--control-token-file", "t"])).unwrap();
        assert_eq!(with_file.and_then(|a| a.token_file), Some(PathBuf::from("t")));
        for bad in [&["--control"][..], &["--control", "x"], &["--control", "0"], &["--control-token-file", "t"], &["--other"]] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn engine_calls_run_as_control() {
        let dir = tempfile::tempdir().unwrap();
        let (session, _) = Session::open(sshcraft_engine::SessionConfig::for_tests(dir.path())).unwrap();
        let token = Token::generate();
        let queue = ControlQueue::new(egui::Context::default());
        let server = Server::bind(0, token.clone(), Arc::new(ControlHandler::new(session.clone(), queue))).await.unwrap();
        let client = sshcraft_link::Client::connect(std::net::SocketAddr::from(([127, 0, 0, 1], server.port())), &token).await.unwrap();
        let r = client.call("engine.execute", json!({ "command": "host.delete", "params": { "host": "nowhere" } })).await;
        assert!(r.is_err());
        let log = session.execute(sshcraft_engine::Caller::User, "agent.log", json!({})).await.unwrap();
        assert_eq!(log[0]["agent"], "control", "the control channel goes through the gate as itself: {log}");
        assert!(session.journal().iter().all(|e| e.caller != "user"), "never as the user");
    }
}
