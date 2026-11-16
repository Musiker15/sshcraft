//! Colour, badge and theme per host or group, as the tabs and terminals take them.

use sshcraft_model::appearance::Appearance;
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId};
use sshcraft_ui_egui::data::{GroupRow, HostRow};
use sshcraft_ui_egui::session_tab::host_appearance;
use sshcraft_ui_egui::testing::app_harness;

#[test]
fn a_host_takes_what_it_does_not_set_from_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let group = Group {
        id: GroupId::new(),
        name: "prod".into(),
        defaults: ConnectionSettings {
            appearance: Some(Appearance { color: Some([200, 0, 0]), badge: Some("PROD".into()), theme: Some("sshcraft-light".into()) }),
            ..Default::default()
        },
        location: None,
        parent_id: None,
        tags: Vec::new(),
    };
    let host = Host {
        id: HostId::new(),
        name: "db1".into(),
        group_id: Some(group.id),
        settings: ConnectionSettings { appearance: Some(Appearance { badge: Some("DB".into()), ..Default::default() }), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    let app = h.state_mut();
    app.data.groups = vec![GroupRow { group, has_password: false, proxy_has_password: false }];
    app.data.hosts = vec![HostRow { host: host.clone(), has_password: false, proxy_has_password: false, has_totp: false }];
    let look = host_appearance(app, Some(host.id));
    assert_eq!(look.color, Some([200, 0, 0]));
    assert_eq!(look.badge.as_deref(), Some("DB"));
    assert_eq!(look.theme.as_deref(), Some("sshcraft-light"));
    assert_eq!(host_appearance(app, None), Appearance::default());
}
