//! Shared state of the HTTP handlers.

use crate::error::ApiError;
use crate::hub::Hub;
use crate::store::Store;
use rand::RngExt;
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
/// Most open challenges at once; beyond that the server answers `busy`.
pub const MAX_CHALLENGES: usize = 10_000;

/// Open login challenges, each usable once.
#[derive(Default)]
pub struct Challenges {
    open: Mutex<HashMap<[u8; 32], i64>>,
}

impl Challenges {
    /// A new random challenge, valid for [`CHALLENGE_TTL`] seconds.
    pub fn issue(&self, now: i64) -> Result<[u8; 32], ApiError> {
        let mut open = self.open.lock().map_err(ApiError::internal)?;
        if open.len() >= MAX_CHALLENGES {
            open.retain(|_, expires| *expires >= now);
            if open.len() >= MAX_CHALLENGES {
                return Err(ApiError::Busy);
            }
        }
        let mut challenge = [0u8; 32];
        rand::rng().fill(&mut challenge[..]);
        open.insert(challenge, now + CHALLENGE_TTL);
        Ok(challenge)
    }

    /// Redeems a challenge. It is removed either way; `true` only if it was open and not expired.
    pub fn take(&self, challenge: &[u8; 32], now: i64) -> bool {
        match self.open.lock() {
            Ok(mut open) => open.remove(challenge).is_some_and(|expires| expires >= now),
            Err(_) => false,
        }
    }
}
