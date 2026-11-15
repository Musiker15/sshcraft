//! Host keys as SSHCraft keeps them (algorithm, key, fingerprint), and OpenSSH's
//! `UpdateHostKeys`: after sign-in a server may announce all its host keys
//! (`hostkeys-00@openssh.com`). New ones are kept only after the server proved it holds them
//! (`hostkeys-prove-00@openssh.com`); the verifier decides what to keep.

use std::sync::{Arc, Weak};

use russh::client;
use russh::keys::{HashAlg, PublicKey};

use crate::HostKeyVerifier;
use sshcraft_model::policy::MAX_ANNOUNCED_KEYS;

/// A server's host key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedKey {
    /// For example `ssh-ed25519`.
    pub algorithm: String,
    /// The key in OpenSSH base64, as in a `known_hosts` line.
    pub public_key: String,
    /// `SHA256:…`
    pub fingerprint: String,
}

impl PresentedKey {
    pub fn of(key: &russh::keys::ssh_key::public::KeyData) -> Self {
        let public = PublicKey::from(key.clone());
        let line = public.to_openssh().unwrap_or_default();
        let mut words = line.split_whitespace();
        let algorithm = words.next().unwrap_or_default().to_owned();
        let public_key = words.next().unwrap_or_default().to_owned();
        Self { algorithm, public_key, fingerprint: key.fingerprint(HashAlg::Sha256).to_string() }
    }
}

/// What to do with an announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnounceAction {
    /// Change nothing.
    Ignore,
    /// Nothing new to prove (keys only go away): apply at once.
    Apply,
    /// Apply after the server proved it holds these keys.
    Prove(Vec<PresentedKey>),
}

/// Asks the server to prove it holds keys (the connection's handle; a fake in tests).
#[async_trait::async_trait]
pub(crate) trait KeyProver: Send + Sync {
    async fn prove(&self, keys: Vec<PublicKey>) -> bool;
}

#[async_trait::async_trait]
impl<H: client::Handler> KeyProver for client::Handle<H> {
    async fn prove(&self, keys: Vec<PublicKey>) -> bool {
        self.hostkeys_prove(keys).await.is_ok()
    }
}

/// The connection (held weakly, so it can close) and what the announcement is about.
pub(crate) struct Announced {
    pub address: String,
    pub port: u16,
    /// The fingerprint this connection was verified with.
    pub used: Arc<std::sync::Mutex<Option<String>>>,
}

/// Handles one announcement: at most [`MAX_ANNOUNCED_KEYS`], new keys only after proof.
pub(crate) async fn take(prover: &dyn KeyProver, verifier: &dyn HostKeyVerifier, about: &Announced, keys: Vec<PublicKey>) {
    if keys.len() > MAX_ANNOUNCED_KEYS {
        return;
    }
    let Some(used) = about.used.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() else {
        return;
    };
    let presented: Vec<PresentedKey> = keys.iter().map(|k| PresentedKey::of(k.key_data())).collect();
    match verifier.announced(&about.address, about.port, &used, &presented) {
        AnnounceAction::Ignore => {}
        AnnounceAction::Apply => verifier.apply(&about.address, about.port, &used, &presented, &[]),
        AnnounceAction::Prove(new) => {
            let to_prove: Vec<PublicKey> =
                keys.iter().filter(|k| new.iter().any(|n| n.fingerprint == k.key_data().fingerprint(HashAlg::Sha256).to_string())).cloned().collect();
            if to_prove.len() == new.len() && prover.prove(to_prove).await {
                verifier.apply(&about.address, about.port, &used, &presented, &new);
            }
        }
    }
}

/// Takes announcements for as long as the connection lives.
pub(crate) fn spawn<H: client::Handler + 'static>(
    handle: Weak<client::Handle<H>>,
    verifier: Arc<dyn HostKeyVerifier>,
    about: Announced,
    mut announcements: tokio::sync::mpsc::UnboundedReceiver<Vec<PublicKey>>,
) {
    tokio::spawn(async move {
        while let Some(keys) = announcements.recv().await {
            let Some(handle) = handle.upgrade() else {
                return;
            };
            take(handle.as_ref(), verifier.as_ref(), &about, keys).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use russh::keys::{Algorithm, PrivateKey};

    use super::*;

    struct FakeProver(bool, AtomicUsize);

    #[async_trait::async_trait]
    impl KeyProver for FakeProver {
        async fn prove(&self, _keys: Vec<PublicKey>) -> bool {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0
        }
    }

    /// Wants every key it has not seen proven, and notes what it applied.
    #[derive(Default)]
    struct Noting {
        applied: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl HostKeyVerifier for Noting {
        async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
            true
        }
        fn announced(&self, _: &str, _: u16, used: &str, keys: &[PresentedKey]) -> AnnounceAction {
            AnnounceAction::Prove(keys.iter().filter(|k| k.fingerprint != used).cloned().collect())
        }
        fn apply(&self, _: &str, _: u16, _: &str, keys: &[PresentedKey], _proven: &[PresentedKey]) {
            self.applied.lock().unwrap().push(keys.iter().map(|k| k.fingerprint.clone()).collect());
        }
    }

    fn key() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().public_key().clone()
    }

    fn about(used: &PublicKey) -> Announced {
        Announced { address: "a".into(), port: 22, used: Arc::new(Mutex::new(Some(used.key_data().fingerprint(HashAlg::Sha256).to_string()))) }
    }

    #[tokio::test]
    async fn announced_keys_are_proven_and_stored() {
        let (used, new) = (key(), key());
        let (prover, verifier) = (FakeProver(true, AtomicUsize::new(0)), Noting::default());
        take(&prover, &verifier, &about(&used), vec![used.clone(), new]).await;
        assert_eq!(prover.1.load(Ordering::SeqCst), 1);
        assert_eq!(verifier.applied.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unproven_host_keys_are_not_stored() {
        let (used, new) = (key(), key());
        let (prover, verifier) = (FakeProver(false, AtomicUsize::new(0)), Noting::default());
        take(&prover, &verifier, &about(&used), vec![used.clone(), new]).await;
        assert_eq!(prover.1.load(Ordering::SeqCst), 1);
        assert!(verifier.applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn too_many_announced_keys_are_ignored() {
        let used = key();
        let keys: Vec<PublicKey> = std::iter::once(used.clone()).chain((0..MAX_ANNOUNCED_KEYS).map(|_| key())).collect();
        let (prover, verifier) = (FakeProver(true, AtomicUsize::new(0)), Noting::default());
        take(&prover, &verifier, &about(&used), keys).await;
        assert_eq!(prover.1.load(Ordering::SeqCst), 0);
        assert!(verifier.applied.lock().unwrap().is_empty());
    }

    #[test]
    fn a_presented_key_has_its_algorithm_and_base64() {
        let k = key();
        let p = PresentedKey::of(k.key_data());
        assert_eq!(p.algorithm, "ssh-ed25519");
        assert!(p.public_key.starts_with("AAAAC3NzaC1lZDI1NTE5"), "{}", p.public_key);
        assert!(p.fingerprint.starts_with("SHA256:"));
    }
}
