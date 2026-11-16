//! The forms for a host and a group: their connection settings, with the values inherited from
//! the groups above shown as hints naming the group they come from. A stored password is never
//! loaded into a field; the form only says that one is stored and offers to replace or remove it.

use egui::Ui;
use serde_json::json;
use sshcraft_model::hooks::ConnectStep;
use sshcraft_model::login_script::LoginStep;
use sshcraft_model::model::Protocol::Telnet;
use sshcraft_model::model::{ConnectionSettings, EnvVar, Group, GroupId, Host, HostId, HostOs, KeyRef, Proxy, ProxyEndpoint};
use sshcraft_model::secret_ref::{Manager, SecretRef, SecretRefError};

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
    pub x11: From<bool>,
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
        if out.x11.is_none()
            && let Some(f) = d.x11
        {
            out.x11 = Some((f, name()));
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
    /// The proxy as the form opened; a command that differs at save was entered here.
    pub opened_proxy: Option<Proxy>,
    /// The password reference as the form opened.
    pub opened_ref: Option<SecretRef>,
    /// A new TOTP secret; empty keeps what is stored.
    pub totp: String,
    pub has_totp: bool,
    pub remove_totp: bool,
    /// The login script as the form opened.
    pub opened_script: Option<Vec<LoginStep>>,
    /// The steps before and after connecting as the form opened.
    pub opened_steps: (Option<Vec<ConnectStep>>, Option<Vec<ConnectStep>>),
    /// Allow the login script on this device with this save.
    pub allow_script: bool,
    /// X11 forwarding as the form opened, and whether to allow it on this device with this save.
    pub opened_x11: Option<bool>,
    pub allow_x11: bool,
    /// The protocol as the form opened, and whether to allow Telnet on this device with this save.
    pub opened_protocol: Option<sshcraft_model::model::Protocol>,
    pub allow_telnet: bool,
    /// The launch actions as the form opened, and whether to allow them on this device.
    pub opened_launch: Option<Vec<sshcraft_model::model::SnippetId>>,
    pub allow_launch: bool,
}

impl HostForm {
    pub fn new(host: Host, editing: bool, has_password: bool) -> Self {
        let tags = host.tags.join(", ");
        let port = host.settings.port.map(|p| p.to_string()).unwrap_or_default();
        let opened_proxy = host.settings.proxy.clone();
        let opened_ref = host.settings.password_ref.clone();
        let opened_script = host.settings.login_script.clone();
        let opened_steps = (host.settings.before_connect.clone(), host.settings.after_disconnect.clone());
        Self {
            opened_steps,
            opened_x11: host.settings.x11,
            allow_x11: false,
            opened_protocol: host.settings.protocol,
            allow_telnet: false,
            opened_launch: host.settings.launch.clone(),
            allow_launch: false,
            host,
            editing,
            password: String::new(),
            has_password,
            remove_password: false,
            tags,
            port,
            error: None,
            opened_proxy,
            opened_ref,
            totp: String::new(),
            has_totp: false,
            remove_totp: false,
            opened_script,
            allow_script: false,
        }
    }
}

/// The password from a password manager: a manager and a reference, checked as typed.
fn password_ref_field(ui: &mut Ui, reference: &mut Option<SecretRef>) {
    let mut on = reference.is_some();
    ui.horizontal(|ui| {
        ui.label("");
        if ui.checkbox(&mut on, tr("From a password manager")).changed() {
            *reference = on.then(|| SecretRef { manager: Manager::OnePassword, reference: String::new() });
        }
    });
    let Some(r) = reference.as_mut() else { return };
    ui.horizontal(|ui| {
        ui.label("");
        egui::ComboBox::from_id_salt("sshcraft-secret-manager").selected_text(r.manager.label()).show_ui(ui, |ui| {
            for m in [Manager::ProtonPass, Manager::OnePassword, Manager::Bitwarden, Manager::KeePassXc] {
                ui.selectable_value(&mut r.manager, m, m.label());
            }
        });
        let label = ui.label(tr("Reference"));
        ui.add(egui::TextEdit::singleline(&mut r.reference).hint_text(r.manager.example()).font(egui::TextStyle::Monospace).desired_width(260.0))
            .labelled_by(label.id);
    });
    if !r.reference.is_empty()
        && let Err(e) = r.check()
    {
        let text = match e {
            SecretRefError::Shape(manager, example) => {
                trf("A {manager} reference looks like {example}", &[("manager", manager), ("example", example)])
            }
            _ => tr("The reference holds a control character or starts with '-'").to_owned(),
        };
        ui.colored_label(ui.visuals().error_fg_color, text);
    } else {
        ui.label(
            egui::RichText::new(trf("Fetched with {program} when connecting; SSHCraft keeps no copy.", &[("program", r.manager.program())])).small(),
        );
    }
}

