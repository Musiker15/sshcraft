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

/// The id of the answer field `line` of question `prompt`: each question has fields of its own,
/// so nothing typed for one question is in the next one's field.
pub fn answer_id(prompt: u64, line: usize) -> egui::Id {
    egui::Id::unique(("sshcraft-prompt-answer", prompt, line))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    HostKey {
        host_name: String,
        address: String,
        port: u16,
        fingerprint: String,
    },
    Auth {
        host_name: String,
        username: String,
        name: String,
        instructions: String,
        lines: Vec<(String, bool)>,
    },
    /// A proxy command that was not set on this device, waiting for the user's yes or no.
    ProxyCommand {
        /// `proxy_command` or `password_ref`.
        what: String,
        host_name: String,
        command: String,
    },
    /// An AI agent's action waiting for the user's yes or no.
    Agent {
        agent: String,
        action: String,
        target: String,
        detail: String,
    },
}

/// Why a command an agent wants to run deserves a second look: several lines, over 300
/// characters, or characters that hide or reorder text.
pub fn command_warning(detail: &str) -> Option<&'static str> {
    if detail.contains('\n') || detail.contains('\r') {
        return Some(tr("This command has more than one line: read every line."));
    }
    if detail.chars().count() > 300 {
        return Some(tr("This command is long: read all of it."));
    }
    (crate::widgets::visible(detail) != detail).then_some(tr("This command has hidden or reordering characters, shown as ⟨U+…⟩."))
}

#[derive(Debug, Clone)]
pub struct PromptView {
    pub id: u64,
    /// The terminal session that asks, if a terminal asks.
    pub session: Option<sshcraft_engine::SessionId>,
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
            "agent" => PromptKind::Agent { agent: text("agent"), action: text("action"), target: text("target"), detail: text("detail") },
            "proxy_command" => PromptKind::ProxyCommand { what: text("what"), host_name: text("host_name"), command: text("command") },
            _ => return None,
        };
        let answers = match &kind {
            PromptKind::Auth { lines, .. } => vec![String::new(); lines.len()],
            PromptKind::HostKey { .. } | PromptKind::Agent { .. } | PromptKind::ProxyCommand { .. } => Vec::new(),
        };
        let session = v.get("session").and_then(|s| serde_json::from_value(s.clone()).ok());
        Some(Self { id, session, kind, answers })
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

    /// The id of the newest question known, 0 for none.
    pub fn newest(&self) -> u64 {
        self.open.iter().map(|p| p.id).max().unwrap_or(0)
    }

    /// Takes the engine's list of open questions (asked for when `asked_after` was the newest
    /// known): a question not in it was answered or withdrawn, unless it came by event after the
    /// list was asked for. What was typed into a question that stays open is kept.
    pub fn merge_listed(&mut self, listed: Vec<PromptView>, asked_after: u64) {
        let mut open = listed;
        for p in &mut open {
            if let Some(old) = self.open.iter().find(|o| o.id == p.id) {
                p.answers.clone_from(&old.answers);
            }
        }
        let newer: Vec<PromptView> = self.open.iter().filter(|p| p.id > asked_after && !open.iter().any(|o| o.id == p.id)).cloned().collect();
        open.extend(newer);
        open.sort_by_key(|p| p.id);
        self.open = open;
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
    let asked_after = app.prompts.newest();
    app.runner.run(
        "prompt.list",
        json!({}),
        Box::new(move |app, result| {
            if let Ok(Value::Array(list)) = result {
                app.prompts.merge_listed(list.iter().filter_map(PromptView::from_json).collect(), asked_after);
            }
        }),
    );
}

enum Answer {
    Agent(bool),
    ProxyCommand(bool),
    HostKey(bool),
    Auth(Option<Vec<String>>),
}

/// A button that answers a question: it takes a click (or a screen reader's click) but never
/// the keyboard focus, so Tab and Enter typed for a terminal never answer.
fn answer_button(ui: &mut egui::Ui, ready: bool, text: &str) -> bool {
    ui.add_enabled(ready, egui::Button::new(text).sense(egui::Sense::CLICK)).clicked()
}

pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    // The tab that asks, by number and title (several tabs may show the same host).
    let asking =
        app.prompts.open.first().and_then(|p| p.session).and_then(|id| {
            crate::session_tab::pane_of(app, id).map(|(t, _)| (t, app.terminals.get(&id).map(|s| s.title.clone()).unwrap_or_default()))
        });
    // The answer field takes the keyboard only when the asking tab is on screen: typing meant for
    // another page never lands in a question the user has not seen.
    let on_screen = asking.as_ref().is_some_and(|(index, _)| app.ui.page == crate::state::Page::Tab(*index));
    let Some(prompt) = app.prompts.open.first_mut() else {
        app.prompts.shown = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let (since, first_frame) = match app.prompts.shown {
        Some((id, since)) if id == prompt.id => (since, false),
        _ => {
            app.prompts.shown = Some((prompt.id, now));
            (now, true)
        }
    };
    let waited = now - since;
    let ready = waited >= ANSWER_DELAY;
    if !ready {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(ANSWER_DELAY - waited));
    }
    let tokens = Tokens::get(ctx);
    let mut answer = None;
    let mut show_tab = None;
    crate::widgets::modal(ctx, egui::Id::unique("sshcraft-prompt")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 520.0));
        if let Some((index, title)) = &asking {
            ui.horizontal(|ui| {
                let number = (index + 1).to_string();
                ui.label(
                    egui::RichText::new(trf("Asked by tab {number}: {title}", &[("number", &number), ("title", &visible(title))]))
                        .color(tokens.text_dim),
                );
                if ui.small_button(tr("Show tab")).clicked() {
                    show_tab = Some(*index);
                }
            });
            ui.add_space(4.0);
        }
        match &prompt.kind {
            PromptKind::HostKey { host_name, address, port, fingerprint } => {
                crate::widgets::buttons::title(ui, tr("Unknown host key"));
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
                    if answer_button(ui, ready, tr("Cancel")) {
                        answer = Some(Answer::HostKey(false));
                    }
                    if answer_button(ui, ready, tr("Accept")) {
                        answer = Some(Answer::HostKey(true));
                    }
                });
            }
            PromptKind::ProxyCommand { what, host_name, command } => {
                if what == "password_ref" {
                    crate::widgets::buttons::title(ui, tr("Send a password from your password manager?"));
                    ui.add_space(6.0);
                    ui.label(trf(
                        "{host} wants this password from your password manager, sent to the server below. It was not set up for this server on this device (it came with sync or a share, or the address changed). Allow it only if the password belongs to this server.",
                        &[("host", &visible(host_name))],
                    ));
                } else {
                    crate::widgets::buttons::title(ui, tr("Run a program to connect?"));
                    ui.add_space(6.0);
                    ui.label(trf(
                        "{host} connects through this program on your computer. It was not set up on this device (it came with sync, a share or an agent, or it changed). Allow it only if you know it.",
                        &[("host", &visible(host_name))],
                    ));
                }
                ui.add_space(6.0);
                if let Some(warning) = command_warning(command) {
                    ui.label(egui::RichText::new(warning).color(tokens.warning));
                }
                ui.label(egui::RichText::new(crate::widgets::visible_lines(command)).monospace().color(tokens.text_strong));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if answer_button(ui, ready, tr("Cancel")) {
                        answer = Some(Answer::ProxyCommand(false));
                    }
                    if answer_button(ui, ready, tr("Allow on this device")) {
                        answer = Some(Answer::ProxyCommand(true));
                    }
                });
            }
            PromptKind::Agent { agent, action, target, detail } => {
                crate::widgets::buttons::title(ui, tr("An AI agent asks"));
                ui.add_space(6.0);
                egui::Grid::new(("sshcraft-agent-question", prompt.id)).num_columns(2).min_col_width(90.0).show(ui, |ui| {
                    ui.label(tr("Agent"));
                    ui.label(egui::RichText::new(visible(agent)).strong());
                    ui.end_row();
                    ui.label(tr("Action"));
                    ui.label(visible(action));
                    ui.end_row();
                    if !target.is_empty() {
                        ui.label(tr("Target"));
                        ui.label(visible(target));
                        ui.end_row();
                    }
                });
                if !detail.is_empty() {
                    if let Some(warning) = command_warning(detail) {
                        ui.label(egui::RichText::new(warning).color(tokens.warning));
                    }
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(crate::widgets::visible_lines(detail)).monospace());
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if answer_button(ui, ready, tr("Deny")) {
                        answer = Some(Answer::Agent(false));
                    }
                    if answer_button(ui, ready, tr("Allow")) {
                        answer = Some(Answer::Agent(true));
                    }
                });
            }
            PromptKind::Auth { host_name, username, name, instructions, lines } => {
                let who = if username.is_empty() { visible(host_name) } else { format!("{}@{}", visible(username), visible(host_name)) };
                crate::widgets::buttons::title(ui, trf("Sign in to {host}", &[("host", &who)]));
                if !name.is_empty() {
                    ui.label(visible(name));
                }
                if !instructions.is_empty() {
                    ui.label(egui::RichText::new(crate::widgets::visible_lines(instructions)).color(tokens.text_dim));
                }
                ui.add_space(6.0);
                egui::Grid::new(("sshcraft-prompt-lines", prompt.id)).num_columns(2).min_col_width(130.0).show(ui, |ui| {
                    for (i, ((text, echo), value)) in lines.iter().zip(prompt.answers.iter_mut()).enumerate() {
                        ui.label(visible(text));
                        let field = crate::widgets::secret_field(ui, value, answer_id(prompt.id, i), !echo, "", 300.0);
                        // The keyboard goes to the first answer, not to the terminal behind.
                        if first_frame && on_screen && i == 0 {
                            field.request_focus();
                        }
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
    if let Some(index) = show_tab {
        app.ui.page = crate::state::Page::Tab(index);
    }
    let Some(answer) = answer else { return };
    let id = prompt.id;
    app.prompts.withdraw(id);
    match answer {
        Answer::Agent(allow) => app.run_reported("prompt.agent", json!({ "prompt_id": id, "allow": allow })),
        Answer::ProxyCommand(allow) => app.run_reported("prompt.proxy_command", json!({ "prompt_id": id, "allow": allow })),
        Answer::HostKey(accept) => app.run_reported("prompt.host_key", json!({ "prompt_id": id, "accept": accept })),
        Answer::Auth(answers) => app.run_reported("prompt.auth", json!({ "prompt_id": id, "answers": answers })),
    }
}
