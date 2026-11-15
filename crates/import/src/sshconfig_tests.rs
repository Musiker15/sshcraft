use super::*;
use sshcraft_model::model::KeyRef;

fn host<'a>(parsed: &'a ParsedConfig, alias: &str) -> &'a ConfigHost {
    parsed.hosts.iter().find(|h| h.alias == alias).unwrap_or_else(|| panic!("no host {alias}"))
}

#[test]
fn parses_basic_host_block() {
    let parsed = parse("Host web\n  HostName example.org\n  Port 2222\n  User deploy\n  IdentityFile ~/.ssh/web\n");
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
    assert_eq!(parsed.hosts.len(), 1, "the * block is not a host of its own");
}

#[test]
fn equals_sign_quotes_bom_and_crlf() {
    let text = "\u{feff}Host q\r\n  HostName=example.org\r\n  IdentityFile \"C:\\Users\\Max Mustermann\\.ssh\\id key.pub\"\r\n  USER = admin\r\n";
    let parsed = parse(text);
    let q = host(&parsed, "q");
    assert_eq!(q.hostname.as_deref(), Some("example.org"));
    assert_eq!(q.user.as_deref(), Some("admin"));
    assert_eq!(q.identity_files, vec!["C:\\Users\\Max Mustermann\\.ssh\\id key.pub".to_owned()]);
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
    assert_eq!(host(&parsed, "app.example.org").user.as_deref(), Some("team"));
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
    let parsed = parse("Host srv\n  HostName example.org\n  IdentityFile ~/.ssh/proton.pub\n  IdentitiesOnly yes\n");
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&["SHA256:proton".to_owned()]));
    assert_eq!(plan[0].settings.key, Some(KeyRef::Agent { fingerprint: "SHA256:proton".into() }));
    assert_eq!(plan[0].key_note, None);
}

#[test]
fn unknown_key_gets_a_note() {
    let parsed = parse("Host srv\n  HostName example.org\n  IdentityFile ~/.ssh/local\n");
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&["SHA256:proton".to_owned()]));
    assert_eq!(plan[0].settings.key, None);
    assert_eq!(plan[0].key_note, Some(KeyNote::NotInAgent));
}

#[test]
fn same_hostname_becomes_one_group() {
    let text = "Host srv-root\n  HostName example.org\n  User root\nHost srv-deploy\n  HostName example.org\n  User deploy\nHost single\n  HostName other.org\n";
    let plan = plan_import(&parse(text).hosts, &fingerprints, Some(&[]));
    let root = plan.iter().find(|c| c.name == "srv-root").unwrap();
    assert_eq!(root.group_path, vec!["example.org".to_owned()]);
    assert_eq!(root.settings.address, None, "address comes from the group");
    assert_eq!(root.settings.username.as_deref(), Some("root"));
    let single = plan.iter().find(|c| c.name == "single").unwrap();
    assert!(single.group_path.is_empty());
    assert_eq!(single.settings.address.as_deref(), Some("other.org"));
}

#[test]
fn alias_without_hostname_uses_alias_as_address() {
    let plan = plan_import(&parse("Host example.net\n  User u\n").hosts, &fingerprints, Some(&[]));
    assert_eq!(plan[0].settings.address.as_deref(), Some("example.net"));
}

fn candidate(alias: &str, group: Option<&str>, address: Option<&str>) -> ImportCandidate {
    ImportCandidate {
        name: alias.into(),
        group_path: group.map(Into::into).into_iter().collect(),
        group_defaults: sshcraft_model::model::ConnectionSettings { address: group.map(Into::into), ..Default::default() },
        settings: sshcraft_model::model::ConnectionSettings { address: address.map(Into::into), username: Some("u".into()), ..Default::default() },
        ..ImportCandidate::default()
    }
}

fn apply_import(store: &sshcraft_store::store::Store, candidates: &[ImportCandidate]) -> Result<usize, sshcraft_store::store::StoreError> {
    crate::import::apply_import(store, candidates, None).map(|o| o.created)
}

