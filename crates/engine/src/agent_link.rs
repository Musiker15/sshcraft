//! The agent link: while the user lets agents connect, the engine serves them on loopback through
//! `sshcraft-link` and publishes the port in `agent-connection.json`. A connection introduces
//! itself (`hello`), then runs engine commands, each as that agent through the gate. It never
//! reaches the window: no `ui.*`, no screenshots, no journal. When the connection ends, its
//! running requests (and their questions) end with it, and so do the sessions it opened.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use sshcraft_link::{ConnId, ConnectionFile, Handler, Server, TOKEN_FILE, Token};

use crate::caller::Via;
use crate::error::EngineError;
use crate::gate::AgentConnection;
use crate::{Inner, Session};

/// The link while it runs.
pub(crate) struct Running {
    server: Server,
    dir: PathBuf,
    handler: Arc<LinkHandler>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // Stopped or the engine closing: the connections end and the file goes.
        self.server.stop();
        self.handler.agents.lock().unwrap_or_else(PoisonError::into_inner).clear();
        let _ = ConnectionFile::remove(&self.dir);
    }
}

/// Where agents connect, as the window shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct AgentLinkStatus {
    /// The port agents connect on, while the link runs.
    pub port: Option<u16>,
    /// The folder of the token and connection files.
    pub dir: Option<PathBuf>,
    /// Signed-in connections.
    pub connections: usize,
}

struct LinkHandler {
    engine: Weak<Inner>,
    /// The agent each connection introduced itself as.
    agents: Mutex<HashMap<ConnId, Arc<AgentConnection>>>,
}

fn via_of(text: &str) -> Option<Via> {
    match text {
        "mcp" => Some(Via::Mcp),
        "cli" => Some(Via::Cli),
        "launcher" => Some(Via::Launcher),
        _ => None,
    }
}

/// A method name as it may appear in an error: short and printable.
fn shown(method: &str) -> String {
    method.chars().filter(|c| !c.is_control()).take(64).collect()
}

impl LinkHandler {
    fn session(&self) -> Option<Session> {
        self.engine.upgrade().map(Session)
    }

    fn agent(&self, conn: ConnId) -> Option<Arc<AgentConnection>> {
        self.agents.lock().unwrap_or_else(PoisonError::into_inner).get(&conn).cloned()
    }

    fn hello(&self, conn: ConnId, params: &Value) -> Result<Value, String> {
        let session = self.session().ok_or("SSHCraft is closing")?;
        if self.agent(conn).is_some() {
            return Err("this connection already said hello".into());
        }
        let name = params.get("agent").and_then(Value::as_str).unwrap_or_default();
        let via = params.get("via").and_then(Value::as_str).and_then(via_of).ok_or("hello: via is mcp, cli or launcher")?;
        let agent = session.agent_connection(name, via).map_err(|e| e.to_string())?;
        self.agents.lock().unwrap_or_else(PoisonError::into_inner).insert(conn, Arc::new(agent));
        Ok(json!({ "app": env!("CARGO_PKG_VERSION") }))
    }

    fn commands(&self) -> Result<Value, String> {
        let session = self.session().ok_or("SSHCraft is closing")?;
        let list: Vec<Value> = session
            .commands()
            .into_iter()
            .filter(|c| c.agents)
            .map(|c| json!({ "id": c.id, "label": c.label, "params": c.params, "enabled": c.enabled }))
            .collect();
        Ok(Value::Array(list))
    }
}

impl Handler for LinkHandler {
    fn call(&self, conn: ConnId, method: String, params: Value) -> BoxFuture<'static, Result<Value, String>> {
        let ready = match method.as_str() {
            "ping" => Ok(json!("pong")),
            "hello" => self.hello(conn, &params),
            "engine.commands" | "engine.execute" if self.agent(conn).is_none() => Err("say hello first".to_string()),
            "engine.commands" => self.commands(),
            "engine.execute" => {
                let (Some(session), Some(agent)) = (self.session(), self.agent(conn)) else {
                    return futures::future::ready(Err("SSHCraft is closing".to_string())).boxed();
                };
                let command = params.get("command").and_then(Value::as_str).unwrap_or_default().to_string();
                let args = match params.get("params") {
                    None | Some(Value::Null) => json!({}),
                    Some(v) => v.clone(),
                };
                return async move {
                    // The connection lives while its calls run.
                    let caller = agent.caller();
                    session.execute(caller, &command, args).await.map_err(|e: EngineError| e.to_string())
                }
                .boxed();
            }
            other => Err(format!("unknown method {}", shown(other))),
        };
        futures::future::ready(ready).boxed()
    }

    fn closed(&self, conn: ConnId) {
        // Dropping the connection ends the sessions it opened.
        let gone = self.agents.lock().unwrap_or_else(PoisonError::into_inner).remove(&conn);
        drop(gone);
    }
}

impl Session {
    /// Starts serving agents on `127.0.0.1:port` (0: any free port), with a fresh token in
    /// `dir/agent-control.token` and the port in `dir/agent-connection.json`.
    /// Running already, it stays as it is.
    pub async fn start_agent_link(&self, dir: &Path, port: u16) -> Result<AgentLinkStatus, EngineError> {
        if self.agent_link_status().port.is_some() {
            return Ok(self.agent_link_status());
        }
        let token_file = dir.join(TOKEN_FILE);
        // A token for this start only (AGENTS.md): one read earlier is no good now.
        let token = Token::create_fresh(&token_file).map_err(|e| EngineError::Settings(e.to_string()))?;
        let handler = Arc::new(LinkHandler { engine: Arc::downgrade(&self.0), agents: Mutex::new(HashMap::new()) });
        let server = Server::bind(port, token, handler.clone()).await.map_err(|e| EngineError::Settings(e.to_string()))?;
        let file = ConnectionFile::new(server.port(), token_file, env!("CARGO_PKG_VERSION").to_string());
        file.write(dir).map_err(|e| EngineError::Settings(e.to_string()))?;
        *self.0.agent_link.lock().unwrap_or_else(PoisonError::into_inner) = Some(Running { server, dir: dir.to_path_buf(), handler });
        self.0.events.send(crate::EngineEvent::AgentLinkChanged);
        Ok(self.agent_link_status())
    }

    /// Stops serving agents: every connection ends (with its requests and sessions) and the
    /// connection file goes.
    pub fn stop_agent_link(&self) {
        let running = self.0.agent_link.lock().unwrap_or_else(PoisonError::into_inner).take();
        if running.is_some() {
            drop(running);
            self.0.events.send(crate::EngineEvent::AgentLinkChanged);
        }
    }

    pub fn agent_link_status(&self) -> AgentLinkStatus {
        let guard = self.0.agent_link.lock().unwrap_or_else(PoisonError::into_inner);
        match guard.as_ref() {
            Some(running) => AgentLinkStatus {
                port: Some(running.server.port()),
                dir: Some(running.dir.clone()),
                connections: running.handler.agents.lock().unwrap_or_else(PoisonError::into_inner).len(),
            },
            None => AgentLinkStatus::default(),
        }
    }
}

#[cfg(test)]
#[path = "agent_link_tests.rs"]
mod tests;
