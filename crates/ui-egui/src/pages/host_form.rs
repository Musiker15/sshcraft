//! The forms for a host and a group: their connection settings, with the values inherited from
//! the groups above shown as hints naming the group they come from. A stored password is never
//! loaded into a field; the form only says that one is stored and offers to replace or remove it.

use egui::Ui;
use serde_json::json;
use sshcraft_model::model::{ConnectionSettings, EnvVar, Group, GroupId, Host, HostId, HostOs, KeyRef, Proxy, ProxyEndpoint};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::widgets::visible;

/// Groups nest at most this deep (the engine refuses deeper chains as well).
const MAX_DEPTH: usize = 64;

/// A value and the name of the group it comes from.
pub type From<T> = Option<(T, String)>;

/// What a host or group would take from the groups above it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inherited {
    pub address: From<String>,
    pub port: From<u16>,
    pub username: From<String>,
    pub key: From<KeyRef>,
    pub agent_forwarding: From<bool>,
}

/// The groups from `group` upwards, nearest first.
pub fn chain(group: Option<GroupId>, groups: &[Group]) -> Vec<&Group> {
    let mut out = Vec::new();
    let mut next = group;
    while let Some(id) = next {
        let Some(g) = groups.iter().find(|g| g.id == id) else { break };
        if out.len() >= MAX_DEPTH || out.iter().any(|seen: &&Group| seen.id == g.id) {
            break;
        }
        out.push(g);
        next = g.parent_id;
    }
    out
}

/// The values the groups from `group` upwards give, each from the nearest group that sets it.
pub fn inherited(group: Option<GroupId>, groups: &[Group]) -> Inherited {
    let mut out = Inherited::default();
    for g in chain(group, groups) {
        let d = &g.defaults;
        let name = || g.name.clone();
        if out.address.is_none()
            && let Some(a) = d.address.clone().filter(|a| !a.is_empty())
        {
            out.address = Some((a, name()));
        }
        if out.port.is_none()
            && let Some(p) = d.port
        {
            out.port = Some((p, name()));
        }
        if out.username.is_none()
            && let Some(u) = d.username.clone().filter(|u| !u.is_empty())
        {
            out.username = Some((u, name()));
        }
        if out.key.is_none()
            && let Some(k) = d.key.clone()
        {
            out.key = Some((k, name()));
        }
        if out.agent_forwarding.is_none()
            && let Some(f) = d.agent_forwarding
        {
            out.agent_forwarding = Some((f, name()));
        }
    }
    out
}

/// Where a host or group can go: the top level and every group, except `moving` itself and the
/// groups inside it. Each with its path (`prod / eu`).
pub fn group_targets(groups: &[Group], moving: Option<GroupId>) -> Vec<(Option<GroupId>, String)> {
    let mut out = vec![(None, tr("Top level").to_string())];
    for g in groups {
        let path = chain(Some(g.id), groups);
        if moving.is_some_and(|m| path.iter().any(|p| p.id == m)) {
            continue;
        }
        let names: Vec<String> = path.iter().rev().map(|p| visible(&p.name)).collect();
        out.push((Some(g.id), names.join(" / ")));
    }
    out.sort_by_key(|(_, path)| path.to_lowercase());
    out
}

/// A host being created or edited.
#[derive(Debug, Clone)]
pub struct HostForm {
    pub host: Host,
    pub editing: bool,
    /// A new password; empty keeps what is stored.
    pub password: String,
    /// A password is stored now.
    pub has_password: bool,
    pub remove_password: bool,
    pub tags: String,
    pub port: String,
    pub error: Option<String>,
}

impl HostForm {
    pub fn new(host: Host, editing: bool, has_password: bool) -> Self {
        let tags = host.tags.join(", ");
        let port = host.settings.port.map(|p| p.to_string()).unwrap_or_default();
        Self { host, editing, password: String::new(), has_password, remove_password: false, tags, port, error: None }
    }
}

/// A group being created or edited.
#[derive(Debug, Clone)]
pub struct GroupForm {
    pub group: Group,
    pub editing: bool,
    pub password: String,
    pub has_password: bool,
    pub remove_password: bool,
    pub tags: String,
    pub port: String,
    pub error: Option<String>,
}

