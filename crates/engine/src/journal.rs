//! The commands run so far, replayable, without secrets.

use std::collections::VecDeque;

use serde::Serialize;
use serde_json::Value;

/// At most this many entries; the oldest go first.
pub const MAX_ENTRIES: usize = 1000;
/// At most this many bytes of parameters in all; the oldest go first.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Parameters larger than this are not kept, only noted.
const MAX_ENTRY_BYTES: usize = 64 * 1024;
const REDACTED: &str = "<redacted>";

/// Parameter keys whose values never reach the journal, at any depth.
/// Besides secrets, the text of snippets, comments and commands: the journal is not sealed.
pub const SECRET_KEYS: &[&str] = &["password", "passphrase", "secret", "api_key", "account_key", "answers", "text", "data", "content", "command"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JournalEntry {
    pub caller: String,
    pub command: String,
    pub params: Value,
}

#[derive(Debug, Default)]
pub struct Journal {
    entries: VecDeque<(JournalEntry, usize)>,
    bytes: usize,
}

impl Journal {
    pub fn push(&mut self, caller: String, command: &str, params: &Value) {
        let mut params = redact(params);
        let mut size = serde_json::to_vec(&params).map(|v| v.len()).unwrap_or(MAX_ENTRY_BYTES);
        if size > MAX_ENTRY_BYTES {
            params = serde_json::json!({ "omitted": "parameters too large" });
            size = 40;
        }
        self.entries.push_back((JournalEntry { caller, command: command.to_string(), params }, size));
        self.bytes = self.bytes.saturating_add(size);
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some((_, old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(old);
        }
    }

    pub fn entries(&self) -> Vec<JournalEntry> {
        self.entries.iter().map(|(entry, _)| entry.clone()).collect()
    }
}

/// A copy of `value` with every value under a key from [`SECRET_KEYS`] replaced. Walks the
/// value iteratively, so a deep value cannot overflow the stack.
pub fn redact(value: &Value) -> Value {
    let mut out = value.clone();
    let mut stack: Vec<&mut Value> = vec![&mut out];
    while let Some(node) = stack.pop() {
        match node {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    if SECRET_KEYS.contains(&key.as_str()) {
                        *child = Value::String(REDACTED.to_string());
                    } else {
                        stack.push(child);
                    }
                }
            }
            Value::Array(items) => stack.extend(items.iter_mut()),
            _ => {}
        }
    }
    out
}

/// The nesting depth of `value`, measured iteratively; stops counting past `limit`.
pub fn depth(value: &Value, limit: usize) -> usize {
    let mut max = 0;
    let mut stack = vec![(value, 1usize)];
    while let Some((node, level)) = stack.pop() {
        max = max.max(level);
        if max > limit {
            return max;
        }
        match node {
            Value::Object(map) => stack.extend(map.values().map(|v| (v, level + 1))),
            Value::Array(items) => stack.extend(items.iter().map(|v| (v, level + 1))),
            _ => {}
        }
    }
    max
}