/// Whether the user entered a proxy command in this form (it is new or changed).
fn entered_command(opened: &Option<Proxy>, now: &Option<Proxy>) -> bool {
    matches!(now, Some(Proxy::Command { .. })) && opened != now
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
    pub opened_proxy: Option<Proxy>,
    pub opened_script: Option<Vec<LoginStep>>,
    pub allow_script: bool,
    pub opened_steps: (Option<Vec<ConnectStep>>, Option<Vec<ConnectStep>>),
    pub opened_x11: Option<bool>,
    pub allow_x11: bool,
    /// The protocol as the form opened, and whether to allow Telnet on this device with this save.
    pub opened_protocol: Option<sshcraft_model::model::Protocol>,
    pub allow_telnet: bool,
    /// The launch actions as the form opened, and whether to allow them on this device.
    pub opened_launch: Option<Vec<sshcraft_model::model::SnippetId>>,
    pub allow_launch: bool,
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

/// Tab colour, badge and terminal theme of a host or group; each unset field is inherited.
fn appearance(ui: &mut Ui, value: &mut Option<sshcraft_model::appearance::Appearance>) {
    use sshcraft_model::appearance::{MAX_BADGE_CHARS, THEMES};
    let mut a = value.clone().unwrap_or_default();
    ui.label(tr("Tab colour"));
    ui.horizontal(|ui| {
        let mut own = a.color.is_some();
        if ui.checkbox(&mut own, tr("Own colour")).changed() {
            a.color = own.then_some([0x3f, 0x8f, 0xd2]);
        }
        if let Some(color) = &mut a.color {
            let mut picked = egui::Color32::from_rgb(color[0], color[1], color[2]);
            if ui.color_edit_button_srgba(&mut picked).changed() {
                *color = [picked.r(), picked.g(), picked.b()];
            }
        }
    });
    ui.end_row();
    ui.label(tr("Badge"));
    let mut badge = a.badge.clone().unwrap_or_default();
    if ui.add(egui::TextEdit::singleline(&mut badge).char_limit(MAX_BADGE_CHARS).hint_text(tr("e.g. PROD")).desired_width(120.0)).changed() {
        a.badge = (!badge.is_empty()).then_some(badge);
    }
    ui.end_row();
    ui.label(tr("Terminal theme"));
    let name = |theme: Option<&str>| match theme {
        Some("sshcraft-light") => tr("Light"),
        Some(_) => tr("Dark"),
        None => tr("Inherit"),
    };
    egui::ComboBox::from_id_salt("sshcraft-host-theme").selected_text(name(a.theme.as_deref())).show_ui(ui, |ui| {
        ui.selectable_value(&mut a.theme, None, tr("Inherit"));
        for theme in THEMES {
            ui.selectable_value(&mut a.theme, Some(theme.to_string()), name(Some(theme)));
        }
    });
    ui.end_row();
    *value = (!a.is_empty()).then_some(a);
}

/// How the session runs: kept in tmux or screen, and over SSH or Telnet.
fn session_rows(app: &mut SshcraftApp, ui: &mut Ui, s: &mut ConnectionSettings, host: Option<HostId>) {
    ui.add_space(8.0);
    egui::Grid::new("sshcraft-session-rows").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        container_row(app, ui, &mut s.container, host);
        persistence_row(ui, &mut s.persistence);
        protocol_row(ui, &mut s.protocol);
    });
}

