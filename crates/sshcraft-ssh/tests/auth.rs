use std::sync::{Arc, Mutex};

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ssh::{
    Auth, AuthPrompter, AuthRequest, ConnectParams, Credential, HostKeyVerifier, SshError, connect,
};
use zeroize::Zeroizing;

const STORED: &str = "stored-secret";

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(
        &self,
        _host_name: &str,
        _address: &str,
        _port: u16,
        _fingerprint: &str,
    ) -> bool {
        true
    }
}

/// Answers each request with the next prepared answer (`None`: cancel) and keeps the requests.
struct Recording {
    answers: Mutex<Vec<Option<Vec<String>>>>,
    seen: Mutex<Vec<AuthRequest>>,
}

impl Recording {
    fn new(answers: Vec<Option<Vec<&str>>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(
                answers
                    .into_iter()
                    .map(|a| a.map(|a| a.into_iter().map(str::to_owned).collect()))
                    .collect(),
            ),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl AuthPrompter for Recording {
    async fn answer(&self, request: AuthRequest) -> Option<Vec<Zeroizing<String>>> {
        self.seen.lock().unwrap().push(request);
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() {
            return None;
        }
        answers
            .remove(0)
            .map(|a| a.into_iter().map(Zeroizing::new).collect())
    }
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn params(port: u16, auth: Auth) -> ConnectParams {
    ConnectParams {
        address: "127.0.0.1".into(),
        port,
        username: "tester".into(),
        auth,
    }
}

fn with_password(password: &str) -> Auth {
    Auth {
        key: None,
        password: Some(Zeroizing::new(password.to_owned())),
    }
}

fn round(lines: &[(&str, bool, &str)]) -> Vec<(String, bool, String)> {
    lines
        .iter()
        .map(|(q, e, a)| ((*q).to_owned(), *e, (*a).to_owned()))
        .collect()
}

async fn run(server: &TestServer, auth: Auth, prompter: Arc<Recording>) -> Result<(), SshError> {
    connect(params(server.port, auth), Arc::new(Trust), prompter)
        .await
        .map(|_| ())
}

#[tokio::test]
async fn password_auth_succeeds() {
    let server = TestServer::start_with(TestAuth {
        password: Some(STORED.into()),
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![]);
    run(&server, with_password(STORED), prompter.clone())
        .await
        .unwrap();
    assert_eq!(prompter.asked(), 0);
    assert_eq!(server.received_passwords(), vec![STORED.to_owned()]);
}

#[tokio::test]
async fn wrong_password_is_rejected() {
    let server = TestServer::start_with(TestAuth {
        password: Some("right".into()),
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![]);
    let err = run(&server, with_password("wrong"), prompter.clone())
        .await
        .unwrap_err();
    assert_eq!(err, SshError::PasswordRejected);
    assert_eq!(prompter.asked(), 0);
}

#[tokio::test]
async fn stored_password_answers_single_hidden_kbd_prompt() {
    let server = TestServer::start_with(TestAuth {
        kbd: vec![round(&[("Password: ", false, STORED)])],
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![]);
    run(&server, with_password(STORED), prompter.clone())
        .await
        .unwrap();
    assert_eq!(prompter.asked(), 0);
}

#[tokio::test]
async fn visible_kbd_prompt_is_never_answered_with_stored_password() {
    let server = TestServer::start_with(TestAuth {
        kbd: vec![round(&[("Username: ", true, "tester")])],
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["tester"])]);
    run(&server, with_password(STORED), prompter.clone())
        .await
        .unwrap();
    assert_eq!(prompter.asked(), 1);
    assert!(!server.received_passwords().contains(&STORED.to_owned()));
}

#[tokio::test]
async fn two_prompt_round_asks_user() {
    let server = TestServer::start_with(TestAuth {
        kbd: vec![round(&[
            ("Password: ", false, "pw"),
            ("Code: ", false, "123456"),
        ])],
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["pw", "123456"])]);
    run(&server, with_password(STORED), prompter.clone())
        .await
        .unwrap();
    let seen = prompter.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let lines: Vec<(&str, bool)> = seen[0]
        .prompts
        .iter()
        .map(|p| (p.text.as_str(), p.echo))
        .collect();
    assert_eq!(lines, vec![("Password: ", false), ("Code: ", false)]);
    assert_eq!(seen[0].username, "tester");
    assert!(!server.received_passwords().contains(&STORED.to_owned()));
}

#[tokio::test]
async fn key_then_otp_partial_success() {
    let k = key();
    let server = TestServer::start_with(TestAuth {
        key: Some(k.public_key().clone()),
        kbd: vec![round(&[("Verification code: ", false, "424242")])],
        key_then_kbd: true,
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["424242"])]);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        password: None,
    };
    run(&server, auth, prompter.clone()).await.unwrap();
    assert_eq!(prompter.asked(), 1);
}

#[tokio::test]
async fn cancel_gives_auth_cancelled() {
    let server = TestServer::start_with(TestAuth {
        kbd: vec![round(&[("Code: ", true, "1")])],
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![None]);
    let err = run(
        &server,
        Auth {
            key: None,
            password: None,
        },
        prompter,
    )
    .await
    .unwrap_err();
    assert_eq!(err, SshError::AuthCancelled);
}

#[tokio::test]
async fn no_stored_password_asks_once_for_password_method() {
    let server = TestServer::start_with(TestAuth {
        password: Some("typed".into()),
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["typed"])]);
    run(
        &server,
        Auth {
            key: None,
            password: None,
        },
        prompter.clone(),
    )
    .await
    .unwrap();
    let seen = prompter.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].prompts.len(), 1);
    assert!(!seen[0].prompts[0].echo);
}

#[tokio::test]
async fn wrong_typed_password_is_rejected() {
    let server = TestServer::start_with(TestAuth {
        password: Some("typed".into()),
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["nope"])]);
    let err = run(
        &server,
        Auth {
            key: None,
            password: None,
        },
        prompter,
    )
    .await
    .unwrap_err();
    assert_eq!(err, SshError::PasswordRejected);
}

#[tokio::test]
async fn key_auth_unchanged() {
    let k = key();
    let server = TestServer::start(k.public_key().clone()).await;
    let prompter = Recording::new(vec![]);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        password: None,
    };
    run(&server, auth, prompter.clone()).await.unwrap();
    assert_eq!(prompter.asked(), 0);
}

#[tokio::test]
async fn wrong_key_without_other_methods_fails() {
    let server = TestServer::start(key().public_key().clone()).await;
    let auth = Auth {
        key: Some(Credential::Key(key())),
        password: None,
    };
    let err = run(&server, auth, Recording::new(vec![]))
        .await
        .unwrap_err();
    assert_eq!(err, SshError::AuthFailed);
}

#[tokio::test]
async fn password_then_otp_does_not_send_the_password_as_code() {
    let server = TestServer::start_with(TestAuth {
        password: Some(STORED.into()),
        kbd: vec![round(&[("Verification code: ", false, "424242")])],
        password_then_kbd: true,
        ..Default::default()
    })
    .await;
    let prompter = Recording::new(vec![Some(vec!["424242"])]);
    run(&server, with_password(STORED), prompter.clone())
        .await
        .unwrap();
    assert_eq!(prompter.asked(), 1);
    assert_eq!(
        server.received_passwords(),
        vec![STORED.to_owned(), "424242".to_owned()]
    );
}
