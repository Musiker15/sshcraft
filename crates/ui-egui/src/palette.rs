//! The command palette: every `ui.*` command, found by its label in the interface language, its
//! English label or its id; arrows move, Enter runs, Escape closes.

use crate::app::SshcraftApp;
use crate::commands::ui_commands;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;

/// Entries shown at once.
const MAX_SHOWN: usize = 12;

#[derive(Debug, Clone, Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

impl PaletteState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: &'static str,
    /// What the entry acts on (a workspace's id), for entries that are not plain commands.
    pub arg: Option<String>,
    pub local: String,
    pub english: &'static str,
    pub shortcut: Option<String>,
    pub enabled: bool,
}

/// How well `query` matches `text`: a prefix beats a word start beats anything inside.
fn score(query: &str, text: &str) -> Option<i32> {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        Some(300)
    } else if text.split([' ', '.', '-']).any(|w| w.starts_with(query)) {
        Some(200)
    } else if text.contains(query) {
        Some(100)
    } else {
        None
    }
}

/// The entries matching `query`, best first, disabled ones after all enabled ones.
pub fn rank<'a>(query: &str, entries: &'a [Entry]) -> Vec<&'a Entry> {
    let query = query.trim().to_lowercase();
    let mut scored: Vec<(i32, &Entry)> = entries
        .iter()
        .filter_map(|e| {
            if query.is_empty() {
                return Some((0, e));
            }
            let s = score(&query, &e.local).or_else(|| score(&query, e.english).map(|s| s - 10)).or_else(|| score(&query, e.id).map(|s| s - 50))?;
            Some((s, e))
        })
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| b.enabled.cmp(&a.enabled).then(sb.cmp(sa)).then(a.local.cmp(&b.local)));
    scored.into_iter().map(|(_, e)| e).take(MAX_SHOWN).collect()
}

pub fn entries(app: &SshcraftApp) -> Vec<Entry> {
    ui_commands()
        .iter()
        .filter(|c| c.in_palette)
        .map(|c| Entry {
            id: c.id,
            arg: None,
            local: tr(c.label).to_string(),
            english: c.label,
            shortcut: app.bindings.chords(c.id).first().map(ToString::to_string),
            enabled: (c.enabled)(app),
        })
        .chain(app.data.templates.iter().filter_map(|t| {
            let name = t.get("name").and_then(serde_json::Value::as_str)?;
            Some(Entry {
                id: OPEN_WORKSPACE,
                arg: t.get("id").and_then(serde_json::Value::as_str).map(str::to_string),
                local: trf("Workspace: {name}", &[("name", &crate::widgets::visible(name))]),
                english: "Workspace",
                shortcut: None,
                enabled: true,
            })
        }))
        .collect()
}

/// The palette entry that opens a workspace template (its id in `Entry::arg`).
pub const OPEN_WORKSPACE: &str = "ui.workspace.open";

/// Draws the palette when open; returns the command chosen.
pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) -> Option<(&'static str, Option<String>)> {
    if !app.ui.palette.open {
        return None;
    }
    let all = entries(app);
    let tokens = Tokens::get(ctx);
    let (enter, escape, down, up) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
        )
    });
    let state = &mut app.ui.palette;
    if escape {
        state.open = false;
        return None;
    }
    let mut chosen = None;
    egui::Window::new(tr("Command palette"))
        .title_bar(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .fixed_size(egui::vec2(520.0, 0.0))
        .frame(egui::Frame::window(&ctx.global_style()).fill(tokens.panel))
        .show(ctx, |ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(tr("Type a command")).desired_width(f32::INFINITY));
            field.request_focus();
            let ranked = rank(&state.query, &all);
            if ranked.is_empty() {
                ui.label(egui::RichText::new(tr("No command matches")).color(tokens.text_dim));
                return;
            }
            let last = ranked.len().saturating_sub(1);
            if down {
                state.selected = (state.selected + 1).min(last);
            }
            if up {
                state.selected = state.selected.saturating_sub(1);
            }
            state.selected = state.selected.min(last);
            for (i, entry) in ranked.iter().enumerate() {
                let text = match &entry.shortcut {
                    Some(keys) => format!("{}    {keys}", entry.local),
                    None => entry.local.clone(),
                };
                let label = egui::RichText::new(text).color(if entry.enabled { tokens.text } else { tokens.text_dim });
                let row = ui.add_enabled(entry.enabled, egui::Button::selectable(i == state.selected, label));
                if row.clicked() || (enter && i == state.selected && entry.enabled) {
                    chosen = Some((entry.id, entry.arg.clone()));
                }
            }
        });
    if chosen.is_some() {
        app.ui.palette.open = false;
    }
    chosen
}