/// Snippets typed once when a session first connects, at most 8; launch actions that were there
/// when the form opened may have come from elsewhere and need the box to run here.
fn launch_rows(
    app: &SshcraftApp,
    ui: &mut Ui,
    value: &mut Option<Vec<sshcraft_model::model::SnippetId>>,
    opened: &Option<Vec<sshcraft_model::model::SnippetId>>,
    allow: &mut bool,
) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Launch actions")).strong());
    ui.label(egui::RichText::new(tr("Snippets typed once when a session starts, not again after a reconnect.")).small());
    let chosen = value.get_or_insert_with(Vec::new);
    egui::ScrollArea::vertical().id_salt("sshcraft-launch").max_height(120.0).show(ui, |ui| {
        for row in &app.data.snippets {
            let Ok(uuid) = uuid::Uuid::parse_str(&row.id) else { continue };
            let id = sshcraft_model::model::SnippetId(uuid);
            let mut on = chosen.contains(&id);
            let full = chosen.len() >= sshcraft_model::launch::MAX_LAUNCH;
            if ui.add_enabled(on || !full, egui::Checkbox::new(&mut on, crate::widgets::visible(&row.name))).changed() {
                if on {
                    chosen.push(id);
                } else {
                    chosen.retain(|c| *c != id);
                }
            }
        }
    });
    if chosen.is_empty() {
        *value = None;
    } else if value == opened {
        ui.checkbox(allow, tr("Allow the launch actions on this device"))
            .on_hover_text(tr("Launch actions that came through sync, a share or an import are not typed until allowed here."));
    }
}

/// A container on the host to open instead of its shell: runtime, name (picked from the host's
/// list or typed), namespace for Kubernetes, shell.
fn container_row(app: &mut SshcraftApp, ui: &mut Ui, value: &mut Option<sshcraft_model::container::Container>, host: Option<HostId>) {
    use sshcraft_model::container::{Container, Runtime, Shell, check};
    ui.label(tr("Container"));
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let mut runtime = value.as_ref().map(|c| c.runtime);
            let shown = match runtime {
                None => tr("No"),
                Some(Runtime::Docker) => "Docker",
                Some(Runtime::Podman) => "Podman",
                Some(Runtime::Kubectl) => "Kubernetes",
            };
            egui::ComboBox::from_id_salt("sshcraft-container-runtime").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut runtime, None, tr("No"));
                ui.selectable_value(&mut runtime, Some(Runtime::Docker), "Docker");
                ui.selectable_value(&mut runtime, Some(Runtime::Podman), "Podman");
                ui.selectable_value(&mut runtime, Some(Runtime::Kubectl), "Kubernetes");
            });
            let Some(runtime) = runtime else {
                *value = None;
                return;
            };
            let c = value.get_or_insert_with(|| Container { runtime, name: String::new(), namespace: None, shell: Shell::Sh });
            if c.runtime != runtime {
                c.runtime = runtime;
                c.namespace = None;
            }
            ui.add(egui::TextEdit::singleline(&mut c.name).hint_text(tr("name")).desired_width(140.0).char_limit(253));
            if let Some(id) = host
                && ui.small_button(tr("List")).on_hover_text(tr("Ask the host which containers run there")).clicked()
            {
                app.runner.run(
                    "host.containers",
                    serde_json::json!({ "host_id": id, "runtime": runtime }),
                    Box::new(|app, result| match result {
                        Ok(v) => app.ui.container_names = serde_json::from_value(v).unwrap_or_default(),
                        Err(e) => app.toasts.error(&e.to_string()),
                    }),
                );
            }
            if !app.ui.container_names.is_empty() {
                egui::ComboBox::from_id_salt("sshcraft-container-pick").selected_text(tr("Pick")).show_ui(ui, |ui| {
                    for name in &app.ui.container_names {
                        if ui.selectable_label(c.name == *name, crate::widgets::visible(name)).clicked() {
                            c.name = name.clone();
                        }
                    }
                });
            }
        });
        let Some(c) = value.as_mut() else { return };
        ui.horizontal(|ui| {
            if c.runtime == Runtime::Kubectl {
                let mut ns = c.namespace.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut ns).hint_text(tr("namespace")).desired_width(120.0).char_limit(253)).changed() {
                    c.namespace = (!ns.trim().is_empty()).then(|| ns.trim().to_string());
                }
            }
            egui::ComboBox::from_id_salt("sshcraft-container-shell").selected_text(format!("{:?}", c.shell).to_lowercase()).show_ui(ui, |ui| {
                for shell in [Shell::Sh, Shell::Bash, Shell::Zsh, Shell::Ash] {
                    ui.selectable_value(&mut c.shell, shell, format!("{shell:?}").to_lowercase());
                }
            });
            if !c.name.is_empty()
                && let Err(e) = check(c)
            {
                ui.label(egui::RichText::new(e.to_string()).color(crate::theme::Tokens::get(ui.ctx()).danger));
            }
        });
    });
    ui.end_row();
}

