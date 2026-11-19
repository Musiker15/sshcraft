use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::sftp::{CHUNK, EntryKind, SftpClient, SftpError};
use sshcraft_ssh::testing::TestServer;
use sshcraft_ssh::{Auth, ConnectParams, Credential, HostKeyVerifier, NoPrompter, Session, connect};

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(&self, _host_name: &str, _address: &str, _port: u16, _fingerprint: &str) -> bool {
        true
    }
}

async fn connected() -> (TestServer, Session) {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start(key.public_key().clone()).await;
    let params = ConnectParams { address: "127.0.0.1".into(), port: server.port, username: "tester".into(), auth: Auth::key(Credential::Key(key)) };
    let session = connect(params, Arc::new(Trust), Arc::new(NoPrompter)).await.unwrap();
    (server, session)
}

async fn served() -> (TestServer, Session, SftpClient, tempfile::TempDir) {
    let (server, session) = connected().await;
    let root = tempfile::tempdir().unwrap();
    server.serve_sftp(root.path());
    let sftp = session.sftp().await.unwrap();
    (server, session, sftp, root)
}

#[tokio::test]
async fn a_server_without_sftp_refuses_the_subsystem() {
    let (_server, session) = connected().await;
    assert!(session.sftp().await.is_err());
}

#[tokio::test]
async fn lists_a_folder_without_dot_entries() {
    let (_server, _session, sftp, root) = served().await;
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("a.txt"), b"hello").unwrap();
    assert_eq!(sftp.canonicalize(".").await.unwrap(), "/");
    let mut listing = sftp.list("/", 100).await.unwrap();
    listing.entries.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<_> = listing.entries.iter().map(|e| (e.name.as_str(), e.attrs.kind)).collect();
    assert_eq!(names, [("a.txt", EntryKind::File), ("docs", EntryKind::Dir)]);
    assert_eq!(listing.entries[0].attrs.size, 5);
    assert!(!listing.truncated);
}

/// A folder with more entries than the limit gives the limit and says it stopped.
#[tokio::test]
async fn a_listing_stops_at_its_limit() {
    let (_server, _session, sftp, root) = served().await;
    for i in 0..12 {
        std::fs::write(root.path().join(format!("f{i}")), b"").unwrap();
    }
    let listing = sftp.list("/", 5).await.unwrap();
    assert_eq!(listing.entries.len(), 5);
    assert!(listing.truncated);
}

#[tokio::test]
async fn writes_and_reads_a_file_in_chunks() {
    let (_server, _session, sftp, root) = served().await;
    let data: Vec<u8> = (0..(CHUNK as usize * 2 + 100)).map(|i| (i % 251) as u8).collect();
    let file = sftp.create_new("/blob").await.unwrap();
    for (n, chunk) in data.chunks(CHUNK as usize).enumerate() {
        file.write_at((n * CHUNK as usize) as u64, chunk.to_vec()).await.unwrap();
    }
    file.close().await.unwrap();
    assert_eq!(std::fs::read(root.path().join("blob")).unwrap(), data);

    let file = sftp.open_read("/blob").await.unwrap();
    let mut back = Vec::new();
    loop {
        let piece = file.read_at(back.len() as u64, CHUNK).await.unwrap();
        if piece.is_empty() {
            break;
        }
        back.extend(piece);
    }
    file.close().await.unwrap();
    assert_eq!(back, data);
}

#[tokio::test]
async fn creating_a_file_that_exists_fails_instead_of_overwriting() {
    let (_server, _session, sftp, root) = served().await;
    std::fs::write(root.path().join("keep"), b"mine").unwrap();
    assert!(sftp.create_new("/keep").await.is_err());
    assert_eq!(std::fs::read(root.path().join("keep")).unwrap(), b"mine");
}

#[tokio::test]
async fn folders_rename_remove_and_permissions() {
    let (server, _session, sftp, root) = served().await;
    sftp.mkdir("/new").await.unwrap();
    assert!(root.path().join("new").is_dir());
    sftp.rename("/new", "/renamed").await.unwrap();
    assert!(root.path().join("renamed").is_dir());
    std::fs::write(root.path().join("renamed/f"), b"x").unwrap();
    sftp.chmod("/renamed/f", 0o100640).await.unwrap();
    assert_eq!(server.sftp_mode("/renamed/f"), Some(0o640), "only the permission bits are sent");
    sftp.remove("/renamed/f").await.unwrap();
    sftp.rmdir("/renamed").await.unwrap();
    assert!(!root.path().join("renamed").exists());
    assert_eq!(sftp.stat("/renamed").await.unwrap_err(), SftpError::NotFound);
}
