//! Settings > Sync against a sync server in this process: an account created with its key shown
//! once, a second device joining with it, devices revoked (never the recovery one), items moved,
//! the server's state, and the list of connections SSHCraft makes by itself.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session};
use sshcraft_store::vault::{MasterKeyProvider, MemoryMasterKey, VaultError};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::sync::{CreateForm, JoinForm};
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::{app_harness, app_harness_with};
use zeroize::Zeroizing;

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

/// A sync server in this process; its origin and API key. It stops when the sender is dropped.
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

fn sync_page(h: &mut H) {
    h.state_mut().ui.page = Page::Settings;
    h.run_steps(3);
    h.get_by_label("Sync").click();
    h.run_steps(3);
}

/// Creates the account through the dialog; the account key it showed.
fn create(h: &mut H, origin: &str, api_key: &str) -> String {
    sync_page(h);
    h.get_by_label("Create account").click();
    h.run_steps(3);
    *h.state_mut().ui.sync.create.as_mut().expect("the form opened") = CreateForm {
        server_name: "home".into(),
        origin: origin.into(),
        api_key: api_key.into(),
        name: "Alice".into(),
        email: "alice@example.org".into(),
        device_name: "laptop".into(),
    };
    h.run_steps(2);
    h.get_by_label("Create").click();
    until(h, "the account key", |app| app.ui.sync.account_key().is_some());
    // The dialog that shows it is drawn on the next frames.
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.query_by_label("I have kept the key").is_none() {
        assert!(Instant::now() < deadline, "the account key dialog");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.state().ui.sync.account_key().unwrap().to_string()
}

#[test]
fn removing_a_server_asks_first_and_says_what_it_takes() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&_session, server_dir.path());
    create(&mut h, &origin, &api_key);
    h.get_by_label("I have kept the key").click();
    until(&mut h, "the server", |app| app.data.servers.iter().any(|s| s["name"] == "home"));
    // A notice ("Account created") may lie over the button: the notices go first.
    h.state_mut().toasts = Default::default();
    h.run_steps(2);
    h.get_by_label("Remove server").click();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.query_by_label_contains("account key").is_none() {
        assert!(Instant::now() < deadline, "it says the account key is needed to join again");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::SERVERS;
    h.run_steps(5);
    assert!(h.state().data.servers.iter().any(|s| s["name"] == "home"), "nothing is removed before the confirmation");
    h.get_by_label("Remove and sign out").click();
    until(&mut h, "the removal", |app| app.data.servers.is_empty());
}

#[test]
fn an_account_is_created_and_its_key_shown_once() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&session, server_dir.path());
    let key = create(&mut h, &origin, &api_key);
    assert!(h.query_by_label("Account key as QR code").is_some(), "a QR code to scan");
    assert!(format!("{:?}", h.root()).contains(&key), "the key is shown");
    h.get_by_label("I have kept the key").click();
    h.run_steps(3);
    assert!(h.state().ui.sync.account_key().is_none(), "the window lets the key go");
    assert!(!format!("{:?}", h.state().ui).contains(&key), "nothing keeps the key");
    until(&mut h, "the server", |app| app.data.servers.iter().any(|s| s["name"] == "home"));
    let connections = session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "app.connections", serde_json::Value::Null)).unwrap();
    assert!(connections.to_string().contains(&origin), "the connections SSHCraft makes by itself list the server");
    until(&mut h, "the server's state", |app| app.data.sync_status.values().any(|s| s["state"] == "idle"));
    // The state may change again (a sync runs at once): the label follows it.
    let deadline = Instant::now() + Duration::from_secs(10);
    // The page and the sidebar both say it.
    while h.query_all_by_label_contains("Up to date").next().is_none() {
        assert!(Instant::now() < deadline, "the page says it is up to date");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn a_second_device_joins_and_devices_are_revoked_but_never_the_recovery_one() {
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&session, server_dir.path());
    let key = create(&mut h, &origin, &api_key);
    h.get_by_label("I have kept the key").click();
    h.run_steps(3);

    let (mut other, _other_session) = app_harness(other_dir.path());
    sync_page(&mut other);
    other.get_by_label("Join an account").click();
    other.run_steps(3);
    *other.state_mut().ui.sync.join.as_mut().expect("the join form") =
        JoinForm { server_name: "home".into(), origin: origin.clone(), api_key: api_key.clone(), account_key: key, device_name: "desktop".into() };
    other.run_steps(2);
    other.get_by_label("Join").click();
    until(&mut other, "the joined server", |app| app.data.servers.iter().any(|s| s["name"] == "home"));

    until(&mut h, "the server", |app| !app.data.servers.is_empty());
    h.get_by_label("Devices").click();
    until(&mut h, "the devices", |app| app.ui.sync.devices.as_ref().is_some_and(|d| d.len() >= 3));
    let devices: Vec<Value> = h.state().ui.sync.devices.clone().unwrap();
    let recovery = devices.iter().position(|d| d["recovery"] == true).expect("the recovery device");
    let desktop = devices.iter().position(|d| d["name"] == "desktop").expect("the joined device");
    let buttons: Vec<bool> = h.query_all_by_label("Revoke").map(|n| n.accesskit_node().is_disabled()).collect();
    assert!(buttons[recovery], "the recovery device cannot be revoked");
    h.get_all_by_label("Revoke").nth(desktop).unwrap().click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("may have been copied").is_some(), "it warns about copied keys");
    h.get_by_label("Revoke the device").click();
    until(&mut h, "the revoked device", |app| {
        app.ui.sync.devices.as_ref().is_some_and(|d| d.iter().any(|x| x["name"] == "desktop" && x["active"] == false))
    });
}

