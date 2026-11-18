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
/// The width of an entry in the grid.
const CELL_WIDTH: f32 = 340.0;
/// The most characters a tag chip shows; the whole tag is in its tooltip.
const MAX_TAG_CHARS: usize = 40;

/// What the hosts page shows: the group opened, the tags chosen, the search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostsView {
    pub group: Option<GroupId>,
    pub tags: Vec<String>,
    pub search: String,
    /// The dynamic group applied: hosts from every group are shown.
    pub smart: Option<String>,
    /// The name being typed for a new dynamic group.
    #[serde(skip)]
    pub smart_draft: Option<String>,
    #[serde(skip)]
    pub cache: HostsCache,
    #[serde(skip)]
    pub reachability: Reachability,
    #[serde(skip)]
    pub selection: Selection,
    /// "Show all" left the overview for the list of everything at the top level.
    #[serde(skip)]
    pub show_all: bool,
}

/// Hosts chosen with Ctrl or Shift click, for one change to all of them.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub hosts: std::collections::HashSet<HostId>,
    /// Where a Shift click counts from: a position in the shown hosts.
    pub anchor: Option<usize>,
    /// The fields of the bar.
    pub tag: String,
    pub user: String,
}

/// A click on the host at position `pos` of the shown list: Ctrl toggles it, Shift takes the
/// range from the last click, a plain click clears the choice.
pub(crate) fn select(app: &mut SshcraftApp, pos: usize, id: HostId, modifiers: egui::Modifiers) {
    let selection = &mut app.ui.hosts.selection;
    if modifiers.shift
        && let Some(anchor) = selection.anchor
    {
        let (from, to) = (anchor.min(pos), anchor.max(pos));
        let range: Vec<HostId> =
            app.ui.hosts.cache.shown.get(from..=to).unwrap_or_default().iter().filter_map(|&k| app.data.hosts.get(k)).map(|r| r.host.id).collect();
        selection.hosts.extend(range);
    } else if modifiers.command {
        if !selection.hosts.remove(&id) {
            selection.hosts.insert(id);
        }
        selection.anchor = Some(pos);
    } else {
        selection.hosts.clear();
        selection.anchor = Some(pos);
    }
}

/// Whether host `id` runs tmux in control mode (set on it or on a group above it).
pub fn uses_tmux_control(app: &SshcraftApp, id: HostId) -> bool {
    let Some(row) = app.data.hosts.iter().find(|r| r.host.id == id) else { return false };
    let groups: Vec<Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    let chain = crate::pages::host_form::chain(row.host.group_id, &groups);
    let persistence = row.host.settings.persistence.clone().or_else(|| chain.iter().find_map(|g| g.defaults.persistence.clone()));
    // A Telnet host has no tmux behind it: the engine never runs it there.
    let protocol = row.host.settings.protocol.or_else(|| chain.iter().find_map(|g| g.defaults.protocol)).unwrap_or_default();
    // Nor does a container: the engine opens it instead, from any level.
    let container = row.host.settings.container.is_some() || chain.iter().any(|g| g.defaults.container.is_some());
    protocol == sshcraft_model::model::Protocol::Ssh
        && !container
        && persistence.is_some_and(|p| p.tool == sshcraft_model::persist::Tool::TmuxControl)
}

/// A shared host whose owner's sign-in was not shared, and that has none of its own yet.
pub fn needs_own_sign_in(host: &sshcraft_model::model::Host) -> bool {
    let s = &host.settings;
    s.credentials_from_member == Some(true) && s.key.is_none() && s.password.is_none() && s.identity.is_none() && s.password_ref.is_none()
}

/// One change to every chosen host.
fn bulk(app: &mut SshcraftApp, patch: serde_json::Value) {
    let ids: Vec<HostId> = app.ui.hosts.selection.hosts.iter().copied().collect();
    app.run_reported("host.bulk_update", json!({ "ids": ids, "patch": patch }));
}

