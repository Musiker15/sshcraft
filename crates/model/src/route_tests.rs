use super::*;

fn group(settings: ConnectionSettings) -> Group {
    Group { id: GroupId::new(), name: "g".into(), defaults: settings, location: None, parent_id: None, tags: Vec::new() }
}

fn host(name: &str, group: Option<&Group>, settings: ConnectionSettings) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: name.into(),
        group_id: group.map(|g| g.id),
        settings,
        location: None,
        tags: Vec::new(),
        os: None,
    }
}

fn reachable(address: &str) -> ConnectionSettings {
    ConnectionSettings { address: Some(address.into()), username: Some("u".into()), ..Default::default() }
}

fn identity(username: &str, key: Option<KeyRef>, password: Option<PasswordId>) -> Identity {
    Identity { id: IdentityId::new(), name: "id".into(), username: Some(username.into()), key, password, location: None }
}

fn stored() -> KeyRef {
    KeyRef::Stored { id: KeyId::new() }
}

fn env(pairs: &[(&str, &str)]) -> Vec<EnvVar> {
    pairs.iter().map(|(n, v)| EnvVar { name: (*n).into(), value: (*v).into() }).collect()
}

#[test]
fn identity_fills_what_the_level_leaves_open() {
    let k = stored();
    let id = identity("deploy", Some(k.clone()), None);
    let h =
        host("h", None, ConnectionSettings { address: Some("a".into()), username: Some("root".into()), identity: Some(id.id), ..Default::default() });
    let r = resolve(&h, &[], std::slice::from_ref(&id)).unwrap();
    assert_eq!(r.username, "root");
    assert_eq!(r.key, Some(k));
}

#[test]
fn host_identity_beats_group_value() {
    let id = identity("b", None, None);
    let g = group(ConnectionSettings { address: Some("a".into()), username: Some("a".into()), ..Default::default() });
    let h = host("h", Some(&g), ConnectionSettings { identity: Some(id.id), ..Default::default() });
    assert_eq!(resolve(&h, &[&g], &[id]).unwrap().username, "b");
}

#[test]
fn group_identity_inherits_down() {
    let p = PasswordId::new();
    let id = identity("ops", None, Some(p));
    let top = group(ConnectionSettings { address: Some("a".into()), identity: Some(id.id), ..Default::default() });
    let mut mid = group(ConnectionSettings::default());
    mid.parent_id = Some(top.id);
    let h = host("h", Some(&mid), ConnectionSettings::default());
    let r = resolve(&h, &[&mid, &top], &[id]).unwrap();
    assert_eq!(r.username, "ops");
    assert_eq!(r.password, Some(p));
}

#[test]
fn missing_identity_is_ignored() {
    let h = host("h", None, ConnectionSettings { identity: Some(IdentityId::new()), ..reachable("a") });
    let r = resolve(&h, &[], &[]).unwrap();
    assert_eq!(r.username, "u");
    assert_eq!(r.key, None);
}

#[test]
fn no_key_no_password_resolves() {
    let r = resolve(&host("h", None, reachable("a")), &[], &[]).unwrap();
    assert_eq!((r.key, r.password), (None, None));
}

#[test]
fn proxy_direct_stops_inheritance() {
    let g = group(ConnectionSettings {
        proxy: Some(Proxy::Socks5(ProxyEndpoint { address: "p".into(), port: 1080, username: None, password: None })),
        ..reachable("a")
    });
    let mut h = host("h", Some(&g), ConnectionSettings::default());
    assert!(matches!(resolve(&h, &[&g], &[]).unwrap().proxy, Some(Proxy::Socks5(_))));
    h.settings.proxy = Some(Proxy::Direct);
    assert_eq!(resolve(&h, &[&g], &[]).unwrap().proxy, None);
}

#[test]
fn jump_hosts_empty_list_stops_inheritance() {
    let j = HostId::new();
    let g = group(ConnectionSettings { jump_hosts: Some(vec![j]), ..reachable("a") });
    let mut h = host("h", Some(&g), ConnectionSettings::default());
    assert_eq!(resolve(&h, &[&g], &[]).unwrap().jump_hosts, vec![j]);
    h.settings.jump_hosts = Some(Vec::new());
    assert!(resolve(&h, &[&g], &[]).unwrap().jump_hosts.is_empty());
}

