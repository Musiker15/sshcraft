//! The settings: general (language), appearance, terminal (with the hosts that may always write
//! the clipboard) and shortcuts. A change is saved at once through `settings.save`; the engine
//! checks it and the page shows what it kept.

use egui::Ui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sshcraft_engine::settings::{AppTheme, EngineSettings, FONT_SIZES, HostView, LanguageSetting, Osc52, TERMINAL_TYPES};

use crate::app::SshcraftApp;
use crate::commands::{Scope, ui_commands};
use crate::i18n::{tr, trf};
use crate::keymap::{BindError, chord_from_egui, works_in_terminal};
use crate::theme::Tokens;
use crate::widgets::visible;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    #[default]
    General,
    Appearance,
    Terminal,
    Shortcuts,
    Sync,
    Agents,
    Connections,
    Trash,
}

impl Section {
    const ALL: [Self; 8] =
        [Self::General, Self::Appearance, Self::Terminal, Self::Shortcuts, Self::Sync, Self::Agents, Self::Connections, Self::Trash];

    fn label(self) -> &'static str {
        match self {
            Self::General => tr("General"),
            Self::Appearance => tr("Appearance"),
            Self::Terminal => tr("Terminal"),
            Self::Shortcuts => tr("Shortcuts"),
            Self::Sync => tr("Sync"),
            Self::Agents => tr("AI agents"),
            Self::Connections => tr("Connections"),
            Self::Trash => tr("Trash"),
        }
    }
}

/// The terminal themes there are; an unknown id in the settings draws as the first.
pub const TERMINAL_THEMES: [(&str, &str); 2] = [("sshcraft-dark", "SSHCraft dark"), ("sshcraft-light", "SSHCraft light")];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsView {
    pub section: Section,
    /// The shortcut row waiting for a key combination (an index into the commands).
    #[serde(skip)]
    pub recording: Option<usize>,
    /// Why the last shortcut was not taken.
    #[serde(skip)]
    pub error: Option<String>,
    /// The font family being typed, saved when the field is left.
    #[serde(skip)]
    pub font_family: Option<String>,
    /// The dropdown shortcut being typed, taken when the field is left and only when valid.
    #[serde(skip)]
    pub dropdown_shortcut: Option<String>,
    /// The macro being edited: its place in the list (`None` for a new one), name and steps.
    #[serde(skip)]
    pub macro_draft: Option<(Option<usize>, String, String)>,
    /// The shells this device offers, fetched when the terminal section is first shown.
    #[serde(skip)]
    pub shells: Option<Vec<Value>>,
    /// Highlights being edited, valid or not; only valid ones are saved.
    #[serde(skip)]
    pub highlights: Option<Vec<sshcraft_engine::settings::Highlight>>,
    /// Triggers being edited, valid or not; only valid ones are saved.
    #[serde(skip)]
    pub triggers: Option<Vec<sshcraft_engine::settings::Trigger>>,
}

