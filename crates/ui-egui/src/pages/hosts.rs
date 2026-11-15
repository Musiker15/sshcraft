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
/// The height of one row of entries, and the width of an entry in the grid.
const ROW_HEIGHT: f32 = 26.0;
const CELL_WIDTH: f32 = 460.0;
/// The most characters a tag chip shows; the whole tag is in its tooltip.
const MAX_TAG_CHARS: usize = 40;

/// What the hosts page shows: the group opened, the tags chosen, the search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostsView {
    pub group: Option<GroupId>,
    pub tags: Vec<String>,
    pub search: String,
    #[serde(skip)]
    pub cache: HostsCache,
}

/// The page's lists, worked out again only when the data or the view changed: with thousands of
/// hosts, filtering and sorting them in every frame would not fit a frame.
#[derive(Debug, Clone, Default)]
pub struct HostsCache {
    key: Option<(u64, Option<GroupId>, Vec<String>, String)>,
    groups: Vec<Group>,
    /// The groups shown at this level.
    sub: Vec<Group>,
    /// The tags of the hosts at this level or found, for the chips.
    tags: Vec<String>,
    /// The hosts shown (indices into `data.hosts`), by name.
    shown: Vec<usize>,
}

fn update_cache(app: &mut SshcraftApp) {
    let view = &app.ui.hosts;
    let key = (app.data.version, view.group, view.tags.clone(), view.search.clone());
    if view.cache.key.as_ref() == Some(&key) {
        return;
    }
    let groups: Vec<Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    let query = view.search.trim().to_lowercase();
    let searching = !query.is_empty();
    let rows: Vec<usize> = app
        .data
        .hosts
        .iter()
        .enumerate()
        .filter(|(_, r)| if searching { matches(r, &query, &groups) } else { r.host.group_id == view.group })
        .map(|(i, _)| i)
        .collect();
    let mut tags: Vec<String> = rows.iter().filter_map(|&i| app.data.hosts.get(i)).flat_map(|r| r.host.tags.iter().cloned()).collect();
    tags.sort_by_cached_key(|t| t.to_lowercase());
    tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut shown: Vec<usize> = rows.into_iter().filter(|&i| app.data.hosts.get(i).is_some_and(|r| has_all_tags(&r.host.tags, &view.tags))).collect();
    shown.sort_by_cached_key(|&i| app.data.hosts.get(i).map(|r| r.host.name.to_lowercase()));
    let mut sub: Vec<Group> = if searching { Vec::new() } else { groups.iter().filter(|g| g.parent_id == view.group).cloned().collect() };
    sub.sort_by_key(|g| g.name.to_lowercase());
    app.ui.hosts.cache = HostsCache { key: Some(key), groups, sub, tags, shown };
}

/// Something that waits for "Delete" to be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    DeleteHost(HostId, String),
    DeleteGroup(GroupId, String),
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

fn tag_chips(app: &mut SshcraftApp, ui: &mut Ui) {
    let all = app.ui.hosts.cache.tags.clone();
    if all.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for tag in all.iter().take(64) {
            let on = app.ui.hosts.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
            let shown: String = visible(tag).chars().take(MAX_TAG_CHARS).collect();
            let shown = if shown.chars().count() < visible(tag).chars().count() { format!("{shown}…") } else { shown };
            if ui.selectable_label(on, shown).on_hover_text(visible(tag)).clicked() {
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
            if ui.button(tr("Move to…")).clicked() {
                crate::pages::sync::ask_move(app, "group", &id.0.to_string(), &group.name);
            }
            if ui.button(tr("Share…")).clicked() {
                crate::pages::sharing::ask_share(app, &id.0.to_string(), None);
            }
            if ui.button(tr("Hide from agents")).clicked() {
                crate::pages::agents::set_hidden(app, &id.0.to_string(), true);
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
            elided_label(ui, address, 140.0, tokens.text_dim, FontId::proportional(13.0));
        }
        let id = row.host.id;
        if response.double_clicked() {
            app.ui.connect_request = Some(ConnectRequest::Host(id));
        }
        response.context_menu(|ui| {
            if ui.button(tr("Connect")).clicked() {
                app.ui.connect_request = Some(ConnectRequest::Host(id));
            }
            if ui.button(tr("Move to…")).clicked() {
                crate::pages::sync::ask_move(app, "host", &id.0.to_string(), &row.host.name);
            }
            if ui.button(tr("Share…")).clicked() {
                let group = row.host.group_id.map(|g| g.0.to_string()).unwrap_or_default();
                crate::pages::sharing::ask_share(app, &group, Some(&id.0.to_string()));
            }
            if ui.button(tr("Hide from agents")).clicked() {
                crate::pages::agents::set_hidden(app, &id.0.to_string(), true);
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
    update_cache(app);
    let all_groups = app.ui.hosts.cache.groups.clone();
    ui.horizontal(|ui| {
        ui.heading(tr("Hosts"));
        ui.add_space(16.0);
        if ui.button(tr("New host")).clicked() {
            new_host(app);
        }
        if ui.button(tr("New group")).clicked() {
            new_group(app);
        }
        if ui.button(tr("Import")).clicked() {
            app.ui.import = Some(crate::pages::import_export::ImportState::default());
        }
        if ui.button(tr("Export")).clicked() {
            app.ui.export = Some(crate::pages::import_export::ExportState { group: app.ui.hosts.group, ..Default::default() });
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
    tag_chips(app, ui);
    ui.add_space(6.0);
    let sub = app.ui.hosts.cache.sub.clone();
    let shown = app.ui.hosts.cache.shown.clone();
    let count = sub.len() + shown.len();
    if count == 0 {
        ui.label(egui::RichText::new(tr("No hosts here yet.")).color(Tokens::get(ui.ctx()).text_dim));
    }
    let grid = app.data.settings.appearance.host_view == HostView::Grid;
    let per_row = if grid { ((ui.available_width() / CELL_WIDTH).floor() as usize).max(1) } else { 1 };
    // Only the rows in view are drawn.
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, count.div_ceil(per_row), |ui, range| {
        for line in range {
            ui.horizontal(|ui| {
                ui.set_height(ROW_HEIGHT);
                for i in line * per_row..((line + 1) * per_row).min(count) {
                    let width = if grid { CELL_WIDTH } else { ui.available_width() };
                    ui.allocate_ui_with_layout(vec2(width, ROW_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(width);
                        if let Some(g) = sub.get(i) {
                            group_entry(app, ui, g);
                        } else if let Some(row) = i.checked_sub(sub.len()).and_then(|j| shown.get(j)).and_then(|&k| app.data.hosts.get(k)).cloned() {
                            host_entry(app, ui, &row, &all_groups);
                        }
                    });
                }
            });
        }
    });
    let ctx = ui.ctx().clone();
    host_form::show_host(app, &ctx);
    host_form::show_group(app, &ctx);
    confirm(app, &ctx);
    crate::pages::import_export::show_import(app, &ctx);
    crate::pages::import_export::show_export(app, &ctx);
}
