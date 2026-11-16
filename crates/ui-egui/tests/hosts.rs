//! The hosts page: hosts and groups created, edited, deleted; groups opened and left; tags and
//! search; inherited values; passwords never shown; names that try to hide or reorder text.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session};
use sshcraft_model::model::{Group, GroupId, HostId};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::{host_form, hosts};
use sshcraft_ui_egui::testing::app_harness;

fn run(session: &Session, id: &str, params: Value) -> Value {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap()
}

fn add_group(session: &Session, name: &str, parent: Option<GroupId>, defaults: Value) -> GroupId {
    let id = GroupId(uuid::Uuid::new_v4());
    run(session, "group.save", json!({ "group": { "id": id, "name": name, "parent_id": parent, "defaults": defaults } }));
    id
}

fn add_host(session: &Session, name: &str, group: Option<GroupId>, tags: &[&str]) -> HostId {
    let id = HostId(uuid::Uuid::new_v4());
    let host = json!({ "id": id, "name": name, "group_id": group, "settings": { "address": "10.0.0.5" }, "tags": tags });
    run(session, "host.save", json!({ "host": host }));
    id
}

/// Two presses of the left button on the element labelled `label`, as a double click.
fn double_click(h: &mut Harness<'static, SshcraftApp>, label: &str) {
    let rect = h.get_by_label(label).rect();
    let at = egui::pos2(rect.center().x, rect.center().y);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(1);
    for _ in 0..2 {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        // One frame per click: the two clicks fall inside egui's double-click time.
        h.step();
    }
}

/// Steps until `done` holds (data comes back from the engine asynchronously).
fn until(h: &mut Harness<'static, SshcraftApp>, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(1);
}

#[test]
fn a_host_is_created_edited_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.get_by_label("New host").click();
    h.run_steps(2);
    {
        let form = h.state_mut().ui.host_form.as_mut().expect("the form opened");
        form.host.name = "web-01".into();
        form.host.settings.address = Some("192.0.2.10".into());
    }
    h.get_by_label("Save").click();
    until(&mut h, "the new host", |app| app.data.hosts.iter().any(|r| r.host.name == "web-01"));
    assert!(h.state().ui.host_form.is_none(), "the form closed");

    let id = h.state().data.hosts[0].host.id;
    hosts::edit_host(h.state_mut(), id);
    h.run_steps(2);
    h.state_mut().ui.host_form.as_mut().unwrap().host.name = "web-02".into();
    h.get_by_label("Save").click();
    until(&mut h, "the renamed host", |app| app.data.hosts.iter().any(|r| r.host.name == "web-02"));

    hosts::ask_delete_host(h.state_mut(), id);
    h.run_steps(2);
    h.get_by_label("Delete").click();
    until(&mut h, "the deletion", |app| app.data.hosts.is_empty());
}

#[test]
fn groups_open_by_double_click_and_the_path_leads_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let prod = add_group(&session, "prod", None, json!({}));
    add_host(&session, "db", Some(prod), &[]);
    until(&mut h, "the data", |app| !app.data.groups.is_empty() && !app.data.hosts.is_empty());
    assert!(h.query_by_label("db").is_none(), "hosts in a group are inside it");
    double_click(&mut h, "prod");
    h.run_steps(2);
    assert_eq!(h.state().ui.hosts.group, Some(prod));
    assert!(h.query_by_label("db").is_some());
    h.get_by_label("All hosts").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.hosts.group, None);
}

#[test]
fn tags_filter_with_and() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    add_host(&session, "alpha", None, &["web", "eu"]);
    add_host(&session, "beta", None, &["web"]);
    add_host(&session, "gamma", None, &["eu"]);
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 3);
    h.state_mut().ui.hosts.tags = vec!["web".into(), "eu".into()];
    h.run_steps(2);
    assert!(h.query_by_label("alpha").is_some());
    assert!(h.query_by_label("beta").is_none());
    assert!(h.query_by_label("gamma").is_none());
}