/// Saves `change` applied to the current settings; the page shows it at once.
pub(crate) fn save(app: &mut SshcraftApp, change: impl FnOnce(&mut EngineSettings)) {
    let mut settings = app.data.settings.clone();
    change(&mut settings);
    if settings == app.data.settings {
        return;
    }
    app.data.settings = settings.clone().sanitized();
    app.run_reported("settings.save", json!({ "settings": settings }));
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    ui.heading(tr("Settings"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for section in Section::ALL {
            if ui.selectable_label(app.ui.settings.section == section, section.label()).clicked() {
                app.ui.settings.section = section;
                app.ui.settings.recording = None;
                app.ui.settings.error = None;
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.ui.settings.section {
        Section::General => general(app, ui),
        Section::Appearance => appearance(app, ui),
        Section::Terminal => terminal(app, ui),
        Section::Shortcuts => shortcuts(app, ui),
        Section::Sync => crate::pages::sync::show(app, ui),
        Section::Agents => crate::pages::agents::show(app, ui),
        Section::Connections => crate::pages::connections::show(app, ui),
        Section::Trash => crate::pages::trash::show(app, ui),
    });
}

fn general(app: &mut SshcraftApp, ui: &mut Ui) {
    let mut language = app.data.settings.general.language;
    ui.label(egui::RichText::new(tr("Language")).strong());
    ui.horizontal(|ui| {
        ui.radio_value(&mut language, LanguageSetting::Auto, tr("System"));
        // Each language in its own name, whatever the current one is.
        ui.radio_value(&mut language, LanguageSetting::En, "English");
        ui.radio_value(&mut language, LanguageSetting::De, "Deutsch");
    });
    save(app, |s| s.general.language = language);
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Window")).strong());
    let mut system = app.data.settings.general.system_title_bar;
    ui.checkbox(&mut system, tr("Use system title bar"));
    ui.label(egui::RichText::new(tr("The desktop draws the title bar. Takes effect after a restart.")).weak());
    save(app, |s| s.general.system_title_bar = system);
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Host keys")).strong());
    let mut update = app.data.settings.general.update_host_keys;
    ui.checkbox(&mut update, tr("Take new host keys a server announces"));
    ui.label(
        egui::RichText::new(tr("After sign-in a server may name all its host keys. New ones are kept once the server proved it holds them, and keys it no longer has are dropped (UpdateHostKeys)."))
            .weak(),
    );
    save(app, |s| s.general.update_host_keys = update);
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("App lock")).strong());
    let mut minutes = app.data.settings.general.lock_after_minutes;
    ui.add_enabled_ui(crate::lock::supported(), |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("Lock after"));
            ui.add(egui::DragValue::new(&mut minutes).range(sshcraft_engine::settings::LOCK_MINUTES).suffix(tr(" minutes without input")));
        });
    });
    ui.label(
        egui::RichText::new(if crate::lock::supported() {
            tr("0 never locks. Unlocking asks Windows Hello, and keys and passwords ask it again.")
        } else {
            tr("Not on this system yet: unlocking needs Windows Hello.")
        })
        .weak(),
    );
    save(app, |s| s.general.lock_after_minutes = minutes);
    ui.add_space(12.0);
    crate::pages::backup::section(app, ui);
    let ctx = ui.ctx().clone();
    crate::pages::backup::dialog(app, &ctx);
    ui.add_space(12.0);
    ui.label(egui::RichText::new("~/.ssh/config").strong());
    let mut live = app.data.settings.integrations.ssh_config_live;
    ui.checkbox(&mut live, tr("Keep the hosts of ~/.ssh/config in step"));
    ui.label(
        egui::RichText::new(tr("Its hosts and the files it includes appear in the group ~/.ssh/config and follow the file when it changes. A host that leaves the file stays here. Writing back happens only from a host's menu."))
            .weak(),
    );
    save(app, |s| s.integrations.ssh_config_live = live);
}

fn appearance(app: &mut SshcraftApp, ui: &mut Ui) {
    let mut a = app.data.settings.appearance.clone();
    egui::Grid::new("sshcraft-appearance").num_columns(2).min_col_width(160.0).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label(tr("Theme"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut a.app_theme, AppTheme::System, tr("System"));
            ui.radio_value(&mut a.app_theme, AppTheme::Dark, tr("Dark"));
            ui.radio_value(&mut a.app_theme, AppTheme::Light, tr("Light"));
        });
        ui.end_row();
        ui.label(tr("Terminal colours"));
        ui.horizontal(|ui| {
            for (id, label) in TERMINAL_THEMES {
                ui.radio_value(&mut a.terminal_theme, id.to_string(), tr(label));
            }
        });
        ui.end_row();
        ui.label(tr("Terminal font"));
        let typed = app.ui.settings.font_family.get_or_insert_with(|| a.font_family.clone());
        let response = ui.add(egui::TextEdit::singleline(typed).desired_width(300.0).hint_text(tr("comma separated")));
        if response.lost_focus() {
            a.font_family = typed.trim().to_string();
            app.ui.settings.font_family = None;
        }
        ui.end_row();
        ui.label(tr("Font size"));
        ui.add(egui::DragValue::new(&mut a.font_size).range(FONT_SIZES));
        ui.end_row();
        ui.label(tr("Ligatures"));
        ui.checkbox(&mut a.ligatures, tr("Join characters where the font has ligatures (->, !=)"));
        ui.end_row();
        ui.label(tr("Dropdown window"));
        ui.horizontal(|ui| {
            // Typed as a draft: half a combination is never saved or registered system-wide.
            let typed = app.ui.settings.dropdown_shortcut.get_or_insert_with(|| a.dropdown_shortcut.clone().unwrap_or_default());
            let response = ui
                .add(egui::TextEdit::singleline(typed).hint_text("Ctrl+`").desired_width(120.0).char_limit(40))
                .on_hover_text(tr("A shortcut that shows SSHCraft at the top of the screen from any program, and hides it again. Empty: off."));
            let text = typed.trim().to_string();
            let valid = text.is_empty() || sshcraft_engine::settings::valid_combo(&text);
            if response.lost_focus() && valid {
                a.dropdown_shortcut = (!text.is_empty()).then_some(text);
                app.ui.settings.dropdown_shortcut = None;
            } else if !valid {
                ui.label(egui::RichText::new(tr("Like Ctrl+` or Ctrl+Alt+T")).color(crate::theme::Tokens::get(ui.ctx()).danger));
            }
        });
        ui.end_row();
        ui.label(tr("Reachability"));
        ui.checkbox(&mut a.show_reachability, tr("Show in the host list whether each host answers (connects to them every minute)"));
        ui.end_row();
        ui.label(tr("Hosts as"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut a.host_view, HostView::Grid, tr("Grid"));
            ui.radio_value(&mut a.host_view, HostView::List, tr("List"));
        });
        ui.end_row();
    });
    save(app, |s| s.appearance = a);
}

