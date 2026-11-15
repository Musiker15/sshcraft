//! The keys page: generating and importing keys, the public part only ever on screen and in the
//! clipboard, security keys only where the device has them, identities with a key.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::keys;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

fn until(h: &mut Harness<'static, SshcraftApp>, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn keys_page() -> (Harness<'static, SshcraftApp>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().ui.page = Page::Keys;
    h.run_steps(3);
    (h, dir)
}

#[test]
fn a_key_is_generated_and_listed() {
    let (mut h, _dir) = keys_page();
    h.get_by_label("Generate key").click();
    h.run_steps(3);
    h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::generate_mut).expect("the dialog opened").name = "deploy".into();
    h.get_by_label("Generate").click();
    until(&mut h, "the key", |app| app.data.keys.iter().any(|k| k.name == "deploy"));
    assert!(h.query_by_label("deploy").is_some());
}

#[test]
fn the_private_key_leaves_the_screen_after_import() {
    let (mut h, _dir) = keys_page();
    let private = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().to_openssh(LineEnding::LF).unwrap().to_string();
    h.get_by_label("Import key").click();
    h.run_steps(3);
    {
        let import = h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::import_mut).expect("the dialog opened");
        import.name = "imported".into();
        import.text = private.clone();
    }
    h.get_by_label("Import").click();
    until(&mut h, "the key", |app| app.data.keys.iter().any(|k| k.name == "imported"));
    assert!(h.state().ui.key_dialog.is_none(), "the dialog closed");
    let tree = format!("{:?}", h.root());
    assert!(!tree.contains("PRIVATE KEY"), "no private key on screen");
    let line = private.lines().nth(1).unwrap();
    assert!(!tree.contains(line), "no part of it either");
    let fingerprint = h.state().data.keys[0].fingerprint.clone();
    assert!(tree.contains(&fingerprint), "the fingerprint is shown");
}

#[test]
fn copy_public_key_copies_only_the_public_part() {
    let (mut h, _dir) = keys_page();
    h.get_by_label("Generate key").click();
    h.run_steps(3);
    h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::generate_mut).unwrap().name = "deploy".into();
    h.get_by_label("Generate").click();
    until(&mut h, "the key", |app| !app.data.keys.is_empty());
    let id = h.state().data.keys[0].id.clone();
    let ctx = h.ctx.clone();
    keys::copy_public_key(h.state_mut(), &id, ctx);
    h.run_steps(1);
    let copied = h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let copied = copied.expect("something was copied");
    assert!(copied.starts_with("ssh-ed25519 "), "{copied}");
    assert!(!copied.contains("PRIVATE"));
}

#[test]
fn security_keys_only_when_supported() {
    let (mut h, _dir) = keys_page();
    until(&mut h, "the support check", |app| app.data.security.is_some());
    // The test engine has no authenticator.
    assert!(h.query_by_label("Security key").is_none());
}

#[test]
fn an_identity_uses_a_key() {
    let (mut h, _dir) = keys_page();
    h.get_by_label("Generate key").click();
    h.run_steps(3);
    h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::generate_mut).unwrap().name = "ops-key".into();
    h.get_by_label("Generate").click();
    until(&mut h, "the key", |app| !app.data.keys.is_empty());
    h.get_by_label("New identity").click();
    h.run_steps(3);
    let key = h.state().data.keys[0].id.clone();
    {
        let form = h.state_mut().ui.identity_form.as_mut().expect("the form opened");
        form.name = "ops".into();
        form.username = "root".into();
        form.key = Some(key.clone());
    }
    h.get_by_label("Save").click();
    until(&mut h, "the identity", |app| app.data.identities.iter().any(|i| i.name == "ops"));
    let identity = h.state().data.identities.iter().find(|i| i.name == "ops").unwrap().clone();
    assert_eq!(identity.username.as_deref(), Some("root"));
    assert!(identity.key.as_ref().is_some_and(|k| k.to_string().contains(&key)), "{:?}", identity.key);
}

#[test]
fn the_keys_page_lists_known_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    sshcraft_engine::testing::set_known_host(&session, "known.example", 2222, "SHA256:abc");
    h.state_mut().ui.page = Page::Keys;
    until(&mut h, "the known hosts", |app| app.data.host_keys.as_ref().is_some_and(|k| k.len() == 1));
    h.get_by_label("known.example:2222");
    h.get_by_label("Remove").click();
    until(&mut h, "the removed key", |app| app.data.host_keys.as_ref().is_some_and(Vec::is_empty));
    assert!(sshcraft_engine::testing::known_host(&session, "known.example", 2222).is_none());
}
