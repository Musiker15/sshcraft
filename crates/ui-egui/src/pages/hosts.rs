//! The hosts page: one group level at a time with the path above it, tag chips that filter with
//! "and", a search over names, addresses, users, the groups above and tags, a grid or a list, and
//! a menu on every host and group.

use egui::{FontId, Sense, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sshcraft_engine::settings::HostView;
use sshcraft_model::model::{Group, GroupId, Host, HostId};

use crate::app::SshcraftApp;
use crate::data::HostRow;
use crate::i18n::{tr, trf};
use crate::pages::host_form::{self, GroupForm, HostForm, chain};
use crate::state::ConnectRequest;
use crate::theme::Tokens;
use crate::widgets::icons::{Icon, draw};
use crate::widgets::{elided_label, visible};

/// The widest a host or group name is drawn; longer names are cut.
pub const MAX_NAME_WIDTH: f32 = 260.0;

/// What the hosts page shows: the group opened, the tags chosen, the search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostsView {
    pub group: Option<GroupId>,
    pub tags: Vec<String>,
    pub search: String,
}

/// Something that waits for "Delete" to be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    DeleteHost(HostId, String),
    DeleteGroup(GroupId, String),
}

fn groups(app: &SshcraftApp) -> Vec<Group> {
    app.data.groups.iter().map(|g| g.group.clone()).collect()
}

pub fn new_host(app: &mut SshcraftApp) {
    let host = Host {
        id: HostId::new(),
        name: String::new(),
        group_id: app.ui.hosts.group,
        settings: Default::default(),
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    app.ui.host_form = Some(HostForm::new(host, false, false));
}

pub fn edit_host(app: &mut SshcraftApp, id: HostId) {
    if let Some(row) = app.data.hosts.iter().find(|r| r.host.id == id) {
        app.ui.host_form = Some(HostForm::new(row.host.clone(), true, row.has_password));
    }
}

pub fn duplicate_host(app: &mut SshcraftApp, id: HostId) {
    if let Some(row) = app.data.hosts.iter().find(|r| r.host.id == id) {
        let mut host = row.host.clone();
        host.id = HostId::new();
        host.name = trf("{name} (copy)", &[("name", &host.name)]);
        // A stored password stays with the original; the copy asks for its own.
        host.settings.password = None;
        app.ui.host_form = Some(HostForm::new(host, false, false));
    }
}

pub fn new_group(app: &mut SshcraftApp) {
    let group = Group {
        id: GroupId::new(),
        name: String::new(),
        defaults: Default::default(),
        location: None,
        parent_id: app.ui.hosts.group,
        tags: Vec::new(),
    };
    app.ui.group_form = Some(GroupForm::new(group, false, false));
}

pub fn edit_group(app: &mut SshcraftApp, id: GroupId) {
    if let Some(row) = app.data.groups.iter().find(|g| g.group.id == id) {
        app.ui.group_form = Some(GroupForm::new(row.group.clone(), true, row.has_password));
    }
}

pub fn ask_delete_host(app: &mut SshcraftApp, id: HostId) {
    if let Some(row) = app.data.hosts.iter().find(|r| r.host.id == id) {
        app.ui.confirm = Some(Confirm::DeleteHost(id, row.host.name.clone()));
    }
}

pub fn ask_delete_group(app: &mut SshcraftApp, id: GroupId) {
    if let Some(row) = app.data.groups.iter().find(|g| g.group.id == id) {
        app.ui.confirm = Some(Confirm::DeleteGroup(id, row.group.name.clone()));
    }
}

/// Whether `row` matches the search: its name, address, user, tags or a group above it.
fn matches(row: &HostRow, query: &str, all_groups: &[Group]) -> bool {
    if query.is_empty() {
        return true;
    }
    let h = &row.host;
    let has = |text: &str| text.to_lowercase().contains(query);
    has(&h.name)
        || h.settings.address.as_deref().is_some_and(has)
        || h.settings.username.as_deref().is_some_and(has)
        || h.tags.iter().any(|t| has(t))
        || chain(h.group_id, all_groups).iter().any(|g| has(&g.name))
}

fn has_all_tags(tags: &[String], wanted: &[String]) -> bool {
    wanted.iter().all(|w| tags.iter().any(|t| t.eq_ignore_ascii_case(w)))
}

fn path(app: &mut SshcraftApp, ui: &mut Ui, all_groups: &[Group]) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        if elided_label(ui, tr("All hosts"), MAX_NAME_WIDTH, tokens.accent, FontId::proportional(14.0)).clicked() {
            app.ui.hosts.group = None;
        }
        for g in chain(app.ui.hosts.group, all_groups).into_iter().rev() {
            ui.label(egui::RichText::new("/").color(tokens.text_dim));
            if elided_label(ui, &g.name, MAX_NAME_WIDTH, tokens.text, FontId::proportional(14.0)).clicked() {
                app.ui.hosts.group = Some(g.id);
            }
        }
    });
}

fn tag_chips(app: &mut SshcraftApp, ui: &mut Ui, hosts: &[&HostRow]) {
    let mut all: Vec<String> = hosts.iter().flat_map(|r| r.host.tags.iter().cloned()).collect();
    all.sort_by_key(|t| t.to_lowercase());
    all.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if all.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for tag in all.iter().take(64) {
            let on = app.ui.hosts.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
            if ui.selectable_label(on, visible(tag)).clicked() {
                if on {
                    app.ui.hosts.tags.retain(|t| !t.eq_ignore_ascii_case(tag));
                } else {
                    app.ui.hosts.tags.push(tag.clone());
                }
            }
        }
    });
}