/// The bar over the list while hosts are chosen.
fn selection_bar(app: &mut SshcraftApp, ui: &mut Ui, all_groups: &[Group]) {
    let count = app.ui.hosts.selection.hosts.len();
    if count == 0 {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(trf("{count} hosts chosen", &[("count", &count.to_string())]));
        ui.menu_button(tr("Move to group"), |ui| {
            for (target, path) in host_form::group_targets(all_groups, None) {
                if ui.button(path).clicked() {
                    let patch = match target {
                        Some(group) => json!({ "group_id": group }),
                        None => json!({ "to_top_level": true }),
                    };
                    bulk(app, patch);
                }
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.hosts.selection.tag)
                .hint_text(tr("Tag"))
                .desired_width(crate::widgets::fit(ui, 100.0))
                .char_limit(32),
        );
        let tag = app.ui.hosts.selection.tag.trim().to_string();
        if ui.add_enabled(!tag.is_empty(), egui::Button::new(tr("Add tag"))).clicked() {
            bulk(app, json!({ "add_tags": [tag] }));
        }
        if ui.add_enabled(!tag.is_empty(), egui::Button::new(tr("Remove tag"))).clicked() {
            bulk(app, json!({ "remove_tags": [tag] }));
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.hosts.selection.user)
                .hint_text(tr("User"))
                .desired_width(crate::widgets::fit(ui, 100.0))
                .char_limit(100),
        );
        if ui.button(tr("Set user")).on_hover_text(tr("Empty: take the user from the group again")).clicked() {
            let user = app.ui.hosts.selection.user.trim().to_string();
            bulk(app, json!({ "username": user }));
        }
        if ui.button(tr("Clear choice")).clicked() {
            app.ui.hosts.selection = Selection::default();
        }
    });
    ui.add_space(4.0);
}

/// The last answer of `host.reachable`: `None` behind a jump host or proxy.
#[derive(Debug, Clone, Default)]
pub struct Reachability {
    pub hosts: std::collections::HashMap<HostId, Option<bool>>,
    /// When the last check started, in the window's time.
    pub checked_at: Option<f64>,
    pub loading: bool,
}

/// How often the list checks again while it is shown.
const REACHABILITY_EVERY: f64 = 60.0;
/// The most hosts one check asks for (the command's limit).
const REACHABILITY_MAX: usize = 500;

/// Checks the shown hosts once a minute while the setting is on.
/// Whether the page shows the overview: the top level with no search, tag, dynamic group or
/// "Show all".
fn is_overview(view: &HostsView) -> bool {
    view.group.is_none() && view.search.trim().is_empty() && view.tags.is_empty() && view.smart.is_none() && !view.show_all
}

fn refresh_reachability(app: &mut SshcraftApp, ctx: &egui::Context) {
    if !app.data.settings.appearance.show_reachability {
        app.ui.hosts.reachability = Reachability::default();
        return;
    }
    let now = ctx.input(|i| i.time);
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(REACHABILITY_EVERY));
    if app.ui.hosts.reachability.loading {
        return;
    }
    // The overview shows hosts in group cards and under "Recently connected" too, not only the
    // ones without a group in `shown`: there every host gets its dot.
    let ids: Vec<HostId> = if is_overview(&app.ui.hosts) {
        app.data.hosts.iter().map(|r| r.host.id).take(REACHABILITY_MAX).collect()
    } else {
        app.ui.hosts.cache.shown.iter().filter_map(|&k| app.data.hosts.get(k)).map(|r| r.host.id).take(REACHABILITY_MAX).collect()
    };
    // Due every minute, and at once for a host without a result yet (just loaded or added). An
    // empty list (the hosts are not loaded yet) checks nothing and starts no wait.
    let state = &app.ui.hosts.reachability;
    let unknown = ids.iter().any(|id| !state.hosts.contains_key(id));
    let due = unknown || state.checked_at.is_none_or(|at| now - at >= REACHABILITY_EVERY);
    if !due || ids.is_empty() {
        return;
    }
    app.ui.hosts.reachability.checked_at = Some(now);
    app.ui.hosts.reachability.loading = true;
    let asked = ids.clone();
    app.runner.run(
        "host.reachable",
        json!({ "ids": ids }),
        Box::new(move |app, result| {
            app.ui.hosts.reachability.loading = false;
            if let Ok(serde_json::Value::Array(rows)) = result {
                for row in rows {
                    if let Ok(id) = serde_json::from_value::<HostId>(row["id"].clone()) {
                        app.ui.hosts.reachability.hosts.insert(id, row["reachable"].as_bool());
                    }
                }
            }
            // Every host asked about has an entry now, so a failed check waits its minute
            // instead of being asked again in the next frame.
            for id in asked {
                app.ui.hosts.reachability.hosts.entry(id).or_insert(None);
            }
        }),
    );
}

/// What the lists were worked out for: the data version, the group, the tags, the search and
/// whether a dynamic group is applied.
type CacheKey = (u64, Option<GroupId>, Vec<String>, String, bool);

