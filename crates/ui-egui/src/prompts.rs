//! The engine's questions: whether to trust a host key, and sign-in questions. One shows at a
//! time, over everything else. Its answer buttons take a click only [`ANSWER_DELAY`] after it
//! appeared, so a click meant for the question before never answers the next one.

use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::visible;

/// How long a new question's answer buttons wait before they take a click, in seconds.
pub const ANSWER_DELAY: f64 = 0.4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    HostKey { host_name: String, address: String, port: u16, fingerprint: String },
    Auth { host_name: String, username: String, name: String, instructions: String, lines: Vec<(String, bool)> },
}

#[derive(Debug, Clone)]
pub struct PromptView {
    pub id: u64,
    pub kind: PromptKind,
    /// What the user typed, one per line of a sign-in question.
    pub answers: Vec<String>,
}

impl PromptView {
    /// A question as `prompt.list` and the events describe it; `None` for kinds the window does
    /// not answer here (an agent's action).
    pub fn from_json(v: &Value) -> Option<Self> {
        let text = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let id = v.get("prompt_id")?.as_u64()?;
        let kind = match v.get("kind")?.as_str()? {
            "host_key" => PromptKind::HostKey {
                host_name: text("host_name"),
                address: text("address"),
                port: v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).unwrap_or(0),
                fingerprint: text("fingerprint"),
            },
            "auth" => PromptKind::Auth {
                host_name: text("host_name"),
                username: text("username"),
                name: text("name"),
                instructions: text("instructions"),
                lines: v
                    .get("prompts")
                    .and_then(Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .take(MAX_LINES)
                            .map(|l| {
                                (
                                    l.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                                    l.get("echo").and_then(Value::as_bool).unwrap_or(false),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            _ => return None,
        };
        let answers = match &kind {
            PromptKind::Auth { lines, .. } => vec![String::new(); lines.len()],
            PromptKind::HostKey { .. } => Vec::new(),
        };
        Some(Self { id, kind, answers })
    }
}

/// Most questions one sign-in round shows; a server asking more gets the first ones.
const MAX_LINES: usize = 16;

/// The open questions, oldest first.
#[derive(Debug, Default)]
pub struct Prompts {
    pub open: Vec<PromptView>,
    /// The question on screen and when it appeared (egui time).
    shown: Option<(u64, f64)>,
    /// Events were lost: ask the engine for the open questions again.
    pub stale: bool,
}

impl Prompts {
    /// No question yet, and the open ones still to be fetched from the engine.
    pub fn fetch_first() -> Self {
        Self { stale: true, ..Self::default() }
    }

    pub fn add(&mut self, prompt: PromptView) {
        if !self.open.iter().any(|p| p.id == prompt.id) {
            self.open.push(prompt);
        }
    }

    pub fn add_from(&mut self, prompt: Option<PromptView>) {
        if let Some(prompt) = prompt {
            self.add(prompt);
        }
    }

    pub fn withdraw(&mut self, id: u64) {
        self.open.retain(|p| p.id != id);
    }
}

/// The question from an engine event, if it is one this window answers.
pub fn from_event(event: &impl serde::Serialize, kind: &str) -> Option<PromptView> {
    let mut v = serde_json::to_value(event).ok()?;
    v.as_object_mut()?.insert("kind".into(), json!(kind));
    PromptView::from_json(&v)
}

/// Asks the engine for the open questions when events were lost.
pub fn refresh(app: &mut SshcraftApp) {
    if !app.prompts.stale {
        return;
    }
    app.prompts.stale = false;
    app.runner.run(
        "prompt.list",
        json!({}),
        Box::new(|app, result| {
            if let Ok(Value::Array(list)) = result {
                let mut open: Vec<PromptView> = list.iter().filter_map(PromptView::from_json).collect();
                // Keep what was typed into a question that is still open.
                for p in &mut open {
                    if let Some(old) = app.prompts.open.iter().find(|o| o.id == p.id) {
                        p.answers.clone_from(&old.answers);
                    }
                }
                app.prompts.open = open;
            }
        }),
    );
}

enum Answer {
    HostKey(bool),
    Auth(Option<Vec<String>>),
}

pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(prompt) = app.prompts.open.first_mut() else {
        app.prompts.shown = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let since = match app.prompts.shown {
        Some((id, since)) if id == prompt.id => since,
        _ => {
            app.prompts.shown = Some((prompt.id, now));
            now
        }
    };
    let waited = now - since;
    let ready = waited >= ANSWER_DELAY;
    if !ready {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(ANSWER_DELAY - waited));
    }
    let tokens = Tokens::get(ctx);
    let mut answer = None;
    egui::Modal::new(egui::Id::unique("sshcraft-prompt")).show(ctx, |ui| {
        ui.set_width(520.0);
        match &prompt.kind {
            PromptKind::HostKey { host_name, address, port, fingerprint } => {
                ui.heading(tr("Unknown host key"));
                ui.add_space(6.0);
                let at = format!("{}:{port}", visible(address));
                ui.label(trf(
                    "{host} ({address}) is not known yet. Compare the fingerprint with the one its administrator gave you before you accept it.",
                    &[("host", &visible(host_name)), ("address", &at)],
                ));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(visible(fingerprint)).monospace().color(tokens.text_strong));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready, egui::Button::new(tr("Accept"))).clicked() {
                        answer = Some(Answer::HostKey(true));
                    }
                    if ui.add_enabled(ready, egui::Button::new(tr("Cancel"))).clicked() {
                        answer = Some(Answer::HostKey(false));
                    }
                });
            }
            PromptKind::Auth { host_name, username, name, instructions, lines } => {
                let who = if username.is_empty() { visible(host_name) } else { format!("{}@{}", visible(username), visible(host_name)) };
                ui.heading(trf("Sign in to {host}", &[("host", &who)]));
                if !name.is_empty() {
                    ui.label(visible(name));
                }
                if !instructions.is_empty() {
                    ui.label(egui::RichText::new(visible(instructions)).color(tokens.text_dim));
                }
                ui.add_space(6.0);
                egui::Grid::new("sshcraft-prompt-lines").num_columns(2).min_col_width(130.0).show(ui, |ui| {
                    for ((text, echo), value) in lines.iter().zip(prompt.answers.iter_mut()) {
                        ui.label(visible(text));
                        ui.add(egui::TextEdit::singleline(value).password(!echo).desired_width(300.0));
                        ui.end_row();
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready, egui::Button::new(tr("Sign in"))).clicked() {
                        answer = Some(Answer::Auth(Some(std::mem::take(&mut prompt.answers))));
                    }
                    if ui.add_enabled(ready, egui::Button::new(tr("Cancel"))).clicked() {
                        answer = Some(Answer::Auth(None));
                    }
                });
            }
        }
    });
    let Some(answer) = answer else { return };
    let id = prompt.id;
    app.prompts.withdraw(id);
    match answer {
        Answer::HostKey(accept) => app.run_reported("prompt.host_key", json!({ "prompt_id": id, "accept": accept })),
        Answer::Auth(answers) => app.run_reported("prompt.auth", json!({ "prompt_id": id, "answers": answers })),
    }
}