#[test]
fn apply_import_creates_exactly_the_given_candidates() {
    let store = sshcraft_store::store::Store::open_in_memory().unwrap();
    let created = apply_import(
        &store,
        &[candidate("a", Some("example.org"), None), candidate("b", Some("example.org"), None), candidate("c", None, Some("other.org"))],
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
    let store = sshcraft_store::store::Store::open_in_memory().unwrap();
    apply_import(&store, &[candidate("a", Some("example.org"), None)]).unwrap();
    let created = apply_import(&store, &[candidate("a", Some("example.org"), None), candidate("d", Some("example.org"), None)]).unwrap();
    assert_eq!(created, 1);
    assert_eq!(store.list_groups().unwrap().len(), 1);
    assert_eq!(store.list_hosts().unwrap().len(), 2);
}

#[test]
fn preview_without_agent_says_agent_unavailable() {
    let parsed = parse(
        "Host a
  HostName a.org
  IdentityFile ~/.ssh/proton.pub
Host b
  HostName b.org
",
    );
    let plan = plan_import(&parsed.hosts, &fingerprints, None);
    assert_eq!(plan[0].key_note, Some(KeyNote::AgentUnavailable));
    assert_eq!(plan[1].key_note, Some(KeyNote::NoIdentityFile));
}

#[test]
fn proxy_jump_is_warned_and_proxy_command_taken_over() {
    let parsed = parse(
        "Host a
  HostName a.org
  ProxyJump bastion
Host b
  HostName b.org
  ProxyCommand ssh -W %h:%p gw
Host c
  HostName c.org
",
    );
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&[]));
    assert_eq!(plan[0].warnings, vec![ImportWarning::ProxyIgnored { directive: "ProxyJump bastion".into() }]);
    // A ProxyCommand without shell syntax is taken over (F1); see proxy_command_is_imported_as_arguments.
    assert!(plan[1].warnings.is_empty());
    assert!(matches!(plan[1].settings.proxy, Some(sshcraft_model::model::Proxy::Command { .. })));
    assert!(plan[2].warnings.is_empty());
}

#[test]
fn percent_h_in_hostname_is_the_alias() {
    let parsed = parse(
        "Host web
  HostName %h.example.org
Host pct
  HostName a%%b.org
",
    );
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&[]));
    assert_eq!(plan[0].settings.address.as_deref(), Some("web.example.org"));
    assert!(plan[0].warnings.is_empty());
    assert_eq!(plan[1].settings.address.as_deref(), Some("a%b.org"));
}

#[test]
fn other_tokens_are_warned() {
    let parsed = parse(
        "Host x
  HostName %r.x
",
    );
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&[]));
    assert_eq!(plan[0].settings.address.as_deref(), Some("%r.x"));
    assert_eq!(plan[0].warnings, vec![ImportWarning::UnsupportedToken { token: "%r".into() }]);
}

#[test]
fn candidate_without_warnings_field_still_deserializes() {
    let json = r#"{"name":"a","settings":{"address":"a.org","port":null,"username":null,"key":null},"key_note":null}"#;
    let c: ImportCandidate = serde_json::from_str(json).unwrap();
    assert!(c.warnings.is_empty());
    assert!(c.group_path.is_empty() && c.tags.is_empty());
}

#[test]
fn proxy_none_is_not_a_proxy() {
    let parsed = parse("Host a\n  HostName a.org\n  ProxyCommand none\nHost b\n  HostName b.org\n  ProxyJump NONE\nHost *\n  ProxyJump bastion\n");
    let plan = plan_import(&parsed.hosts, &fingerprints, Some(&[]));
    assert!(plan[0].warnings.is_empty(), "{:?}", plan[0].warnings);
    assert!(plan[1].warnings.is_empty(), "{:?}", plan[1].warnings);
}

fn imported(text: &str) -> sshcraft_store::store::Store {
    let parsed = parse(text);
    let plan = plan_import(&parsed.hosts, &|_| None, Some(&[]));
    let store = sshcraft_store::store::Store::open_in_memory().unwrap();
    crate::import::apply_import(&store, &plan, None).unwrap();
    store
}

