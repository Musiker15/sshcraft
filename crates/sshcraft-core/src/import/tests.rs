use super::*;
use crate::store::Store;

fn host(name: &str, path: &[&str], address: Option<&str>) -> ImportCandidate {
    ImportCandidate {
        name: name.into(),
        group_path: path.iter().map(|s| s.to_string()).collect(),
        settings: ConnectionSettings {
            address: address.map(Into::into),
            username: Some("u".into()),
            ..Default::default()
        },
        ..ImportCandidate::default()
    }
}

#[test]
fn group_paths_create_nested_groups() {
    let store = Store::open_in_memory().unwrap();
    let outcome = apply_import(
        &store,
        &[
            host("a", &["Kunden", "Müller"], Some("a.example.org")),
            host("b", &["Kunden", "Schmidt"], Some("b.example.org")),
            host("c", &[], Some("c.example.org")),
        ],
        None,
    )
    .unwrap();
    assert_eq!(outcome.created, 3);
    let groups = store.list_groups().unwrap();
    assert_eq!(groups.len(), 3);
    let kunden = groups.iter().find(|g| g.name == "Kunden").unwrap();
    let mueller = groups.iter().find(|g| g.name == "Müller").unwrap();
    assert_eq!(mueller.parent_id, Some(kunden.id));
    let a = store
        .list_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.name == "a")
        .unwrap();
    assert_eq!(a.group_id, Some(mueller.id));
}

#[test]
fn existing_groups_are_reused() {
    let store = Store::open_in_memory().unwrap();
    apply_import(
        &store,
        &[host("a", &["Kunden", "Müller"], Some("a.org"))],
        None,
    )
    .unwrap();
    apply_import(
        &store,
        &[host("b", &["Kunden", "Müller"], Some("b.org"))],
        None,
    )
    .unwrap();
    assert_eq!(store.list_groups().unwrap().len(), 2);
    // The same name on another level is another group.
    apply_import(&store, &[host("c", &["Müller"], Some("c.org"))], None).unwrap();
    assert_eq!(store.list_groups().unwrap().len(), 3);
}

#[test]
fn same_host_twice_is_one_host() {
    let store = Store::open_in_memory().unwrap();
    let first = apply_import(&store, &[host("a", &["G"], Some("a.org"))], None).unwrap();
    assert_eq!((first.created, first.duplicates), (1, 0));
    let again = apply_import(
        &store,
        &[
            host("a", &["G"], Some("a.org")),
            host("a", &["G"], Some("a.org")),
        ],
        None,
    )
    .unwrap();
    assert_eq!((again.created, again.duplicates), (0, 2));
    assert_eq!(store.list_hosts().unwrap().len(), 1);
    // Another user on the same address is another host.
    let mut other = host("a", &["G"], Some("a.org"));
    other.settings.username = Some("root".into());
    assert_eq!(apply_import(&store, &[other], None).unwrap().created, 1);
}

#[test]
fn inherited_address_counts_for_duplicates() {
    let store = Store::open_in_memory().unwrap();
    let mut grouped = host("a", &["a.org"], None);
    grouped.group_defaults.address = Some("a.org".into());
    apply_import(&store, std::slice::from_ref(&grouped), None).unwrap();
    assert_eq!(
        apply_import(&store, &[grouped], None).unwrap().duplicates,
        1
    );
    let groups = store.list_groups().unwrap();
    assert_eq!(groups[0].defaults.address.as_deref(), Some("a.org"));
}

#[test]
fn bad_port_is_dropped_with_a_warning() {
    let mut warnings = Vec::new();
    assert_eq!(port_from("2222", &mut warnings), Some(2222));
    assert_eq!(port_from("0", &mut warnings), None);
    assert_eq!(port_from("99999", &mut warnings), None);
    assert_eq!(port_from("ssh", &mut warnings), None);
    assert_eq!(port_from("", &mut warnings), None);
    assert_eq!(warnings, vec![ImportWarning::BadPort; 3]);
}

#[test]
fn names_are_cleaned() {
    assert_eq!(clean("  web\u{0}\u{7}01\n "), "web01");
    assert_eq!(clean(&"x".repeat(500)).chars().count(), MAX_NAME);
}

#[test]
fn location_goes_to_new_top_level_items() {
    let store = Store::open_in_memory().unwrap();
    let server = crate::model::ServerId::new();
    let outcome = apply_import(
        &store,
        &[
            host("a", &["G", "H"], Some("a.org")),
            host("b", &[], Some("b.org")),
        ],
        Some(server),
    )
    .unwrap();
    assert_eq!(outcome.groups.len(), 2);
    assert_eq!(outcome.hosts.len(), 2);
    assert!(
        store
            .list_groups()
            .unwrap()
            .iter()
            .all(|g| g.location == Some(server))
    );
    assert!(
        store
            .list_hosts()
            .unwrap()
            .iter()
            .all(|h| h.location == Some(server))
    );
}

// ---------- known_hosts and key files

use russh::keys::ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};

fn ed25519() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn public_line(key: &PrivateKey) -> String {
    key.public_key().to_openssh().unwrap()
}

fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