/// SSH, or Telnet for devices that speak nothing else (with a warning: nothing is encrypted).
fn protocol_row(ui: &mut Ui, value: &mut Option<sshcraft_model::model::Protocol>) {
    use sshcraft_model::model::Protocol;
    ui.label(tr("Protocol"));
    ui.horizontal(|ui| {
        let mut chosen = value.unwrap_or_default();
        ui.radio_value(&mut chosen, Protocol::Ssh, "SSH");
        ui.radio_value(&mut chosen, Protocol::Telnet, "Telnet");
        *value = (chosen == Protocol::Telnet).then_some(Protocol::Telnet);
        if chosen == Protocol::Telnet {
            ui.label(
                egui::RichText::new(tr("Not encrypted: anyone on the way can read along. Stored passwords and keys are never sent."))
                    .color(crate::theme::Tokens::get(ui.ctx()).warning),
            );
        }
    });
    ui.end_row();
}

/// Whether the shell runs inside tmux or screen, and the session's name.
fn persistence_row(ui: &mut Ui, value: &mut Option<sshcraft_model::persist::Persistence>) {
    use sshcraft_model::persist::{Persistence, Tool, check};
    ui.label(tr("Keep the session"));
    ui.horizontal(|ui| {
        let mut tool = value.as_ref().map(|p| p.tool);
        let shown = match tool {
            None => tr("No"),
            Some(Tool::Tmux) => "tmux",
            Some(Tool::Screen) => "screen",
            Some(Tool::TmuxControl) => tr("tmux, windows as tabs"),
        };
        egui::ComboBox::from_id_salt("sshcraft-persistence").selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut tool, None, tr("No"));
            ui.selectable_value(&mut tool, Some(Tool::Tmux), "tmux");
            ui.selectable_value(&mut tool, Some(Tool::Screen), "screen");
            ui.selectable_value(&mut tool, Some(Tool::TmuxControl), tr("tmux, windows as tabs"));
        });
        match tool {
            None => *value = None,
            Some(tool) => {
                let p = value.get_or_insert_with(|| Persistence { tool, name: "sshcraft".into() });
                p.tool = tool;
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(140.0).char_limit(64))
                    .on_hover_text(tr("Every connect attaches to this session again, also after the connection dropped."));
                if check(&p.name).is_err() {
                    ui.label(
                        egui::RichText::new(tr("Letters, digits, dots, dashes and underscores")).color(crate::theme::Tokens::get(ui.ctx()).danger),
                    );
                }
            }
        }
    });
    ui.end_row();
}

/// X11 forwarding that was already on when the form opened may have come through sync or a
/// share: it is used only once allowed on this device.
fn x11_allow(ui: &mut Ui, x11: Option<bool>, opened: Option<bool>, allow: &mut bool) {
    if x11 == Some(true) && opened == Some(true) {
        ui.checkbox(allow, tr("Allow X11 forwarding on this device"))
            .on_hover_text(tr("X11 forwarding that came through sync, a share or an import stays off until allowed here."));
    }
}

/// The step programs in `before` and `after` that were not there when the form opened.
fn entered_programs(
    opened: &(Option<Vec<ConnectStep>>, Option<Vec<ConnectStep>>),
    before: &Option<Vec<ConnectStep>>,
    after: &Option<Vec<ConnectStep>>,
) -> Vec<Vec<String>> {
    let was: Vec<&Vec<String>> = [&opened.0, &opened.1].into_iter().flatten().flat_map(|s| sshcraft_model::hooks::commands(s)).collect();
    [before, after].into_iter().flatten().flat_map(|s| sshcraft_model::hooks::commands(s)).filter(|argv| !was.contains(argv)).cloned().collect()
}

