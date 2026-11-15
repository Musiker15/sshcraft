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
    Trash,
}

impl Section {
    const ALL: [Self; 7] = [Self::General, Self::Appearance, Self::Terminal, Self::Shortcuts, Self::Sync, Self::Agents, Self::Trash];

    fn label(self) -> &'static str {
        match self {
            Self::General => tr("General"),
            Self::Appearance => tr("Appearance"),
            Self::Terminal => tr("Terminal"),
            Self::Shortcuts => tr("Shortcuts"),
            Self::Sync => tr("Sync"),
            Self::Agents => tr("AI agents"),
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
    /// The shells this device offers, fetched when the terminal section is first shown.
    #[serde(skip)]
    pub shells: Option<Vec<Value>>,
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
