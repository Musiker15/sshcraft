//! Keys from elsewhere on the keys page: the SSH agent's keys (listed, never stored) and keys
//! imported from Proton Pass through `pass-cli`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use sshcraft_engine::SessionConfig;
use sshcraft_engine::passcli::{PassCliStatus, PassKey};
use sshcraft_engine::testing::{FakePassCli, TestAgents};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness_with;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn keys_page(dir: &std::path::Path, configure: impl FnOnce(&mut SessionConfig)) -> H {
    let (mut h, _session) = app_harness_with(dir, configure);
    h.state_mut().ui.page = Page::Keys;
    h.run_steps(3);
    h
}

fn tree(h: &H) -> String {
    format!("{:?}", h.root())
}

#[test]
fn the_agents_keys_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
    let mut h = keys_page(dir.path(), |c| c.agent = Arc::new(TestAgents { keys: vec![key], reachable: true }));
    until(&mut h, "the agent's keys", |app| app.ui.agent_keys.keys().is_some_and(|k| !k.is_empty()));
    assert!(tree(&h).contains(&fingerprint), "the fingerprint is shown");
    assert!(h.state().data.keys.is_empty(), "listed, not stored");
}

#[test]
fn an_unreachable_agent_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = keys_page(dir.path(), |_| {});
    until(&mut h, "the agent's answer", |app| app.ui.agent_keys.is_loaded());
    assert!(h.query_by_label_contains("cannot be reached").is_some());
}

#[test]
fn a_key_is_imported_from_proton_pass() {
    let dir = tempfile::tempdir().unwrap();
    let private = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().to_openssh(LineEnding::LF).unwrap().to_string();
    let line = private.lines().nth(1).unwrap().to_string();
    let mut h = keys_page(dir.path(), move |c| {
        let cli = FakePassCli {
            status: PassCliStatus::Ready,
            keys: vec![PassKey { share_id: "s1".into(), item_id: "i1".into(), title: "server-root".into() }],
            private_key: private,
        };
        let cli: Arc<dyn sshcraft_engine::passcli::PassCli> = Arc::new(cli);
        c.pass_cli = Arc::new(move || Some(cli.clone()));
    });
    h.get_by_label("Import from Proton Pass").click();
    until(&mut h, "the list", |app| app.ui.passcli.as_ref().is_some_and(|p| p.items().is_some_and(|i| !i.is_empty())));
    assert!(h.query_by_label("server-root").is_some());
    h.get_by_label("Import").click();
    until(&mut h, "the imported key", |app| app.data.keys.iter().any(|k| k.name == "server-root"));
    assert!(!tree(&h).contains(&line), "the private key never reaches the screen");
}

#[test]
fn a_missing_pass_cli_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = keys_page(dir.path(), |_| {});
    h.get_by_label("Import from Proton Pass").click();
    until(&mut h, "the status", |app| app.ui.passcli.as_ref().is_some_and(|p| p.items().is_none() && !p.is_loading()));
    assert!(h.query_by_label_contains("pass-cli is not installed").is_some());
}