/// A list of steps before connecting or after the session (`which` names it). Lines are kept
/// as typed; a step takes them once they read.
fn steps_editor(ui: &mut Ui, title: &str, which: &str, owner: Option<HostId>, value: &mut Option<Vec<ConnectStep>>) {
    use sshcraft_model::hooks::{MAX_STEPS, check};
    use sshcraft_model::proxy_command;
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong());
    let steps = value.get_or_insert_with(Vec::new);
    let mut remove = None;
    for (i, step) in steps.iter_mut().enumerate() {
        let id = egui::Id::unique("sshcraft-step").with((which, owner, i));
        ui.horizontal(|ui| {
            match step {
                ConnectStep::WakeOnLan { mac, broadcast, wait_secs, .. } => {
                    ui.label(tr("Wake-on-LAN"));
                    ui.add(egui::TextEdit::singleline(mac).hint_text("aa:bb:cc:dd:ee:ff").desired_width(140.0));
                    let mut address = broadcast.clone().unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut address).hint_text("255.255.255.255").desired_width(110.0)).changed() {
                        *broadcast = (!address.trim().is_empty()).then(|| address.trim().to_string());
                    }
                    ui.label(tr("then wait"));
                    ui.add(egui::DragValue::new(wait_secs).range(0..=300).suffix(" s"));
                }
                ConnectStep::Knock { ports, delay_ms } => {
                    ui.label(tr("Knock on ports"));
                    let joined = ports.iter().map(u16::to_string).collect::<Vec<_>>().join(", ");
                    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or(joined);
                    if ui.add(egui::TextEdit::singleline(&mut text).hint_text("7000, 8000, 9000").desired_width(160.0)).changed() {
                        *ports = text.split([',', ' ']).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
                    }
                    ui.data_mut(|d| d.insert_temp(id, text));
                    ui.label(tr("pause"));
                    ui.add(egui::DragValue::new(delay_ms).range(0..=5000).suffix(" ms"));
                }
                ConnectStep::Command { argv, wait_secs } => {
                    ui.label(tr("Program"));
                    let mut line = ui
                        .data(|d| d.get_temp::<String>(id))
                        .filter(|l| proxy_command::split(l).map_or(true, |w| w == *argv))
                        .unwrap_or_else(|| proxy_command::join(argv));
                    let edited = ui
                        .add(
                            egui::TextEdit::singleline(&mut line)
                                .hint_text("wg-quick up office")
                                .font(egui::TextStyle::Monospace)
                                .desired_width(220.0),
                        )
                        .changed();
                    if edited && let Ok(words) = proxy_command::split(&line) {
                        *argv = words;
                    }
                    ui.data_mut(|d| d.insert_temp(id, line));
                    ui.label(tr("then wait"));
                    ui.add(egui::DragValue::new(wait_secs).range(0..=300).suffix(" s"));
                }
            }
            if ui.small_button("×").on_hover_text(tr("Remove")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        steps.remove(i);
    }
    if steps.len() < MAX_STEPS {
        ui.horizontal(|ui| {
            if ui.small_button(tr("Add Wake-on-LAN")).clicked() {
                steps.push(ConnectStep::WakeOnLan { mac: String::new(), broadcast: None, port: None, wait_secs: 30 });
            }
            if ui.small_button(tr("Add knocking")).clicked() {
                steps.push(ConnectStep::Knock { ports: Vec::new(), delay_ms: 200 });
            }
            if ui.small_button(tr("Add a program")).clicked() {
                steps.push(ConnectStep::Command { argv: Vec::new(), wait_secs: 0 });
            }
        });
    }
    if let Err(e) = check(steps) {
        ui.label(egui::RichText::new(e.to_string()).color(crate::theme::Tokens::get(ui.ctx()).danger));
    }
    if steps.is_empty() {
        *value = None;
    }
}

