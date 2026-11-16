//! Program status (OSC 7501, revision 0.2): a program reports what it does, idle, working, done,
//! blocked or error, so the tab can show it. A report breaking any rule is discarded whole; the
//! records are bounded, and so is every field.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// The longest report, `ESC ] 7501 ;` to the terminator, in bytes.
pub const MAX_REPORT_BYTES: usize = 4096;
/// Records kept; the one updated longest ago goes first.
pub const MAX_RECORDS: usize = 256;
const MAX_KEY_BYTES: usize = 16;
const MAX_ID_BYTES: usize = 128;
const MAX_ID_SEGMENTS: usize = 8;
const MAX_SEGMENT_BYTES: usize = 32;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;

/// What a program says it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl ProgramState {
    /// Higher is more urgent: blocked, error, done, working, idle.
    fn urgency(self) -> u8 {
        match self {
            Self::Blocked => 4,
            Self::Error => 3,
            Self::Done => 2,
            Self::Working => 1,
            Self::Idle => 0,
        }
    }
}

/// Why a blocked program waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// What one program (or part of one, by id) last reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramRecord {
    /// `a/b/c`, or empty for the root record.
    pub id: String,
    pub app: Option<String>,
    pub state: ProgramState,
    /// 0 to 100, only while working or blocked.
    pub progress: Option<u8>,
    pub kind: Option<BlockedKind>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

/// A parsed report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Report {
    /// `?`: does the terminal speak the protocol.
    Query,
    Update(ProgramRecord),
    /// `state=clear`, for one id and everything below it, or for all.
    Clear(Option<String>),
}

fn name_ok(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

fn id_ok(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES && id.split('/').count() <= MAX_ID_SEGMENTS && id.split('/').all(|s| name_ok(s, MAX_SEGMENT_BYTES))
}

/// Decoded base64 text, without control and bidi characters; `None` when it breaks a limit.
fn text(value: &str, max_encoded: usize, max_decoded: usize) -> Option<String> {
    if value.len() > max_encoded {
        return None;
    }
    let bytes = STANDARD.decode(value).ok()?;
    if bytes.len() > max_decoded {
        return None;
    }
    let decoded = String::from_utf8(bytes).ok()?;
    Some(crate::clean_text(&decoded, usize::MAX))
}

/// The report in the body of `ESC ] 7501 ; <body> ST`; `None` when it breaks a rule.
pub(crate) fn parse(body: &[u8]) -> Option<Report> {
    if body.len() > MAX_REPORT_BYTES {
        return None;
    }
    let body = std::str::from_utf8(body).ok()?;
    if body == "?" {
        return Some(Report::Query);
    }
    let mut state = None;
    let (mut id, mut app, mut progress, mut kind, mut title, mut msg) = (String::new(), None, None, None, None, None);
    let mut seen: Vec<&str> = Vec::new();
    for pair in body.split(':') {
        let (key, value) = pair.split_once('=')?;
        if key.is_empty() || key.len() > MAX_KEY_BYTES || seen.contains(&key) {
            return None;
        }
        seen.push(key);
        match key {
            "state" => state = Some(value),
            "id" => {
                if !value.is_empty() && !id_ok(value) {
                    return None;
                }
                id = value.to_string();
            }
            "app" => {
                if !name_ok(value, MAX_SEGMENT_BYTES) {
                    return None;
                }
                app = Some(value.to_string());
            }
            "progress" => progress = Some(value.parse::<u8>().ok().filter(|p| *p <= 100)?),
            "kind" => {
                kind = match value {
                    "permission" => Some(BlockedKind::Permission),
                    "question" => Some(BlockedKind::Question),
                    "auth" => Some(BlockedKind::Auth),
                    _ => None,
                }
            }
            "title" => title = Some(text(value, MAX_TITLE_ENCODED, MAX_TITLE_DECODED)?),
            "msg" => msg = Some(text(value, MAX_MSG_ENCODED, MAX_MSG_DECODED)?),
            // Keys of later revisions are skipped; the format of the pair was checked.
            _ => {}
        }
    }
    let state = match state? {
        "clear" => return Some(Report::Clear((!id.is_empty()).then_some(id))),
        "idle" => ProgramState::Idle,
        "working" => ProgramState::Working,
        "done" => ProgramState::Done,
        "blocked" => ProgramState::Blocked,
        "error" => ProgramState::Error,
        _ => return None,
    };
    if progress.is_some() && !matches!(state, ProgramState::Working | ProgramState::Blocked) {
        return None;
    }
    let kind = kind.filter(|_| state == ProgramState::Blocked);
    Some(Report::Update(ProgramRecord { id, app, state, progress, kind, title, msg }))
}

/// The records of one terminal, the one updated longest ago first.
#[derive(Debug, Default)]
pub(crate) struct Programs {
    records: Vec<ProgramRecord>,
}

impl Programs {
    /// Applies `report`; returns whether anything changed.
    pub(crate) fn apply(&mut self, report: Report) -> bool {
        match report {
            Report::Query => false,
            Report::Update(record) => {
                self.records.retain(|r| r.id != record.id);
                if self.records.len() >= MAX_RECORDS {
                    self.records.remove(0);
                }
                self.records.push(record);
                true
            }
            Report::Clear(None) => {
                let changed = !self.records.is_empty();
                self.records.clear();
                changed
            }
            Report::Clear(Some(id)) => {
                let before = self.records.len();
                let below = format!("{id}/");
                self.records.retain(|r| r.id != id && !r.id.starts_with(&below));
                self.records.len() != before
            }
        }
    }

    /// Drops records in `states`; returns whether any went.
    pub(crate) fn drop_states(&mut self, states: &[ProgramState]) -> bool {
        let before = self.records.len();
        self.records.retain(|r| !states.contains(&r.state));
        self.records.len() != before
    }

    pub(crate) fn records(&self) -> Vec<ProgramRecord> {
        self.records.clone()
    }

    pub(crate) fn summary(&self) -> Option<ProgramState> {
        self.records.iter().map(|r| r.state).max_by_key(|s| s.urgency())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_recognised() {
        assert_eq!(parse(b"?"), Some(Report::Query));
    }

    #[test]
    fn keys_may_not_repeat() {
        assert_eq!(parse(b"state=done:state=error"), None);
    }

    #[test]
    fn id_segments_are_checked() {
        assert!(id_ok("a/b.c/d-e+f_g"));
        assert!(!id_ok("a//b"));
        assert!(!id_ok(&["x"; 9].join("/")));
        assert!(!id_ok(&"y".repeat(33)));
    }

    #[test]
    fn kind_counts_only_when_blocked() {
        let Some(Report::Update(r)) = parse(b"state=working:kind=question") else { panic!("an update") };
        assert_eq!(r.kind, None);
    }

    #[test]
    fn an_overlong_report_is_discarded() {
        let body = format!("state=done:x={}", "a".repeat(MAX_REPORT_BYTES));
        assert_eq!(parse(body.as_bytes()), None);
    }
}