#[test]
fn agent_forwarding_defaults_off_and_inherits() {
    let g = group(ConnectionSettings { agent_forwarding: Some(true), ..reachable("a") });
    let mut h = host("h", Some(&g), ConnectionSettings::default());
    assert!(resolve(&h, &[&g], &[]).unwrap().agent_forwarding);
    h.group_id = None;
    h.settings = reachable("a");
    assert!(!resolve(&h, &[], &[]).unwrap().agent_forwarding);
}

#[test]
fn env_merges_top_down_host_wins() {
    let g = group(ConnectionSettings { env: env(&[("A", "1"), ("B", "1")]), ..reachable("a") });
    let h = host("h", Some(&g), ConnectionSettings { env: env(&[("B", "2")]), ..Default::default() });
    assert_eq!(resolve(&h, &[&g], &[]).unwrap().env, env(&[("A", "1"), ("B", "2")]));
}

#[test]
fn normalize_env_drops_bad_names() {
    let long = "x".repeat(4097);
    let input = env(&[("1X", "a"), ("A-B", "a"), ("", "a"), ("OK", &long), ("LANG", "de_DE.UTF-8"), ("_x9", "1")]);
    assert_eq!(normalize_env(&input), env(&[("LANG", "de_DE.UTF-8"), ("_x9", "1")]));
}

#[test]
fn normalize_env_last_value_wins_at_first_place() {
    let input = env(&[("A", "1"), ("B", "1"), ("A", "2")]);
    assert_eq!(normalize_env(&input), env(&[("A", "2"), ("B", "1")]));
}

#[test]
fn normalize_env_keeps_at_most_64() {
    let input: Vec<EnvVar> = (0..70).map(|i| EnvVar { name: format!("V{i}"), value: "1".into() }).collect();
    assert_eq!(normalize_env(&input).len(), MAX_ENV);
}

struct World {
    groups: Vec<Group>,
    hosts: Vec<Host>,
    identities: Vec<Identity>,
}

impl World {
    fn dir(&self) -> Directory<'_> {
        Directory { groups: &self.groups, hosts: &self.hosts, identities: &self.identities }
    }
}

#[test]
fn route_resolves_jumps_with_their_own_credentials() {
    let p1 = PasswordId::new();
    let p2 = PasswordId::new();
    let jump = host("jump", None, ConnectionSettings { password: Some(p1), ..reachable("bastion") });
    let target = host("target", None, ConnectionSettings { password: Some(p2), jump_hosts: Some(vec![jump.id]), ..reachable("inner") });
    let w = World { groups: vec![], hosts: vec![jump.clone(), target.clone()], identities: vec![] };
    let route = resolve_route(&w.dir(), target.id).unwrap();
    assert_eq!(route.target.password, Some(p2));
    assert_eq!(route.jumps.len(), 1);
    assert_eq!(route.jumps[0].host_id, jump.id);
    assert_eq!(route.jumps[0].address, "bastion");
    assert_eq!(route.jumps[0].password, Some(p1));
}

#[test]
fn route_ignores_jump_hosts_own_proxy_and_jumps() {
    let other = host("other", None, reachable("o"));
    let jump = host(
        "jump",
        None,
        ConnectionSettings {
            jump_hosts: Some(vec![other.id]),
            proxy: Some(Proxy::Http(ProxyEndpoint { address: "p".into(), port: 3128, username: None, password: None })),
            ..reachable("bastion")
        },
    );
    let target = host("target", None, ConnectionSettings { jump_hosts: Some(vec![jump.id]), ..reachable("inner") });
    let w = World { groups: vec![], hosts: vec![other, jump, target.clone()], identities: vec![] };
    let route = resolve_route(&w.dir(), target.id).unwrap();
    assert_eq!(route.jumps.len(), 1);
    assert!(route.jumps[0].jump_hosts.is_empty());
    assert_eq!(route.jumps[0].proxy, None);
    assert_eq!(route.target.proxy, None);
}

fn chain_world(jumps: usize) -> (World, HostId) {
    let hops: Vec<Host> = (0..jumps).map(|i| host(&format!("j{i}"), None, reachable(&format!("j{i}")))).collect();
    let target = host("target", None, ConnectionSettings { jump_hosts: Some(hops.iter().map(|h| h.id).collect()), ..reachable("t") });
    let id = target.id;
    let mut hosts = hops;
    hosts.push(target);
    (World { groups: vec![], hosts, identities: vec![] }, id)
}

