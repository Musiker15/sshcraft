//! The MCP server against a real engine and its agent link: the protocol, the tools, calls
//! side by side, cancelling, and what ends when the agent's program goes away.

// Tests report by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cancel;
mod protocol;
mod tools;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session, SessionConfig};
use sshcraft_mcp::{Backend, Remote};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

pub struct World {
    pub session: Session,
    pub link: tempfile::TempDir,
    _data: tempfile::TempDir,
    _server: TestServer,
}

pub async fn world() -> World {
    let data = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(data.path())).unwrap();
    let (server, _host) = test_host(&session, "web", TestAuth::default(), true).await;
    let link = tempfile::tempdir().unwrap();
    session.start_agent_link(link.path(), 0).await.unwrap();
    World { session, link, _data: data, _server: server }
}

impl World {
    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::new(Remote::window(self.link.path().to_path_buf(), "mcp"))
    }

    pub async fn agent_log(&self) -> Vec<Value> {
        self.session.execute(Caller::User, "agent.log", json!({})).await.unwrap().as_array().cloned().unwrap_or_default()
    }

    /// The questions open in the window.
    pub async fn questions(&self) -> usize {
        self.session.execute(Caller::User, "prompt.list", json!({})).await.unwrap().as_array().map_or(0, Vec::len)
    }

    /// Waits until `n` questions are open.
    pub async fn until_questions(&self, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.questions().await != n {
            assert!(Instant::now() < deadline, "timed out waiting for {n} questions");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Sets the agents' right to connect: `allow`, `ask` or `deny`.
    pub async fn set_connect(&self, level: &str) {
        self.session.execute(Caller::User, "agent.set_level", json!({ "category": "connect", "level": level })).await.unwrap();
    }
}

/// An MCP client talking to `serve` over an in-memory pipe.
pub struct Mcp {
    write: WriteHalf<DuplexStream>,
    read: BufReader<ReadHalf<DuplexStream>>,
    pub task: tokio::task::JoinHandle<()>,
}

pub fn start(backend: Arc<dyn Backend>, agent: Option<&str>) -> Mcp {
    let (client, server) = tokio::io::duplex(4 << 20);
    let (server_read, server_write) = tokio::io::split(server);
    let agent = agent.map(str::to_string);
    let task = tokio::spawn(async move {
        sshcraft_mcp::serve(backend, agent, BufReader::new(server_read), server_write).await.unwrap();
    });
    let (read, write) = tokio::io::split(client);
    Mcp { write, read: BufReader::new(read), task }
}

impl Mcp {
    pub async fn send(&mut self, message: Value) {
        self.write.write_all(format!("{message}\n").as_bytes()).await.unwrap();
    }

    pub async fn send_raw(&mut self, bytes: &[u8]) {
        self.write.write_all(bytes).await.unwrap();
    }

    pub async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(15), self.read.read_line(&mut line)).await.expect("an answer within 15 s").unwrap();
        serde_json::from_str(&line).unwrap()
    }

    /// Sends request `id` and waits for its answer (answers to other requests are skipped).
    pub async fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await;
        loop {
            let answer = self.recv().await;
            if answer["id"] == id {
                return answer;
            }
        }
    }

    pub async fn initialize(&mut self, client_name: &str) -> Value {
        self.request(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": client_name, "version": "1"}, "capabilities": {}}),
        )
        .await
    }

    pub async fn call(&mut self, id: u64, tool: &str, arguments: Value) -> Value {
        self.request(id, "tools/call", json!({"name": tool, "arguments": arguments})).await
    }

    /// Closes stdin, as an ending agent program does.
    pub async fn close(self) {
        let Self { write, read, task } = self;
        // Both halves: the pipe ends only when the whole client end is gone.
        drop(write);
        drop(read);
        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
    }
}

pub fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}