fn stored<'a>(hosts: &'a [sshcraft_model::model::Host], name: &str) -> &'a sshcraft_model::model::Host {
    hosts.iter().find(|h| h.name == name).unwrap()
}

#[test]
fn proxyjump_to_imported_alias_sets_jump_hosts() {
    let text = "Host bastion\n  HostName gw.example.org\nHost inner\n  HostName 10.0.0.5\n  ProxyJump bastion\n";
    let plan = plan_import(&parse(text).hosts, &|_| None, Some(&[]));
    let inner = plan.iter().find(|c| c.name == "inner").unwrap();
    assert!(inner.warnings.is_empty(), "{:?}", inner.warnings);
    let store = imported(text);
    let hosts = store.list_hosts().unwrap();
    let bastion = stored(&hosts, "bastion");
    assert_eq!(stored(&hosts, "inner").settings.jump_hosts, Some(vec![bastion.id]));
    assert_eq!(bastion.settings.jump_hosts, None);
}

#[test]
fn proxyjump_chain_keeps_its_order() {
    let text = "Host a\n  HostName a.org\nHost b\n  HostName b.org\nHost t\n  HostName t.org\n  ProxyJump b,a\n";
    let store = imported(text);
    let hosts = store.list_hosts().unwrap();
    assert_eq!(stored(&hosts, "t").settings.jump_hosts, Some(vec![stored(&hosts, "b").id, stored(&hosts, "a").id]));
}

#[test]
fn proxyjump_to_unknown_keeps_warning() {
    let text = "Host inner\n  HostName 10.0.0.5\n  ProxyJump admin@gw.example.org:2222\n";
    let plan = plan_import(&parse(text).hosts, &|_| None, Some(&[]));
    assert_eq!(plan[0].warnings, vec![ImportWarning::ProxyIgnored { directive: "ProxyJump admin@gw.example.org:2222".into() }]);
    let store = imported(text);
    assert_eq!(store.list_hosts().unwrap()[0].settings.jump_hosts, None);
}

#[test]
fn forwardagent_and_setenv_imported() {
    let text = "Host web\n  HostName web.org\n  ForwardAgent yes\n  SetEnv LANG=de_DE.UTF-8 MOTTO=\"hallo welt\"\nHost db\n  HostName db.org\n  ForwardAgent no\n";
    let store = imported(text);
    let hosts = store.list_hosts().unwrap();
    let web = stored(&hosts, "web");
    assert_eq!(web.settings.agent_forwarding, Some(true));
    let env: Vec<(String, String)> = web.settings.env.iter().map(|v| (v.name.clone(), v.value.clone())).collect();
    assert_eq!(env, vec![("LANG".to_owned(), "de_DE.UTF-8".to_owned()), ("MOTTO".to_owned(), "hallo welt".to_owned()),]);
    assert_eq!(stored(&hosts, "db").settings.agent_forwarding, Some(false));
}

// ---------- Include, and ProxyCommand as arguments

/// Files in memory; `glob` matches `*` in the file name only, as `RealFs` does.
#[derive(Default)]
struct MemFs(std::collections::BTreeMap<std::path::PathBuf, String>);

impl MemFs {
    fn with(mut self, path: &str, text: &str) -> Self {
        self.0.insert(std::path::PathBuf::from(path), text.to_owned());
        self
    }
}

impl ConfigFs for MemFs {
    fn read(&self, path: &std::path::Path) -> Option<String> {
        self.0.get(path).cloned()
    }
    fn glob(&self, pattern: &std::path::Path) -> Vec<std::path::PathBuf> {
        let dir = pattern.parent().unwrap_or(std::path::Path::new(""));
        let name = pattern.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        self.0
            .keys()
            .filter(|p| p.parent() == Some(dir) && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| glob_matches(name, n)))
            .cloned()
            .collect()
    }
}

fn tree(fs: &MemFs) -> ParsedConfig {
    parse_tree(fs, std::path::Path::new("/home/u/.ssh/config"), std::path::Path::new("/home/u"))
}

