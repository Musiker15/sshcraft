use super::*;
use crate::model::KeyRef;

fn host<'a>(parsed: &'a ParsedConfig, alias: &str) -> &'a ConfigHost {
    parsed
        .hosts
        .iter()
        .find(|h| h.alias == alias)
        .unwrap_or_else(|| panic!("no host {alias}"))
}

#[test]
fn parses_basic_host_block() {
    let parsed = parse(
        "Host web\n  HostName example.org\n  Port 2222\n  User deploy\n  IdentityFile ~/.ssh/web\n",
    );
    let h = host(&parsed, "web");
    assert_eq!(h.hostname.as_deref(), Some("example.org"));
    assert_eq!(h.port, Some(2222));
    assert_eq!(h.user.as_deref(), Some("deploy"));
    assert_eq!(h.identity_files, vec!["~/.ssh/web".to_owned()]);
}

#[test]
fn first_value_wins_and_star_block_fills_gaps() {
    let text = "Host a\n  User root\n  User ignored\nHost *\n  User fallback\n  Port 2222\n";
    let parsed = parse(text);
    let a = host(&parsed, "a");
    assert_eq!(a.user.as_deref(), Some("root"));
    assert_eq!(a.port, Some(2222));
    assert_eq!(
        parsed.hosts.len(),
        1,
        "the * block is not a host of its own"
    );
}

#[test]
fn equals_sign_quotes_bom_and_crlf() {
    let text = "\u{feff}Host q\r\n  HostName=example.org\r\n  IdentityFile \"C:\\Users\\Max Mustermann\\.ssh\\id key.pub\"\r\n  USER = admin\r\n";
    let parsed = parse(text);
    let q = host(&parsed, "q");
    assert_eq!(q.hostname.as_deref(), Some("example.org"));
    assert_eq!(q.user.as_deref(), Some("admin"));
    assert_eq!(
        q.identity_files,
        vec!["C:\\Users\\Max Mustermann\\.ssh\\id key.pub".to_owned()]
    );
}

#[test]
fn wildcard_and_negated_aliases_are_not_imported() {
    let parsed = parse("Host *.example.org !bad.example.org good\n  User x\nHost db?\n  User y\n");
    let aliases: Vec<_> = parsed.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert_eq!(aliases, vec!["good"]);
}

#[test]
fn pattern_blocks_apply_to_matching_aliases_only() {
    let text = "Host app.example.org other\nHost *.example.org !skip.example.org\n  User team\nHost skip.example.org\n";
    let parsed = parse(text);
    assert_eq!(
        host(&parsed, "app.example.org").user.as_deref(),
        Some("team")
    );
    assert_eq!(host(&parsed, "other").user, None);
    assert_eq!(host(&parsed, "skip.example.org").user, None);
}

#[test]
fn include_and_match_are_skipped_and_reported() {
    let text = "Include ~/.ssh/config.d/*\nHost a\n  User u\nMatch host b\n  User fromMatch\nHost c\n  User v\n";
    let parsed = parse(text);
    assert_eq!(host(&parsed, "a").user.as_deref(), Some("u"));
    assert_eq!(host(&parsed, "c").user.as_deref(), Some("v"));
    assert_eq!(parsed.skipped.len(), 2);
    assert!(parsed.skipped[0].starts_with("Include"));
    assert!(parsed.skipped[1].starts_with("Match"));
}

#[test]
fn multiple_aliases_on_one_host_line() {
    let parsed = parse("Host a b\n  HostName example.org\n");
    assert_eq!(host(&parsed, "a").hostname.as_deref(), Some("example.org"));
    assert_eq!(host(&parsed, "b").hostname.as_deref(), Some("example.org"));
}

#[test]
fn garbage_does_not_panic() {
    let parsed = parse("Host\nPort\nHost x\n  Port abc\n  =\n  \"unterminated\n\u{0}\u{1}\n");
    assert_eq!(host(&parsed, "x").port, None);
}

