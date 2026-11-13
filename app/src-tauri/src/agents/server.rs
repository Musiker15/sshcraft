//! The app's end of the local connection: one task per client, requests answered in order. A
//! client that goes away while a question is open takes the question with it.

use std::sync::Arc;

use sshcraft_agent::framing::{FrameError, read_message, write_message};
use sshcraft_agent::{AgentError, ErrorKind, Request, Response};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::mpsc;

use super::backend::{AgentBackend, Confirmer};
use super::gate::Gate;

/// Answers the requests of one client until it disconnects.
pub async fn serve_connection<S, B, C>(stream: S, gate: Arc<Gate<B, C>>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
    B: AgentBackend,
    C: Confirmer,
{
    let (read, mut write) = tokio::io::split(stream);
    // Reading runs on its own, so the end of the connection is noticed while a request waits
    // for an answer.
    let (requests, mut incoming) = mpsc::channel::<Result<Request, FrameError>>(8);
    let reader = tokio::spawn(async move {
        let mut read = BufReader::new(read);
        loop {
            match read_message::<Request, _>(&mut read).await {
                Ok(Some(request)) => {
                    if requests.send(Ok(request)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(err) => {
                    let fatal = !matches!(err, FrameError::Malformed(_));
                    let _ = requests.send(Err(err)).await;
                    if fatal {
                        return;
                    }
                }
            }
        }
    });
    while let Some(next) = incoming.recv().await {
        let response = match next {
            Ok(request) => {
                let handled = gate.handle(request);
                tokio::pin!(handled);
                // `incoming` closing means the client is gone: drop the request (and its
                // question) instead of running it later.
                tokio::select! {
                    response = &mut handled => response,
                    () = closed(&reader) => break,
                }
            }
            Err(err) => Response::error(0, AgentError::new(ErrorKind::Invalid, err.to_string())),
        };
        if write_message(&mut write, &response).await.is_err() {
            break;
        }
    }
    reader.abort();
}

/// Resolves once the reading task ended (the client closed the connection).
async fn closed(reader: &tokio::task::JoinHandle<()>) {
    while !reader.is_finished() {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Listens on this user's endpoint for the rest of the app's life.
pub fn start<B: AgentBackend, C: Confirmer>(gate: Arc<Gate<B, C>>) {
    tauri::async_runtime::spawn(async move {
        let Ok(mut listener) = sshcraft_agent::transport::listen() else {
            // Another SSHCraft of this user already listens, or the name is taken.
            return;
        };
        while let Ok(stream) = listener.accept().await {
            tauri::async_runtime::spawn(serve_connection(stream, gate.clone()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::backend::AgentPrompt;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Never answers; notes when its question is dropped.
    struct Silent {
        dropped: Arc<AtomicBool>,
    }

    struct Flag(Arc<AtomicBool>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl Confirmer for Silent {
        async fn confirm(&self, _: AgentPrompt) -> bool {
            let _flag = Flag(self.dropped.clone());
            std::future::pending().await
        }
    }

    use crate::agents::gate::tests_support::Fake;

    #[tokio::test]
    async fn requests_are_answered_in_order_and_a_gone_client_withdraws_its_question() {
        let dropped = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(Gate::new(
            Arc::new(Fake::with_web()),
            Arc::new(Silent {
                dropped: dropped.clone(),
            }),
        ));
        let (client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(serve_connection(server, gate));
        let (read, mut write) = tokio::io::split(client);
        let mut read = BufReader::new(read);
        for id in [1, 2] {
            write_message(
                &mut write,
                &Request {
                    id,
                    agent: "test".into(),
                    action: sshcraft_agent::Action::ListHosts,
                },
            )
            .await
            .unwrap();
        }
        for id in [1, 2] {
            let r: Response = read_message(&mut read).await.unwrap().unwrap();
            assert_eq!(r.id, id);
            assert!(r.ok.is_some());
        }
        // A request that asks; then the client goes away.
        write_message(
            &mut write,
            &Request {
                id: 3,
                agent: "test".into(),
                action: sshcraft_agent::Action::SessionOpen { host: "web".into() },
            },
        )
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(write);
        drop(read);
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn garbage_gets_an_error_and_the_connection_stays() {
        let gate = Arc::new(Gate::new(
            Arc::new(Fake::with_web()),
            Arc::new(Silent {
                dropped: Arc::default(),
            }),
        ));
        let (client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(serve_connection(server, gate));
        let (read, mut write) = tokio::io::split(client);
        let mut read = BufReader::new(read);
        use tokio::io::AsyncWriteExt;
        write.write_all(b"{not json}\n").await.unwrap();
        let r: Response = read_message(&mut read).await.unwrap().unwrap();
        assert_eq!(r.error.unwrap().kind, ErrorKind::Invalid);
        write_message(
            &mut write,
            &Request {
                id: 9,
                agent: "test".into(),
                action: sshcraft_agent::Action::ListGroups,
            },
        )
        .await
        .unwrap();
        let r: Response = read_message(&mut read).await.unwrap().unwrap();
        assert_eq!(r.id, 9);
    }
}