/// The login script: steps that wait for a text and send a line. Changing it here allows it on
/// this device with the save; a script that came another way (sync, an agent) is allowed only by
/// ticking the box.
fn login_script_editor(ui: &mut Ui, value: &mut Option<Vec<LoginStep>>, opened: &Option<Vec<LoginStep>>, allow: &mut bool) {
    use sshcraft_model::login_script::{MAX_EXPECT_CHARS, MAX_SEND_CHARS, MAX_STEPS, check};
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Login script")).strong());
    ui.label(tr("After connecting: wait for a text, then send a line. Never put a password here."));
    let steps = value.get_or_insert_with(Vec::new);
    let mut remove = None;
    for (i, step) in steps.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(tr("Wait for"));
            ui.add(egui::TextEdit::singleline(&mut step.expect).char_limit(MAX_EXPECT_CHARS).desired_width(140.0));
            ui.label(tr("send"));
            ui.add(egui::TextEdit::singleline(&mut step.send).char_limit(MAX_SEND_CHARS).desired_width(200.0));
            if ui.small_button("×").on_hover_text(tr("Remove")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        steps.remove(i);
    }
    if steps.len() < MAX_STEPS && ui.button(tr("Add a step")).clicked() {
        steps.push(LoginStep { expect: String::new(), send: String::new() });
    }
    if let Err(e) = check(steps) {
        ui.label(egui::RichText::new(e.to_string()).color(crate::theme::Tokens::get(ui.ctx()).danger));
    }
    if steps.is_empty() {
        *value = opened.as_ref().filter(|o| o.is_empty()).cloned();
        return;
    }
    if *value != *opened {
        *allow = true;
    }
    ui.checkbox(allow, tr("Allow this login script on this device"));
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
                Some(Proxy::Command { .. }) => 4,
            };
            let mut chosen = kind;
            ui.horizontal(|ui| {
                ui.radio_value(&mut chosen, 0, tr("Inherit"));
                ui.radio_value(&mut chosen, 1, tr("Direct"));
                ui.radio_value(&mut chosen, 2, "HTTP");
                ui.radio_value(&mut chosen, 3, "SOCKS5");
                ui.radio_value(&mut chosen, 4, tr("Command"));
            });
            if chosen != kind {
                let endpoint = || ProxyEndpoint { address: String::new(), port: 1080, username: None, password: None };
                s.proxy = match chosen {
                    1 => Some(Proxy::Direct),
                    2 => Some(Proxy::Http(endpoint())),
                    3 => Some(Proxy::Socks5(endpoint())),
                    4 => Some(Proxy::Command { argv: Vec::new() }),
                    _ => None,
                };
            }
            if let Some(Proxy::Http(e) | Proxy::Socks5(e)) = &mut s.proxy {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut e.address).hint_text(tr("Proxy address")).desired_width(180.0));
                    ui.add(egui::DragValue::new(&mut e.port).range(1..=65535));
                });
            }
            if let Some(Proxy::Command { argv }) = &mut s.proxy {
                proxy_command_field(ui, argv, own_host);
            }
        });
        ui.end_row();

        tri_state(ui, tr("Agent forwarding"), &mut s.agent_forwarding, &inherited.agent_forwarding);
        tri_state(ui, tr("X11 forwarding"), &mut s.x11, &inherited.x11);
        tri_state(ui, tr("Record sessions"), &mut s.session_log, &None);
        appearance(ui, &mut s.appearance);

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