#[test]
fn route_rejects_target_in_chain() {
    let (mut w, id) = chain_world(1);
    let t = w.hosts.iter_mut().find(|h| h.id == id).unwrap();
    t.settings.jump_hosts.as_mut().unwrap().push(id);
    assert_eq!(resolve_route(&w.dir(), id), Err(ResolveError::JumpLoop));
}

#[test]
fn route_rejects_duplicate_hop() {
    let (mut w, id) = chain_world(2);
    let first = w.hosts[0].id;
    let t = w.hosts.iter_mut().find(|h| h.id == id).unwrap();
    t.settings.jump_hosts.as_mut().unwrap().push(first);
    assert_eq!(resolve_route(&w.dir(), id), Err(ResolveError::JumpLoop));
}

#[test]
fn route_allows_eight_jumps() {
    let (w, id) = chain_world(MAX_JUMPS);
    assert_eq!(resolve_route(&w.dir(), id).unwrap().jumps.len(), 8);
}

#[test]
fn route_rejects_nine_jumps() {
    let (w, id) = chain_world(9);
    assert_eq!(resolve_route(&w.dir(), id), Err(ResolveError::TooManyJumps));
}

#[test]
fn route_reports_deleted_jump() {
    let (mut w, id) = chain_world(1);
    w.hosts.remove(0);
    assert_eq!(resolve_route(&w.dir(), id), Err(ResolveError::JumpMissing));
}

#[test]
fn route_reports_jump_without_address() {
    let (mut w, id) = chain_world(1);
    w.hosts[0].settings.address = None;
    let jump = w.hosts[0].id;
    assert_eq!(resolve_route(&w.dir(), id), Err(ResolveError::Jump { host_id: jump, error: Box::new(ResolveError::MissingAddress) }));
}

#[test]
fn route_of_unknown_host_is_not_found() {
    let (w, _) = chain_world(0);
    assert_eq!(resolve_route(&w.dir(), HostId::new()), Err(ResolveError::NotFound));
}

#[test]
fn route_uses_identities_of_jump_hosts_groups() {
    let id = identity("hop-user", None, None);
    let g = group(ConnectionSettings { address: Some("bastion".into()), identity: Some(id.id), ..Default::default() });
    let jump = host("jump", Some(&g), ConnectionSettings::default());
    let target = host("target", None, ConnectionSettings { jump_hosts: Some(vec![jump.id]), ..reachable("t") });
    let w = World { groups: vec![g], hosts: vec![jump, target.clone()], identities: vec![id] };
    let route = resolve_route(&w.dir(), target.id).unwrap();
    assert_eq!(route.jumps[0].username, "hop-user");
}

#[test]
fn old_settings_json_reads() {
    let json = r#"{"address":"a","port":null,"username":"u","key":null}"#;
    let s: ConnectionSettings = serde_json::from_str(json).unwrap();
    assert_eq!(s.password, None);
    assert_eq!(s.jump_hosts, None);
    assert!(s.env.is_empty());
}

#[test]
fn proxy_json_shape() {
    let p = Proxy::Http(ProxyEndpoint { address: "p".into(), port: 3128, username: Some("u".into()), password: None });
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, r#"{"kind":"http","address":"p","port":3128,"username":"u","password":null}"#);
    assert_eq!(serde_json::from_str::<Proxy>(&json).unwrap(), p);
    assert_eq!(serde_json::to_string(&Proxy::Direct).unwrap(), r#"{"kind":"direct"}"#);
}

#[test]
fn flatten_resolves_identities_level_by_level() {
    // Nearer group: its own user; farther group: an identity with user and password.
    let p = PasswordId::new();
    let id = identity("far-user", None, Some(p));
    let near = ConnectionSettings { username: Some("near-user".into()), ..Default::default() };
    let far = ConnectionSettings { identity: Some(id.id), env: env(&[("A", "1")]), ..Default::default() };
    let host = ConnectionSettings { env: env(&[("B", "2")]), ..Default::default() };
    let flat = flatten(&[&host, &near, &far], &[id]);
    assert_eq!(flat.username.as_deref(), Some("near-user"));
    assert_eq!(flat.password, Some(p));
    assert_eq!(flat.identity, None);
    assert_eq!(flat.env, env(&[("A", "1"), ("B", "2")]));
    assert_eq!(flat.address, None);
}
