//! Requests to the running app over the local connection.

use serde_json::Value;
use sshcraft_agent::framing::{read_message, write_message};
use sshcraft_agent::transport::ClientStream;
use sshcraft_agent::{Action, AgentError, ErrorKind, Request, Response};
use tokio::io::{BufReader, ReadHalf, WriteHalf};

/// Sends actions somewhere and gets their results (the app, or a fake in tests).
#[async_trait::async_trait]
pub trait Caller: Send {
    async fn call(&mut self, agent: &str, action: Action) -> Result<Value, AgentError>;
}

pub fn not_running() -> AgentError {
    AgentError::new(
        ErrorKind::NotRunning,
        "SSHCraft is not running. Start the SSHCraft app and try again.",
    )
}

/// A connection to the app, opened on first use and opened again after it broke.
#[derive(Default)]
pub struct AppClient {
    conn: Option<(BufReader<ReadHalf<ClientStream>>, WriteHalf<ClientStream>)>,
    next_id: u64,
}

impl AppClient {
    async fn connection(
        &mut self,
    ) -> Result<&mut (BufReader<ReadHalf<ClientStream>>, WriteHalf<ClientStream>), AgentError> {
        if self.conn.is_none() {
            let stream = sshcraft_agent::transport::connect().await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    AgentError::new(ErrorKind::Denied, e.to_string())
                } else {
                    not_running()
                }
            })?;
            let (read, write) = tokio::io::split(stream);
            self.conn = Some((BufReader::new(read), write));
        }
        Ok(self.conn.as_mut().expect("just set"))
    }
}

#[async_trait::async_trait]
impl Caller for AppClient {
    async fn call(&mut self, agent: &str, action: Action) -> Result<Value, AgentError> {
        self.next_id += 1;
        let request = Request {
            id: self.next_id,
            agent: agent.to_owned(),
            action,
        };
        let result = async {
            let (read, write) = self.connection().await?;
            write_message(write, &request)
                .await
                .map_err(|_| not_running())?;
            read_message::<Response, _>(read)
                .await
                .map_err(|_| not_running())?
                .ok_or_else(not_running)
        }
        .await;
        let response = match result {
            Ok(response) => response,
            Err(err) => {
                self.conn = None;
                return Err(err);
            }
        };
        match (response.ok, response.error) {
            (_, Some(error)) => Err(error),
            (Some(value), None) => Ok(value),
            (None, None) => Ok(Value::Null),
        }
    }
}