/// The page's lists, worked out again only when the data or the view changed: with thousands of
/// hosts, filtering and sorting them in every frame would not fit a frame.
#[derive(Debug, Clone, Default)]
pub struct HostsCache {
    key: Option<CacheKey>,
    groups: Vec<Group>,
    /// The groups shown at this level.
    sub: Vec<Group>,
    /// The tags of the hosts at this level or found, for the chips.
    tags: Vec<String>,
    /// The hosts shown (indices into `data.hosts`), by name.
    shown: Vec<usize>,
    /// Every group's own hosts (indices into `data.hosts`), by name, for the group cards.
    by_group: std::collections::HashMap<GroupId, Vec<usize>>,
    /// How many subgroups each group has.
    subgroups: std::collections::HashMap<GroupId, usize>,
}

impl HostsCache {
    /// Every group, for working out what a host inherits.
    pub(crate) fn groups(&self) -> &[Group] {
        &self.groups
    }
}

fn update_cache(app: &mut SshcraftApp) {
    let view = &app.ui.hosts;
    let everywhere = view.smart.is_some();
    let key = (app.data.version, view.group, view.tags.clone(), view.search.clone(), everywhere);
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
        .filter(|(_, r)| if searching { matches(r, &query, &groups) } else { everywhere || r.host.group_id == view.group })
        .map(|(i, _)| i)
        .collect();
    let mut tags: Vec<String> = rows.iter().filter_map(|&i| app.data.hosts.get(i)).flat_map(|r| r.host.tags.iter().cloned()).collect();
    tags.sort_by_cached_key(|t| t.to_lowercase());
    tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut shown: Vec<usize> = rows.into_iter().filter(|&i| app.data.hosts.get(i).is_some_and(|r| has_all_tags(&r.host.tags, &view.tags))).collect();
    shown.sort_by_cached_key(|&i| app.data.hosts.get(i).map(|r| r.host.name.to_lowercase()));
    let mut sub: Vec<Group> =
        if searching || everywhere { Vec::new() } else { groups.iter().filter(|g| g.parent_id == view.group).cloned().collect() };
    sub.sort_by_key(|g| g.name.to_lowercase());
    let mut by_group: std::collections::HashMap<GroupId, Vec<usize>> = std::collections::HashMap::new();
    for (i, row) in app.data.hosts.iter().enumerate() {
        if let Some(g) = row.host.group_id {
            by_group.entry(g).or_default().push(i);
        }
    }
    for list in by_group.values_mut() {
        list.sort_by_cached_key(|&i| app.data.hosts.get(i).map(|r| r.host.name.to_lowercase()));
    }
    let mut subgroups: std::collections::HashMap<GroupId, usize> = std::collections::HashMap::new();
    for g in &groups {
        if let Some(parent) = g.parent_id {
            *subgroups.entry(parent).or_default() += 1;
        }
    }
    app.ui.hosts.cache = HostsCache { key: Some(key), groups, sub, tags, shown, by_group, subgroups };
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
        let mut form = HostForm::new(row.host.clone(), true, row.has_password);
        form.has_totp = row.has_totp;
        app.ui.host_form = Some(form);
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
            leave_smart(app);
            app.ui.hosts.group = None;
            app.ui.hosts.show_all = false;
        }
        for g in chain(app.ui.hosts.group, all_groups).into_iter().rev() {
            ui.label(egui::RichText::new("/").color(tokens.text_dim));
            if elided_label(ui, &g.name, MAX_NAME_WIDTH, tokens.text, FontId::proportional(14.0)).clicked() {
                leave_smart(app);
                app.ui.hosts.group = Some(g.id);
            }
        }
    });
}

/// Leaves the dynamic group applied, with its filter.
fn leave_smart(app: &mut SshcraftApp) {
    if app.ui.hosts.smart.take().is_some() {
        app.ui.hosts.tags.clear();
        app.ui.hosts.search.clear();
    }
}

/// What a dynamic group filters, for its tooltip.
fn describe(group: &sshcraft_engine::settings::SmartGroup) -> String {
    let mut parts = Vec::new();
    if !group.tags.is_empty() {
        parts.push(trf("Tags: {tags}", &[("tags", &visible(&group.tags.join(", ")))]));
    }
    if !group.query.trim().is_empty() {
        parts.push(trf("Search: {text}", &[("text", &visible(&group.query))]));
    }
    parts.join("\n")
}

