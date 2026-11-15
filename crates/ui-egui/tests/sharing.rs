//! Sharing with another account on the same server: only after the safety number was compared,
//! never past a changed key, recipients removed with a warning when keys went along, a share
//! ended.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn run(session: &Session, id: &str, params: Value) -> Value {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap()
}

fn sync_server(session: &Session, dir: &std::path::Path) -> (String, String, tokio::sync::oneshot::Sender<()>) {
    session.runtime().block_on(async {
        let database_url = format!("sqlite://{}", dir.join("server.db").display());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = format!("http://127.0.0.1:{}", addr.port());
        let config = sshcraft_server_lib::config::ServeConfig {
            listen: addr,
            database_url: database_url.clone(),
            public_origin: origin.clone(),
            ws_recheck: Duration::from_millis(200),
        };
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = sshcraft_server_lib::serve_on(listener, config, async {
                let _ = stopped.await;
            })
            .await;
        });
        for _ in 0..100 {
            if sshcraft_server_lib::health(addr).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let api_key = sshcraft_server_lib::store::open(&database_url).await.unwrap().api_key().await.unwrap().0;
        (origin, api_key, stop)
    })
}

fn account(session: &Session, origin: &str, api_key: &str, name: &str, email: &str) -> String {
    let input = json!({ "server_name": "home", "origin": origin, "api_key": api_key, "name": name, "email": email, "device_name": "pc" });
    run(session, "sync.create_account", json!({ "input": input }))["server_id"].as_str().unwrap().to_string()
}

/// Alice (in the window) with a group on the server, and Bob's account on the same server.
struct Two {
    h: H,
    alice: Session,
    server: String,
    group: String,
    _bob: (tempfile::TempDir, Session),
    origin: String,
    api_key: String,
    _stop: tokio::sync::oneshot::Sender<()>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

fn two() -> Two {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (h, alice) = app_harness(dir.path());
    let (origin, api_key, stop) = sync_server(&alice, server_dir.path());
    let server = account(&alice, &origin, &api_key, "Alice", "alice@example.org");
    let bob_dir = tempfile::tempdir().unwrap();
    let bob = {
        let _in = alice.runtime().enter();
        Session::open(sshcraft_engine::SessionConfig::for_tests(bob_dir.path())).unwrap().0
    };
    account(&bob, &origin, &api_key, "Bob", "bob@example.org");
    let group = uuid::Uuid::new_v4().to_string();
    run(&alice, "group.save", json!({ "group": { "id": group, "name": "web", "defaults": {} } }));
    run(
        &alice,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "web-01", "group_id": group, "settings": { "address": "192.0.2.1" } } }),
    );
    run(&alice, "item.move", json!({ "kind": "group", "item_id": group, "location": server }));
    Two { h, alice, server, group, _bob: (bob_dir, bob), origin, api_key, _stop: stop, _dirs: (dir, server_dir) }
}

fn find_bob(t: &mut Two) {
    t.h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::DATA;
    until(&mut t.h, "the group", |app| !app.data.groups.is_empty() && !app.data.servers.is_empty());
    let group = t.group.clone();
    sshcraft_ui_egui::pages::sharing::ask_share(t.h.state_mut(), &group, None);
    t.h.run_steps(3);
    t.h.state_mut().ui.share.as_mut().expect("the share dialog").email = "bob@example.org".into();
    t.h.get_by_label("Find").click();
    until(&mut t.h, "Bob", |app| app.ui.share.as_ref().is_some_and(|s| s.recipient.is_some()));
}

fn shares(t: &Two) -> Vec<Value> {
    run(&t.alice, "share.list", json!({ "server": t.server })).as_array().cloned().unwrap_or_default()
}

