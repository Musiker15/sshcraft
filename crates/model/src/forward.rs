//! Port forwarding rules. A rule belongs to a host and travels with it (sync, shares).

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

pub const MAX_FORWARDS_PER_HOST: usize = 50;
const MAX_NAME: usize = 100;
const MAX_ADDRESS: usize = 253;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ForwardId(pub Uuid);

impl ForwardId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ForwardId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardKind {
    /// `-L`: a port here reaches a target as seen from the server.
    Local,
    /// `-R`: a port on the server reaches a target as seen from here.
    Remote,
    /// `-D`: a SOCKS5 proxy here; every connection picks its own target.
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardRule {
    pub id: ForwardId,
    #[serde(default)]
    pub name: String,
    pub kind: ForwardKind,
    pub bind_address: String,
    pub bind_port: u16,
    /// Empty for [`ForwardKind::Dynamic`].
    #[serde(default)]
    pub target_host: String,
    /// 0 for [`ForwardKind::Dynamic`].
    #[serde(default)]
    pub target_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ForwardError {
    #[error("invalid forwarding rule: {0}")]
    Invalid(&'static str),
    #[error("more than {MAX_FORWARDS_PER_HOST} forwarding rules")]
    TooMany,
    #[error("two forwarding rules share an id")]
    DuplicateId,
}

fn address_ok(address: &str) -> bool {
    !address.is_empty()
        && address.chars().count() <= MAX_ADDRESS
        && !address.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn validate(rule: &ForwardRule) -> Result<(), ForwardError> {
    if rule.name.chars().count() > MAX_NAME || rule.name.chars().any(char::is_control) {
        return Err(ForwardError::Invalid("name"));
    }
    if !address_ok(&rule.bind_address) || rule.bind_port == 0 {
        return Err(ForwardError::Invalid("bind"));
    }
    match rule.kind {
        ForwardKind::Dynamic if !rule.target_host.is_empty() || rule.target_port != 0 => {
            Err(ForwardError::Invalid("target"))
        }
        ForwardKind::Dynamic => Ok(()),
        _ if !address_ok(&rule.target_host) || rule.target_port == 0 => {
            Err(ForwardError::Invalid("target"))
        }
        _ => Ok(()),
    }
}

pub fn validate_all(rules: &[ForwardRule]) -> Result<(), ForwardError> {
    if rules.len() > MAX_FORWARDS_PER_HOST {
        return Err(ForwardError::TooMany);
    }
    let mut seen = std::collections::HashSet::new();
    for rule in rules {
        validate(rule)?;
        if !seen.insert(rule.id) {
            return Err(ForwardError::DuplicateId);
        }
    }
    Ok(())
}

/// Reads rules one by one and keeps the valid ones: a broken rule (from an older or newer
/// version, or from another device) never takes its host with it.
pub fn lenient_rules<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ForwardRule>, D::Error> {
    let values = Option::<Vec<serde_json::Value>>::deserialize(d)?.unwrap_or_default();
    Ok(parse_rules(values))
}

/// Forwarding rules from stored JSON values; a value that is no valid rule is skipped.
pub fn parse_rules(values: Vec<serde_json::Value>) -> Vec<ForwardRule> {
    let mut rules: Vec<ForwardRule> = Vec::new();
    for value in values {
        let Ok(rule) = serde_json::from_value::<ForwardRule>(value) else {
            continue;
        };
        if validate(&rule).is_ok()
            && rules.len() < MAX_FORWARDS_PER_HOST
            && !rules.iter().any(|r| r.id == rule.id)
        {
            rules.push(rule);
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: ForwardKind) -> ForwardRule {
        let (target_host, target_port) = match kind {
            ForwardKind::Dynamic => (String::new(), 0),
            _ => ("db.internal".to_owned(), 5432),
        };
        ForwardRule {
            id: ForwardId::new(),
            name: "Datenbank".into(),
            kind,
            bind_address: "127.0.0.1".into(),
            bind_port: 15432,
            target_host,
            target_port,
        }
    }

    fn invalid(r: ForwardRule) -> bool {
        matches!(validate(&r), Err(ForwardError::Invalid(_)))
    }

    #[test]
    fn valid_rules_of_every_kind() {
        for kind in [
            ForwardKind::Local,
            ForwardKind::Remote,
            ForwardKind::Dynamic,
        ] {
            assert_eq!(validate(&rule(kind)), Ok(()), "{kind:?}");
        }
    }

    #[test]
    fn bad_rules_are_refused() {
        let mut r = rule(ForwardKind::Local);
        r.bind_port = 0;
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.target_port = 0;
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.target_host = String::new();
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.bind_address = "127.0.0.1 ".into();
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Remote);
        r.target_host = "a\nb".into();
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.bind_address = String::new();
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.target_host = "x".repeat(254);
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Local);
        r.name = "n".repeat(101);
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Dynamic);
        r.target_host = "db".into();
        assert!(invalid(r));
        let mut r = rule(ForwardKind::Dynamic);
        r.target_port = 22;
        assert!(invalid(r));
    }

    #[test]
    fn rule_lists_are_limited_and_ids_unique() {
        let many: Vec<_> = (0..51).map(|_| rule(ForwardKind::Local)).collect();
        assert_eq!(validate_all(&many), Err(ForwardError::TooMany));
        assert_eq!(validate_all(&many[..50]), Ok(()));
        let twice = vec![rule(ForwardKind::Local), rule(ForwardKind::Local)];
        let mut same = twice.clone();
        same[1].id = same[0].id;
        assert_eq!(validate_all(&same), Err(ForwardError::DuplicateId));
        let mut broken = twice;
        broken[1].bind_port = 0;
        assert!(matches!(
            validate_all(&broken),
            Err(ForwardError::Invalid(_))
        ));
    }

    #[test]
    fn broken_rules_are_left_out_when_reading() {
        let good = rule(ForwardKind::Local);
        let mut bad = serde_json::to_value(rule(ForwardKind::Local)).unwrap();
        bad["bind_port"] = serde_json::json!(70000);
        let mut zero = serde_json::to_value(rule(ForwardKind::Remote)).unwrap();
        zero["bind_port"] = serde_json::json!(0);
        let values = vec![
            serde_json::to_value(&good).unwrap(),
            bad,
            zero,
            serde_json::json!("x"),
        ];
        assert_eq!(parse_rules(values), vec![good]);
    }
}