#[test]
fn search_covers_the_group_path() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let prod = add_group(&session, "production", None, json!({}));
    add_host(&session, "db", Some(prod), &[]);
    until(&mut h, "the data", |app| !app.data.groups.is_empty() && !app.data.hosts.is_empty());
    h.state_mut().ui.hosts.search = "product".into();
    h.run_steps(2);
    assert!(h.query_by_label("db").is_some(), "the search finds hosts by the groups above them");
}

#[test]
fn inherited_values_show_their_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let top = add_group(&session, "prod", None, json!({ "username": "deploy", "port": 2222 }));
    let sub = add_group(&session, "eu", Some(top), json!({ "port": 2200 }));
    let host = add_host(&session, "db", Some(sub), &[]);
    until(&mut h, "the data", |app| app.data.groups.len() == 2 && !app.data.hosts.is_empty());
    let groups: Vec<Group> = h.state().data.groups.iter().map(|g| g.group.clone()).collect();
    let row = h.state().data.hosts.iter().find(|r| r.host.id == host).unwrap().host.clone();
    let inherited = host_form::inherited(row.group_id, &groups);
    assert_eq!(inherited.username, Some(("deploy".to_string(), "prod".to_string())));
    assert_eq!(inherited.port, Some((2200, "eu".to_string())), "the nearest group wins");
}

#[test]
fn a_stored_password_is_never_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let host = add_host(&session, "db", None, &[]);
    run(&session, "password.set", json!({ "owner": { "kind": "host", "id": host }, "secret": "S3cret-pw" }));
    until(&mut h, "the host", |app| app.data.hosts.iter().any(|r| r.has_password));
    hosts::edit_host(h.state_mut(), host);
    h.run_steps(3);
    let form = h.state().ui.host_form.as_ref().unwrap();
    assert!(form.password.is_empty(), "the field starts empty");
    assert!(form.has_password, "it says a password is stored");
    let tree = format!("{:?}", h.root());
    assert!(!tree.contains("S3cret"), "the password never reaches the screen");
}

#[test]
fn a_group_cannot_move_into_itself() {
    let a =
        Group { id: GroupId(uuid::Uuid::new_v4()), name: "a".into(), defaults: Default::default(), location: None, parent_id: None, tags: vec![] };
    let b = Group { id: GroupId(uuid::Uuid::new_v4()), name: "b".into(), parent_id: Some(a.id), ..a.clone() };
    let c = Group { id: GroupId(uuid::Uuid::new_v4()), name: "c".into(), parent_id: None, ..a.clone() };
    let groups = vec![a.clone(), b.clone(), c.clone()];
    let targets = host_form::group_targets(&groups, Some(a.id));
    assert!(!targets.iter().any(|(id, _)| *id == Some(a.id) || *id == Some(b.id)), "{targets:?}");
    assert!(targets.iter().any(|(id, _)| *id == Some(c.id)));
    assert!(targets.iter().any(|(id, _)| id.is_none()), "the top level is a target");
}

#[test]
fn hostile_names_are_shown_safely() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let name = format!("ok\u{202e}evil\u{7}{}", "x".repeat(5000));
    add_host(&session, &name, None, &[]);
    until(&mut h, "the host", |app| !app.data.hosts.is_empty());
    let node = h.get_by_label(&name);
    assert!(node.rect().width() <= hosts::MAX_NAME_WIDTH + 1.0, "the name is cut: {}", node.rect().width());
    assert!(sshcraft_ui_egui::widgets::visible(&name).starts_with("ok⟨U+202E⟩evil"));
}

#[test]
fn a_failed_save_keeps_the_typed_password() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.get_by_label("New host").click();
    h.run_steps(3);
    {
        let form = h.state_mut().ui.host_form.as_mut().unwrap();
        form.host.name = "web".into();
        form.password = "pw-typed".into();
        // A sync server this device does not know: the engine refuses the host.
        form.host.location = Some(sshcraft_model::model::ServerId::new());
    }
    h.run_steps(1);
    h.get_by_label("Save").click();
    until(&mut h, "the error", |app| app.ui.host_form.as_ref().is_some_and(|f| f.error.is_some()));
    assert_eq!(h.state().ui.host_form.as_ref().unwrap().password, "pw-typed");
}

