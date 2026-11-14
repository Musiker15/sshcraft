mod common;

use common::TestServer;
use sshcraft_proto::ProtoError;
use sshcraft_proto::area::{AreaKey, AreaManifest, KeyEnvelope, open_area_key};
use sshcraft_store::store::{ServerRow, Store};
use sshcraft_store::vault::{MemoryMasterKey, Vault};
use sshcraft_sync::SyncError;
use sshcraft_sync::account::{JoinAccount, NewAccount, account_key_qr_svg, create_account, device_keys, join_account};
use sshcraft_sync::client::ServerClient;
use std::sync::Mutex;

struct Device {
    store: Mutex<Store>,
    vault: Vault,
}

fn device() -> Device {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    Device { store: Mutex::new(store), vault }
}

fn new_account(server: &TestServer, email: &str) -> NewAccount {
    NewAccount {
        server_name: "home".into(),
        origin: server.origin.clone(),
        api_key: server.api_key.clone(),
        name: "Alice".into(),
        email: email.into(),
        device_name: "laptop".into(),
        platform: "windows".into(),
    }
}

fn join(server: &TestServer, key: &str) -> JoinAccount {
    JoinAccount {
        server_name: "home".into(),
        origin: server.origin.clone(),
        api_key: server.api_key.clone(),
        account_key: key.into(),
        device_name: "desktop".into(),
        platform: "linux".into(),
    }
}

/// The area keys this device can open from what it stored.
fn area_keys(d: &Device) -> Vec<AreaKey> {
    let store = d.store.lock().unwrap();
    let row: ServerRow = store.servers().unwrap().pop().unwrap();
    let keys = device_keys(&d.vault, &row).unwrap();
    store
        .areas(row.id)
        .unwrap()
        .into_iter()
        .map(|a| {
            let manifest: AreaManifest = serde_json::from_str(&a.manifest).unwrap();
            let envelope: KeyEnvelope = serde_json::from_str(&a.envelope).unwrap();
            open_area_key(&envelope, row.device_id, &keys, &manifest).unwrap()
        })
        .collect()
}

#[tokio::test]
async fn create_then_join_on_a_second_device() {
    let server = TestServer::start().await;
    let a = device();
    let (_, export) = create_account(&a.store, &a.vault, new_account(&server, "a@example.org"), 1).await.unwrap();
    assert!(export.starts_with("sshcraft-account-1:"));
    let b = device();
    join_account(&b.store, &b.vault, join(&server, &export), 2).await.unwrap();
    let (ka, kb) = (area_keys(&a), area_keys(&b));
    assert_eq!((ka.len(), kb.len()), (1, 1));
    assert!(ka[0].same_key_as(&kb[0]));

    let row = b.store.lock().unwrap().servers().unwrap().pop().unwrap();
    let mut client = ServerClient::new(&server.origin).unwrap();
    client.login(row.device_id, &device_keys(&b.vault, &row).unwrap()).await.unwrap();
    let devices = client.my_devices().await.unwrap();
    assert_eq!(devices.len(), 3, "laptop, desktop and the recovery device");
}

#[tokio::test]
async fn join_works_with_every_other_device_gone() {
    let server = TestServer::start().await;
    let export = {
        let a = device();
        create_account(&a.store, &a.vault, new_account(&server, "a@example.org"), 1).await.unwrap().1
    };
    let b = device();
    join_account(&b.store, &b.vault, join(&server, &export), 2).await.unwrap();
    assert_eq!(area_keys(&b).len(), 1);
}

#[tokio::test]
async fn failed_setup_leaves_nothing_local() {
    let mut server = TestServer::start().await;
    let taken = device();
    create_account(&taken.store, &taken.vault, new_account(&server, "taken@example.org"), 1).await.unwrap();

    let d = device();
    let mut wrong_key = new_account(&server, "x@example.org");
    wrong_key.api_key = "wrong".into();
    let err = create_account(&d.store, &d.vault, wrong_key, 1).await.unwrap_err();
    assert_eq!(err.server_code(), Some("bad_api_key"));

    let err = create_account(&d.store, &d.vault, new_account(&server, "TAKEN@example.org"), 1).await.unwrap_err();
    assert_eq!(err.server_code(), Some("email_taken"));

    server.stop().await;
    let err = create_account(&d.store, &d.vault, new_account(&server, "y@example.org"), 1).await.unwrap_err();
    assert!(matches!(err, SyncError::Network(_)), "{err:?}");

    assert!(d.store.lock().unwrap().servers().unwrap().is_empty());
}