fn group_entry(app: &mut SshcraftApp, ui: &mut Ui, group: &Group) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        draw(ui.painter(), Icon::Group, icon, tokens.accent);
        let response = elided_label(ui, &group.name, MAX_NAME_WIDTH, tokens.text, FontId::proportional(14.0));
        if response.double_clicked() {
            app.ui.hosts.group = Some(group.id);
        }
        let id = group.id;
        response.context_menu(|ui| {
            if ui.button(tr("Open")).clicked() {
                app.ui.hosts.group = Some(id);
            }
            if ui.button(tr("Edit")).clicked() {
                edit_group(app, id);
            }
            if ui.button(tr("Delete")).clicked() {
                ask_delete_group(app, id);
            }
        });
    });
}

fn host_entry(app: &mut SshcraftApp, ui: &mut Ui, row: &HostRow, all_groups: &[Group]) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        draw(ui.painter(), Icon::Host, icon, tokens.text_dim);
        let response = elided_label(ui, &row.host.name, MAX_NAME_WIDTH, tokens.text, FontId::proportional(14.0));
        if let Some(address) = &row.host.settings.address {
            ui.label(egui::RichText::new(visible(address)).color(tokens.text_dim));
        }
        let id = row.host.id;
        if response.double_clicked() {
            app.ui.connect_request = Some(ConnectRequest::Host(id));
        }
        response.context_menu(|ui| {
            if ui.button(tr("Connect")).clicked() {
                app.ui.connect_request = Some(ConnectRequest::Host(id));
            }
            if ui.button(tr("Edit")).clicked() {
                edit_host(app, id);
            }
            if ui.button(tr("Duplicate")).clicked() {
                duplicate_host(app, id);
            }
            ui.menu_button(tr("Move to"), |ui| {
                for (target, path) in host_form::group_targets(all_groups, None) {
                    if ui.button(path).clicked() {
                        let mut host = row.host.clone();
                        host.group_id = target;
                        app.run_reported("host.save", json!({ "host": host }));
                    }
                }
            });
            if ui.button(tr("Forget host key")).clicked() {
                app.run_reported("host.forget_key", json!({ "host_id": id }));
            }
            if ui.button(tr("Delete")).clicked() {
                ask_delete_host(app, id);
            }
        });
    });
}

fn confirm(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(confirm) = app.ui.confirm.clone() else { return };
    let text = match &confirm {
        Confirm::DeleteHost(_, name) => trf("Delete the host {name}?", &[("name", &visible(name))]),
        Confirm::DeleteGroup(_, name) => trf("Delete the group {name} with its subgroups and all their hosts?", &[("name", &visible(name))]),
    };
    egui::Modal::new(egui::Id::unique("sshcraft-confirm")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(text);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Delete")).clicked() {
                match &confirm {
                    Confirm::DeleteHost(id, _) => app.run_reported("host.delete", json!({ "id": id })),
                    Confirm::DeleteGroup(id, _) => {
                        if app.ui.hosts.group == Some(*id) {
                            app.ui.hosts.group = None;
                        }
                        app.run_reported("group.delete", json!({ "id": id }));
                    }
                }
                app.ui.confirm = None;
            }
            if ui.button(tr("Cancel")).clicked() {
                app.ui.confirm = None;
            }
        });
    });
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let all_groups = groups(app);
    ui.horizontal(|ui| {
        ui.heading(tr("Hosts"));
        ui.add_space(16.0);
        if ui.button(tr("New host")).clicked() {
            new_host(app);
        }
        if ui.button(tr("New group")).clicked() {
            new_group(app);
        }
        ui.add(egui::TextEdit::singleline(&mut app.ui.hosts.search).hint_text(tr("Search")).desired_width(220.0));
        let grid = app.data.settings.appearance.host_view == HostView::Grid;
        let toggle = if grid { tr("List") } else { tr("Grid") };
        if ui.button(toggle).clicked() {
            let mut settings = app.data.settings.clone();
            settings.appearance.host_view = if grid { HostView::List } else { HostView::Grid };
            app.run_reported("settings.save", json!({ "settings": settings }));
        }
    });
    ui.add_space(6.0);
    path(app, ui, &all_groups);
    let query = app.ui.hosts.search.trim().to_lowercase();
    let searching = !query.is_empty();
    let rows: Vec<HostRow> = app
        .data
        .hosts
        .iter()
        .filter(|r| if searching { matches(r, &query, &all_groups) } else { r.host.group_id == app.ui.hosts.group })
        .cloned()
        .collect();
    let refs: Vec<&HostRow> = rows.iter().collect();
    tag_chips(app, ui, &refs);
    ui.add_space(6.0);
    let wanted = app.ui.hosts.tags.clone();
    let mut shown: Vec<&HostRow> = rows.iter().filter(|r| has_all_tags(&r.host.tags, &wanted)).collect();
    shown.sort_by_key(|r| r.host.name.to_lowercase());
    let mut sub: Vec<Group> = if searching { Vec::new() } else { all_groups.iter().filter(|g| g.parent_id == app.ui.hosts.group).cloned().collect() };
    sub.sort_by_key(|g| g.name.to_lowercase());
    let grid = app.data.settings.appearance.host_view == HostView::Grid;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut body = |ui: &mut Ui| {
            for g in &sub {
                group_entry(app, ui, g);
            }
            for row in &shown {
                host_entry(app, ui, row, &all_groups);
            }
        };
        if grid {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(24.0, 10.0);
                body(ui);
            });
        } else {
            body(ui);
        }
        if sub.is_empty() && shown.is_empty() {
            ui.label(egui::RichText::new(tr("No hosts here yet.")).color(Tokens::get(ui.ctx()).text_dim));
        }
    });
    let ctx = ui.ctx().clone();
    host_form::show_host(app, &ctx);
    host_form::show_group(app, &ctx);
    confirm(app, &ctx);
}