#[test]
fn long_addresses_and_tags_stay_in_their_space() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let long = "a".repeat(400);
    run(
        &session,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "web", "group_id": null, "tags": [long.clone()], "settings": { "address": format!("{long}.example") } } }),
    );
    until(&mut h, "the host", |app| !app.data.hosts.is_empty());
    h.run_steps(3);
    for node in h.get_all_by_label_contains("aaaaaaaa") {
        assert!(node.rect().width() <= 400.0, "{:?}", node.rect());
    }
}

/// A proxy command is typed as one line and kept as arguments (the engine tests that saving it
/// here approves it on this device).
#[test]
fn the_host_form_offers_a_proxy_command() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = add_host(&session, "behind", None, &[]);
    h.run_steps(3);
    hosts::edit_host(h.state_mut(), id);
    h.run_steps(2);
    h.get_by_label("Command").click();
    h.run_steps(2);
    h.get_by_label("Proxy command").click();
    h.run_steps(1);
    h.get_by_label("Proxy command").type_text("ssh -W %h:%p 'my bastion'");
    h.run_steps(2);
    h.get_by_label("Save").click();
    let want = vec!["ssh".to_string(), "-W".into(), "%h:%p".into(), "my bastion".into()];
    until(&mut h, "the saved command", |app| {
        app.data.hosts.iter().any(|r| r.host.settings.proxy == Some(sshcraft_model::model::Proxy::Command { argv: want.clone() }))
    });
}

#[test]
fn a_deleted_host_is_restored_from_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = add_host(&session, "gone", None, &[]);
    run(&session, "host.delete", json!({ "id": id }));
    h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Settings;
    h.state_mut().ui.settings.section = sshcraft_ui_egui::pages::settings::Section::Trash;
    until(&mut h, "the trash", |app| app.data.trash.as_ref().is_some_and(|t| t.len() == 1));
    h.get_by_label("gone");
    h.get_by_label("Restore").click();
    until(&mut h, "the restored host", |app| app.data.hosts.iter().any(|r| r.host.name == "gone"));
    until(&mut h, "an empty trash", |app| app.data.trash.as_ref().is_some_and(Vec::is_empty));
}

#[test]
fn the_host_form_takes_a_password_reference() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = add_host(&session, "vaulted", None, &[]);
    h.run_steps(3);
    hosts::edit_host(h.state_mut(), id);
    h.run_steps(2);
    h.get_by_label("From a password manager").click();
    h.run_steps(2);
    h.get_by_label("Reference").click();
    h.run_steps(1);
    h.get_by_label("Reference").type_text("op://Servers/vaulted/password");
    h.run_steps(2);
    h.get_by_label("Save").click();
    until(&mut h, "the saved reference", |app| {
        app.data.hosts.iter().any(|r| r.host.settings.password_ref.as_ref().is_some_and(|p| p.reference == "op://Servers/vaulted/password"))
    });
}

#[test]
fn reachability_is_checked_only_when_switched_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let id = HostId(uuid::Uuid::new_v4());
    let port = listener.local_addr().unwrap().port();
    run(&session, "host.save", json!({ "host": { "id": id, "name": "up", "settings": { "address": "127.0.0.1", "port": port } } }));
    until(&mut h, "the host", |app| app.data.hosts.iter().any(|r| r.host.id == id));
    h.run_steps(5);
    assert!(h.state().ui.hosts.reachability.hosts.is_empty(), "off by default: nothing is checked");
    let mut settings = run(&session, "settings.get", json!({}));
    settings["appearance"]["show_reachability"] = json!(true);
    run(&session, "settings.save", json!({ "settings": settings }));
    until(&mut h, "the check", |app| app.ui.hosts.reachability.hosts.get(&id) == Some(&Some(true)));
}