/// The proxy command as one line. It is kept as typed while it does not split; the arguments
/// change only when it does.
fn proxy_command_field(ui: &mut Ui, argv: &mut Vec<String>, host: Option<HostId>) {
    use sshcraft_model::proxy_command;
    let id = egui::Id::unique("sshcraft-proxy-command").with(host);
    // A line kept from another form (it splits into other arguments) starts over.
    let mut line = ui
        .data(|d| d.get_temp::<String>(id))
        .filter(|line| proxy_command::split(line).map_or(true, |words| words == *argv))
        .unwrap_or_else(|| proxy_command::join(argv));
    let edit = ui
        .horizontal(|ui| {
            let label = ui.label(tr("Proxy command"));
            ui.add(egui::TextEdit::singleline(&mut line).hint_text("ssh -W %h:%p bastion").font(egui::TextStyle::Monospace).desired_width(320.0))
                .labelled_by(label.id)
        })
        .inner;
    let parsed = proxy_command::split(&line);
    if edit.changed()
        && let Ok(words) = &parsed
    {
        argv.clone_from(words);
    }
    match parsed {
        Err(proxy_command::ProxyCommandError::Empty) => {}
        Err(e) => {
            let text = match e {
                proxy_command::ProxyCommandError::ShellSyntax(c) => {
                    trf("This needs a shell ({sign}); SSHCraft runs the command without one", &[("sign", &c.escape_default().to_string())])
                }
                proxy_command::ProxyCommandError::TooLong => tr("the proxy command is too long").to_owned(),
                _ => tr("a quote or a backslash is not closed").to_owned(),
            };
            ui.colored_label(ui.visuals().error_fg_color, text);
        }
        Ok(_) => {
            ui.label(egui::RichText::new(tr("Runs without a shell. %h, %p, %r and %n stand for address, port, user and name.")).small());
        }
    }
    ui.data_mut(|d| d.insert_temp(id, line));
}

