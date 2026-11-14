mod common;

use common::{Dev, TestServer, create, engine, join, sync};
use sshcraft_model::model::{
    ConnectionSettings, Group, GroupId, Host, HostId, group_chain, resolve,
};
use sshcraft_store::store::ItemKind;
use sshcraft_sync::engine::{record_group_hosts, record_local_change};

fn group(name: &str, parent: Option<GroupId>, server: sshcraft_model::model::ServerId) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: parent,
        tags: Vec::new(),
    }
}

fn save_group(dev: &Dev, g: &Group, at: i64) {
    let store = dev.store.lock().unwrap();
    store.upsert_group(g).unwrap();
    record_local_change(&store, ItemKind::Group, &g.id.0.to_string(), at).unwrap();
}

fn find(dev: &Dev, id: GroupId) -> Group {
    dev.store
        .lock()
        .unwrap()
        .list_groups()
        .unwrap()
        .into_iter()
        .find(|g| g.id == id)
        .unwrap()
}

#[tokio::test]
async fn subgroups_reach_other_devices() {
    let server = TestServer::start().await;
    let (x, y) = (Dev::new(), Dev::new());
    let export = create(&server, &x, "x@example.org").await;
    join(&server, &y, &export).await;
    let s = x.row().id;
    let mut top = group("top", None, s);
    top.defaults.address = Some("top.example.org".into());
    let mut middle = group("middle", Some(top.id), s);
    middle.defaults.username = Some("deploy".into());
    let bottom = group("bottom", Some(middle.id), s);
    save_group(&x, &top, 1);
    save_group(&x, &middle, 2);
    save_group(&x, &bottom, 3);
    let host = Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: "h".into(),
        group_id: Some(bottom.id),
        settings: ConnectionSettings {
            key: Some(sshcraft_model::model::KeyRef::Agent {
                fingerprint: "SHA256:x".into(),
            }),
            ..Default::default()
        },
        location: Some(s),
        tags: vec!["prod".into()],
        os: Some(sshcraft_model::model::HostOs::Debian),
    };
    {
        let store = x.store.lock().unwrap();
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 4).unwrap();
    }
    sync(&x).await;
    sync(&y).await;
    let groups = y.store.lock().unwrap().list_groups().unwrap();
    let chain = group_chain(&groups, bottom.id);
    assert_eq!(chain.len(), 3);
    let on_y = y.hosts().pop().unwrap();
    assert_eq!(on_y.tags, vec!["prod".to_owned()]);
    assert_eq!(on_y.os, Some(sshcraft_model::model::HostOs::Debian));
    let refs: Vec<&Group> = chain.iter().collect();
    let resolved = resolve(&on_y, &refs, &[]).unwrap();
    assert_eq!(
        (resolved.address.as_str(), resolved.username.as_str()),
        ("top.example.org", "deploy")
    );
}

#[tokio::test]
async fn child_before_parent_is_applied_later() {
    let server = TestServer::start().await;
    let (x, y) = (Dev::new(), Dev::new());
    let export = create(&server, &x, "x@example.org").await;
    join(&server, &y, &export).await;
    let s = x.row().id;
    let parent = group("parent", None, s);
    let child = group("child", Some(parent.id), s);
    {
        let store = x.store.lock().unwrap();
        store.upsert_group(&parent).unwrap();
        store.upsert_group(&child).unwrap();
        // The child goes out first.
        record_local_change(&store, ItemKind::Group, &child.id.0.to_string(), 1).unwrap();
        record_local_change(&store, ItemKind::Group, &parent.id.0.to_string(), 2).unwrap();
    }
    sync(&x).await;
    sync(&y).await;
    sync(&y).await;
    assert_eq!(find(&y, child.id).parent_id, Some(parent.id));
}

#[tokio::test]
async fn cycle_from_the_server_is_refused() {
    let server = TestServer::start().await;
    let (x, y) = (Dev::new(), Dev::new());
    let export = create(&server, &x, "x@example.org").await;
    join(&server, &y, &export).await;
    let s = x.row().id;
    let a = group("a", None, s);
    let b = group("b", None, s);
    save_group(&x, &a, 1);
    save_group(&x, &b, 1);
    sync(&x).await;
    sync(&y).await;
    // X puts A under B, Y (later) puts B under A.
    save_group(
        &x,
        &Group {
            parent_id: Some(b.id),
            ..a.clone()
        },
        100,
    );
    save_group(
        &y,
        &Group {
            parent_id: Some(a.id),
            ..find(&y, b.id)
        },
        200,
    );
    sync(&x).await;
    let mut ey = engine(&y).await;
    let report = ey.pull(&y.store, &y.vault).await.unwrap();
    assert!(
        !report.rejected.is_empty(),
        "the older change is refused: {report:?}"
    );
    ey.push(&y.store, &y.vault, 0).await.unwrap();
    for _ in 0..2 {
        sync(&x).await;
        sync(&y).await;
    }
    for dev in [&x, &y] {
        assert_eq!(
            find(dev, b.id).parent_id,
            Some(a.id),
            "the later change wins"
        );
        assert_eq!(find(dev, a.id).parent_id, None);
    }
}

#[tokio::test]
async fn moving_a_group_takes_subgroups_along() {
    let server = TestServer::start().await;
    let x = Dev::new();
    create(&server, &x, "x@example.org").await;
    let s = x.row().id;
    let mut top = group("top", None, s);
    top.location = None;
    let mut sub = group("sub", Some(top.id), s);
    sub.location = None;
    let host = Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: "h".into(),
        group_id: Some(sub.id),
        settings: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
        os: None,
    };
    {
        let store = x.store.lock().unwrap();
        store.upsert_group(&top).unwrap();
        store.upsert_group(&sub).unwrap();
        store.upsert_host(&host).unwrap();
        store
            .set_location(ItemKind::Group, &top.id.0.to_string(), Some(s))
            .unwrap();
        record_local_change(&store, ItemKind::Group, &top.id.0.to_string(), 5).unwrap();
        record_group_hosts(&store, &top.id.0.to_string(), 5).unwrap();
        let outbox = store.outbox(s).unwrap();
        assert!(outbox.contains(&(ItemKind::Group, sub.id.0.to_string())));
        assert!(outbox.contains(&(ItemKind::Host, host.id.0.to_string())));
    }
    assert_eq!(find(&x, sub.id).location, Some(s));
}