fn fingerprints(path: &str) -> Option<String> {
    match path {
        "~/.ssh/proton.pub" => Some("SHA256:proton".into()),
        "~/.ssh/local" => Some("SHA256:local".into()),
        _ => None,
    }
}

#[test]
fn identity_file_with_pub_maps_to_agent_key() {
    let parsed = parse(
        "Host srv\n  HostName example.org\n  IdentityFile ~/.ssh/proton.pub\n  IdentitiesOnly yes\n",
    );
    let plan = plan_import(&parsed.hosts, &fingerprints, &["SHA256:proton".to_owned()]);
    assert_eq!(
        plan[0].settings.key,
        Some(KeyRef::Agent {
            fingerprint: "SHA256:proton".into()
        })
    );
    assert_eq!(plan[0].key_note, None);
}

#[test]
fn unknown_key_gets_a_note() {
    let parsed = parse("Host srv\n  HostName example.org\n  IdentityFile ~/.ssh/local\n");
    let plan = plan_import(&parsed.hosts, &fingerprints, &["SHA256:proton".to_owned()]);
    assert_eq!(plan[0].settings.key, None);
    assert!(plan[0].key_note.is_some());
}

#[test]
fn same_hostname_becomes_one_group() {
    let text = "Host srv-root\n  HostName example.org\n  User root\nHost srv-deploy\n  HostName example.org\n  User deploy\nHost single\n  HostName other.org\n";
    let plan = plan_import(&parse(text).hosts, &fingerprints, &[]);
    let root = plan.iter().find(|c| c.alias == "srv-root").unwrap();
    assert_eq!(root.group.as_deref(), Some("example.org"));
    assert_eq!(root.settings.address, None, "address comes from the group");
    assert_eq!(root.settings.username.as_deref(), Some("root"));
    let single = plan.iter().find(|c| c.alias == "single").unwrap();
    assert_eq!(single.group, None);
    assert_eq!(single.settings.address.as_deref(), Some("other.org"));
}

#[test]
fn alias_without_hostname_uses_alias_as_address() {
    let plan = plan_import(
        &parse("Host example.net\n  User u\n").hosts,
        &fingerprints,
        &[],
    );
    assert_eq!(plan[0].settings.address.as_deref(), Some("example.net"));
}

fn candidate(alias: &str, group: Option<&str>, address: Option<&str>) -> ImportCandidate {
    ImportCandidate {
        alias: alias.into(),
        group: group.map(Into::into),
        settings: crate::model::ConnectionSettings {
            address: address.map(Into::into),
            username: Some("u".into()),
            ..Default::default()
        },
        key_note: None,
    }
}

#[test]
fn apply_import_creates_exactly_the_given_candidates() {
    let store = crate::store::Store::open_in_memory().unwrap();
    let created = apply_import(
        &store,
        &[
            candidate("a", Some("example.org"), None),
            candidate("b", Some("example.org"), None),
            candidate("c", None, Some("other.org")),
        ],
    )
    .unwrap();
    assert_eq!(created, 3);
    let groups = store.list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].defaults.address.as_deref(), Some("example.org"));
    let hosts = store.list_hosts().unwrap();
    assert_eq!(hosts.len(), 3);
    let a = hosts.iter().find(|h| h.name == "a").unwrap();
    assert_eq!(a.group_id, Some(groups[0].id));
    assert_eq!(a.settings.username.as_deref(), Some("u"));
}

#[test]
fn apply_import_skips_existing_hosts_and_reuses_groups() {
    let store = crate::store::Store::open_in_memory().unwrap();
    apply_import(&store, &[candidate("a", Some("example.org"), None)]).unwrap();
    let created = apply_import(
        &store,
        &[
            candidate("a", Some("example.org"), None),
            candidate("d", Some("example.org"), None),
        ],
    )
    .unwrap();
    assert_eq!(created, 1);
    assert_eq!(store.list_groups().unwrap().len(), 1);
    assert_eq!(store.list_hosts().unwrap().len(), 2);
}
