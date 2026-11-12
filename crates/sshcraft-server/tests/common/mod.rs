#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use sshcraft_proto::devicelog::{DeviceLog, LogEntry};
use sshcraft_proto::identity::{AccountKey, AccountPublic, DeviceCertificate, DeviceKeys};
use sshcraft_server::store::{AccountRow, Store};
use sshcraft_server::{AppState, app};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

pub const ORIGIN: &str = "https://sync.example.org";

pub struct TestEnv {
    pub state: AppState,
    _dir: tempfile::TempDir,
}

impl TestEnv {
    pub async fn new() -> Self {
        let (store, dir) = fresh_sqlite().await;
        let state = AppState::new(store, ORIGIN.to_owned(), Duration::from_secs(30));
        Self { state, _dir: dir }
    }

    pub fn store(&self) -> &dyn Store {
        self.state.store.as_ref()
    }

    /// Sends one request through the router and returns the status and the JSON body
    /// (`Value::Null` when the body is empty or not JSON).
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("request");
        let response = app(self.state.clone())
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }
}

/// A new SQLite store in a temporary directory; keep the directory alive as long as the store.
pub async fn fresh_sqlite() -> (Arc<dyn Store>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let url = format!("sqlite://{}", dir.path().join("test.db").display());
    let store = sshcraft_server::store::open(&url)
        .await
        .expect("open sqlite");
    (store, dir)
}

/// One device of a test person: its id, private keys and certificate.
pub struct Dev {
    pub id: Uuid,
    pub keys: DeviceKeys,
    pub cert: DeviceCertificate,
}

/// A user with an account key, a device log and devices, built with the real protocol types.
pub struct Person {
    pub key: AccountKey,
    pub log: DeviceLog,
    pub devices: Vec<Dev>,
    pub name: String,
    pub email: String,
}

impl Person {
    /// A person with one device; the email is `<name>@example.org`.
    pub fn new(name: &str) -> Self {
        let mut person = Self {
            key: AccountKey::generate(),
            log: DeviceLog::default(),
            devices: Vec::new(),
            name: name.to_owned(),
            email: format!("{name}@example.org"),
        };
        person.add_device();
        person
    }

    pub fn public(&self) -> AccountPublic {
        self.key.public()
    }

    /// Adds a device to the log and returns its index and the new log entry.
    pub fn add_device(&mut self) -> (usize, LogEntry) {
        let keys = DeviceKeys::generate();
        let id = Uuid::new_v4();
        let index = self.devices.len();
        let cert = DeviceCertificate::issue(
            &self.key,
            id,
            keys.public(),
            &format!("{}-{index}", self.name),
            "linux",
            1,
        );
        self.log.append_add(&self.key, cert.clone(), 1);
        self.devices.push(Dev { id, keys, cert });
        (index, self.log.entries.last().cloned().expect("entry"))
    }

    /// Revokes device `target`, signed by device `by`; returns the new entry.
    pub fn revoke_by_device(&mut self, by: usize, target: usize) -> LogEntry {
        let (by_id, target_id) = (self.devices[by].id, self.devices[target].id);
        self.log
            .append_revoke_by_device(by_id, &self.devices[by].keys, target_id, 2);
        self.log.entries.last().cloned().expect("entry")
    }

    pub fn row(&self) -> AccountRow {
        AccountRow {
            account: self.public(),
            name: self.name.clone(),
            email: self.email.clone(),
            created_at: 1,
            locked: false,
        }
    }

    pub fn email_norm(&self) -> String {
        self.email.to_lowercase()
    }

    /// Creates the account with its first device directly in the store.
    pub async fn create_in(&self, store: &dyn Store) {
        store
            .create_account(
                &self.row(),
                &self.email_norm(),
                &self.log.entries[0],
                &self.devices[0].cert,
            )
            .await
            .expect("create account");
    }
}
