use std::sync::{Arc, Mutex};

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::testing::{TestAuth, TestServer, user_certificate};
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
        certificate: None,
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
        certificate: None,
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
            certificate: None,
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
            certificate: None,
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
            certificate: None,
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
        certificate: None,
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
        certificate: None,
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

/// Signs like a hardware key would, but with an ordinary key in memory.
struct SoftSigner {
    key: PrivateKey,
    cancel: bool,
}

#[async_trait::async_trait]
impl sshcraft_ssh::KeySigner for SoftSigner {
    async fn sign(&mut self, data: &[u8]) -> Result<Vec<u8>, SshError> {
        use russh::keys::signature::Signer;
        use russh::keys::ssh_encoding::Encode;
        if self.cancel {
            return Err(SshError::SignerCancelled);
        }
        let signature: russh::keys::ssh_key::Signature = self
            .key
            .try_sign(data)
            .map_err(|e| SshError::SignerFailed(e.to_string()))?;
        Ok(signature.encode_vec().unwrap())
    }
}

fn signer(key: &PrivateKey, cancel: bool) -> Credential {
    Credential::Signer {
        public_key: key.public_key().clone(),
        signer: Box::new(SoftSigner {
            key: key.clone(),
            cancel,
        }),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn signer_signs_in() {
    let k = key();
    let server = TestServer::start(k.public_key().clone()).await;
    let auth = Auth {
        key: Some(signer(&k, false)),
        ..Default::default()
    };
    run(&server, auth, Recording::new(vec![])).await.unwrap();
}

#[tokio::test]
async fn signer_cancel_is_reported() {
    let k = key();
    let server = TestServer::start(k.public_key().clone()).await;
    let auth = Auth {
        key: Some(signer(&k, true)),
        ..Default::default()
    };
    let err = run(&server, auth, Recording::new(vec![]))
        .await
        .unwrap_err();
    assert_eq!(err, SshError::SignerCancelled);
}

#[tokio::test]
async fn certificate_signs_in_where_key_alone_is_not_accepted() {
    let (k, ca) = (key(), key());
    let server = TestServer::start_with(TestAuth {
        ca: Some(ca.public_key().clone()),
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "tester", 0, u64::MAX);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        certificate: Some(cert),
        ..Default::default()
    };
    run(&server, auth, Recording::new(vec![])).await.unwrap();
    assert_eq!(server.received_certificates(), 1);
}

#[tokio::test]
async fn certificate_for_other_principal_is_rejected() {
    let (k, ca) = (key(), key());
    let server = TestServer::start_with(TestAuth {
        ca: Some(ca.public_key().clone()),
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "someone-else", 0, u64::MAX);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        certificate: Some(cert),
        ..Default::default()
    };
    let err = run(&server, auth, Recording::new(vec![]))
        .await
        .unwrap_err();
    assert_eq!(err, SshError::AuthFailed);
}

#[tokio::test]
async fn rejected_certificate_falls_back_to_key() {
    let (k, ca, other_ca) = (key(), key(), key());
    let server = TestServer::start_with(TestAuth {
        key: Some(k.public_key().clone()),
        ca: Some(other_ca.public_key().clone()),
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "tester", 0, u64::MAX);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        certificate: Some(cert),
        ..Default::default()
    };
    let session = connect(
        params(server.port, auth),
        Arc::new(Trust),
        Recording::new(vec![]),
    )
    .await
    .unwrap();
    assert_eq!(server.received_certificates(), 1);
    assert!(!session.certificate_expired());
}

#[tokio::test]
async fn expired_certificate_is_not_sent() {
    let (k, ca) = (key(), key());
    let server = TestServer::start_with(TestAuth {
        key: Some(k.public_key().clone()),
        ca: Some(ca.public_key().clone()),
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "tester", 0, now() - 60);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        certificate: Some(cert),
        ..Default::default()
    };
    let session = connect(
        params(server.port, auth),
        Arc::new(Trust),
        Recording::new(vec![]),
    )
    .await
    .unwrap();
    assert_eq!(server.received_certificates(), 0);
    assert!(session.certificate_expired());
}

#[tokio::test]
async fn certificate_with_signer_signs_in() {
    let (k, ca) = (key(), key());
    let server = TestServer::start_with(TestAuth {
        ca: Some(ca.public_key().clone()),
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "tester", 0, u64::MAX);
    let auth = Auth {
        key: Some(signer(&k, false)),
        certificate: Some(cert),
        ..Default::default()
    };
    run(&server, auth, Recording::new(vec![])).await.unwrap();
    assert_eq!(server.received_certificates(), 1);
}

#[test]
fn certificate_validity_window() {
    let (k, ca) = (key(), key());
    let cert = |after, before| user_certificate(k.public_key(), &ca, "tester", after, before);
    assert!(sshcraft_ssh::certificate_valid_now(&cert(0, u64::MAX)));
    assert!(!sshcraft_ssh::certificate_valid_now(&cert(0, now() - 60)));
    assert!(!sshcraft_ssh::certificate_valid_now(&cert(
        now() + 3600,
        u64::MAX
    )));
}

fn rsa_key() -> PrivateKey {
    use russh::keys::ssh_key::private::{KeypairData, RsaKeypair};
    let pair = RsaKeypair::random(&mut rand::rng(), 2048).unwrap();
    PrivateKey::new(KeypairData::from(pair), "rsa").unwrap()
}

#[tokio::test]
async fn rsa_certificate_signs_with_sha2() {
    let (k, ca) = (rsa_key(), key());
    let server = TestServer::start_with(TestAuth {
        ca: Some(ca.public_key().clone()),
        refuse_sha1_rsa: true,
        ..Default::default()
    })
    .await;
    let cert = user_certificate(k.public_key(), &ca, "tester", 0, u64::MAX);
    let auth = Auth {
        key: Some(Credential::Key(k)),
        certificate: Some(cert),
        ..Default::default()
    };
    run(&server, auth, Recording::new(vec![])).await.unwrap();
    assert_eq!(server.received_certificates(), 1);
}
