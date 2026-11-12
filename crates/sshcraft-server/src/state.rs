//! Shared state of the HTTP handlers.

use crate::hub::Hub;
use crate::store::Store;
use std::sync::Arc;
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

/// Open login challenges (Task 4 fills this in).
#[derive(Default)]
pub struct Challenges {}