/// The dynamic groups: saved filters over the hosts of every group. A click applies one, a second
/// click leaves it; the tags and search chosen can be saved as a new one.
fn smart_chips(app: &mut SshcraftApp, ui: &mut Ui) {
    use sshcraft_engine::settings::{MAX_SMART_GROUPS, SmartGroup};
    let saved = app.data.settings.appearance.smart_groups.clone();
    // One removed on this device is no longer applied.
    if app.ui.hosts.smart.as_ref().is_some_and(|s| !saved.iter().any(|g| &g.name == s)) {
        app.ui.hosts.smart = None;
    }
    let view = &app.ui.hosts;
    let can_save = view.smart.is_none()
        && view.smart_draft.is_none()
        && (!view.tags.is_empty() || !view.search.trim().is_empty())
        && saved.len() < MAX_SMART_GROUPS;
    if saved.is_empty() && !can_save && view.smart_draft.is_none() {
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for g in &saved {
            let on = app.ui.hosts.smart.as_deref() == Some(g.name.as_str());
            let response = crate::widgets::buttons::chip(ui, &visible(&g.name), on).on_hover_text(describe(g));
            if response.clicked() {
                if on {
                    leave_smart(app);
                } else {
                    app.ui.hosts.smart = Some(g.name.clone());
                    app.ui.hosts.tags = g.tags.clone();
                    app.ui.hosts.search = g.query.clone();
                }
            }
            response.context_menu(|ui| {
                if ui.button(tr("Delete")).clicked() {
                    remove = Some(g.name.clone());
                }
            });
        }
        if can_save && ui.small_button(tr("Save as dynamic group")).clicked() {
            app.ui.hosts.smart_draft = Some(String::new());
        }
    });
    if let Some(name) = remove {
        if app.ui.hosts.smart.as_ref() == Some(&name) {
            leave_smart(app);
        }
        crate::pages::settings::save(app, |s| s.appearance.smart_groups.retain(|g| g.name != name));
    }
    let Some(mut name) = app.ui.hosts.smart_draft.take() else { return };
    let group = SmartGroup { name: name.trim().to_string(), tags: app.ui.hosts.tags.clone(), query: app.ui.hosts.search.trim().to_string() };
    let clash = saved.iter().any(|g| sshcraft_engine::settings::same_name(&g.name, &group.name));
    let mut keep = true;
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("Name")).char_limit(64).desired_width(crate::widgets::fit(ui, 200.0)));
        if ui.add_enabled(group.valid() && !clash, crate::widgets::buttons::main_action(tr("Save"))).clicked() {
            let applied = group.name.clone();
            crate::pages::settings::save(app, |s| s.appearance.smart_groups.push(group));
            app.ui.hosts.smart = Some(applied);
            keep = false;
        }
        if ui.button(tr("Cancel")).clicked() {
            keep = false;
        }
        if clash {
            ui.label(egui::RichText::new(tr("A dynamic group has that name already.")).color(Tokens::get(ui.ctx()).danger));
        }
    });
    if keep {
        app.ui.hosts.smart_draft = Some(name);
    }
}

fn tag_chips(app: &mut SshcraftApp, ui: &mut Ui) {
    // The overview shows every host (in cards too), so it offers every tag; a level or a search
    // offers the tags found there.
    let all = if is_overview(&app.ui.hosts) {
        let mut tags: Vec<String> = Vec::new();
        for tag in app.data.hosts.iter().flat_map(|r| r.host.tags.iter()) {
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                tags.push(tag.clone());
            }
        }
        tags.sort_by_key(|t| t.to_lowercase());
        tags
    } else {
        app.ui.hosts.cache.tags.clone()
    };
    if all.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for tag in all.iter().take(64) {
            let on = app.ui.hosts.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
            let shown: String = visible(tag).chars().take(MAX_TAG_CHARS).collect();
            let shown = if shown.chars().count() < visible(tag).chars().count() { format!("{shown}…") } else { shown };
            if crate::widgets::buttons::chip(ui, &shown, on).on_hover_text(visible(tag)).clicked() {
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
        response.context_menu(|ui| group_menu(app, ui, group));
    });
}