fn host_label(app: &SshcraftApp, host: sshcraft_model::model::HostId) -> String {
    app.data.hosts.iter().find(|r| r.host.id == host).map_or_else(|| tr("Unknown host").to_string(), |r| visible(&r.host.name))
}

fn terminal(app: &mut SshcraftApp, ui: &mut Ui) {
    if app.ui.settings.shells.is_none() {
        app.ui.settings.shells = Some(Vec::new());
        app.runner.run(
            "local.shells",
            json!({}),
            Box::new(|app, result| {
                if let Ok(Value::Array(shells)) = result {
                    app.ui.settings.shells = Some(shells);
                }
            }),
        );
    }
    let tokens = Tokens::get(ui.ctx());
    let mut t = app.data.settings.terminal.clone();
    let mut logs = app.data.settings.logs.clone();
    let mut local_shell: Option<Value> = t.local_shell.as_ref().and_then(|s| serde_json::to_value(s).ok());
    let shells = app.ui.settings.shells.clone().unwrap_or_default();
    egui::Grid::new("sshcraft-terminal").num_columns(2).min_col_width(160.0).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label(tr("Terminal type"));
        egui::ComboBox::from_id_salt("sshcraft-terminal-type").selected_text(visible(&t.terminal_type)).show_ui(ui, |ui| {
            for kind in TERMINAL_TYPES {
                ui.selectable_value(&mut t.terminal_type, kind.to_string(), kind);
            }
        });
        ui.end_row();
        ui.label(tr("Local shell"));
        let label = |v: &Option<Value>| {
            v.as_ref().and_then(|v| v.get("label")).and_then(Value::as_str).map_or_else(|| tr("System default").to_string(), visible)
        };
        egui::ComboBox::from_id_salt("sshcraft-local-shell").selected_text(label(&local_shell)).show_ui(ui, |ui| {
            ui.selectable_value(&mut local_shell, None, tr("System default"));
            for shell in &shells {
                let choice = Some(shell.clone());
                let text = label(&choice);
                ui.selectable_value(&mut local_shell, choice, text);
            }
        });
        ui.end_row();
        ui.label(tr("Scrollback"));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut t.scrollback).range(sshcraft_engine::settings::SCROLLBACK).speed(100.0));
            ui.label(egui::RichText::new(tr("lines, for new tabs")).color(tokens.text_dim));
        });
        ui.end_row();
        ui.label(tr("Reconnect"));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut t.reconnect_attempts).range(sshcraft_engine::settings::RECONNECT_ATTEMPTS));
            ui.label(
                egui::RichText::new(tr("attempts after a dropped connection, waiting up to a minute between them; 0 turns it off"))
                    .color(tokens.text_dim),
            );
        });
        ui.end_row();
        ui.label(tr("Notifications"));
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut t.notify_after_secs).range(sshcraft_engine::settings::NOTIFY_AFTER_SECS));
                ui.label(
                    egui::RichText::new(tr("seconds a command must run to notify when it ends in the background; 0 turns it off"))
                        .color(tokens.text_dim),
                );
            });
            ui.checkbox(&mut t.server_notifications, tr("Servers may show desktop notifications, always with the host's name"));
        });
        ui.end_row();
        ui.label(tr("Connections"));
        ui.checkbox(&mut t.reuse_connections, tr("Tabs, port forwards and commands to one host share its connection"));
        ui.end_row();
        ui.label(tr("Server resources"));
        ui.checkbox(&mut t.resource_bar, tr("Show load, memory and disk under SSH terminals (runs a command on the server every 10 seconds)"));
        ui.end_row();
        ui.label(tr("History"));
        ui.checkbox(&mut t.history, tr("Keep the commands typed in terminals"));
        ui.end_row();
        ui.label(tr("Suggestions"));
        ui.checkbox(&mut t.autocomplete, tr("Suggest while typing"));
        ui.end_row();
        ui.label(tr("Session logs"));
        ui.vertical(|ui| {
            ui.checkbox(&mut logs.session_logs, tr("Record sessions unless a host or group says otherwise"));
            egui::ComboBox::from_id_salt("sshcraft-log-retention").selected_text(retention(logs.log_retention_days)).show_ui(ui, |ui| {
                for days in sshcraft_engine::settings::LOG_RETENTION_DAYS {
                    ui.selectable_value(&mut logs.log_retention_days, days, retention(days));
                }
            });
        });
        ui.end_row();
        ui.label(tr("Clipboard"));
        let mut write = t.clipboard_osc52 == Osc52::Write;
        ui.checkbox(&mut write, tr("Programs may write the clipboard (OSC 52), after asking"));
        t.clipboard_osc52 = if write { Osc52::Write } else { Osc52::Off };
        ui.end_row();
    });
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Always allowed to write the clipboard")).strong());
    if t.clipboard_hosts.is_empty() {
        ui.label(egui::RichText::new(tr("No host yet.")).color(tokens.text_dim));
    }
    let mut remove = None;
    for (i, host) in t.clipboard_hosts.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(host_label(app, *host));
            if ui.button(tr("Remove")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        t.clipboard_hosts.remove(i);
    }
    patterns(app, ui, &mut t);
    // The shell is kept as the engine has it; only a choice in the list changes it.
    let chosen = local_shell.and_then(|v| serde_json::from_value(v).ok());
    if chosen != t.local_shell {
        t.local_shell = chosen;
    }
    // Turning the history off deletes what it kept.
    if app.data.settings.terminal.history && !t.history {
        app.run_reported("history.clear", json!({}));
        crate::session_tab::forget_history(app, None);
    }
    save(app, |s| {
        s.terminal = t;
        s.logs = logs;
    });
    macros(app, ui);
}

