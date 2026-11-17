//! Desktop notifications. A command that ran longer than `terminal.notify_after_secs` and ended
//! while the window was in the background says so; a message from the server (OSC 9, OSC 777,
//! or a program waiting for the user) goes out with the host's name first, so a server cannot
//! pretend to be SSHCraft, and at most one every [`SERVER_INTERVAL`] per tab.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The shortest time between two server messages of one tab.
pub const SERVER_INTERVAL: Duration = Duration::from_secs(5);

/// Shows a notification on the desktop.
pub trait Notifier: Send {
    fn notify(&self, title: &str, body: &str);
}

/// The operating system's notifications.
pub struct Desktop;

impl Notifier for Desktop {
    #[cfg(target_os = "android")]
    fn notify(&self, _title: &str, _body: &str) {
        // Android notifications need the activity's Java side; M1 shows none (docs/gaps.md).
    }

    #[cfg(not(target_os = "android"))]
    fn notify(&self, title: &str, body: &str) {
        let (title, body) = (title.to_string(), body.to_string());
        // Off the interface thread: the desktop service may take a moment to answer.
        std::thread::spawn(move || {
            // A desktop without a notification service shows nothing; there is no one to tell.
            let _shown = notify_rust::Notification::new().appname("SSHCraft").summary(&title).body(&markup_safe(&body)).show();
        });
    }
}

/// `text` safe for notification servers that read markup (the freedesktop ones render links and
/// images in the body): `&`, `<` and `>` become entities there and stay as they are elsewhere.
pub fn markup_safe(text: &str) -> String {
    if cfg!(all(unix, not(target_os = "macos"))) { text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") } else { text.to_string() }
}

/// Keeps notifications instead of showing them (tests, and every app made for tests).
#[derive(Debug, Default, Clone)]
pub struct Recording(Arc<Mutex<Vec<(String, String)>>>);

impl Recording {
    /// The notifications so far, title and body, and forgets them.
    pub fn taken(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Notifier for Recording {
    fn notify(&self, title: &str, body: &str) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push((title.to_string(), body.to_string()));
    }
}

/// One message per [`SERVER_INTERVAL`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RateLimit {
    last: Option<Instant>,
}

impl RateLimit {
    /// Whether a message may go out at `now`; counts it when it may.
    pub fn allow(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|last| now.saturating_duration_since(last) < SERVER_INTERVAL) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// Whether a finished command that ran for `elapsed` is worth a notification: it ran at least
/// `after_secs` (0 turns this off) and the user was not looking at it.
pub fn command_worth_it(after_secs: u32, elapsed: Duration, watched: bool) -> bool {
    after_secs > 0 && !watched && elapsed >= Duration::from_secs(u64::from(after_secs))
}