/// The menu of a group, on a right click on its name in a list or on its card.
fn group_menu(app: &mut SshcraftApp, ui: &mut Ui, group: &Group) {
    let id = group.id;
    if ui.button(tr("Open")).clicked() {
        app.ui.hosts.group = Some(id);
    }
    if ui.button(tr("Edit")).clicked() {
        edit_group(app, id);
    }
    if ui.button(tr("Move to server…")).clicked() {
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
}

fn host_entry(app: &mut SshcraftApp, ui: &mut Ui, pos: usize, row: &HostRow, all_groups: &[Group]) {
    crate::pages::host_items::host_row(app, ui, pos, row, all_groups);
}

/// The menu of a host, on a right click in any of its rows.
pub(crate) fn host_menu(app: &mut SshcraftApp, ui: &mut Ui, row: &HostRow, all_groups: &[Group]) {
    let id = row.host.id;
    if ui.add(crate::widgets::buttons::main_action(tr("Connect"))).clicked() {
        app.ui.connect_request = Some(ConnectRequest::Host(id));
    }
    if ui.button(tr("Move to server…")).clicked() {
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
    if ui.button(tr("Earlier versions…")).clicked() {
        crate::pages::trash::open_versions(app, "host", &id.0.to_string(), &row.host.name);
    }
    ui.menu_button(tr("Move to group"), |ui| {
        for (target, path) in host_form::group_targets(all_groups, None) {
            if ui.button(path).clicked() {
                let mut host = row.host.clone();
                host.group_id = target;
                app.run_reported("host.save", json!({ "host": host }));
            }
        }
    });
    if app.form == crate::app::Form::Desktop && ui.button(tr("Desktop shortcut")).clicked() {
        app.runner.run(
            "host.shortcut",
            json!({ "host_id": id }),
            Box::new(|app, result| match result {
                Ok(v) => {
                    let path = v.get("path").and_then(serde_json::Value::as_str).map(visible).unwrap_or_default();
                    app.toasts.info(&trf("The shortcut is at {path}.", &[("path", &path)]));
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        );
    }
    if ui.button(tr("Forget host key")).clicked() {
        app.run_reported("host.forget_key", json!({ "host_id": id }));
    }
    if ui.button(tr("Write to ~/.ssh/config")).clicked() {
        app.runner.run(
            "sshconfig.write",
            json!({ "host_ids": [id] }),
            Box::new(|app, result| match result {
                Ok(v) => {
                    let files: Vec<String> =
                        v.get("files").and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(|f| f.as_str()).map(visible).collect();
                    app.toasts
                        .info(&trf("Written to {files}; a copy of the old file is in ~/.ssh/.sshcraft-backup.", &[("files", &files.join(", "))]));
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        );
    }
    if ui.button(tr("Delete")).clicked() {
        ask_delete_host(app, id);
    }
}

fn confirm(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(confirm) = app.ui.confirm.clone() else { return };
    let text = match &confirm {
        Confirm::DeleteHost(_, name) => trf("Delete the host {name}?", &[("name", &visible(name))]),
        Confirm::DeleteGroup(_, name) => trf("Delete the group {name} with its subgroups and all their hosts?", &[("name", &visible(name))]),
    };
    crate::widgets::modal(ctx, egui::Id::unique("sshcraft-confirm")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
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
    refresh_reachability(app, ui.ctx());
    let all_groups = app.ui.hosts.cache.groups.clone();
    if app.ui.hosts.group.is_some() || !app.ui.hosts.search.trim().is_empty() {
        app.ui.hosts.show_all = false;
    }
    let overview = is_overview(&app.ui.hosts);
    header(app, ui, overview);
    if overview {
        ui.add_space(12.0);
        smart_chips(app, ui);
        tag_chips(app, ui);
        selection_bar(app, ui, &all_groups);
        ui.add_space(16.0);
        overview_sections(app, ui, &all_groups);
        new_host_bar(app, ui);
        dialogs(app, ui);
        return;
    }
    ui.add_space(6.0);
    smart_chips(app, ui);
    path(app, ui, &all_groups);
    tag_chips(app, ui);
    selection_bar(app, ui, &all_groups);
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
    let row_height = crate::pages::host_items::row_height(app);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_height, count.div_ceil(per_row), |ui, range| {
        for line in range {
            ui.horizontal(|ui| {
                ui.set_height(row_height);
                for i in line * per_row..((line + 1) * per_row).min(count) {
                    let width = if grid { CELL_WIDTH } else { ui.available_width() };
                    ui.allocate_ui_with_layout(vec2(width, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(width);
                        if let Some(g) = sub.get(i) {
                            group_entry(app, ui, g);
                        } else if let Some(row) = i.checked_sub(sub.len()).and_then(|j| shown.get(j)).and_then(|&k| app.data.hosts.get(k)).cloned() {
                            host_entry(app, ui, i - sub.len(), &row, &all_groups);
                        }
                    });
                }
            });
        }
    });
    new_host_bar(app, ui);
    dialogs(app, ui);
}

/// The page's title, the search field and the buttons beside it, laid out for the window's width:
/// a wide window shows "New group" and "New host" by name, a medium one a blue plus, a phone only
/// the search (its bar holds "New group", "New host" floats at the bottom). Import, export and the
/// list or grid choice sit behind the button with three dots.
fn header(app: &mut SshcraftApp, ui: &mut Ui, overview: bool) {
    use crate::shell::Layout as L;
    use crate::widgets::buttons;
    let layout = crate::shell::layout_for(ui.ctx().content_rect().width());
    if layout != L::Narrow {
        let heading = if overview { tr("Where do you want to connect?") } else { tr("Hosts") };
        let size = if layout == L::Wide { 28.0 } else { 26.0 };
        buttons::heading(ui, heading, size);
        ui.add_space(14.0);
    }
    // Both read or write files: offered where one can be picked.
    let files = crate::file_dialogs::can_pick(app);
    let height = if crate::pages::host_items::touch(app) { 54.0 } else { 50.0 };
    let gap = 10.0;
    let more = files || !overview || layout == L::Medium;
    let mut right = 0.0;
    if layout == L::Wide {
        right += buttons::width_for(ui, tr("New group"), false, height) + gap;
        right += buttons::width_for(ui, tr("New host"), true, height) + gap;
    } else if layout == L::Medium {
        right += height + gap;
    }
    if more && layout != L::Narrow {
        right += height + gap;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let width = (ui.available_width() - right).max(160.0);
        buttons::search(ui, &mut app.ui.hosts.search, tr("Search hosts"), vec2(width, height));
        if layout == L::Wide {
            if buttons::ghost(ui, tr("New group"), height).clicked() {
                new_group(app);
            }
            if buttons::primary(ui, Some(Icon::Plus), tr("New host"), height).clicked() {
                new_host(app);
            }
        } else if layout == L::Medium && buttons::square(ui, Icon::Plus, tr("New host"), height, true).clicked() {
            new_host(app);
        }
        if more && layout != L::Narrow {
            let response = buttons::square(ui, Icon::More, tr("More"), height, false);
            egui::Popup::menu(&response).show(|ui| {
                ui.set_min_width(180.0);
                if layout == L::Medium && ui.button(tr("New group")).clicked() {
                    new_group(app);
                }
                if files && ui.button(tr("Import")).clicked() {
                    app.ui.import = Some(crate::pages::import_export::ImportState::default());
                }
                if files && ui.button(tr("Export")).clicked() {
                    app.ui.export = Some(crate::pages::import_export::ExportState { group: app.ui.hosts.group, ..Default::default() });
                }
                if !overview {
                    view_toggle(app, ui);
                }
            });
        }
    });
    if layout == L::Narrow && !overview {
        ui.add_space(6.0);
        view_toggle(app, ui);
    }
}

/// Switches the hosts between a list and a grid.
fn view_toggle(app: &mut SshcraftApp, ui: &mut Ui) {
    let grid = app.data.settings.appearance.host_view == HostView::Grid;
    let toggle = if grid { tr("List") } else { tr("Grid") };
    if ui.button(toggle).clicked() {
        let mut settings = app.data.settings.clone();
        settings.appearance.host_view = if grid { HostView::List } else { HostView::Grid };
        app.run_reported("settings.save", json!({ "settings": settings }));
    }
}

/// On a phone, "New host" floats as a wide blue button at the bottom of the page.
fn new_host_bar(app: &mut SshcraftApp, ui: &mut Ui) {
    if crate::shell::layout_for(ui.ctx().content_rect().width()) != crate::shell::Layout::Narrow {
        return;
    }
    let screen = ui.ctx().content_rect();
    let rect =
        egui::Rect::from_min_max(egui::pos2(screen.left() + 14.0, screen.bottom() - 76.0), egui::pos2(screen.right() - 14.0, screen.bottom() - 22.0));
    let mut clicked = false;
    egui::Area::new(egui::Id::unique("sshcraft-new-host-bar")).order(egui::Order::Middle).fixed_pos(rect.min).show(ui.ctx(), |ui| {
        let tokens = Tokens::get(ui.ctx());
        let (at, response) = ui.allocate_exact_size(rect.size(), Sense::click());
        ui.painter().rect_filled(at.translate(vec2(0.0, 6.0)), 16.0, egui::Color32::from_black_alpha(70));
        ui.painter().rect_filled(at, 16.0, if response.hovered() { tokens.accent_strong } else { tokens.accent });
        let galley = ui.painter().layout_no_wrap(tr("New host").to_string(), FontId::proportional(16.0), egui::Color32::WHITE);
        let start = at.center().x - (galley.size().x + 30.0) / 2.0;
        draw(ui.painter(), Icon::Plus, egui::Rect::from_center_size(egui::pos2(start + 10.0, at.center().y), vec2(20.0, 20.0)), egui::Color32::WHITE);
        ui.painter().galley(egui::pos2(start + 30.0, at.center().y - galley.size().y / 2.0), galley, egui::Color32::WHITE);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("New host")));
        clicked = response.clicked();
    });
    if clicked {
        new_host(app);
    }
}

/// The forms and questions the Hosts page opens.
fn dialogs(app: &mut SshcraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    host_form::show_host(app, &ctx);
    host_form::show_group(app, &ctx);
    confirm(app, &ctx);
    crate::pages::trash::versions_dialog(app, &ctx);
    crate::pages::import_export::show_import(app, &ctx);
    crate::pages::import_export::show_export(app, &ctx);
}

/// At most this many hosts in a group card; the rest behind "Show all".
const GROUP_CARD_HOSTS: usize = 8;
/// At most this many hosts in "Without a group".
const LOOSE_HOSTS: usize = 12;
/// The gap between cards and tiles.
const GAP: f32 = 14.0;

/// Lays `count` items out in columns at least `min_width` wide; `item(ui, index, width, height)`
/// draws one, at least `height` high: the tallest of its row in the last frame, so a row's cards
/// line up at the bottom.
fn columns(ui: &mut Ui, id: &str, count: usize, min_width: f32, mut item: impl FnMut(&mut Ui, usize, f32, f32)) {
    let full = ui.available_width();
    let cols = ((full + GAP) / (min_width + GAP)).floor().max(1.0) as usize;
    let width = ((full - GAP * (cols as f32 - 1.0)) / cols as f32).max(1.0);
    let mut i = 0;
    let mut line = 0;
    while i < count {
        let key = ui.scope_id().with((id, line, cols));
        let last: f32 = ui.ctx().data(|d| d.get_temp(key)).unwrap_or(0.0);
        let mut tallest: f32 = 0.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for j in i..(i + cols).min(count) {
                let shown = ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(width);
                    item(ui, j, width, last);
                });
                tallest = tallest.max(shown.response.rect.height());
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, tallest));
        ui.add_space(GAP);
        i += cols;
        line += 1;
    }
}

/// A section's heading.
fn section(ui: &mut Ui, title: &str) {
    let tokens = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(title).size(14.0).strong().color(tokens.text_dim));
    ui.add_space(12.0);
}

/// A host row on a tile of its own, as in "Without a group".
fn host_tile(app: &mut SshcraftApp, ui: &mut Ui, pos: usize, row: &HostRow, all_groups: &[Group], width: f32) {
    let tokens = Tokens::get(ui.ctx());
    let height = crate::pages::host_items::row_height(app) + 8.0;
    egui::Frame::new().fill(tokens.panel).stroke(egui::Stroke::new(1.0, tokens.separator)).corner_radius(14).inner_margin(1.0).show(ui, |ui| {
        ui.set_max_width(width - 2.0);
        let inner = (width - 6.0).max(1.0);
        ui.allocate_ui_with_layout(vec2(inner, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(inner, height));
            crate::pages::host_items::host_row(app, ui, pos, row, all_groups);
        });
    });
}