#[test]
fn a_group_moves_with_its_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&session, server_dir.path());
    create(&mut h, &origin, &api_key);
    h.get_by_label("I have kept the key").click();
    let group = uuid::Uuid::new_v4();
    let run = |id: &str, params| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("group.save", json!({ "group": { "id": group, "name": "web", "defaults": {} } }));
    run("host.save", json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "web-01", "group_id": group, "settings": { "address": "192.0.2.1" } } }));
    until(&mut h, "the server and the group", |app| !app.data.servers.is_empty() && !app.data.groups.is_empty());
    let server = h.state().data.servers[0]["id"].as_str().unwrap().to_string();
    sshcraft_ui_egui::pages::sync::ask_move(h.state_mut(), "group", &group.to_string(), "web");
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "home").click();
    h.run_steps(1);
    h.get_by_label("Move").click();
    until(&mut h, "the moved group", |app| app.data.hosts.first().and_then(|r| r.host.location.map(|l| l.0.to_string())) == Some(server.clone()));
}

/// Sync runs again after a restart, without touching its switch: the app starts it when it
/// opens (it ran only in the session that set the server up, and every restart showed
/// "Sync off" until the switch was flipped).
#[test]
fn sync_runs_again_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    // The server lives on its own runtime, so it outlives the first app.
    let server_runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    // One master key for both apps, as the OS keyring keeps it across restarts.
    let key = Arc::new(MemoryMasterKey::default());
    let (origin, api_key, _stop) = {
        let (mut h, session) = app_harness_with(dir.path(), shared_key(&key));
        let _ = &mut h;
        let started = server_runtime.block_on(async {
            let database_url = format!("sqlite://{}", server_dir.path().join("server.db").display());
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
        });
        create(&mut h, &started.0, &started.1);
        drop(session);
        started
    };
    let _ = (&origin, &api_key);
    // The restart: a new app on the same data.
    let (mut h, _session) = app_harness_with(dir.path(), shared_key(&key));
    sync_page(&mut h);
    until(&mut h, "sync to run after the restart", |app| app.data.sync_status.values().any(|s| s["state"] == "idle"));
}
/// A master key two sessions share.
struct SharedKey(Arc<MemoryMasterKey>);

impl MasterKeyProvider for SharedKey {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        self.0.load()
    }

    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        self.0.save(key)
    }
}

fn shared_key(key: &Arc<MemoryMasterKey>) -> impl FnOnce(&mut sshcraft_engine::SessionConfig) {
    let key = Arc::clone(key);
    move |config| config.master_key = Box::new(SharedKey(key))
}

/// A host in a group cannot move on its own: asking to move it moves its top group, and the
/// dialog says so (the move used to fail without a word, and the host stayed on the device).
#[test]
fn moving_a_host_in_a_group_moves_its_top_group() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&session, server_dir.path());
    create(&mut h, &origin, &api_key);
    h.get_by_label("I have kept the key").click();
    let (top, sub, host) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let run = |id: &str, params| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("group.save", json!({ "group": { "id": top, "name": "web", "defaults": {} } }));
    run("group.save", json!({ "group": { "id": sub, "name": "frontend", "parent_id": top, "defaults": {} } }));
    run("host.save", json!({ "host": { "id": host, "name": "web-01", "group_id": sub, "settings": { "address": "192.0.2.1" } } }));
    until(&mut h, "the server and the groups", |app| !app.data.servers.is_empty() && app.data.groups.len() == 2 && !app.data.hosts.is_empty());
    let server = h.state().data.servers[0]["id"].as_str().unwrap().to_string();
    sshcraft_ui_egui::pages::sync::ask_move(h.state_mut(), "host", &host.to_string(), "web-01");
    h.run_steps(3);
    assert!(h.query_by_label_contains("the whole group web").is_some(), "the dialog says the group moves");
    h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "home").click();
    h.run_steps(1);
    h.get_by_label("Move").click();
    until(&mut h, "the moved host", |app| app.data.hosts.first().and_then(|r| r.host.location.map(|l| l.0.to_string())) == Some(server.clone()));
    assert!(h.state().data.groups.iter().all(|g| g.group.location.map(|l| l.0.to_string()) == Some(server.clone())), "both groups moved with it");
}

/// The switch "Sync everything" moves what lives only on this device to the server and stays
/// on; it starts off.
#[test]
fn sync_everything_moves_what_is_here() {
    let dir = tempfile::tempdir().unwrap();
    let server_dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (origin, api_key, _stop) = sync_server(&session, server_dir.path());
    create(&mut h, &origin, &api_key);
    h.get_by_label("I have kept the key").click();
    let run = |id: &str, params| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("host.save", json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "web-01", "settings": { "address": "192.0.2.1" } } }));
    until(&mut h, "the server and the host", |app| !app.data.servers.is_empty() && !app.data.hosts.is_empty());
    let server = h.state().data.servers[0]["id"].as_str().unwrap().to_string();
    assert_eq!(h.state().data.servers[0]["sync_all"], false, "it starts off");
    assert_eq!(h.state().data.hosts[0].host.location, None);
    h.get_by_label("Sync everything").click();
    until(&mut h, "the host on the server", |app| {
        app.data.hosts.first().and_then(|r| r.host.location.map(|l| l.0.to_string())) == Some(server.clone())
    });
    until(&mut h, "the switch on", |app| app.data.servers.first().is_some_and(|s| s["sync_all"] == true));
}
