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
