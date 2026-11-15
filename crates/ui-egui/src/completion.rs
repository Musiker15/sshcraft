//! Suggestions while typing at a shell prompt: the pane's history, then snippets, then paths
//! (`session.list_dir`, cached 10 s). No AI. A suggestion never takes a key by itself: Down
//! selects, Tab or Enter take the selected one (typing only the missing rest, never running it),
//! Escape closes the list.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::data::SnippetRow;

/// Characters typed before anything is suggested.
pub const MIN_CHARS: usize = 2;
/// Suggestions shown at most.
pub const MAX_SUGGESTIONS: usize = 8;
/// How long a directory listing is used.
pub const PATH_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// What the list shows.
    pub label: String,
    /// The bytes taking it types: the missing rest, or Backspaces and a snippet.
    pub insert: Vec<u8>,
    /// `history`, `snippet` or `path`.
    pub kind: &'static str,
}

/// The suggestions for `line`, most useful first. `bracketed`: the shell takes pastes as such
/// (a snippet of several lines is offered only then).
pub fn suggest(line: &str, history: &[String], snippets: &[SnippetRow], paths: &[String], bracketed: bool) -> Vec<Suggestion> {
    if line.chars().count() < MIN_CHARS || line.ends_with('\n') {
        return Vec::new();
    }
    let mut out: Vec<Suggestion> = Vec::new();
    let push = |s: Suggestion, out: &mut Vec<Suggestion>| {
        if !out.iter().any(|o| o.label == s.label) && out.len() < MAX_SUGGESTIONS {
            out.push(s);
        }
    };
    for entry in history {
        if let Some(rest) = entry.strip_prefix(line).filter(|r| !r.is_empty()) {
            push(Suggestion { label: entry.clone(), insert: rest.as_bytes().to_vec(), kind: "history" }, &mut out);
        }
    }
    let lower = line.to_lowercase();
    for s in snippets {
        if let Some(rest) = s.content.strip_prefix(line).filter(|r| !r.is_empty() && !s.content.contains('\n')) {
            push(Suggestion { label: s.content.clone(), insert: rest.as_bytes().to_vec(), kind: "snippet" }, &mut out);
        } else if s.name.to_lowercase().starts_with(&lower) && (bracketed || !s.content.contains(['\n', '\r'])) {
            // Found by its name: the line goes, the snippet comes as a paste, so nothing runs.
            let mut insert = vec![0x7f; line.chars().count()];
            insert.extend(crate::pages::snippets::paste_bytes(&s.content, bracketed, false));
            push(Suggestion { label: s.name.clone(), insert, kind: "snippet" }, &mut out);
        }
    }
    if let Some((dir, prefix)) = path_word(line) {
        for name in paths {
            if let Some(rest) = name.strip_prefix(&prefix).filter(|r| !r.is_empty()) {
                push(Suggestion { label: format!("{dir}{name}"), insert: rest.as_bytes().to_vec(), kind: "path" }, &mut out);
            }
        }
    }
    out
}

/// The last word of `line` when it names a path: its directory (up to the last `/`) and the
/// start of the name. A path starts with `/`, `~`, `./` or `../`, or has a `/` in it.
pub fn path_word(line: &str) -> Option<(String, String)> {
    if line.ends_with(char::is_whitespace) {
        return None;
    }
    let word = line.rsplit(char::is_whitespace).next()?;
    let slash = word.rfind('/')?;
    let (dir, prefix) = word.split_at(slash + 1);
    Some((dir.to_string(), prefix.to_string()))
}

/// Directory listings of one pane, each used for [`PATH_TTL`].
#[derive(Debug, Clone, Default)]
pub struct PathCache {
    listings: HashMap<String, (Instant, Vec<String>)>,
}

impl PathCache {
    pub fn put(&mut self, dir: &str, names: Vec<String>, now: Instant) {
        // An old cache goes before it grows.
        self.listings.retain(|_, (at, _)| now.duration_since(*at) < PATH_TTL);
        if self.listings.len() < 64 {
            self.listings.insert(dir.to_string(), (now, names));
        }
    }

    pub fn get(&self, dir: &str, now: Instant) -> Option<&[String]> {
        self.listings.get(dir).filter(|(at, _)| now.duration_since(*at) < PATH_TTL).map(|(_, names)| names.as_slice())
    }
}