#[test]
fn plain_and_bracketed_hosts() {
    let (k1, k2) = (ed25519(), ed25519());
    let text = format!(
        "example.org,10.0.0.1 {}\n[example.org]:2222 {}\n",
        public_line(&k1),
        public_line(&k2)
    );
    let parsed = knownhosts::parse(&text);
    assert_eq!(parsed.entries.len(), 3);
    assert!(parsed.entries.contains(&knownhosts::KnownEntry {
        address: "10.0.0.1".into(),
        port: 22,
        fingerprint: fingerprint(&k1),
    }));
    assert!(parsed.entries.contains(&knownhosts::KnownEntry {
        address: "example.org".into(),
        port: 2222,
        fingerprint: fingerprint(&k2),
    }));
    let names: Vec<_> = parsed.candidates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["example.org", "10.0.0.1", "example.org:2222"]);
    assert_eq!(parsed.candidates[2].settings.port, Some(2222));
}

#[test]
fn hashed_names_are_skipped_and_counted() {
    let k = ed25519();
    let text = format!(
        "|1|c2FsdA==|aGFzaA== {line}\n@cert-authority *.example.org {line}\n@revoked x.org {line}\n*.wild.org {line}\n",
        line = public_line(&k)
    );
    let parsed = knownhosts::parse(&text);
    assert!(parsed.entries.is_empty());
    assert_eq!(parsed.skipped.len(), 4);
}

#[test]
fn known_fingerprint_is_not_overwritten() {
    let store = Store::open_in_memory().unwrap();
    store
        .set_known_host("example.org", 22, "SHA256:old")
        .unwrap();
    store.set_known_host("same.org", 22, "SHA256:same").unwrap();
    let entry = |address: &str, fingerprint: &str| knownhosts::KnownEntry {
        address: address.into(),
        port: 22,
        fingerprint: fingerprint.into(),
    };
    let outcome = knownhosts::apply(
        &store,
        &[
            entry("example.org", "SHA256:new"),
            entry("same.org", "SHA256:same"),
            entry("fresh.org", "SHA256:fresh"),
        ],
    )
    .unwrap();
    assert_eq!(outcome.added, 1);
    assert_eq!(outcome.same, 1);
    assert_eq!(outcome.conflicts, vec![("example.org".to_owned(), 22)]);
    assert_eq!(
        store.known_host("example.org", 22).unwrap().as_deref(),
        Some("SHA256:old")
    );
    assert_eq!(
        store.known_host("fresh.org", 22).unwrap().as_deref(),
        Some("SHA256:fresh")
    );
}

#[test]
fn garbage_lines_are_reported() {
    let k = ed25519();
    let text = format!(
        "# comment\n\nonlyonefield\nhost ssh-ed25519 notbase64!!\n[broken:22 {line}\nok.org {line}\n",
        line = public_line(&k)
    );
    let parsed = knownhosts::parse(&text);
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.skipped.len(), 3);
    assert!(
        parsed.skipped.iter().all(|s| !s.contains("notbase64")),
        "no contents in messages"
    );
}

#[test]
fn scan_finds_openssh_pem_and_ppk_keys() {
    let dir = tempfile::tempdir().unwrap();
    let plain = ed25519();
    std::fs::write(
        dir.path().join("id_ed25519"),
        plain.to_openssh(LineEnding::LF).unwrap().as_bytes(),
    )
    .unwrap();
    std::fs::write(dir.path().join("id_ed25519.pub"), public_line(&plain)).unwrap();
    let mut pem = Vec::new();
    russh::keys::encode_pkcs8_pem(&ed25519(), &mut pem).unwrap();
    std::fs::write(dir.path().join("deploy.pem"), &pem).unwrap();
    std::fs::write(
        dir.path().join("putty.ppk"),
        include_str!("fixtures/id_ed25519.ppk"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("putty_enc.ppk"),
        include_str!("fixtures/id_ed25519_enc.ppk"),
    )
    .unwrap();
    std::fs::write(dir.path().join("known_hosts"), "x.org ssh-ed25519 AAAA").unwrap();
    std::fs::write(dir.path().join("config"), "Host x").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    let mut found = keyfiles::scan(dir.path());
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<_> = found.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["deploy.pem", "id_ed25519", "putty.ppk", "putty_enc.ppk"]
    );
    let id = found.iter().find(|k| k.name == "id_ed25519").unwrap();
    assert_eq!(
        id.fingerprint.as_deref(),
        Some(fingerprint(&plain).as_str())
    );
    assert_eq!(id.format, keyfiles::KeyFormat::OpenSsh);
    assert!(found.iter().any(|k| k.format == keyfiles::KeyFormat::Pem));
    let enc = found.iter().find(|k| k.name == "putty_enc.ppk").unwrap();
    assert!(enc.encrypted);
    assert_eq!(enc.format, keyfiles::KeyFormat::Ppk);
}

#[test]
fn encrypted_key_needs_the_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("putty_enc.ppk");
    std::fs::write(&path, include_str!("fixtures/id_ed25519_enc.ppk")).unwrap();
    let store = Store::open_in_memory().unwrap();
    let vault =
        crate::vault::Vault::unlock(&store, Box::new(crate::vault::MemoryMasterKey::default()))
            .unwrap();
    assert!(keyfiles::import(&vault, &store, &path, None).is_err());
    assert!(keyfiles::import(&vault, &store, &path, Some("wrong")).is_err());
    let info = keyfiles::import(&vault, &store, &path, Some("123")).unwrap();
    assert_eq!(info.name, "putty_enc");
    // PEM works too.
    let pem_path = dir.path().join("deploy.pem");
    let mut pem = Vec::new();
    russh::keys::encode_pkcs8_pem(&ed25519(), &mut pem).unwrap();
    std::fs::write(&pem_path, &pem).unwrap();
    keyfiles::import(&vault, &store, &pem_path, None).unwrap();
    assert_eq!(store.list_key_records().unwrap().len(), 2);
}