/// The secret for one-time codes (TOTP), as an authenticator app gets it.
fn totp_field(ui: &mut Ui, secret: &mut String, has_totp: bool, remove: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(tr("One-time codes"));
        let hint = if has_totp { tr("Stored; type to replace") } else { tr("TOTP secret or otpauth:// link") };
        crate::widgets::secret_field(ui, secret, egui::Id::unique("sshcraft-form-totp"), true, hint, 220.0);
        if has_totp {
            ui.checkbox(remove, tr("Remove"));
        }
    });
    if !secret.is_empty() && sshcraft_model::totp::decode_secret(secret).is_none() {
        ui.colored_label(ui.visuals().error_fg_color, tr("This is not a TOTP secret (Base32 or an otpauth:// link)."));
    }
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
                if crate::pages::hosts::needs_own_sign_in(&form.host) {
                    let warning = crate::theme::Tokens::get(ui.ctx()).warning;
                    ui.label(
                        egui::RichText::new(tr(
                            "This host was shared without its key or password. Choose a key or password of your own; it stays on your devices.",
                        ))
                        .color(warning),
                    );
                }
                settings_editor(ui, app, &mut form.host.settings, &mut form.port, &inherited, Some(form.host.id));
                password_field(ui, &mut form.password, form.has_password, &mut form.remove_password);
                password_ref_field(ui, &mut form.host.settings.password_ref);
                totp_field(ui, &mut form.totp, form.has_totp, &mut form.remove_totp);
                // Rarer settings last.
                x11_allow(ui, form.host.settings.x11, form.opened_x11, &mut form.allow_x11);
                login_script_editor(ui, &mut form.host.settings.login_script, &form.opened_script, &mut form.allow_script);
                steps_editor(ui, tr("Before connecting"), "before", Some(form.host.id), &mut form.host.settings.before_connect);
                steps_editor(ui, tr("After the session"), "after", Some(form.host.id), &mut form.host.settings.after_disconnect);
                session_rows(app, ui, &mut form.host.settings, form.editing.then_some(form.host.id));
                launch_rows(app, ui, &mut form.host.settings.launch, &form.opened_launch, &mut form.allow_launch);
                if form.host.settings.protocol == Some(Telnet) && form.opened_protocol == Some(Telnet) {
                    ui.checkbox(&mut form.allow_telnet, tr("Allow Telnet on this device"))
                        .on_hover_text(tr("Telnet that came through sync, a share or an import is not used until allowed here."));
                }
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
                let (totp, remove_totp) = (form.totp.clone(), form.remove_totp);
                let totp_owner = json!({ "kind": "host_totp", "id": form.host.id });
                let generation = app.forms.start().unwrap_or_default();
                app.runner.run(
                    "host.save",
                    json!({
                        "host": form.host,
                        // A program the user wrote or changed here is allowed with the save.
                        "approve_command": entered_command(&form.opened_proxy, &form.host.settings.proxy),
                        // Only the programs typed here, never one the form opened with.
                        "approve_programs": entered_programs(&form.opened_steps, &form.host.settings.before_connect, &form.host.settings.after_disconnect),
                        // Switched on here, or allowed with the box below.
                        "approve_x11": form.host.settings.x11 == Some(true) && (form.opened_x11 != Some(true) || form.allow_x11),
                        "approve_telnet": form.host.settings.protocol == Some(Telnet) && (form.opened_protocol != Some(Telnet) || form.allow_telnet),
                        // Picked here, or allowed with the box below.
                        "approve_launch": form.host.settings.launch.as_ref().is_some_and(|l| !l.is_empty()) && (form.host.settings.launch != form.opened_launch || form.allow_launch),
                        "approve_login_script": form.allow_script && form.host.settings.login_script.as_ref().is_some_and(|s| !s.is_empty()),
                        "approve_secret_ref": form.host.settings.password_ref.is_some() && form.host.settings.password_ref != form.opened_ref,
                    }),
                    Box::new(move |app, result| {
                        let current = app.forms.done(generation);
                        match result {
                            Ok(_) => {
                                if !totp.is_empty() {
                                    app.run_reported("password.set", json!({ "owner": totp_owner, "secret": totp }));
                                } else if remove_totp {
                                    app.run_reported("password.clear", json!({ "owner": totp_owner }));
                                }
                                after_save(
                                    app,
                                    owner,
                                    password,
                                    remove,
                                    if current { |app: &mut SshcraftApp| app.ui.host_form = None } else { |_: &mut SshcraftApp| {} },
                                );
                            }
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
                // Rarer settings last.
                x11_allow(ui, form.group.defaults.x11, form.opened_x11, &mut form.allow_x11);
                login_script_editor(ui, &mut form.group.defaults.login_script, &form.opened_script, &mut form.allow_script);
                steps_editor(ui, tr("Before connecting"), "before", None, &mut form.group.defaults.before_connect);
                steps_editor(ui, tr("After the session"), "after", None, &mut form.group.defaults.after_disconnect);
                session_rows(app, ui, &mut form.group.defaults, None);
                launch_rows(app, ui, &mut form.group.defaults.launch, &form.opened_launch, &mut form.allow_launch);
                if form.group.defaults.protocol == Some(Telnet) && form.opened_protocol == Some(Telnet) {
                    ui.checkbox(&mut form.allow_telnet, tr("Allow Telnet on this device"))
                        .on_hover_text(tr("Telnet that came through sync, a share or an import is not used until allowed here."));
                }
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
                    json!({
                        "group": form.group,
                        "approve_command": entered_command(&form.opened_proxy, &form.group.defaults.proxy),
                        // Only the programs typed here, never one the form opened with.
                        "approve_programs": entered_programs(&form.opened_steps, &form.group.defaults.before_connect, &form.group.defaults.after_disconnect),
                        // Switched on here, or allowed with the box below.
                        "approve_x11": form.group.defaults.x11 == Some(true) && (form.opened_x11 != Some(true) || form.allow_x11),
                        "approve_telnet": form.group.defaults.protocol == Some(Telnet) && (form.opened_protocol != Some(Telnet) || form.allow_telnet),
                        // Picked here, or allowed with the box below.
                        "approve_launch": form.group.defaults.launch.as_ref().is_some_and(|l| !l.is_empty()) && (form.group.defaults.launch != form.opened_launch || form.allow_launch),
                        "approve_login_script": form.allow_script && form.group.defaults.login_script.as_ref().is_some_and(|s| !s.is_empty()),
                    }),
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
        let opened_proxy = group.defaults.proxy.clone();
        let opened_script = group.defaults.login_script.clone();
        let opened_steps = (group.defaults.before_connect.clone(), group.defaults.after_disconnect.clone());
        Self {
            opened_steps,
            opened_x11: group.defaults.x11,
            allow_x11: false,
            opened_protocol: group.defaults.protocol,
            allow_telnet: false,
            opened_launch: group.defaults.launch.clone(),
            allow_launch: false,
            group,
            editing,
            password: String::new(),
            has_password,
            remove_password: false,
            tags,
            port,
            error: None,
            opened_proxy,
            opened_script,
            allow_script: false,
        }
    }
}
