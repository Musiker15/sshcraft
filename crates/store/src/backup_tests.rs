use super::*;
use crate::vault::MemoryMasterKey;
use sshcraft_model::model::{ConnectionSettings, HostId, SnippetId};

const PASS: &str = "correct horse battery";

fn device() -> (Store, Vault) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (store, vault)
}

/// A device with a group, a host with a password, a key, a snippet and a host key.
fn filled() -> (Store, Vault, HostId) {
    let (store, vault) = device();
    let group = Group {
        id: sshcraft_model::model::GroupId(uuid::Uuid::new_v4()),
        name: "g".into(),
        defaults: ConnectionSettings::default(),
        location: None,
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&group).unwrap();
    let pw = PasswordId::new();
    vault.store_password(&store, pw, "s3cret", None).unwrap();
    let host = Host {
        id: HostId::new(),
        name: "web".into(),
        group_id: Some(group.id),
        settings: ConnectionSettings { address: Some("web.org".into()), password: Some(pw), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    store.upsert_host(&host).unwrap();
    vault.generate_ed25519(&store, "deploy").unwrap();
    snippets::save(
        &vault,
        &store,
        &Snippet { id: SnippetId::new(), name: "up".into(), package: String::new(), content: "uptime".into(), default_targets: Vec::new() },
    )
    .unwrap();
    store.set_known_host("web.org", 22, "SHA256:k").unwrap();
    (store, vault, host.id)
}

#[test]
fn a_backup_restores_on_an_empty_device() {
    let (store, vault, host) = filled();
    let (text, made) = create(&store, &vault, PASS, 1).unwrap();
    assert_eq!((made.groups, made.hosts, made.keys, made.passwords, made.snippets, made.host_keys), (1, 1, 1, 1, 1, 1));
    assert!(text.starts_with("sshcraft-backup-1\n"));
    assert!(!text.contains("s3cret") && !text.contains("uptime"), "nothing readable without the passphrase");

    // Another device with another master key.
    let (other, other_vault) = device();
    let added = restore_missing(&other, &other_vault, &text, PASS).unwrap();
    assert_eq!((added.groups, added.hosts, added.keys, added.passwords, added.snippets, added.host_keys), (1, 1, 1, 1, 1, 1));
    let back = other.list_hosts().unwrap();
    assert_eq!(back[0].id, host);
    let pw = back[0].settings.password.unwrap();
    assert_eq!(other_vault.password(&other, pw).unwrap().as_str(), "s3cret");
    let key = crate::vault::list_keys(&other).unwrap();
    assert!(other_vault.private_key(&other, key[0].id).is_ok());
    assert_eq!(snippets::list(&other_vault, &other).unwrap()[0].content, "uptime");
}

#[test]
fn a_wrong_passphrase_restores_nothing() {
    let (store, vault, _) = filled();
    let (text, _) = create(&store, &vault, PASS, 1).unwrap();
    let (other, other_vault) = device();
    assert!(matches!(restore_missing(&other, &other_vault, &text, "wrong passphrase!"), Err(BackupError::WrongPassphraseOrChanged)));
    assert!(other.list_hosts().unwrap().is_empty());
    assert!(other.list_groups().unwrap().is_empty());
}

#[test]
fn a_changed_file_is_refused() {
    let (store, vault, _) = filled();
    let (text, _) = create(&store, &vault, PASS, 1).unwrap();
    let (other, other_vault) = device();
    // A weaker key derivation in the header, or a flipped byte in the body.
    let weaker = text.replacen("\"passes\":3", "\"passes\":1", 1);
    assert!(matches!(restore_missing(&other, &other_vault, &weaker, PASS), Err(BackupError::WrongPassphraseOrChanged)));
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let body = lines[2].clone();
    let rest = body.get(1..).unwrap();
    let flipped = if body.starts_with('A') { format!("B{rest}") } else { format!("A{rest}") };
    lines[2] = flipped;
    assert!(restore_missing(&other, &other_vault, &lines.join("\n"), PASS).is_err());
    assert!(matches!(restore_missing(&other, &other_vault, "not a backup", PASS), Err(BackupError::NotABackup)));
    let greedy = text.replacen("\"memory_kib\":65536", "\"memory_kib\":99999999", 1);
    assert!(matches!(restore_missing(&other, &other_vault, &greedy, PASS), Err(BackupError::NotABackup)), "a header cannot ask for gigabytes");
    assert!(other.list_hosts().unwrap().is_empty());
}

#[test]
fn restore_adds_only_what_is_missing() {
    let (store, vault, host) = filled();
    let (text, _) = create(&store, &vault, PASS, 1).unwrap();
    // The host was renamed here since: the backup does not take that back.
    let mut h = store.list_hosts().unwrap().into_iter().find(|h| h.id == host).unwrap();
    h.name = "renamed".into();
    store.upsert_host(&h).unwrap();
    let added = restore_missing(&store, &vault, &text, PASS).unwrap();
    assert_eq!((added.hosts, added.groups, added.keys, added.passwords), (0, 0, 0, 0));
    assert_eq!(store.list_hosts().unwrap()[0].name, "renamed");
}

#[test]
fn a_short_passphrase_is_refused() {
    let (store, vault, _) = filled();
    assert!(matches!(create(&store, &vault, "short", 1), Err(BackupError::WeakPassphrase)));
}