fn split_tags(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// The port field as a port; empty is "not set".
fn parse_port(text: &str) -> Result<Option<u16>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<u16>() {
        Ok(p) if p > 0 => Ok(Some(p)),
        _ => Err(tr("The port is a number from 1 to 65535.").to_string()),
    }
}

/// A text field for an optional value, the inherited value as its hint.
fn optional_text(ui: &mut Ui, label: &str, value: &mut Option<String>, hint: Option<String>) {
    ui.label(label);
    let mut text = value.clone().unwrap_or_default();
    let mut edit = egui::TextEdit::singleline(&mut text).desired_width(280.0);
    if let Some(hint) = hint {
        edit = edit.hint_text(hint);
    }
    if ui.add(edit).changed() {
        *value = if text.trim().is_empty() { None } else { Some(text) };
    }
    ui.end_row();
}

fn from_hint<T: std::fmt::Display>(value: &From<T>) -> Option<String> {
    value.as_ref().map(|(v, group)| trf("{value} (from {group})", &[("value", &visible(&v.to_string())), ("group", &visible(group))]))
}

fn tri_state(ui: &mut Ui, label: &str, value: &mut Option<bool>, inherited: &From<bool>) {
    ui.label(label);
    let inherit = match inherited {
        Some((v, group)) => {
            let state = if *v { tr("on") } else { tr("off") };
            trf("Inherit ({state}, from {group})", &[("state", state), ("group", &visible(group))])
        }
        None => tr("Inherit").to_string(),
    };
    ui.horizontal(|ui| {
        ui.radio_value(value, None, inherit);
        ui.radio_value(value, Some(true), tr("On"));
        ui.radio_value(value, Some(false), tr("Off"));
    });
    ui.end_row();
}

fn key_label(app: &SshcraftApp, key: &KeyRef) -> String {
    match key {
        KeyRef::Stored { id } => {
            app.data.keys.iter().find(|k| k.id == id.0.to_string()).map_or_else(|| tr("Unknown key").to_string(), |k| visible(&k.name))
        }
        KeyRef::Agent { fingerprint } => trf("Agent key {fingerprint}", &[("fingerprint", fingerprint)]),
    }
}

