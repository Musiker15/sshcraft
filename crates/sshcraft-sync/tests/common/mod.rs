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
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
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
