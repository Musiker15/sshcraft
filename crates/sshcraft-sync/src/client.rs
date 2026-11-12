//! HTTP calls of the sync server, one method per endpoint of docs/server-api.md.

use crate::SyncError;
use data_encoding::BASE64URL_NOPAD;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sshcraft_proto::area::{AreaId, AreaManifest, KeyEnvelope};
use sshcraft_proto::devicelog::{DeviceLog, LogEntry};
use sshcraft_proto::identity::{AccountPublic, DeviceKeys, normalize_origin};
use sshcraft_proto::record::SealedRecord;
use std::time::Duration;
use uuid::Uuid;

/// Time limit for one request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub api: u32,
    pub origin: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    pub account: AccountPublic,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceState {
    pub device_id: Uuid,
    pub active: bool,
    pub admin_blocked: bool,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AreaListing {
    pub owner: AccountPublic,
    pub manifest: AreaManifest,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    current: Option<u64>,
}

/// One server, with an optional API key and an optional session token.
#[derive(Clone)]
pub struct ServerClient {
    http: reqwest::Client,
    origin: String,
    api_key: Option<String>,
    token: Option<String>,
}

impl ServerClient {
    /// `origin` must be an `http://` or `https://` address; it is kept in normal form, which is
    /// also what login signatures are made for.
    pub fn new(origin: &str) -> Result<Self, SyncError> {
        let origin = normalize_origin(origin);
        let host = origin
            .strip_prefix("https://")
            .or_else(|| origin.strip_prefix("http://"))
            .unwrap_or_default();
        if host.is_empty() || host.contains(['/', '@', ' ']) {
            return Err(SyncError::BadOrigin);
        }
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| SyncError::Network(e.to_string()))?;
        Ok(Self {
            http,
            origin,
            api_key: None,
            token: None,
        })
    }

    fn fail(&self, op: &str) -> Result<(), SyncError> {
        if failpoints::hit(&self.origin, op) {
            return Err(SyncError::Network(format!("injected failure: {op}")));
        }
        Ok(())
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// `ws://` or `wss://` address of the event stream.
    pub fn events_url(&self) -> String {
        let rest = self
            .origin
            .strip_prefix("https://")
            .map(|h| format!("wss://{h}"))
            .or_else(|| {
                self.origin
                    .strip_prefix("http://")
                    .map(|h| format!("ws://{h}"))
            })
            .unwrap_or_default();
        format!("{rest}/v1/events")
    }

    fn request(&self, method: Method, path: &str, api_key: bool) -> RequestBuilder {
        let mut builder = self.http.request(method, format!("{}{path}", self.origin));
        if api_key && let Some(key) = &self.api_key {
            builder = builder.header("x-api-key", key);
        }
        if !api_key && let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }
        builder
    }

    async fn send<T: DeserializeOwned>(&self, builder: RequestBuilder) -> Result<T, SyncError> {
        let response = builder
            .send()
            .await
            .map_err(|e| SyncError::Network(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| SyncError::Network(e.to_string()))?;
        if !status.is_success() {
            let body: Option<ErrorBody> = serde_json::from_slice(&bytes).ok();
            return Err(SyncError::Server {
                status: status.as_u16(),
                code: body
                    .as_ref()
                    .map_or_else(|| "unknown".into(), |b| b.code.clone()),
                current: body.and_then(|b| b.current),
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| SyncError::Network(format!("bad answer: {e}")))
    }

    pub async fn server_info(&self) -> Result<ServerInfo, SyncError> {
        self.send(self.request(Method::GET, "/v1/server", false))
            .await
    }

    pub async fn register(
        &self,
        account: &AccountPublic,
        name: &str,
        email: &str,
        log: &DeviceLog,
    ) -> Result<(), SyncError> {
        self.fail("register")?;
        let body = json!({ "account": account, "name": name, "email": email, "log": log });
        let _: Value = self
            .send(self.request(Method::POST, "/v1/accounts", true).json(&body))
            .await?;
        Ok(())
    }

    /// Fetches a challenge, signs it for this origin and keeps the session token. Sends the API
    /// key too when one is set (needed for a device's first login).
    pub async fn login(
        &mut self,
        device_id: Uuid,
        keys: &DeviceKeys,
    ) -> Result<Session, SyncError> {
        self.fail("login")?;
        #[derive(Deserialize)]
        struct Challenge {
            challenge: String,
        }
        let challenge: Challenge = self
            .send(self.request(Method::POST, "/v1/auth/challenge", false))
            .await?;
        let bytes = BASE64URL_NOPAD
            .decode(challenge.challenge.as_bytes())
            .map_err(|e| SyncError::Network(format!("bad challenge: {e}")))?;
        let signature = keys.sign_auth(&bytes, device_id, &self.origin);
        let body = json!({
            "device_id": device_id,
            "challenge": challenge.challenge,
            "signature": BASE64URL_NOPAD.encode(&signature),
        });
        let session: Session = self
            .send(
                self.request(Method::POST, "/v1/auth/login", true)
                    .json(&body),
            )
            .await?;
        self.token = Some(session.token.clone());
        Ok(session)
    }

    pub async fn me(&self) -> Result<Me, SyncError> {
        self.send(self.request(Method::GET, "/v1/accounts/me", false))
            .await
    }

    pub async fn my_devices(&self) -> Result<Vec<DeviceState>, SyncError> {
        #[derive(Deserialize)]
        struct Devices {
            devices: Vec<DeviceState>,
        }
        let list: Devices = self
            .send(self.request(Method::GET, "/v1/accounts/me/devices", false))
            .await?;
        Ok(list.devices)
    }

    pub async fn lookup(&self, email: &str) -> Result<(AccountPublic, String), SyncError> {
        #[derive(Deserialize)]
        struct Found {
            account: AccountPublic,
            name: String,
        }
        let found: Found = self
            .send(
                self.request(Method::GET, "/v1/accounts/lookup", false)
                    .query(&[("email", email)]),
            )
            .await?;
        Ok((found.account, found.name))
    }

    fn account_path(account: &AccountPublic) -> String {
        BASE64URL_NOPAD.encode(&account.0)
    }

    pub async fn log(
        &self,
        account: &AccountPublic,
        from: u64,
    ) -> Result<Vec<LogEntry>, SyncError> {
        #[derive(Deserialize)]
        struct Entries {
            entries: Vec<LogEntry>,
        }
        let path = format!("/v1/accounts/{}/log", Self::account_path(account));
        let entries: Entries = self
            .send(
                self.request(Method::GET, &path, false)
                    .query(&[("from", from)]),
            )
            .await?;
        Ok(entries.entries)
    }

    /// Appends to an account's log; with `via_api_key` the API key is used instead of the
    /// session (a new device enrolling itself). Returns the new length.
    pub async fn append_log(
        &self,
        account: &AccountPublic,
        entries: &[LogEntry],
        via_api_key: bool,
    ) -> Result<u64, SyncError> {
        self.fail("append_log")?;
        #[derive(Deserialize)]
        struct Len {
            len: u64,
        }
        let path = format!("/v1/accounts/{}/log", Self::account_path(account));
        let len: Len = self
            .send(
                self.request(Method::POST, &path, via_api_key)
                    .json(&json!({ "entries": entries })),
            )
            .await?;
        Ok(len.len)
    }

    pub async fn areas(&self) -> Result<Vec<AreaListing>, SyncError> {
        #[derive(Deserialize)]
        struct Areas {
            areas: Vec<AreaListing>,
        }
        let areas: Areas = self
            .send(self.request(Method::GET, "/v1/areas", false))
            .await?;
        Ok(areas.areas)
    }

    pub async fn put_manifest(&self, manifest: &AreaManifest) -> Result<u64, SyncError> {
        self.fail("put_manifest")?;
        #[derive(Deserialize)]
        struct Version {
            version: u64,
        }
        let path = format!("/v1/areas/{}/manifest", manifest.area_id.0);
        let v: Version = self
            .send(self.request(Method::PUT, &path, false).json(manifest))
            .await?;
        Ok(v.version)
    }

    pub async fn delete_area(&self, area: AreaId) -> Result<(), SyncError> {
        let path = format!("/v1/areas/{}", area.0);
        let _: Value = self
            .send(self.request(Method::DELETE, &path, false))
            .await?;
        Ok(())
    }

    pub async fn put_envelopes(
        &self,
        area: AreaId,
        envelopes: &[KeyEnvelope],
    ) -> Result<(), SyncError> {
        self.fail("put_envelopes")?;
        let path = format!("/v1/areas/{}/envelopes", area.0);
        let _: Value = self
            .send(
                self.request(Method::PUT, &path, false)
                    .json(&json!({ "envelopes": envelopes })),
            )
            .await?;
        Ok(())
    }

    /// The envelope for this device, or `None` if the owner has not sealed one for it yet.
    pub async fn my_envelope(&self, area: AreaId) -> Result<Option<KeyEnvelope>, SyncError> {
        let path = format!("/v1/areas/{}/envelope", area.0);
        match self.send(self.request(Method::GET, &path, false)).await {
            Ok(envelope) => Ok(Some(envelope)),
            Err(SyncError::Server { status, .. }) if status == StatusCode::NOT_FOUND.as_u16() => {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// Writes a record; returns the area's change number.
    pub async fn put_record(&self, record: &SealedRecord) -> Result<u64, SyncError> {
        self.fail("put_record")?;
        #[derive(Deserialize)]
        struct Seq {
            seq: u64,
        }
        let path = format!(
            "/v1/areas/{}/records/{}",
            record.header.area_id.0, record.header.record_id
        );
        let seq: Seq = self
            .send(self.request(Method::PUT, &path, false).json(record))
            .await?;
        Ok(seq.seq)
    }

    /// Records whose latest write is after `since`; the flag says whether more follow.
    pub async fn records(
        &self,
        area: AreaId,
        since: u64,
        limit: u32,
    ) -> Result<(Vec<(u64, SealedRecord)>, bool), SyncError> {
        self.fail("records")?;
        #[derive(Deserialize)]
        struct Entry {
            seq: u64,
            record: SealedRecord,
        }
        #[derive(Deserialize)]
        struct Page {
            records: Vec<Entry>,
            more: bool,
        }
        let path = format!("/v1/areas/{}/records", area.0);
        let page: Page = self
            .send(
                self.request(Method::GET, &path, false)
                    .query(&[("since", since), ("limit", u64::from(limit))]),
            )
            .await?;
        Ok((
            page.records
                .into_iter()
                .map(|e| (e.seq, e.record))
                .collect(),
            page.more,
        ))
    }
}

/// Injected failures for tests: a request named `op` to `origin` fails with a network error
/// after `skip` successful ones. Keyed by origin, so tests on different servers do not meet.
#[doc(hidden)]
pub mod failpoints {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn armed() -> &'static Mutex<HashMap<(String, String), usize>> {
        static ARMED: OnceLock<Mutex<HashMap<(String, String), usize>>> = OnceLock::new();
        ARMED.get_or_init(Mutex::default)
    }

    pub fn arm(origin: &str, op: &str, skip: usize) {
        if let Ok(mut map) = armed().lock() {
            map.insert((origin.to_owned(), op.to_owned()), skip);
        }
    }

    pub(crate) fn hit(origin: &str, op: &str) -> bool {
        let Ok(mut map) = armed().lock() else {
            return false;
        };
        let key = (origin.to_owned(), op.to_owned());
        match map.get_mut(&key) {
            Some(0) => {
                map.remove(&key);
                true
            }
            Some(skip) => {
                *skip -= 1;
                false
            }
            None => false,
        }
    }
}
