//! The app's end of the local connection. Requests of one client run side by side (a question
//! waiting for the user holds up nothing else), a `cancel` drops one (and its question), and when
//! the client goes away its requests are dropped and its sessions closed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sshcraft_agent::framing::{FrameError, MAX_LINE, read_message, write_message};
use sshcraft_agent::{Action, AgentError, ErrorKind, Request, Response};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::backend::{AgentBackend, Confirmer};
use super::gate::{Connection, Gate};

/// An answer as it goes out: one too big for a line becomes an error instead of breaking the
/// connection.
fn fit(response: Response) -> Response {
    let size = serde_json::to_vec(&response).map_or(usize::MAX, |v| v.len());
    if size > MAX_LINE {
        Response::error(
            response.id,
            AgentError::new(ErrorKind::Failed, "the answer is too large to send"),
        )
    } else {
        response
    }
}

/// Answers the requests of one client until it disconnects.
pub async fn serve_connection<S, B, C>(stream: S, gate: Arc<Gate<B, C>>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
    B: AgentBackend,
    C: Confirmer,
{
    let (read, mut write) = tokio::io::split(stream);
    let (answers, mut outgoing) = mpsc::unbounded_channel::<Response>();
    let writer = tokio::spawn(async move {
        while let Some(response) = outgoing.recv().await {
            if write_message(&mut write, &fit(response)).await.is_err() {
                return;
            }
        }
    });
    let conn = Arc::new(Connection::default());
    let mut running: HashMap<u64, JoinHandle<()>> = HashMap::new();
    let mut read = BufReader::new(read);
    loop {
        running.retain(|_, task| !task.is_finished());
        match read_message::<Request, _>(&mut read).await {
            Ok(Some(Request {
                action: Action::Cancel { request },
                ..
            })) => {
                // Dropping the request withdraws its question.
                if let Some(task) = running.remove(&request) {
                    task.abort();
                }
            }
            Ok(Some(request)) => {
                let id = request.id;
                let gate = gate.clone();
                let conn = conn.clone();
                let answers = answers.clone();
                let task = tokio::spawn(async move {
                    let response = gate.handle(request, &conn).await;
                    let _ = answers.send(response);
                });
                if let Some(old) = running.insert(id, task) {
                    old.abort();
                }
            }
            Ok(None) => break,
            Err(FrameError::Malformed(problem)) => {
                let _ = answers.send(Response::error(
                    0,
                    AgentError::new(ErrorKind::Invalid, problem),
                ));
            }
            Err(_) => break,
        }
    }
    for (_, task) in running {
        task.abort();
    }
    gate.connection_closed(&conn);
    drop(answers);
    let _ = writer.await;
}

/// Listens on this user's endpoint for the rest of the app's life. A broken accept does not end
/// it, and a taken name is tried again now and then.
pub fn start<B: AgentBackend, C: Confirmer>(gate: Arc<Gate<B, C>>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let mut listener = match sshcraft_agent::transport::listen() {
                Ok(listener) => listener,
                Err(e) => {
                    eprintln!("agent connection not available: {e}");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
            };
            loop {
                match listener.accept().await {
                    Ok(stream) => {
                        tauri::async_runtime::spawn(serve_connection(stream, gate.clone()));
                    }
                    Err(e) => {
                        eprintln!("agent connection: {e}");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        // A pipe instance that failed is replaced by listening anew.
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::backend::AgentPrompt;
    use crate::agents::gate::tests_support::Fake;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncWriteExt, ReadHalf, WriteHalf};

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

    type Client = (
        BufReader<ReadHalf<tokio::io::DuplexStream>>,
        WriteHalf<tokio::io::DuplexStream>,
    );

    fn connect(fake: Arc<Fake>, dropped: Arc<AtomicBool>) -> (Client, JoinHandle<()>) {
        let gate = Arc::new(Gate::new(fake, Arc::new(Silent { dropped })));
        let (client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(serve_connection(server, gate));
        let (read, write) = tokio::io::split(client);
        ((BufReader::new(read), write), task)
    }

    async fn send(client: &mut Client, id: u64, action: Action) {
        write_message(
            &mut client.1,
            &Request {
                id,
                agent: "test".into(),
                action,
            },
        )
        .await
        .unwrap();
    }

    async fn answer(client: &mut Client) -> Response {
        tokio::time::timeout(Duration::from_secs(5), read_message(&mut client.0))
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn a_waiting_question_holds_up_nothing_and_cancel_withdraws_it() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (mut client, _task) = connect(Arc::new(Fake::with_web()), dropped.clone());
        send(&mut client, 1, Action::SessionOpen { host: "web".into() }).await;
        send(&mut client, 2, Action::ListHosts).await;
        let r = answer(&mut client).await;
        assert_eq!(r.id, 2);
        assert!(!dropped.load(Ordering::SeqCst));
        send(&mut client, 3, Action::Cancel { request: 1 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_gone_client_withdraws_its_question_and_loses_its_sessions() {
        let dropped = Arc::new(AtomicBool::new(false));
        let fake = Arc::new(Fake::with_web());
        fake.allow_connect();
        let (mut client, task) = connect(fake.clone(), dropped.clone());
        send(&mut client, 1, Action::SessionOpen { host: "web".into() }).await;
        let opened = answer(&mut client).await;
        let session = opened.ok.unwrap()["session"].as_str().unwrap().to_owned();
        fake.deny_connect();
        send(&mut client, 2, Action::ListHosts).await;
        answer(&mut client).await;
        // Ask for something that waits for the user, then go away.
        fake.ask_connect();
        send(&mut client, 3, Action::SessionOpen { host: "web".into() }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(dropped.load(Ordering::SeqCst));
        assert!(fake.closed().contains(&session));
    }

    #[tokio::test]
    async fn garbage_gets_an_error_and_the_connection_stays() {
        let (mut client, _task) = connect(Arc::new(Fake::with_web()), Arc::default());
        client.1.write_all(b"{not json}\n").await.unwrap();
        let r = answer(&mut client).await;
        assert_eq!(r.error.unwrap().kind, ErrorKind::Invalid);
        send(&mut client, 9, Action::ListGroups).await;
        assert_eq!(answer(&mut client).await.id, 9);
    }

    #[test]
    fn an_answer_too_big_for_a_line_becomes_an_error() {
        let big = Response::ok(5, serde_json::json!("\u{0}".repeat(MAX_LINE / 4)));
        let sent = fit(big);
        assert_eq!(sent.id, 5);
        assert_eq!(sent.error.unwrap().kind, ErrorKind::Failed);
        let small = fit(Response::ok(6, serde_json::json!("ok")));
        assert!(small.ok.is_some());
    }
}
