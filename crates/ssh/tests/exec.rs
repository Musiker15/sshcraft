use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::authorized_keys::{InstallOutcome, install_command, parse_outcome};
use sshcraft_ssh::testing::TestServer;
use sshcraft_ssh::{Auth, ConnectParams, Credential, HostKeyVerifier, NoPrompter, connect};

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }
}

#[tokio::test]
async fn install_runs_one_command_over_exec() {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start(key.public_key().clone()).await;
    server.set_exec_output("motd\nSSHCRAFT_ADDED\n");
    let session = connect(
        ConnectParams {
            address: "127.0.0.1".into(),
            port: server.port,
            username: "tester".into(),
            auth: Auth::key(Credential::Key(key.clone())),
        },
        Arc::new(Trust),
        Arc::new(NoPrompter),
    )
    .await
    .unwrap();
    let command = install_command(key.public_key(), false);
    let output = session.exec(&command, 4096).await.unwrap();
    assert_eq!(parse_outcome(&output).unwrap(), InstallOutcome::Added);
    assert_eq!(server.received_commands(), vec![command]);
}
