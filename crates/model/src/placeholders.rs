//! Placeholders in snippets: `{{name}}` asks for a value, `{{name:default}}` offers one, `{{{{`
//! stands for a literal `{{`. The values are typed as given; nothing evaluates them on this side.

use std::collections::BTreeMap;

/// Different placeholders one snippet may have.
pub const MAX_PLACEHOLDERS: usize = 16;
/// The longest placeholder name, in bytes.
pub const MAX_NAME_BYTES: usize = 32;

/// A placeholder of a snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    pub name: String,
    /// The value offered when none is given.
    pub default: Option<String>,
}

/// Why a snippet could not be filled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaceholderError {
    #[error("no value for {0}")]
    Missing(String),
}

fn name_ok(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One piece of a snippet: plain text or a placeholder.
enum Piece<'a> {
    Text(&'a str),
    Field { name: &'a str, default: Option<&'a str> },
}

/// The snippet in pieces. A `{{` that does not start a valid placeholder is plain text.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let (before, after) = rest.split_at(start);
        if !before.is_empty() {
            out.push(Piece::Text(before));
        }
        if let Some(tail) = after.strip_prefix("{{{{") {
            out.push(Piece::Text("{{"));
            rest = tail;
            continue;
        }
        let inner = after.get(2..).unwrap_or_default();
        let field = inner.find("}}").and_then(|end| {
            let body = inner.get(..end)?;
            let (name, default) = match body.split_once(':') {
                Some((name, default)) => (name, Some(default)),
                None => (body, None),
            };
            name_ok(name).then_some((name, default, end))
        });
        match field {
            Some((name, default, end)) => {
                out.push(Piece::Field { name, default });
                rest = inner.get(end + 2..).unwrap_or_default();
            }
            None => {
                out.push(Piece::Text("{{"));
                rest = inner;
            }
        }
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

/// The placeholders of `text`, in order, each name once (the first default counts), at most
/// [`MAX_PLACEHOLDERS`].
pub fn find(text: &str) -> Vec<Placeholder> {
    let mut found: Vec<Placeholder> = Vec::new();
    for piece in pieces(text) {
        if let Piece::Field { name, default } = piece
            && !found.iter().any(|p| p.name == name)
            && found.len() < MAX_PLACEHOLDERS
        {
            found.push(Placeholder { name: name.to_string(), default: default.map(str::to_string) });
        }
    }
    found
}

/// `text` with every placeholder replaced by its value, or its default when `values` has none.
pub fn fill(text: &str, values: &BTreeMap<String, String>) -> Result<String, PlaceholderError> {
    let known = find(text);
    let mut out = String::with_capacity(text.len());
    for piece in pieces(text) {
        match piece {
            Piece::Text(t) => out.push_str(t),
            Piece::Field { name, default } => {
                // A name past the limit stays as written.
                if !known.iter().any(|p| p.name == name) {
                    out.push_str("{{");
                    out.push_str(name);
                    if let Some(d) = default {
                        out.push(':');
                        out.push_str(d);
                    }
                    out.push_str("}}");
                    continue;
                }
                let first_default = known.iter().find(|p| p.name == name).and_then(|p| p.default.as_deref());
                let value = values.get(name).map(String::as_str).or(first_default).or(default);
                out.push_str(value.ok_or_else(|| PlaceholderError::Missing(name.to_string()))?);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn placeholders_are_found_once_with_their_defaults() {
        let found = find("ssh {{user:root}}@{{host}} -p {{port:22}} # {{host}}");
        assert_eq!(found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["user", "host", "port"]);
        assert_eq!(found[0].default.as_deref(), Some("root"));
        assert_eq!(found[1].default, None);
    }

    #[test]
    fn fill_uses_values_then_defaults() {
        let filled = fill("ssh {{user:root}}@{{host}} -p {{port:22}}", &values(&[("host", "db1")])).unwrap();
        assert_eq!(filled, "ssh root@db1 -p 22");
    }

    #[test]
    fn a_placeholder_without_value_or_default_is_an_error() {
        assert_eq!(fill("cd {{dir}}", &values(&[])), Err(PlaceholderError::Missing("dir".into())));
    }

    #[test]
    fn escapes_and_broken_braces_stay_text() {
        assert_eq!(
            fill("echo {{{{x}} and {{ not a field }} and {{bad name}}", &values(&[])).unwrap(),
            "echo {{x}} and {{ not a field }} and {{bad name}}"
        );
        assert!(find("{{ x }}").is_empty());
    }

    #[test]
    fn values_are_typed_as_given() {
        let filled = fill("echo {{msg}}", &values(&[("msg", "$(rm -rf /); `id`")])).unwrap();
        assert_eq!(filled, "echo $(rm -rf /); `id`");
    }

    #[test]
    fn placeholders_are_bounded() {
        let text: String = (0..30).map(|i| format!("{{{{p{i}:v}}}} ")).collect();
        assert_eq!(find(&text).len(), MAX_PLACEHOLDERS);
        let filled = fill(&text, &values(&[])).unwrap();
        assert!(filled.contains("{{p20:v}}"), "{filled}");
    }
}