#[tokio::test]
async fn mistyped_account_key_is_a_checksum_error() {
    let server = TestServer::start().await;
    let a = device();
    let (_, export) = create_account(&a.store, &a.vault, new_account(&server, "a@example.org"), 1).await.unwrap();
    let mut typo = export.to_string();
    // A character in the middle: the last one partly holds Base32 padding bits, so a
    // change there can be a format error instead of a checksum error.
    let at = typo.len() / 2 + 10;
    let swapped = if typo.as_bytes()[at] == b'A' { "B" } else { "A" };
    typo.replace_range(at..=at, swapped);
    let b = device();
    let err = join_account(&b.store, &b.vault, join(&server, &typo), 2).await.unwrap_err();
    assert_eq!(err, SyncError::Proto(ProtoError::BadChecksum));
    assert!(b.store.lock().unwrap().servers().unwrap().is_empty());
}

#[tokio::test]
async fn account_key_of_another_server_is_not_found() {
    let first = TestServer::start().await;
    let second = TestServer::start().await;
    let a = device();
    let (_, export) = create_account(&a.store, &a.vault, new_account(&first, "a@example.org"), 1).await.unwrap();
    let b = device();
    let err = join_account(&b.store, &b.vault, join(&second, &export), 2).await.unwrap_err();
    assert_eq!(err, SyncError::AccountNotFound);
    assert!(b.store.lock().unwrap().servers().unwrap().is_empty());
}

#[test]
fn qr_svg_contains_an_svg() {
    let svg = account_key_qr_svg("sshcraft-account-1:ABCDEF");
    assert!(svg.contains("<svg"), "{svg}");
}

#[tokio::test]
async fn a_different_address_for_the_server_is_refused_early() {
    let server = TestServer::start().await;
    let d = device();
    let mut input = new_account(&server, "a@example.org");
    // Same server, but not the address it was set up for: logins would fail later.
    input.origin = server.origin.replace("127.0.0.1", "localhost");
    let err = create_account(&d.store, &d.vault, input, 1).await.unwrap_err();
    assert_eq!(err, SyncError::OriginMismatch { server: server.origin.clone() });
    assert!(d.store.lock().unwrap().servers().unwrap().is_empty());
}

#[tokio::test]
async fn an_interrupted_create_hands_out_the_key_to_finish_by_joining() {
    use sshcraft_sync::account::Setup;
    for op in ["login", "put_manifest", "put_envelopes"] {
        let server = TestServer::start().await;
        let d = device();
        sshcraft_sync::client::failpoints::arm(&server.origin, op, 0);
        let (setup, export) = create_account(&d.store, &d.vault, new_account(&server, "a@example.org"), 1).await.unwrap();
        assert!(matches!(setup, Setup::Unfinished(SyncError::Network(_))), "{op}");
        assert!(d.store.lock().unwrap().servers().unwrap().is_empty(), "{op}");
        // The account exists; the key lets this or another device finish the setup.
        let again = device();
        join_account(&again.store, &again.vault, join(&server, &export), 2).await.unwrap();
        assert_eq!(area_keys(&again).len(), 1, "{op}: a personal area to sync into");
    }
}

#[tokio::test]
async fn a_failed_join_revokes_its_half_added_device() {
    let server = TestServer::start().await;
    let a = device();
    let (_, export) = create_account(&a.store, &a.vault, new_account(&server, "a@example.org"), 1).await.unwrap();
    let b = device();
    sshcraft_sync::client::failpoints::arm(&server.origin, "put_envelopes", 0);
    assert!(join_account(&b.store, &b.vault, join(&server, &export), 2).await.is_err());
    join_account(&b.store, &b.vault, join(&server, &export), 3).await.unwrap();
    let row = b.store.lock().unwrap().servers().unwrap().pop().unwrap();
    let mut client = ServerClient::new(&server.origin).unwrap();
    client.login(row.device_id, &device_keys(&b.vault, &row).unwrap()).await.unwrap();
    let active = client.my_devices().await.unwrap().into_iter().filter(|d| d.active).count();
    assert_eq!(active, 3, "first device, recovery device and the joined one; no ghost");
}