/// The macros: run from the command palette on the focused terminal.
fn macros(app: &mut SshcraftApp, ui: &mut Ui) {
    use sshcraft_engine::settings::{MAX_MACROS, Macro};
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Macros")).strong());
    ui.label(egui::RichText::new(tr("Run from the command palette on the focused terminal. One step per line: text to wait for => line to send, or just the line to send at once.")).small());
    let list = app.data.settings.terminal.macros.clone();
    let mut remove = None;
    for (i, m) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(crate::widgets::visible(&m.name));
            ui.label(egui::RichText::new(trf("{count} steps", &[("count", &m.steps.len().to_string())])).small());
            if ui.small_button(tr("Edit")).clicked() {
                app.ui.settings.macro_draft = Some((Some(i), m.name.clone(), crate::macros::steps_text(&m.steps)));
            }
            if ui.small_button(tr("Delete")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        save(app, |s| {
            if i < s.terminal.macros.len() {
                s.terminal.macros.remove(i);
            }
        });
    }
    if app.ui.settings.macro_draft.is_none() && list.len() < MAX_MACROS && ui.button(tr("New macro")).clicked() {
        app.ui.settings.macro_draft = Some((None, String::new(), String::new()));
    }
    let Some((place, mut name, mut text)) = app.ui.settings.macro_draft.take() else { return };
    ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("Name")).char_limit(64).desired_width(240.0));
    ui.add(
        egui::TextEdit::multiline(&mut text)
            .hint_text("cd /srv/app\n$ => make deploy")
            .desired_rows(4)
            .desired_width(420.0)
            .font(egui::TextStyle::Monospace),
    );
    let draft = Macro { name: name.trim().to_string(), steps: crate::macros::parse_steps(&text) };
    let clash = list.iter().enumerate().any(|(i, m)| Some(i) != place && sshcraft_engine::settings::same_name(&m.name, &draft.name));
    let ok = draft.valid() && !clash;
    let (mut keep, mut store) = (true, false);
    ui.horizontal(|ui| {
        store = ui.add_enabled(ok, egui::Button::new(tr("Save"))).clicked();
        if ui.button(tr("Cancel")).clicked() {
            keep = false;
        }
        if !ok && !draft.name.is_empty() {
            let why = if clash { tr("A macro has that name already.") } else { tr("1 to 16 steps, lines without control characters.") };
            ui.label(egui::RichText::new(why).color(crate::theme::Tokens::get(ui.ctx()).danger));
        }
    });
    if store {
        save(app, |s| match place {
            Some(i) if i < s.terminal.macros.len() => s.terminal.macros[i] = draft,
            _ => s.terminal.macros.push(draft),
        });
        keep = false;
    }
    if keep {
        app.ui.settings.macro_draft = Some((place, name, text));
    }
}