/// The connection settings shared by hosts and groups.
fn settings_editor(ui: &mut Ui, app: &SshcraftApp, s: &mut ConnectionSettings, port: &mut String, inherited: &Inherited, own_host: Option<HostId>) {
    egui::Grid::new("sshcraft-settings").num_columns(2).spacing([12.0, 8.0]).min_col_width(130.0).show(ui, |ui| {
        optional_text(ui, tr("Address"), &mut s.address, from_hint(&inherited.address));
        ui.label(tr("Port"));
        let mut edit = egui::TextEdit::singleline(port).desired_width(80.0);
        if let Some(hint) = from_hint(&inherited.port) {
            edit = edit.hint_text(hint);
        }
        ui.add(edit);
        ui.end_row();
        optional_text(ui, tr("User"), &mut s.username, from_hint(&inherited.username));

        ui.label(tr("Key"));
        let shown = s.key.as_ref().map_or_else(
            || {
                inherited.key.as_ref().map_or_else(
                    || tr("None").to_string(),
                    |(k, g)| trf("{value} (from {group})", &[("value", &key_label(app, k)), ("group", &visible(g))]),
                )
            },
            |k| key_label(app, k),
        );
        egui::ComboBox::from_id_salt("sshcraft-key").selected_text(shown).width(280.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.key, None, tr("Inherit or none"));
            for key in &app.data.keys {
                if let Ok(uuid) = uuid::Uuid::parse_str(&key.id) {
                    let id = sshcraft_model::model::KeyId(uuid);
                    ui.selectable_value(&mut s.key, Some(KeyRef::Stored { id }), visible(&key.name));
                }
            }
        });
        ui.end_row();

        ui.label(tr("Identity"));
        let identity = s
            .identity
            .and_then(|id| app.data.identities.iter().find(|i| i.id == id.0.to_string()))
            .map_or_else(|| tr("None").to_string(), |i| visible(&i.name));
        egui::ComboBox::from_id_salt("sshcraft-identity").selected_text(identity).width(280.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.identity, None, tr("None"));
            for i in &app.data.identities {
                if let Ok(uuid) = uuid::Uuid::parse_str(&i.id) {
                    ui.selectable_value(&mut s.identity, Some(sshcraft_model::model::IdentityId(uuid)), visible(&i.name));
                }
            }
        });
        ui.end_row();

        ui.label(tr("Jump hosts"));
        ui.vertical(|ui| {
            let mut jumps = s.jump_hosts.clone().unwrap_or_default();
            for row in app.data.hosts.iter().filter(|r| Some(r.host.id) != own_host) {
                let mut on = jumps.contains(&row.host.id);
                if ui.checkbox(&mut on, visible(&row.host.name)).changed() {
                    if on {
                        jumps.push(row.host.id);
                    } else {
                        jumps.retain(|j| *j != row.host.id);
                    }
                    s.jump_hosts = if jumps.is_empty() { None } else { Some(jumps.clone()) };
                }
            }
        });
        ui.end_row();

        ui.label(tr("Proxy"));
        ui.vertical(|ui| {
            let kind = match &s.proxy {
                None => 0,
                Some(Proxy::Direct) => 1,
                Some(Proxy::Http(_)) => 2,
                Some(Proxy::Socks5(_)) => 3,
            };
            let mut chosen = kind;
            ui.horizontal(|ui| {
                ui.radio_value(&mut chosen, 0, tr("Inherit"));
                ui.radio_value(&mut chosen, 1, tr("Direct"));
                ui.radio_value(&mut chosen, 2, "HTTP");
                ui.radio_value(&mut chosen, 3, "SOCKS5");
            });
            if chosen != kind {
                let endpoint = || ProxyEndpoint { address: String::new(), port: 1080, username: None, password: None };
                s.proxy = match chosen {
                    1 => Some(Proxy::Direct),
                    2 => Some(Proxy::Http(endpoint())),
                    3 => Some(Proxy::Socks5(endpoint())),
                    _ => None,
                };
            }
            if let Some(Proxy::Http(e) | Proxy::Socks5(e)) = &mut s.proxy {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut e.address).hint_text(tr("Proxy address")).desired_width(180.0));
                    ui.add(egui::DragValue::new(&mut e.port).range(1..=65535));
                });
            }
        });
        ui.end_row();

        tri_state(ui, tr("Agent forwarding"), &mut s.agent_forwarding, &inherited.agent_forwarding);
        tri_state(ui, tr("Record sessions"), &mut s.session_log, &None);

        ui.label(tr("Environment"));
        ui.vertical(|ui| {
            let mut remove = None;
            for (i, var) in s.env.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut var.name).hint_text(tr("Name")).desired_width(120.0));
                    ui.add(egui::TextEdit::singleline(&mut var.value).hint_text(tr("Value")).desired_width(150.0));
                    if ui.small_button("×").on_hover_text(tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                s.env.remove(i);
            }
            if ui.small_button(tr("Add variable")).clicked() {
                s.env.push(EnvVar { name: String::new(), value: String::new() });
            }
        });
        ui.end_row();
    });
}

fn password_field(ui: &mut Ui, password: &mut String, has_password: bool, remove: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(tr("Password"));
        let hint = if has_password { tr("Stored; type to replace") } else { tr("None stored") };
        crate::widgets::secret_field(ui, password, egui::Id::unique("sshcraft-form-password"), true, hint, 220.0);
        if has_password {
            ui.checkbox(remove, tr("Remove"));
        }
    });
}

const OSES: [(Option<HostOs>, &str); 8] = [
    (None, "Not set"),
    (Some(HostOs::Linux), "Linux"),
    (Some(HostOs::Debian), "Debian"),
    (Some(HostOs::Ubuntu), "Ubuntu"),
    (Some(HostOs::Fedora), "Fedora"),
    (Some(HostOs::FreeBsd), "FreeBSD"),
    (Some(HostOs::MacOs), "macOS"),
    (Some(HostOs::Windows), "Windows"),
];

