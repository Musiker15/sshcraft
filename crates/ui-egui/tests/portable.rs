//! Portable mode's unlock window: the first start chooses a passphrase, later starts need it.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_engine::portable::{self, PassphraseMasterKey};
use sshcraft_store::store::Store;
use sshcraft_store::vault::{MasterKeyProvider, Vault};
use sshcraft_ui_egui::unlock::Unlock;

const PASSPHRASE: &str = "a long enough passphrase";

fn window(dir: &Path) -> Harness<'static, Unlock> {
    let unlock = Unlock::new(dir.to_path_buf());
    Harness::builder().with_size(vec2(460.0, 320.0)).build_ui_state(|ui, unlock: &mut Unlock| unlock.ui(ui), unlock)
}

fn wait(h: &mut Harness<'static, Unlock>, what: &str, done: impl Fn(&Unlock) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

/// Clicks into the field labelled `label` and types `text`, as a person does.
fn type_into(h: &mut Harness<'static, Unlock>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run_steps(1);
    h.get_by_label(label).type_text(text);
    h.run_steps(1);
}

fn opened(h: &mut Harness<'static, Unlock>) -> PassphraseMasterKey {
    h.state_mut().opened.take().unwrap()
}

/// The first start refuses a short passphrase and two that differ, then makes the key file
/// with the chosen one; the next start opens it only with that passphrase.
#[test]
fn the_first_start_chooses_the_passphrase_the_next_needs_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = window(dir.path());
    h.run_steps(2);
    assert!(h.query_by_label("Choose a passphrase").is_some());
    type_into(&mut h, "Passphrase", "short");
    type_into(&mut h, "Passphrase again", "short");
    h.get_by_label("Create").click();
    h.run_steps(2);
    assert!(h.query_by_label("The passphrase needs at least 12 characters.").is_some());
    assert!(h.state().opened.is_none());

    type_into(&mut h, "Passphrase", " but longer now");
    type_into(&mut h, "Passphrase again", " and different");
    h.get_by_label("Create").click();
    h.run_steps(2);
    assert!(h.query_by_label("The two passphrases differ.").is_some());
    assert!(h.state().opened.is_none());

    // The same text in both fields.
    let mut h = window(dir.path());
    h.run_steps(2);
    type_into(&mut h, "Passphrase", PASSPHRASE);
    type_into(&mut h, "Passphrase again", PASSPHRASE);
    h.get_by_label("Create").click();
    wait(&mut h, "the key was not made", |u| u.opened.is_some());
    // The vault writes the key file when it makes its master key.
    let store = Store::open(&dir.path().join("sshcraft.db")).unwrap();
    let vault = Vault::unlock(&store, Box::new(opened(&mut h))).unwrap();
    let password = sshcraft_model::model::PasswordId::new();
    vault.store_password(&store, password, "s3cret", None).unwrap();
    assert!(!portable::is_new(dir.path()));

    let mut h = window(dir.path());
    h.run_steps(2);
    assert!(h.query_by_label("Unlock SSHCraft").is_some());
    assert!(h.query_by_label("Passphrase again").is_none());
    type_into(&mut h, "Passphrase", "not the passphrase");
    h.get_by_label("Unlock").click();
    h.run_steps(1);
    assert!(h.state().working());
    wait(&mut h, "the wrong passphrase was not answered", |u| !u.working());
    h.run_steps(2);
    assert!(h.query_by_label("The passphrase is wrong.").is_some());
    assert!(h.state().opened.is_none());

    type_into(&mut h, "Passphrase", PASSPHRASE);
    h.get_by_label("Unlock").click();
    wait(&mut h, "the right passphrase did not open", |u| u.opened.is_some());
    let key = opened(&mut h);
    assert!(key.load().unwrap().is_some());
    let vault = Vault::unlock(&store, Box::new(key)).unwrap();
    assert_eq!(vault.password(&store, password).unwrap().as_str(), "s3cret");
}

/// *Quit* ends the start without a key.
#[test]
fn quit_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = window(dir.path());
    h.run_steps(2);
    h.get_by_label("Quit").click();
    h.run_steps(1);
    assert!(h.state().quit && h.state().opened.is_none());
    assert!(portable::is_new(dir.path()));
}