#[test]
fn a_group_is_shared_after_the_safety_number_is_ticked() {
    let mut t = two();
    find_bob(&mut t);
    assert!(t.h.query_by_label_contains("Their safety number").is_some(), "the number to compare");
    assert!(t.h.get_by_label("Share").accesskit_node().is_disabled(), "not before the number was compared");
    t.h.get_by_label("I compared the safety number with Bob").click();
    t.h.run_steps(2);
    t.h.get_by_label("Share").click();
    let deadline = Instant::now() + Duration::from_secs(20);
    while shares(&t).is_empty() {
        assert!(Instant::now() < deadline, "no share; messages: {:?}", t.h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    assert_eq!(shares(&t)[0]["grants"].as_array().map(Vec::len), Some(1));
}

/// Shares the group with Bob through the dialog.
fn share_with_bob(t: &mut Two) {
    find_bob(t);
    t.h.get_by_label("I compared the safety number with Bob").click();
    t.h.run_steps(2);
    t.h.get_by_label("Share").click();
    let deadline = Instant::now() + Duration::from_secs(20);
    while shares(t).is_empty() {
        assert!(Instant::now() < deadline, "no share; messages: {:?}", t.h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
}

#[test]
fn removing_a_recipient_keeps_one_added_since_the_list_was_read() {
    let mut t = two();
    let carol_dir = tempfile::tempdir().unwrap();
    let carol = {
        let _in = t.alice.runtime().enter();
        Session::open(sshcraft_engine::SessionConfig::for_tests(carol_dir.path())).unwrap().0
    };
    account(&carol, &t.origin, &t.api_key, "Carol", "carol@example.org");
    share_with_bob(&mut t);
    t.h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Settings;
    t.h.state_mut().ui.settings.section = sshcraft_ui_egui::pages::settings::Section::Sync;
    t.h.run_steps(2);
    // The "Shared." notice may lie over the button: the notices go first.
    t.h.state_mut().toasts = Default::default();
    t.h.run_steps(2);
    t.h.get_by_label("Shares").click_accesskit();
    until(&mut t.h, "the shares", |app| app.ui.shares.list.as_ref().is_some_and(|l| !l.is_empty()));
    // Another device adds Carol; the window still shows only Bob.
    let found = run(&t.alice, "share.find_recipient", json!({ "server": t.server, "email": "carol@example.org" }));
    run(&t.alice, "share.confirm_recipient", json!({ "server": t.server, "email": "carol@example.org", "account": found["account"] }));
    let share = shares(&t)[0].clone();
    let bob = share["grants"][0].clone();
    run(
        &t.alice,
        "share.set_grants",
        json!({ "server": t.server, "area": share["area"], "grants": [
            { "account": bob["account"], "role": bob["role"] },
            { "account": found["account"], "role": "read" },
        ] }),
    );
    assert_eq!(shares(&t)[0]["grants"].as_array().map(Vec::len), Some(2));
    t.h.get_by_label("Remove recipient").click();
    t.h.run_steps(2);
    t.h.get_by_label("Remove").click();
    let emails = |t: &Two| -> Vec<String> {
        shares(t)
            .iter()
            .flat_map(|s| s["grants"].as_array().cloned().unwrap_or_default())
            .filter_map(|g| g["email"].as_str().map(str::to_string))
            .collect()
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while emails(&t).contains(&"bob@example.org".to_string()) {
        assert!(Instant::now() < deadline, "Bob is still a recipient");
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    until(&mut t.h, "the removal", |app| app.runner.busy() == 0);
    assert_eq!(emails(&t), ["carol@example.org"], "Carol stays, Bob goes");
    drop(carol);
}

#[test]
fn a_changed_recipient_key_stops_the_share() {
    let mut t = two();
    find_bob(&mut t);
    {
        let share = t.h.state_mut().ui.share.as_mut().unwrap();
        let recipient = share.recipient.as_mut().unwrap();
        recipient["known"] = json!({ "state": "changed", "previous_safety_number": "11111 22222 33333 44444 55555 66666" });
    }
    t.h.run_steps(2);
    assert!(t.h.query_by_label_contains("11111 22222 33333 44444 55555 66666").is_some(), "the old number");
    assert!(t.h.query_by_label_contains("changed").is_some(), "the warning");
    assert!(t.h.get_by_label("Share").accesskit_node().is_disabled(), "no share until the new number is accepted");
    t.h.get_by_label("I compared the new safety number with Bob").click();
    t.h.run_steps(2);
    assert!(!t.h.get_by_label("Share").accesskit_node().is_disabled());
}

#[test]
fn removing_a_recipient_warns_and_ending_brings_the_group_back() {
    let mut t = two();
    find_bob(&mut t);
    t.h.state_mut().ui.share.as_mut().unwrap().with_keys = true;
    t.h.get_by_label("I compared the safety number with Bob").click();
    t.h.run_steps(2);
    t.h.get_by_label("Share").click();
    let deadline = Instant::now() + Duration::from_secs(20);
    while shares(&t).is_empty() {
        assert!(Instant::now() < deadline, "no share");
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    t.h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Settings;
    t.h.run_steps(2);
    // The "Shared." notice may lie over the section tabs: the section is chosen directly.
    t.h.state_mut().ui.settings.section = sshcraft_ui_egui::pages::settings::Section::Sync;
    t.h.run_steps(2);
    // The "Shared." notice may lie over the button: the notices go first.
    t.h.state_mut().toasts = Default::default();
    t.h.run_steps(2);
    t.h.get_by_label("Shares").click_accesskit();
    until(&mut t.h, "the shares", |app| app.ui.shares.list.as_ref().is_some_and(|l| !l.is_empty()));
    t.h.get_by_label("Remove recipient").click_accesskit();
    let deadline = Instant::now() + Duration::from_secs(20);
    while t.h.query_by_label_contains("may have been copied").is_none() {
        assert!(Instant::now() < deadline, "keys went along: it warns");
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    t.h.get_by_label("Cancel").click();
    t.h.run_steps(2);
    t.h.get_by_label("End share").click();
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    assert!(!shares(&t).is_empty(), "nothing ends before the confirmation");
    t.h.get_by_label("End sharing").click();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !shares(&t).is_empty() {
        assert!(Instant::now() < deadline, "the share did not end");
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
}

/// The shares come from the engine even while the window's list of servers is reloading.
#[test]
fn the_shares_load_while_the_server_list_reloads() {
    let mut t = two();
    find_bob(&mut t);
    t.h.get_by_label("I compared the safety number with Bob").click();
    t.h.run_steps(2);
    t.h.get_by_label("Share").click();
    let deadline = Instant::now() + Duration::from_secs(20);
    while shares(&t).is_empty() {
        assert!(Instant::now() < deadline, "no share");
        std::thread::sleep(Duration::from_millis(20));
        t.h.run_steps(1);
    }
    // The cached list of servers is empty for a moment (it reloads after a change).
    t.h.state_mut().data.servers.clear();
    sshcraft_ui_egui::pages::sharing::load(t.h.state_mut());
    until(&mut t.h, "the shares", |app| app.ui.shares.list.as_ref().is_some_and(|l| !l.is_empty()));
}
