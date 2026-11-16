//! Runs a host's login script on its session's output: each step waits for its text in the
//! plain output (escape sequences removed), then sends its line and Enter, once. A step that does
//! not see its text within [`STEP_TIMEOUT`] stops the script. Whether a script may run at all is
//! decided before (approval on this device, see `sessions.rs`).

use std::time::{Duration, Instant};

use sshcraft_model::login_script::LoginStep;

/// How long one step waits for its text.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(15);
/// The plain output kept to look for the text, in characters.
const WINDOW_CHARS: usize = 4096;

/// What the output fed in asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    Nothing,
    /// Bytes to send to the session.
    Send(Vec<u8>),
    /// The current step waited too long; the script stopped.
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Esc {
    #[default]
    Ground,
    Escape,
    /// A control sequence, until its final byte.
    Csi,
    /// An OSC or similar string, until BEL or `ESC \`.
    Str,
    StrEscape,
}

/// One script on one session.
#[derive(Debug)]
pub struct LoginScript {
    steps: Vec<LoginStep>,
    next: usize,
    window: String,
    since: Instant,
    stopped: bool,
    esc: Esc,
}

impl LoginScript {
    pub fn new(steps: Vec<LoginStep>, now: Instant) -> Self {
        Self { steps, next: 0, window: String::new(), since: now, stopped: false, esc: Esc::Ground }
    }

    /// Every step ran, or the script stopped.
    pub fn finished(&self) -> bool {
        self.stopped || self.next >= self.steps.len()
    }

    /// Every step ran: the script did not stop on a step that waited too long.
    pub fn completed(&self) -> bool {
        !self.stopped && self.next >= self.steps.len()
    }

    fn plain(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        for c in text.chars() {
            self.esc = match (self.esc, c) {
                (Esc::Ground, '\x1b') => Esc::Escape,
                (Esc::Ground, c) => {
                    if !c.is_control() || c == '\n' {
                        self.window.push(c);
                    }
                    Esc::Ground
                }
                (Esc::Escape, '[') => Esc::Csi,
                (Esc::Escape, ']' | 'P' | '_' | '^' | 'X') => Esc::Str,
                (Esc::Escape, _) => Esc::Ground,
                (Esc::Csi, c) if ('\x40'..='\x7e').contains(&c) => Esc::Ground,
                (Esc::Csi, _) => Esc::Csi,
                (Esc::Str, '\x07') => Esc::Ground,
                (Esc::Str, '\x1b') => Esc::StrEscape,
                (Esc::Str, _) => Esc::Str,
                (Esc::StrEscape, '\\') => Esc::Ground,
                (Esc::StrEscape, _) => Esc::Str,
            };
        }
        let count = self.window.chars().count();
        if count > WINDOW_CHARS {
            self.window = self.window.chars().skip(count - WINDOW_CHARS).collect();
        }
    }

    /// Looks at output `bytes` that arrived at `now`.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) -> Feed {
        if self.finished() {
            return Feed::Nothing;
        }
        if now.saturating_duration_since(self.since) > STEP_TIMEOUT {
            self.stopped = true;
            return Feed::TimedOut;
        }
        self.plain(bytes);
        let Some(step) = self.steps.get(self.next) else { return Feed::Nothing };
        if !self.window.contains(&step.expect) {
            return Feed::Nothing;
        }
        let mut send = step.send.clone().into_bytes();
        send.push(b'\r');
        self.next += 1;
        self.window.clear();
        self.since = now;
        Feed::Send(send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(expect: &str, send: &str) -> LoginStep {
        LoginStep { expect: expect.into(), send: send.into() }
    }

    #[test]
    fn steps_fire_in_order_once_each() {
        let now = Instant::now();
        let mut s = LoginScript::new(vec![step("Choice:", "2"), step("$ ", "cd /srv")], now);
        assert_eq!(s.feed(b"Welcome\r\n$ ", now), Feed::Nothing, "the second step waits for the first");
        assert_eq!(s.feed(b"1) a\r\n2) b\r\nChoice: ", now), Feed::Send(b"2\r".to_vec()));
        assert_eq!(s.feed(b"Choice: ", now), Feed::Nothing, "a step fires once");
        assert_eq!(s.feed(b"\r\n$ ", now), Feed::Send(b"cd /srv\r".to_vec()));
        assert!(s.finished());
        assert_eq!(s.feed(b"$ ", now), Feed::Nothing);
    }

    #[test]
    fn text_split_across_chunks_and_coloured_is_found() {
        let now = Instant::now();
        let mut s = LoginScript::new(vec![step("login:", "admin")], now);
        assert_eq!(s.feed(b"\x1b[32mlog", now), Feed::Nothing);
        assert_eq!(s.feed(b"\x1b]0;title\x07in\x1b[0m:", now), Feed::Send(b"admin\r".to_vec()));
    }

    #[test]
    fn a_script_that_timed_out_did_not_complete() {
        let start = Instant::now();
        let mut script = LoginScript::new(vec![step("menu>", "2")], start);
        assert!(matches!(script.feed(b"", start + STEP_TIMEOUT + Duration::from_secs(1)), Feed::TimedOut));
        assert!(script.finished() && !script.completed());
        let mut done = LoginScript::new(vec![step("menu>", "2")], start);
        assert!(matches!(done.feed(b"menu>", start), Feed::Send(_)));
        assert!(done.completed());
    }

    #[test]
    fn a_step_that_waits_too_long_stops_the_script() {
        let now = Instant::now();
        let mut s = LoginScript::new(vec![step("never", "x")], now);
        assert_eq!(s.feed(b"other", now + STEP_TIMEOUT + Duration::from_secs(1)), Feed::TimedOut);
        assert!(s.finished());
        assert_eq!(s.feed(b"never", now + STEP_TIMEOUT * 2), Feed::Nothing);
    }
}
