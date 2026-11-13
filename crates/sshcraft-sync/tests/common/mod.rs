#![allow(dead_code)]

use sshcraft_proto::devicelog::DeviceLog;
use sshcraft_proto::identity::{AccountKey, DeviceCertificate, DeviceKeys};
use sshcraft_server::config::ServeConfig;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// A real sync server in this process, on a free local port, with SQLite in a temp dir.
pub struct TestServer {
    pub origin: String,
    pub api_key: String,
    pub addr: SocketAddr,
    pub database_url: String,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl TestServer {
    pub async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite://{}", dir.path().join("server.db").display());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = format!("http://127.0.0.1:{}", addr.port());
        let mut server = Self {
            origin,
            api_key: String::new(),
            addr,
            database_url,
            stop: None,
            task: None,
            _dir: dir,
        };
        server.run(listener).await;
        let store = sshcraft_server::store::open(&server.database_url)
            .await
            .unwrap();
        server.api_key = store.api_key().await.unwrap().0;
        server
    }

    async fn run(&mut self, listener: TcpListener) {
        let config = ServeConfig {
            listen: self.addr,
            database_url: self.database_url.clone(),
            public_origin: self.origin.clone(),
            ws_recheck: Duration::from_millis(200),
        };
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = sshcraft_server::serve_on(listener, config, async {
                let _ = stopped.await;
            })
            .await;
        });
        self.stop = Some(stop);
        self.task = Some(task);
        // serve_on opens the store before it accepts; wait until it answers.
        for _ in 0..50 {
            if sshcraft_server::health(self.addr).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("test server did not come up");
    }

    /// Stops the server; the database stays for `restart`.
    pub async fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            // Graceful first, so open WebSockets close as on a real shutdown.
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }

    /// Starts again on the same port with the same database.
    pub async fn restart(&mut self) {
        self.stop().await;
        let listener = loop {
            match TcpListener::bind(self.addr).await {
                Ok(listener) => break listener,
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        self.run(listener).await;
    }

    /// The server's own store, for tests that tamper with what it serves.
    pub async fn store(&self) -> std::sync::Arc<dyn sshcraft_server::store::Store> {
        sshcraft_server::store::open(&self.database_url)
            .await
            .unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// An account built straight from the protocol types, for tests below the account flows.
pub struct RawAccount {
    pub key: AccountKey,
    pub device_id: Uuid,
    pub keys: DeviceKeys,
    pub log: DeviceLog,
}

impl RawAccount {
    pub fn new() -> Self {
        let key = AccountKey::generate();
        let keys = DeviceKeys::generate();
        let device_id = Uuid::new_v4();
        let cert = DeviceCertificate::issue(&key, device_id, keys.public(), "test", "linux", 1);
        let mut log = DeviceLog::default();
        log.append_add(&key, cert, 1);
        Self {
            key,
            device_id,
            keys,
            log,
        }
    }
}

use sshcraft_core::model::Host;
use sshcraft_core::store::{ServerRow, Store};
use sshcraft_core::vault::{MemoryMasterKey, Vault};
use sshcraft_proto::area::{AreaKey, AreaManifest, KeyEnvelope, open_area_key};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use sshcraft_sync::account::{JoinAccount, NewAccount, create_account, device_keys, join_account};
use sshcraft_sync::client::ServerClient;
use sshcraft_sync::items::{ItemBody, SyncItem, encode};
use std::sync::Mutex;

/// One device: its own local store and vault.
pub struct Dev {
    pub store: Mutex<Store>,
    pub vault: Vault,
}

impl Dev {
    pub fn new() -> Self {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        Self {
            store: Mutex::new(store),
            vault,
        }
    }

    pub fn row(&self) -> ServerRow {
        self.store.lock().unwrap().servers().unwrap().pop().unwrap()
    }

    pub fn keys(&self) -> DeviceKeys {
        device_keys(&self.vault, &self.row()).unwrap()
    }

    pub fn hosts(&self) -> Vec<Host> {
        self.store.lock().unwrap().list_hosts().unwrap()
    }

    /// The personal area's verified manifest and key, from what this device stored.
    pub fn area(&self) -> (AreaManifest, AreaKey) {
        let row = self.row();
        let area = self
            .store
            .lock()
            .unwrap()
            .areas(row.id)
            .unwrap()
            .pop()
            .unwrap();
        let manifest: AreaManifest = serde_json::from_str(&area.manifest).unwrap();
        let envelope: KeyEnvelope = serde_json::from_str(&area.envelope).unwrap();
        let key = open_area_key(&envelope, row.device_id, &self.keys(), &manifest).unwrap();
        (manifest, key)
    }

    pub async fn client(&self, server: &TestServer) -> ServerClient {
        let row = self.row();
        let mut client = ServerClient::new(&server.origin).unwrap();
        client.login(row.device_id, &self.keys()).await.unwrap();
        client
    }
}

/// Creates an account on `dev` and returns the account key export.
pub async fn create(server: &TestServer, dev: &Dev, email: &str) -> String {
    let input = NewAccount {
        server_name: "home".into(),
        origin: server.origin.clone(),
        api_key: server.api_key.clone(),
        name: "Alice".into(),
        email: email.into(),
        device_name: "first".into(),
        platform: "windows".into(),
    };
    create_account(&dev.store, &dev.vault, input, 1)
        .await
        .unwrap()
        .1
        .to_string()
}

pub async fn join(server: &TestServer, dev: &Dev, export: &str) {
    let input = JoinAccount {
        server_name: "home".into(),
        origin: server.origin.clone(),
        api_key: server.api_key.clone(),
        account_key: export.into(),
        device_name: "next".into(),
        platform: "linux".into(),
    };
    join_account(&dev.store, &dev.vault, input, 2)
        .await
        .unwrap();
}

/// Seals a host as a record written by `dev` and uploads it; returns the record.
pub async fn write_host(
    server: &TestServer,
    dev: &Dev,
    host: &Host,
    record_id: Uuid,
    version: u64,
    modified_at: i64,
) -> SealedRecord {
    let row = dev.row();
    let (manifest, key) = dev.area();
    let item = SyncItem {
        modified_at,
        body: ItemBody::Host { host: host.clone() },
    };
    let header = RecordHeader {
        record_id,
        area_id: manifest.area_id,
        version,
        key_version: manifest.key_version,
        author_account: sshcraft_proto::identity::AccountPublic(row.account),
        author_device: row.device_id,
    };
    let record = SealedRecord::seal(header, &encode(&item).unwrap(), &key, &dev.keys());
    dev.client(server).await.put_record(&record).await.unwrap();
    record
}

/// `by` revokes `target` in the account's log, without any key rotation.
pub async fn revoke_raw(server: &TestServer, by: &Dev, target: Uuid) {
    let row = by.row();
    let client = by.client(server).await;
    let account = sshcraft_proto::identity::AccountPublic(row.account);
    let mut log = DeviceLog {
        entries: client.log(&account, 0).await.unwrap(),
    };
    log.append_revoke_by_device(row.device_id, &by.keys(), target, 3);
    let entry = log.entries.last().cloned().unwrap();
    client.append_log(&account, &[entry], false).await.unwrap();
}

pub fn sample_host(name: &str) -> Host {
    Host {
        forwards: Vec::new(),
        id: sshcraft_core::model::HostId::new(),
        name: name.into(),
        group_id: None,
        settings: sshcraft_core::model::ConnectionSettings {
            address: Some("10.0.0.1".into()),
            port: None,
            username: Some("root".into()),
            key: None,
            ..Default::default()
        },
        location: None,
        tags: Vec::new(),
        os: None,
    }
}

pub fn account(dev: &Dev) -> sshcraft_proto::identity::AccountPublic {
    sshcraft_proto::identity::AccountPublic(dev.row().account)
}

pub async fn engine(dev: &Dev) -> sshcraft_sync::engine::Engine {
    sshcraft_sync::engine::Engine::load(&dev.store, &dev.vault, dev.row().id)
        .await
        .unwrap()
}

/// Pull, then push.
pub async fn sync(dev: &Dev) {
    let mut e = engine(dev).await;
    e.pull(&dev.store, &dev.vault).await.unwrap();
    e.push(&dev.store, &dev.vault, 0).await.unwrap();
}

/// The stored manifest and key of one area on `dev`.
pub fn stored_area(dev: &Dev, area: Uuid) -> (AreaManifest, AreaKey) {
    let row = dev.row();
    let state = dev
        .store
        .lock()
        .unwrap()
        .areas(row.id)
        .unwrap()
        .into_iter()
        .find(|a| a.area_id == area)
        .expect("area stored");
    let manifest: AreaManifest = serde_json::from_str(&state.manifest).unwrap();
    let envelope: KeyEnvelope = serde_json::from_str(&state.envelope).unwrap();
    let key = open_area_key(&envelope, row.device_id, &dev.keys(), &manifest).unwrap();
    (manifest, key)
}

/// Runs SQL against the test server's SQLite database, as a compromised server could.
pub fn server_sql(server: &TestServer, sql: &str, params: impl rusqlite::Params) {
    let path = server.database_url.trim_start_matches("sqlite://");
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(sql, params).unwrap();
}

/// Seals an item as a record written by `dev` in `area` (with the key `dev` stored for it).
pub fn seal_in(
    dev: &Dev,
    area: Uuid,
    body: ItemBody,
    record_id: Uuid,
    version: u64,
    modified_at: i64,
) -> SealedRecord {
    let row = dev.row();
    let (manifest, key) = stored_area(dev, area);
    let item = SyncItem { modified_at, body };
    let header = RecordHeader {
        record_id,
        area_id: manifest.area_id,
        version,
        key_version: manifest.key_version,
        author_account: sshcraft_proto::identity::AccountPublic(row.account),
        author_device: row.device_id,
    };
    SealedRecord::seal(header, &encode(&item).unwrap(), &key, &dev.keys())
}

/// Puts a record into the server's database directly, as a compromised server could.
pub fn inject_record(server: &TestServer, record: &SealedRecord) {
    server_sql(
        server,
        "INSERT INTO records (area_id, record_id, version, seq, record)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(seq), 0) + 1 FROM records WHERE area_id = ?1), ?4)",
        rusqlite::params![
            record.header.area_id.0.to_string(),
            record.header.record_id.to_string(),
            record.header.version as i64,
            serde_json::to_string(record).unwrap()
        ],
    );
}

pub fn member(dev: &Dev, role: sshcraft_proto::area::Role) -> sshcraft_proto::area::Member {
    sshcraft_proto::area::Member {
        account: account(dev),
        role,
    }
}
