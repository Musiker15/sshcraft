use std::sync::Arc;

use sshcraft_fido::soft::SoftAuthenticator;
use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm, SkSigner, create_key};
use sshcraft_ssh::testing::TestServer;
use sshcraft_ssh::{
    Auth, ConnectParams, Credential, HostKeyVerifier, NoPrompter, SshError, connect,
};

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }
}

fn options(algorithm: SkAlgorithm) -> CreateOptions {
    CreateOptions {
        algorithm,
        attachment: Attachment::CrossPlatform,
        verify_user: true,
        user_name: "tester".into(),
    }
}

async fn sign_in(
    server_key: &russh::keys::PublicKey,
    signer: SkSigner,
    public: russh::keys::PublicKey,
) -> Result<(), SshError> {
    let server = TestServer::start(server_key.clone()).await;
    connect(
        ConnectParams {
            address: "127.0.0.1".into(),
            port: server.port,
            username: "tester".into(),
            auth: Auth::key(Credential::Signer {
                public_key: public,
                signer: Box::new(signer),
            }),
        },
        Arc::new(Trust),
        Arc::new(NoPrompter),
    )
    .await
    .map(|_| ())
}

async fn signs_in_with(algorithm: SkAlgorithm) {
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let key = create_key(token.as_ref(), &options(algorithm), "yubikey")
        .await
        .unwrap();
    let public = key.public_key().clone();
    let signer = SkSigner::new(token.clone(), &key, Attachment::CrossPlatform).unwrap();
    sign_in(&public, signer, public.clone()).await.unwrap();
    assert_eq!(token.assertions(), 1);
}

#[tokio::test]
async fn sk_ecdsa_signs_in() {
    signs_in_with(SkAlgorithm::EcdsaP256).await;
}

#[tokio::test]
async fn sk_ed25519_signs_in() {
    signs_in_with(SkAlgorithm::Ed25519).await;
}

#[tokio::test]
async fn cancel_ends_with_signer_cancelled() {
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let key = create_key(token.as_ref(), &options(SkAlgorithm::EcdsaP256), "k")
        .await
        .unwrap();
    token.set_cancel(true);
    let public = key.public_key().clone();
    let signer = SkSigner::new(token.clone(), &key, Attachment::CrossPlatform).unwrap();
    let err = sign_in(&public, signer, public.clone()).await.unwrap_err();
    assert_eq!(err, SshError::SignerCancelled);
}

#[tokio::test]
async fn signature_from_other_key_is_rejected() {
    // The server knows key A; the token signs with key B but claims A's public half.
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let a = create_key(token.as_ref(), &options(SkAlgorithm::EcdsaP256), "a")
        .await
        .unwrap();
    let b = create_key(token.as_ref(), &options(SkAlgorithm::EcdsaP256), "b")
        .await
        .unwrap();
    let signer = SkSigner::new(token.clone(), &b, Attachment::CrossPlatform).unwrap();
    let err = sign_in(a.public_key(), signer, a.public_key().clone())
        .await
        .unwrap_err();
    assert_eq!(err, SshError::AuthFailed);
}
