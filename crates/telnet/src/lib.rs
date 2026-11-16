//! Telnet for devices that speak nothing else (switches, old routers, lab gear). Telnet is not
//! encrypted: SSHCraft never sends stored passwords or keys over it, the user types what the
//! device asks for.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod negotiate;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sshcraft_model::policy::{Disconnect, ShellEvent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;

use negotiate::Negotiator;

/// The port Telnet uses unless the host says otherwise.
pub const DEFAULT_PORT: u16 = 23;
/// Events buffered between the connection and the terminal.
const EVENT_BUFFER: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum TelnetError {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("timed out")]
    Timeout,
    #[error("{0}")]
    Io(String),
}

/// The sending half: typing and size changes.
#[derive(Clone)]
pub struct TelnetWriter {
    stream: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    negotiator: Arc<Mutex<Negotiator>>,
}

impl TelnetWriter {
    pub async fn write(&self, data: &[u8]) -> Result<(), TelnetError> {
        let bytes = self.negotiator.lock().unwrap_or_else(PoisonError::into_inner).write(data);
        self.stream.lock().await.write_all(&bytes).await.map_err(|e| TelnetError::Io(e.to_string()))
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), TelnetError> {
        let told = self.negotiator.lock().unwrap_or_else(PoisonError::into_inner).resize(cols, rows);
        match told {
            Some(bytes) => self.stream.lock().await.write_all(&bytes).await.map_err(|e| TelnetError::Io(e.to_string())),
            None => Ok(()),
        }
    }

    /// Ends the connection.
    pub async fn close(&self) {
        let _ = self.stream.lock().await.shutdown().await;
    }
}

/// Connects to `address:port` and negotiates as a terminal of type `term` and size `size`.
pub async fn connect(
    address: &str,
    port: u16,
    term: &str,
    size: (u16, u16),
    timeout: Duration,
) -> Result<(TelnetWriter, mpsc::Receiver<ShellEvent>), TelnetError> {
    let address = address.trim_start_matches('[').trim_end_matches(']');
    let stream = tokio::time::timeout(timeout, TcpStream::connect((address, port)))
        .await
        .map_err(|_| TelnetError::Timeout)?
        .map_err(|e| TelnetError::Connect(e.to_string()))?;
    let _ = stream.set_nodelay(true);
    let (mut read, write) = stream.into_split();
    let negotiator = Arc::new(Mutex::new(Negotiator::new(term, size)));
    let writer = TelnetWriter { stream: Arc::new(tokio::sync::Mutex::new(write)), negotiator: negotiator.clone() };
    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    let answers = writer.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 16 * 1024];
        let reason = loop {
            match read.read(&mut buf).await {
                Ok(0) => break Disconnect::Exited(None),
                Ok(n) => {
                    let got = negotiator.lock().unwrap_or_else(PoisonError::into_inner).read(buf.get(..n).unwrap_or_default());
                    if !got.reply.is_empty() && answers.stream.lock().await.write_all(&got.reply).await.is_err() {
                        break Disconnect::Lost("the device stopped answering".into());
                    }
                    if !got.data.is_empty() && tx.send(ShellEvent::Data(got.data)).await.is_err() {
                        return;
                    }
                }
                Err(e) => break Disconnect::Lost(e.to_string()),
            }
        };
        let _ = tx.send(ShellEvent::Ended(reason)).await;
    });
    Ok((writer, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_session_negotiates_and_carries_text_both_ways() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.write_all(&[255, 253, 31]).await.unwrap(); // DO NAWS
            socket.write_all(b"login: ").await.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 64];
            while !got.ends_with(b"root\r\0") {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "closed early: {got:?}");
                got.extend_from_slice(&buf[..n]);
            }
            got
        });
        let (writer, mut events) = connect("127.0.0.1", port, "xterm", (80, 24), Duration::from_secs(5)).await.unwrap();
        match events.recv().await {
            Some(ShellEvent::Data(d)) => assert_eq!(d, b"login: ".to_vec()),
            other => panic!("{other:?}"),
        }
        writer.write(b"root\r").await.unwrap();
        let got = server.await.unwrap();
        // WILL NAWS and the size came first, then the typed line.
        assert!(got.starts_with(&[255, 251, 31, 255, 250, 31, 0, 80, 0, 24, 255, 240]), "{got:?}");
    }
}