/// A card of hosts: a title (which opens the group when `open` is set), how many hosts it has,
/// its first hosts, how many subgroups, and "Show all" when it holds more than fit.
#[allow(clippy::too_many_arguments)]
fn host_group_card(
    app: &mut SshcraftApp,
    ui: &mut Ui,
    title: &str,
    open: Option<&Group>,
    hosts: &[usize],
    subgroups: usize,
    all_groups: &[Group],
    width: f32,
    min_height: f32,
) {
    let tokens = Tokens::get(ui.ctx());
    let row_height = crate::pages::host_items::row_height(app);
    egui::Frame::new().fill(tokens.panel).stroke(egui::Stroke::new(1.0, tokens.separator)).corner_radius(18).inner_margin(8.0).show(ui, |ui| {
        let inner = (width - 18.0).max(1.0);
        ui.set_width(inner);
        ui.set_min_height((min_height - 18.0).max(0.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.allocate_ui_with_layout(vec2(inner, 38.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(inner, 38.0));
            ui.add_space(12.0);
            let name = egui::RichText::new(visible(title)).strong().size(15.5).color(tokens.text_strong);
            let response = ui.add(egui::Label::new(name).sense(if open.is_some() { Sense::click() } else { Sense::hover() }));
            if let Some(group) = open {
                if response.clicked() {
                    app.ui.hosts.group = Some(group.id);
                }
                response.context_menu(|ui| group_menu(app, ui, group));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                let count = if hosts.len() == 1 { tr("1 host").to_string() } else { trf("{count} hosts", &[("count", &hosts.len().to_string())]) };
                ui.label(egui::RichText::new(count).size(13.0).color(tokens.text_dim));
            });
        });
        for (n, &k) in hosts.iter().take(GROUP_CARD_HOSTS).enumerate() {
            if let Some(row) = app.data.hosts.get(k).cloned() {
                ui.allocate_ui_with_layout(vec2(inner, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_size(vec2(inner, row_height));
                    crate::pages::host_items::host_row(app, ui, n, &row, all_groups);
                });
            }
        }
        if subgroups > 0 || hosts.len() > GROUP_CARD_HOSTS {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if subgroups > 0 {
                    ui.label(egui::RichText::new(trf("{count} subgroups", &[("count", &subgroups.to_string())])).size(13.0).color(tokens.text_dim));
                }
                if hosts.len() > GROUP_CARD_HOSTS && ui.link(trf("Show all {count}", &[("count", &hosts.len().to_string())])).clicked() {
                    match open {
                        Some(group) => app.ui.hosts.group = Some(group.id),
                        None => app.ui.hosts.show_all = true,
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// The overview at the root: recently connected hosts, the hosts without a group, the groups.
/// On a phone the recent hosts scroll sideways and the hosts without a group form a card like
/// a group's.
fn overview_sections(app: &mut SshcraftApp, ui: &mut Ui, all_groups: &[Group]) {
    let tokens = Tokens::get(ui.ctx());
    let narrow = crate::shell::layout_for(ui.ctx().content_rect().width()) == crate::shell::Layout::Narrow;
    let recent: Vec<HostRow> = app.data.recent.iter().filter_map(|id| app.data.hosts.iter().find(|r| r.host.id == *id)).take(4).cloned().collect();
    let loose: Vec<usize> = app.ui.hosts.cache.shown.clone();
    let groups: Vec<Group> = app.ui.hosts.cache.sub.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if !recent.is_empty() {
            section(ui, tr("Recently connected"));
            if narrow {
                egui::ScrollArea::horizontal().id_salt("sshcraft-recent").show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for row in &recent {
                            ui.allocate_ui_with_layout(vec2(156.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(crate::widgets::fit(ui, 156.0));
                                crate::pages::host_items::host_card(app, ui, row, true);
                            });
                        }
                    });
                });
                ui.add_space(16.0);
            } else {
                columns(ui, "recent", recent.len(), 230.0, |ui, i, _, _| {
                    if let Some(row) = recent.get(i) {
                        crate::pages::host_items::host_card(app, ui, row, false);
                    }
                });
                ui.add_space(14.0);
            }
        }
        if !loose.is_empty() {
            if narrow {
                let width = ui.available_width();
                host_group_card(app, ui, tr("Without a group"), None, &loose, 0, all_groups, width, 0.0);
                ui.add_space(16.0);
            } else {
                section(ui, tr("Without a group"));
                columns(ui, "loose", loose.len().min(LOOSE_HOSTS), 230.0, |ui, i, width, _| {
                    if let Some(row) = loose.get(i).and_then(|&k| app.data.hosts.get(k)).cloned() {
                        host_tile(app, ui, i, &row, all_groups, width);
                    }
                });
                if loose.len() > LOOSE_HOSTS && ui.link(trf("Show all {count}", &[("count", &loose.len().to_string())])).clicked() {
                    app.ui.hosts.show_all = true;
                }
                ui.add_space(14.0);
            }
        }
        if !groups.is_empty() {
            if !narrow {
                section(ui, tr("Groups"));
            }
            // A tablet shows the groups one under the other, a wide window side by side.
            let min_width = if crate::shell::layout_for(ui.ctx().content_rect().width()) == crate::shell::Layout::Medium { 420.0 } else { 300.0 };
            columns(ui, "groups", groups.len(), min_width, |ui, i, width, height| {
                if let Some(group) = groups.get(i) {
                    let hosts = app.ui.hosts.cache.by_group.get(&group.id).cloned().unwrap_or_default();
                    let subgroups = app.ui.hosts.cache.subgroups.get(&group.id).copied().unwrap_or(0);
                    host_group_card(app, ui, &group.name, Some(group), &hosts, subgroups, all_groups, width, height);
                }
            });
        }
        if recent.is_empty() && loose.is_empty() && groups.is_empty() {
            ui.label(egui::RichText::new(tr("No hosts here yet.")).color(tokens.text_dim));
        }
        // Room under the last card for the floating "New host" of a phone.
        if narrow {
            ui.add_space(84.0);
        }
    });
}
