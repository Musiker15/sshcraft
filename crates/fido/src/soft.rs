//! A software authenticator for tests: real P-256 and Ed25519 keys, signed the way a token
//! signs (`authenticator data || SHA-256(client data)`).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use ciborium::Value;
use sha2::{Digest, Sha256};

use crate::{
    Assertion, Attachment, Attestation, Authenticator, CreateOptions, FLAG_ATTESTED, FLAG_USER_PRESENCE, FLAG_USER_VERIFICATION, FidoError,
    SkAlgorithm,
};

enum Secret {
    P256(p256::ecdsa::SigningKey),
    Ed25519(ed25519_dalek::SigningKey),
}

pub struct SoftAuthenticator {
    attachment: Attachment,
    credentials: Mutex<Vec<(Vec<u8>, Secret)>>,
    counter: AtomicU32,
    assertions: AtomicUsize,
    cancel: AtomicBool,
}

impl SoftAuthenticator {
    pub fn new(attachment: Attachment) -> Self {
        Self {
            attachment,
            credentials: Mutex::new(Vec::new()),
            counter: AtomicU32::new(0),
            assertions: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        }
    }

    /// How many signatures were asked for.
    pub fn assertions(&self) -> usize {
        self.assertions.load(Ordering::SeqCst)
    }

    /// From now on, every request is cancelled as if the user closed the dialog.
    pub fn set_cancel(&self, cancel: bool) {
        self.cancel.store(cancel, Ordering::SeqCst);
    }
}

fn int(i: i64) -> Value {
    Value::Integer(i.into())
}

#[async_trait::async_trait]
impl Authenticator for SoftAuthenticator {
    async fn make_credential(&self, application: &str, options: &CreateOptions, _client_data: &[u8]) -> Result<Attestation, FidoError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(FidoError::Cancelled);
        }
        if options.attachment != self.attachment {
            return Err(FidoError::NotSupported);
        }
        let (secret, cose) = match options.algorithm {
            SkAlgorithm::EcdsaP256 => {
                use p256::elliptic_curve::Generate;
                let key = p256::ecdsa::SigningKey::generate_from_rng(&mut rand::rng());
                let point = key.verifying_key().to_sec1_point(false);
                let cose = Value::Map(vec![
                    (int(1), int(2)),
                    (int(3), int(-7)),
                    (int(-1), int(1)),
                    (int(-2), Value::Bytes(point.x().expect("x").to_vec())),
                    (int(-3), Value::Bytes(point.y().expect("y").to_vec())),
                ]);
                (Secret::P256(key), cose)
            }
            SkAlgorithm::Ed25519 => {
                let key = ed25519_dalek::SigningKey::generate(&mut rand::rng());
                let cose = Value::Map(vec![
                    (int(1), int(1)),
                    (int(3), int(-8)),
                    (int(-1), int(6)),
                    (int(-2), Value::Bytes(key.verifying_key().to_bytes().to_vec())),
                ]);
                (Secret::Ed25519(key), cose)
            }
        };
        let id: [u8; 16] = rand::random();
        let mut data = Sha256::digest(application.as_bytes()).to_vec();
        data.push(FLAG_USER_PRESENCE | FLAG_ATTESTED);
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&[0u8; 16]);
        data.extend_from_slice(&(id.len() as u16).to_be_bytes());
        data.extend_from_slice(&id);
        ciborium::into_writer(&cose, &mut data).map_err(|e| FidoError::Failed(e.to_string()))?;
        self.credentials.lock().unwrap().push((id.to_vec(), secret));
        Ok(Attestation { authenticator_data: data, credential_id: id.to_vec() })
    }

    async fn get_assertion(
        &self,
        application: &str,
        credential_id: &[u8],
        client_data: &[u8],
        verify_user: bool,
        _attachment: Attachment,
    ) -> Result<Assertion, FidoError> {
        self.assertions.fetch_add(1, Ordering::SeqCst);
        if self.cancel.load(Ordering::SeqCst) {
            return Err(FidoError::Cancelled);
        }
        let counter = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let mut data = Sha256::digest(application.as_bytes()).to_vec();
        data.push(FLAG_USER_PRESENCE | if verify_user { FLAG_USER_VERIFICATION } else { 0 });
        data.extend_from_slice(&counter.to_be_bytes());
        let mut message = data.clone();
        message.extend_from_slice(&Sha256::digest(client_data));
        let credentials = self.credentials.lock().unwrap();
        let (_, secret) = credentials.iter().find(|(id, _)| id == credential_id).ok_or_else(|| FidoError::Failed("unknown credential".into()))?;
        let signature = match secret {
            Secret::P256(key) => {
                use p256::ecdsa::signature::Signer;
                let signature: p256::ecdsa::Signature = key.sign(&message);
                signature.to_der().as_bytes().to_vec()
            }
            Secret::Ed25519(key) => {
                use ed25519_dalek::Signer;
                key.sign(&message).to_bytes().to_vec()
            }
        };
        Ok(Assertion { authenticator_data: data, signature })
    }
}