#[test]
fn include_reads_relative_and_globbed_files() {
    let fs = MemFs::default()
        .with("/home/u/.ssh/config", "Include conf.d/*.conf ~/extra\nHost main\n  HostName main.org\n")
        .with("/home/u/.ssh/conf.d/b.conf", "Host b\n  HostName b.org\n")
        .with("/home/u/.ssh/conf.d/a.conf", "Host a\n  HostName a.org\n")
        .with("/home/u/.ssh/conf.d/skip.txt", "Host no\n")
        .with("/home/u/extra", "Host x\n  HostName x.org\n");
    let parsed = tree(&fs);
    let aliases: Vec<&str> = parsed.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert_eq!(aliases, vec!["a", "b", "x", "main"], "files in sorted order, where the Include stood");
    assert_eq!(parsed.files.len(), 4);
    assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
}

#[test]
fn include_inside_host_applies_to_that_host() {
    let fs =
        MemFs::default().with("/home/u/.ssh/config", "Host web\n  Include web.conf\n  Port 2222\n").with("/home/u/.ssh/web.conf", "User deploy\n");
    let parsed = tree(&fs);
    let web = host(&parsed, "web");
    assert_eq!(web.user.as_deref(), Some("deploy"));
    assert_eq!(web.port, Some(2222));
}

#[test]
fn include_cycle_stops() {
    let fs = MemFs::default().with("/home/u/.ssh/config", "Include config\nHost a\n");
    let parsed = tree(&fs);
    assert!(parsed.skipped.iter().any(|s| s.contains("too deep")), "{:?}", parsed.skipped);
    assert!(parsed.files.len() <= MAX_INCLUDE_DEPTH + 1);
}

#[test]
fn too_many_files_stop_with_a_warning() {
    let mut fs = MemFs::default().with("/home/u/.ssh/config", "Include d/*\n");
    for i in 0..(MAX_CONFIG_FILES + 5) {
        fs = fs.with(&format!("/home/u/.ssh/d/{i:04}"), &format!("Host h{i}\n"));
    }
    let parsed = tree(&fs);
    assert_eq!(parsed.files.len(), MAX_CONFIG_FILES);
    assert!(parsed.skipped.iter().any(|s| s.contains("too many")), "{:?}", parsed.skipped);
}

#[test]
fn a_missing_include_is_reported() {
    let fs = MemFs::default().with("/home/u/.ssh/config", "Include gone.conf\nHost a\n");
    let parsed = tree(&fs);
    assert_eq!(parsed.hosts.len(), 1);
    assert!(parsed.skipped.iter().any(|s| s.contains("gone.conf")), "{:?}", parsed.skipped);
}

#[test]
fn proxy_command_is_imported_as_arguments() {
    let parsed = parse("Host inner\n  HostName 10.0.0.5\n  ProxyCommand ssh -W %h:%p 'bastion host'\n");
    let plan = plan_import(&parsed.hosts, &|_| None, Some(&[]));
    assert_eq!(
        plan[0].settings.proxy,
        Some(sshcraft_model::model::Proxy::Command { argv: vec!["ssh".into(), "-W".into(), "%h:%p".into(), "bastion host".into()] })
    );
    assert!(plan[0].warnings.is_empty(), "{:?}", plan[0].warnings);
}

#[test]
fn shell_proxy_command_stays_a_warning() {
    let parsed = parse("Host inner\n  ProxyCommand nc %h %p | tee log\n");
    let plan = plan_import(&parsed.hosts, &|_| None, Some(&[]));
    assert_eq!(plan[0].settings.proxy, None);
    assert_eq!(plan[0].warnings, vec![ImportWarning::ProxyIgnored { directive: "ProxyCommand nc %h %p | tee log".into() }]);
}

#[test]
fn a_glob_skips_dot_files_like_glob3() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.conf", ".a.conf.sshcraft-tmp", ".hidden"] {
        std::fs::write(dir.path().join(name), "Host x\n").unwrap();
    }
    let found = RealFs.glob(&dir.path().join("*"));
    assert_eq!(found, vec![dir.path().join("a.conf")]);
    assert_eq!(RealFs.glob(&dir.path().join(".h*")), vec![dir.path().join(".hidden")]);
}
