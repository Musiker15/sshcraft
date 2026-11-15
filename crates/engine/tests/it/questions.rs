//! The host key and sign-in questions of a session name it, so the window can show which tab
//! asks.

use std::time::Duration;

use serde_json::json;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, EngineEvent, Session, SessionConfig, SessionId};
use sshcraft_ssh::testing::TestAuth;

/// The next question event, within 10 seconds.
async fn next_question(events: &mut tokio::sync::broadcast::Receiver<EngineEvent>) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match events.recv().await {
                Ok(event @ (EngineEvent::HostKeyPrompt(_) | EngineEvent::AuthPrompt(_))) => return event,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("{e}"),
            }
        }
    })
    .await
    .expect("a question within 10 s")
}

async fn open(session: &Session, host: sshcraft_model::model::HostId) -> SessionId {
    let opened = session
        .execute(Caller::User, "session.open", json!({ "target": { "kind": "host", "host_id": host }, "cols": 80, "rows": 24 }))
        .await
        .unwrap();
    serde_json::from_value(opened["session"].clone()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn prompts_name_their_session() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let mut events = session.subscribe();

    let (_unknown, host) = test_host(&session, "new", TestAuth::default(), false).await;
    let id = open(&session, host).await;
    let EngineEvent::HostKeyPrompt(question) = next_question(&mut events).await else { panic!("a host key question") };
    assert_eq!(question.session, Some(id), "the host key question names its session");

    let auth = TestAuth { key_then_kbd: true, kbd: vec![vec![("Password: ".into(), false, "pw".into())]], ..Default::default() };
    let (_known, host) = test_host(&session, "kbd", auth, true).await;
    let id = open(&session, host).await;
    let EngineEvent::AuthPrompt(question) = next_question(&mut events).await else { panic!("a sign-in question") };
    assert_eq!(question.session, Some(id), "the sign-in question names its session");
}

/// Output and status changes wake the window, so it need not look for output 60 times a second.
#[tokio::test(flavor = "multi_thread")]
async fn output_wakes_the_window() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let woken = Arc::new(AtomicUsize::new(0));
    let count = woken.clone();
    session.set_terminal_waker(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let (_server, host) = test_host(&session, "web", TestAuth::default(), true).await;
    open(&session, host).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while woken.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("woken within 10 s");
}