/// The editors of highlights and triggers. Rows are edited in a draft; the settings get the valid
/// ones, so a pattern half typed does not vanish.
fn patterns(app: &mut SshcraftApp, ui: &mut Ui, t: &mut sshcraft_engine::settings::Terminal) {
    use sshcraft_engine::settings::{Highlight, MAX_PATTERNS, Trigger, TriggerAction, compile_pattern};
    let tokens = Tokens::get(ui.ctx());
    let highlights = app.ui.settings.highlights.get_or_insert_with(|| t.highlights.clone());
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Highlights")).strong());
    ui.label(egui::RichText::new(tr("Regular expressions coloured wherever they appear in a terminal, case-insensitive")).color(tokens.text_dim));
    let mut remove = None;
    for (i, h) in highlights.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut h.pattern).desired_width(260.0).hint_text(tr("Pattern")));
            let mut color = crate::terminal::highlight::swatch(h.color);
            if ui.color_edit_button_srgba(&mut color).changed() {
                h.color = [color.r(), color.g(), color.b()];
            }
            if ui.button(tr("Remove")).clicked() {
                remove = Some(i);
            }
            if compile_pattern(&h.pattern).is_none() {
                ui.label(egui::RichText::new(tr("Not a valid pattern")).color(tokens.danger));
            }
        });
    }
    if let Some(i) = remove {
        highlights.remove(i);
    }
    if highlights.len() < MAX_PATTERNS && ui.button(tr("Add a highlight")).clicked() {
        highlights.push(Highlight { pattern: String::new(), color: [0xe5, 0xc0, 0x7b] });
    }
    t.highlights = highlights.iter().filter(|h| compile_pattern(&h.pattern).is_some()).cloned().collect();

    let triggers = app.ui.settings.triggers.get_or_insert_with(|| t.triggers.clone());
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Triggers")).strong());
    ui.label(egui::RichText::new(tr("Regular expressions checked once for each finished output line, case-insensitive")).color(tokens.text_dim));
    let mut remove = None;
    for (i, trigger) in triggers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut trigger.pattern).desired_width(260.0).hint_text(tr("Pattern")));
            let label = |a: TriggerAction| match a {
                TriggerAction::Notify => tr("Notify"),
                TriggerAction::Bell => tr("Mark the tab"),
            };
            egui::ComboBox::from_id_salt(("sshcraft-trigger-action", i)).selected_text(label(trigger.action)).show_ui(ui, |ui| {
                for action in [TriggerAction::Notify, TriggerAction::Bell] {
                    ui.selectable_value(&mut trigger.action, action, label(action));
                }
            });
            if ui.button(tr("Remove")).clicked() {
                remove = Some(i);
            }
            if compile_pattern(&trigger.pattern).is_none() {
                ui.label(egui::RichText::new(tr("Not a valid pattern")).color(tokens.danger));
            }
        });
    }
    if let Some(i) = remove {
        triggers.remove(i);
    }
    if triggers.len() < MAX_PATTERNS && ui.button(tr("Add a trigger")).clicked() {
        triggers.push(Trigger { pattern: String::new(), action: TriggerAction::Notify });
    }
    t.triggers = triggers.iter().filter(|t| compile_pattern(&t.pattern).is_some()).cloned().collect();
}