/// After a save: the password set or removed as the form says, then the form closes.
fn after_save(app: &mut SshcraftApp, owner: serde_json::Value, password: String, remove: bool, close: fn(&mut SshcraftApp)) {
    if !password.is_empty() {
        app.run_reported("password.set", json!({ "owner": owner, "secret": password }));
    } else if remove {
        app.run_reported("password.clear", json!({ "owner": owner }));
    }
    close(app);
}

/// The scrolled part of a form: the window minus room for title and buttons, so Save stays
/// on screen.
fn form_height(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().height() - 200.0).max(160.0)
}

/// Draws the host form when open.
pub fn show_host(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.host_form.take() else { return };
    let busy = app.forms.busy();
    let groups: Vec<Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    let inherited = inherited(form.host.group_id, &groups);
    let mut open = true;
    let mut cancel = false;
    let mut save = false;
    let title = if form.editing { tr("Edit host") } else { tr("New host") };
    let modal = egui::Modal::new(egui::Id::unique(title)).show(ctx, |ui| {
        ui.set_width(540.0);
        ui.heading(title);
        egui::ScrollArea::vertical().max_height(form_height(ui.ctx())).min_scrolled_height(form_height(ui.ctx())).auto_shrink([false, false]).show(
            ui,
            |ui| {
                egui::Grid::new("sshcraft-host").num_columns(2).spacing([12.0, 8.0]).min_col_width(130.0).show(ui, |ui| {
                    ui.label(tr("Name"));
                    ui.add(egui::TextEdit::singleline(&mut form.host.name).desired_width(280.0));
                    ui.end_row();
                    ui.label(tr("Group"));
                    let targets = group_targets(&groups, None);
                    let current =
                        targets.iter().find(|(id, _)| *id == form.host.group_id).map_or_else(|| tr("Top level").to_string(), |(_, p)| p.clone());
                    egui::ComboBox::from_id_salt("sshcraft-host-group").selected_text(current).width(280.0).show_ui(ui, |ui| {
                        for (id, path) in &targets {
                            ui.selectable_value(&mut form.host.group_id, *id, path);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("System"));
                    let os = OSES.iter().find(|(o, _)| *o == form.host.os).map_or("", |(_, l)| *l);
                    egui::ComboBox::from_id_salt("sshcraft-host-os").selected_text(tr(os)).show_ui(ui, |ui| {
                        for (o, label) in OSES {
                            ui.selectable_value(&mut form.host.os, o, tr(label));
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Tags"));
                    ui.add(egui::TextEdit::singleline(&mut form.tags).hint_text(tr("comma separated")).desired_width(280.0));
                    ui.end_row();
                });
                ui.separator();
                settings_editor(ui, app, &mut form.host.settings, &mut form.port, &inherited, Some(form.host.id));
                password_field(ui, &mut form.password, form.has_password, &mut form.remove_password);
            },
        );
        if let Some(error) = &form.error {
            ui.colored_label(crate::theme::Tokens::get(ui.ctx()).danger, error);
        }
        ui.horizontal(|ui| {
            save = ui.add_enabled(!busy, egui::Button::new(tr("Save"))).clicked();
            if busy {
                ui.spinner();
            }
            cancel = ui.button(tr("Cancel")).clicked();
        });
    });
    if cancel || modal.should_close() {
        open = false;
    }
    if save {
        match parse_port(&form.port) {
            Err(e) => form.error = Some(e),
            Ok(_) if form.host.name.trim().is_empty() => form.error = Some(tr("A host needs a name.").to_string()),
            Ok(port) => {
                form.host.settings.port = port;
                form.host.tags = split_tags(&form.tags);
                let owner = json!({ "kind": "host", "id": form.host.id });
                // The typed password stays in the form until the save went through.
                let (password, remove) = (form.password.clone(), form.remove_password);
                let generation = app.forms.start().unwrap_or_default();
                app.runner.run(
                    "host.save",
                    json!({ "host": form.host }),
                    Box::new(move |app, result| {
                        let current = app.forms.done(generation);
                        match result {
                            Ok(_) => after_save(
                                app,
                                owner,
                                password,
                                remove,
                                if current { |app: &mut SshcraftApp| app.ui.host_form = None } else { |_: &mut SshcraftApp| {} },
                            ),
                            Err(e) if current => {
                                if let Some(form) = app.ui.host_form.as_mut() {
                                    form.error = Some(e.to_string());
                                }
                            }
                            Err(e) => app.toasts.error(&e.to_string()),
                        }
                    }),
                );
            }
        }
    }
    if open {
        app.ui.host_form = Some(form);
    }
}

/// Draws the group form when open.
pub fn show_group(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.group_form.take() else { return };
    let busy = app.forms.busy();
    let groups: Vec<Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    let inherited = inherited(form.group.parent_id, &groups);
    let mut open = true;
    let mut cancel = false;
    let mut save = false;
    let title = if form.editing { tr("Edit group") } else { tr("New group") };
    let modal = egui::Modal::new(egui::Id::unique(title)).show(ctx, |ui| {
        ui.set_width(540.0);
        ui.heading(title);
        egui::ScrollArea::vertical().max_height(form_height(ui.ctx())).min_scrolled_height(form_height(ui.ctx())).auto_shrink([false, false]).show(
            ui,
            |ui| {
                egui::Grid::new("sshcraft-group").num_columns(2).spacing([12.0, 8.0]).min_col_width(130.0).show(ui, |ui| {
                    ui.label(tr("Name"));
                    ui.add(egui::TextEdit::singleline(&mut form.group.name).desired_width(280.0));
                    ui.end_row();
                    ui.label(tr("Inside"));
                    let targets = group_targets(&groups, Some(form.group.id));
                    let current =
                        targets.iter().find(|(id, _)| *id == form.group.parent_id).map_or_else(|| tr("Top level").to_string(), |(_, p)| p.clone());
                    egui::ComboBox::from_id_salt("sshcraft-group-parent").selected_text(current).width(280.0).show_ui(ui, |ui| {
                        for (id, path) in &targets {
                            ui.selectable_value(&mut form.group.parent_id, *id, path);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Tags"));
                    ui.add(egui::TextEdit::singleline(&mut form.tags).hint_text(tr("comma separated")).desired_width(280.0));
                    ui.end_row();
                });
                ui.separator();
                settings_editor(ui, app, &mut form.group.defaults, &mut form.port, &inherited, None);
                password_field(ui, &mut form.password, form.has_password, &mut form.remove_password);
            },
        );
        if let Some(error) = &form.error {
            ui.colored_label(crate::theme::Tokens::get(ui.ctx()).danger, error);
        }
        ui.horizontal(|ui| {
            save = ui.add_enabled(!busy, egui::Button::new(tr("Save"))).clicked();
            if busy {
                ui.spinner();
            }
            cancel = ui.button(tr("Cancel")).clicked();
        });
    });
    if cancel || modal.should_close() {
        open = false;
    }
    if save {
        match parse_port(&form.port) {
            Err(e) => form.error = Some(e),
            Ok(_) if form.group.name.trim().is_empty() => form.error = Some(tr("A group needs a name.").to_string()),
            Ok(port) => {
                form.group.defaults.port = port;
                form.group.tags = split_tags(&form.tags);
                let owner = json!({ "kind": "group", "id": form.group.id });
                // The typed password stays in the form until the save went through.
                let (password, remove) = (form.password.clone(), form.remove_password);
                let generation = app.forms.start().unwrap_or_default();
                app.runner.run(
                    "group.save",
                    json!({ "group": form.group }),
                    Box::new(move |app, result| {
                        let current = app.forms.done(generation);
                        match result {
                            Ok(_) => after_save(
                                app,
                                owner,
                                password,
                                remove,
                                if current { |app: &mut SshcraftApp| app.ui.group_form = None } else { |_: &mut SshcraftApp| {} },
                            ),
                            Err(e) if current => {
                                if let Some(form) = app.ui.group_form.as_mut() {
                                    form.error = Some(e.to_string());
                                }
                            }
                            Err(e) => app.toasts.error(&e.to_string()),
                        }
                    }),
                );
            }
        }
    }
    if open {
        app.ui.group_form = Some(form);
    }
}

impl GroupForm {
    pub fn new(group: Group, editing: bool, has_password: bool) -> Self {
        let tags = group.tags.join(", ");
        let port = group.defaults.port.map(|p| p.to_string()).unwrap_or_default();
        Self { group, editing, password: String::new(), has_password, remove_password: false, tags, port, error: None }
    }
}
