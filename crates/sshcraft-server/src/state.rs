//! Shared state of the HTTP handlers.

use crate::error::ApiError;
use crate::hub::Hub;
use crate::store::Store;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngExt;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    /// Public origin in normal form; login signatures must be made for it.
    pub origin: String,
    pub hub: Arc<Hub>,
    pub challenges: Arc<Challenges>,
    pub ws_recheck: Duration,
    clock: Clock,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>, origin: String, ws_recheck: Duration) -> Self {
        Self {
            store,
            origin,
            hub: Arc::new(Hub::default()),
            challenges: Arc::new(Challenges::default()),
            ws_recheck,
            clock: Arc::new(system_now),
        }
    }

    /// Replaces the clock, for tests.
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Unix seconds.
    pub fn now(&self) -> i64 {
        (self.clock)()
    }
}

fn system_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// How long a login challenge stays valid, in seconds.
pub const CHALLENGE_TTL: i64 = 60;
const NONCE_LEN: usize = 16;
const TAG_LEN: usize = 32;
/// Nonce, expiry (big-endian `i64`) and HMAC-SHA256 tag.
pub const CHALLENGE_LEN: usize = NONCE_LEN + 8 + TAG_LEN;

/// Login challenges that need no storage until they are used: each one carries its expiry and an
/// HMAC under a secret that lives only in this process. Only challenges redeemed by a valid
/// login are remembered (until they expire), so flooding the challenge endpoint costs the
/// server nothing and cannot crowd out real logins.
pub struct Challenges {
    secret: [u8; 32],
    redeemed: Mutex<HashMap<Vec<u8>, i64>>,
}

impl Default for Challenges {
    fn default() -> Self {
        let mut secret = [0u8; 32];
        rand::rng().fill(&mut secret[..]);
        Self {
            secret,
            redeemed: Mutex::default(),
        }
    }
}

impl Challenges {
    fn mac(&self) -> Hmac<Sha256> {
        <Hmac<Sha256> as KeyInit>::new_from_slice(&self.secret).expect("any key length")
    }

    /// A new challenge, valid for [`CHALLENGE_TTL`] seconds.
    pub fn issue(&self, now: i64) -> Result<Vec<u8>, ApiError> {
        let mut challenge = vec![0u8; NONCE_LEN];
        rand::rng().fill(&mut challenge[..]);
        challenge.extend_from_slice(&(now + CHALLENGE_TTL).to_be_bytes());
        let mut mac = self.mac();
        mac.update(&challenge);
        challenge.extend_from_slice(&mac.finalize().into_bytes());
        Ok(challenge)
    }

    /// Whether the challenge was issued here, has not expired and was not used yet.
    pub fn valid(&self, challenge: &[u8], now: i64) -> bool {
        if challenge.len() != CHALLENGE_LEN {
            return false;
        }
        let (body, tag) = challenge.split_at(NONCE_LEN + 8);
        let mut mac = self.mac();
        mac.update(body);
        if mac.verify_slice(tag).is_err() {
            return false;
        }
        let mut expiry = [0u8; 8];
        expiry.copy_from_slice(&body[NONCE_LEN..]);
        if i64::from_be_bytes(expiry) < now {
            return false;
        }
        self.redeemed
            .lock()
            .map(|used| !used.contains_key(challenge))
            .unwrap_or(false)
    }

    /// Marks a challenge as used after a successful login; `false` if it already was, which
    /// makes two concurrent logins with one challenge fail on the second.
    pub fn redeem(&self, challenge: &[u8], now: i64) -> bool {
        let Ok(mut used) = self.redeemed.lock() else {
            return false;
        };
        used.retain(|_, expires| *expires >= now);
        let expires = now + CHALLENGE_TTL;
        used.insert(challenge.to_vec(), expires).is_none()
    }
}