#[test]
fn chosen_hosts_change_together() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (a, b, c) = (add_host(&session, "alpha", None, &[]), add_host(&session, "beta", None, &[]), add_host(&session, "gamma", None, &[]));
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 3);
    h.get_by_label("alpha").click();
    h.run_steps(1);
    h.get_by_label("gamma").click_modifiers(egui::Modifiers::SHIFT);
    h.run_steps(1);
    h.get_by_label("beta").click_modifiers(egui::Modifiers::COMMAND);
    h.run_steps(2);
    let chosen = h.state().ui.hosts.selection.hosts.clone();
    assert!(chosen.contains(&a) && chosen.contains(&c) && !chosen.contains(&b), "{chosen:?}");
    h.state_mut().ui.hosts.selection.tag = "prod".into();
    h.run_steps(1);
    h.get_by_label("Add tag").click();
    until(&mut h, "the tags", |app| app.data.hosts.iter().filter(|r| r.host.tags.contains(&"prod".to_string())).count() == 2);
    let beta = h.state().data.hosts.iter().find(|r| r.host.id == b).unwrap().host.tags.clone();
    assert!(beta.is_empty(), "a host not chosen keeps its tags");
}

#[test]
fn a_host_shared_without_its_sign_in_asks_for_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = HostId(uuid::Uuid::new_v4());
    let host = json!({ "id": id, "name": "shared", "settings": { "address": "10.0.0.9", "credentials_from_member": true } });
    run(&session, "host.save", json!({ "host": host }));
    until(&mut h, "the host", |app| app.data.hosts.iter().any(|r| r.host.id == id));
    assert!(h.query_by_label("Add your own sign-in").is_some());
    // With a key of the member's own, the hint goes.
    let mut host = run(&session, "host.list", json!({})).as_array().unwrap().iter().find(|r| r["id"] == json!(id)).unwrap().clone();
    host["settings"]["key"] = json!({ "kind": "agent", "fingerprint": "SHA256:abc" });
    run(&session, "host.save", json!({ "host": host }));
    until(&mut h, "the key", |app| app.data.hosts.iter().any(|r| r.host.id == id && r.host.settings.key.is_some()));
    assert!(h.query_by_label("Add your own sign-in").is_none());
}

fn save_dynamic_groups(session: &Session, groups: Value) {
    let mut settings = run(session, "settings.get", json!({}));
    settings["appearance"]["smart_groups"] = groups;
    run(session, "settings.save", json!({ "settings": settings }));
}

#[test]
fn a_dynamic_group_finds_tagged_hosts_in_every_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let prod = add_group(&session, "production", None, json!({}));
    add_host(&session, "alpha", Some(prod), &["web"]);
    add_host(&session, "beta", None, &["web"]);
    add_host(&session, "gamma", None, &["db"]);
    save_dynamic_groups(&session, json!([{ "name": "Web servers", "tags": ["web"] }]));
    until(&mut h, "the data", |app| app.data.hosts.len() == 3 && !app.data.settings.appearance.smart_groups.is_empty());
    assert!(h.query_by_label("alpha").is_none(), "alpha is in a group below");
    h.get_by_label("Web servers").click();
    h.run_steps(2);
    assert!(h.query_by_label("alpha").is_some(), "found in its group");
    assert!(h.query_by_label("beta").is_some());
    assert!(h.query_by_label("gamma").is_none());
    // A second click leaves the dynamic group.
    h.get_by_label("Web servers").click();
    h.run_steps(2);
    assert!(h.state().ui.hosts.smart.is_none() && h.state().ui.hosts.tags.is_empty());
    assert!(h.query_by_label("alpha").is_none());
    assert!(h.query_by_label("gamma").is_some());
}

#[test]
fn the_current_filter_can_be_saved_as_a_dynamic_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    add_host(&session, "gamma", None, &["db"]);
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 1);
    assert!(h.query_by_label("Save as dynamic group").is_none(), "nothing to save without a filter");
    h.state_mut().ui.hosts.tags = vec!["db".into()];
    h.run_steps(2);
    h.get_by_label("Save as dynamic group").click();
    h.run_steps(2);
    h.state_mut().ui.hosts.smart_draft = Some("Databases".into());
    h.run_steps(1);
    h.get_by_label("Save").click();
    until(&mut h, "the saved group", |app| !app.data.settings.appearance.smart_groups.is_empty());
    let saved = &h.state().data.settings.appearance.smart_groups[0];
    assert_eq!((saved.name.as_str(), saved.tags.clone()), ("Databases", vec!["db".to_string()]));
    assert_eq!(h.state().ui.hosts.smart.as_deref(), Some("Databases"), "applied right away");
}