/// How long finished logs are kept, in words.
fn retention(days: u32) -> String {
    if days == 0 { tr("Keep logs until deleted").to_string() } else { trf("Keep logs {days} days", &[("days", &days.to_string())]) }
}

fn bind_error(text: &str, error: &BindError) -> String {
    match error {
        BindError::Taken { owner } => trf("{shortcut} is taken by {action}.", &[("shortcut", text), ("action", tr(owner))]),
        BindError::NeedsModifier => tr("A shortcut needs Ctrl or Alt (⌘ on macOS), or is a function key.").to_string(),
        BindError::Invalid(why) => trf("Not a key combination: {why}", &[("why", why)]),
    }
}

/// The key combination pressed while a row records, taken out of the input.
fn recorded(ui: &Ui) -> Option<Result<String, ()>> {
    ui.ctx().input_mut(|input| {
        let mut found = None;
        input.events.retain(|event| {
            if found.is_some() {
                return true;
            }
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
            found = Some(if *key == egui::Key::Escape { Err(()) } else { chord_from_egui(*key, modifiers).map(|c| c.to_string()).ok_or(()) });
            false
        });
        found
    })
}

fn shortcuts(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    if let Some(index) = app.ui.settings.recording
        && let Some(result) = recorded(ui)
    {
        app.ui.settings.recording = None;
        if let (Ok(text), Some(command)) = (result, ui_commands().get(index)) {
            let mut bindings = app.bindings.clone();
            match bindings.assign(command.id, &text) {
                Ok(()) => {
                    app.ui.settings.error = None;
                    let stored = bindings.to_settings();
                    app.bindings = bindings;
                    save(app, |s| s.shortcuts = stored);
                }
                Err(e) => app.ui.settings.error = Some(bind_error(&text, &e)),
            }
        }
    }
    ui.label(
        egui::RichText::new(tr("Inside a terminal only Ctrl+Shift and Ctrl+Tab are taken; every other key goes to the server."))
            .color(tokens.text_dim),
    );
    if let Some(error) = &app.ui.settings.error {
        ui.label(egui::RichText::new(error).color(tokens.danger));
    }
    ui.add_space(6.0);
    let mut action: Option<(usize, &'static str)> = None;
    egui::Grid::new("sshcraft-shortcuts").num_columns(3).min_col_width(120.0).spacing([16.0, 8.0]).show(ui, |ui| {
        for (i, command) in ui_commands().iter().enumerate() {
            ui.label(tr(command.label));
            let chords = app.bindings.chords(command.id);
            let text = if app.ui.settings.recording == Some(i) {
                tr("Press a key combination (Escape cancels)").to_string()
            } else if chords.is_empty() {
                tr("None").to_string()
            } else {
                chords
                    .iter()
                    .map(|c| {
                        if command.scope == Scope::App && !works_in_terminal(c) {
                            trf("{shortcut} (outside the terminal only)", &[("shortcut", &c.to_string())])
                        } else {
                            c.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.label(egui::RichText::new(text).monospace());
            ui.horizontal(|ui| {
                if ui.button(tr("Change")).clicked() {
                    action = Some((i, "change"));
                }
                if ui.add_enabled(!chords.is_empty(), egui::Button::new(tr("Remove"))).clicked() {
                    action = Some((i, "remove"));
                }
                if ui.button(tr("Default")).clicked() {
                    action = Some((i, "default"));
                }
            });
            ui.end_row();
        }
    });
    let Some((i, what)) = action else { return };
    let Some(command) = ui_commands().get(i) else { return };
    app.ui.settings.error = None;
    if what == "change" {
        app.ui.settings.recording = Some(i);
        return;
    }
    let mut bindings = app.bindings.clone();
    if what == "remove" {
        bindings.remove(command.id);
    } else {
        bindings.reset(command.id);
    }
    let stored = bindings.to_settings();
    app.bindings = bindings;
    save(app, |s| s.shortcuts = stored);
}
