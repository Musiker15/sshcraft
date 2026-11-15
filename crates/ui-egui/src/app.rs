//! The app: the engine session, the cached data, the UI state, and one frame at a time.

use serde_json::Value;
use sshcraft_engine::{EngineEvent, Session};
use tokio::sync::broadcast::error::TryRecvError;

use crate::data::{self, Data, Refresh};
use crate::i18n::tr;
use crate::keymap::Bindings;
use crate::runner::Runner;
use crate::state::{Page, PaneSlot, Panes, TabState, UiState};
use crate::toasts::Toasts;

pub struct SshcraftApp {
    pub session: Session,
    pub runner: Runner,
    pub data: Data,
    pub ui: UiState,
    pub toasts: Toasts,
    pub bindings: Bindings,
    /// File dialogs: asked in a frame, shown after it, answered on a later frame.
    pub dialogs: craft_dialogs::Dialogs<crate::pages::import_export::DialogFor>,
    /// The terminals of the open session tabs.
    pub terminals: std::collections::HashMap<sshcraft_engine::SessionId, crate::session_tab::SessionTab>,
    /// The engine's open questions.
    pub prompts: crate::prompts::Prompts,
    /// The terminal font loaded for the setting `appearance.font_family`.
    pub(crate) term_font: Option<(String, egui::FontFamily)>,
    /// Checked links the engine asked to open.
    urls: Vec<String>,
    /// Which dialogs are open and the command one waits for.
    pub forms: crate::forms::Forms,
    /// The session log being played back.
    pub log_viewer: Option<crate::log_viewer::LogViewer>,
    /// The shortcut settings the bindings were built from.
    bindings_from: Option<sshcraft_engine::settings::Shortcuts>,
    events: tokio::sync::broadcast::Receiver<EngineEvent>,
    /// Kept alive for the app when it owns the runtime (tests); the app binary owns it itself.
    _runtime: Option<std::sync::Arc<tokio::runtime::Runtime>>,
}

impl SshcraftApp {
    pub fn new(session: Session, ctx: &egui::Context) -> Self {
        // Names from servers and imports can be in any script: the system's fonts fill in.
        ctx.add_font_provider(std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new()));
        let events = session.subscribe();
        // Terminal output wakes the window; nothing polls for it.
        let waker = ctx.clone();
        session.set_terminal_waker(std::sync::Arc::new(move || waker.request_repaint()));
        let data = Data { dirty: Refresh::DATA | Refresh::SETTINGS, ..Data::default() };
        let wake = ctx.clone();
        Self {
            runner: Runner::new(session.clone(), ctx.clone()),
            session,
            data,
            ui: UiState::default(),
            toasts: Toasts::default(),
            bindings: Bindings::default(),
            dialogs: craft_dialogs::Dialogs::new(std::sync::Arc::new(move || wake.request_repaint())),
            terminals: std::collections::HashMap::new(),
            // Questions asked before the window subscribed are fetched once.
            prompts: crate::prompts::Prompts::fetch_first(),
            term_font: None,
            urls: Vec::new(),
            forms: crate::forms::Forms::default(),
            log_viewer: None,
            bindings_from: None,
            events,
            _runtime: None,
        }
    }

    /// Keeps `runtime` alive as long as the app (for tests that build the runtime themselves).
    pub fn keep_runtime(&mut self, runtime: std::sync::Arc<tokio::runtime::Runtime>) {
        self._runtime = Some(runtime);
    }

    /// Runs `id`; an error becomes a message, a success does nothing more.
    pub fn run_reported(&self, id: &str, params: Value) {
        self.runner.run(
            id,
            params,
            Box::new(|app, result| {
                if let Err(e) = result {
                    app.toasts.error(&e.to_string());
                }
            }),
        );
    }

    /// What the engine reported since the last frame.
    fn take_events(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(EngineEvent::DataChanged { .. }) => self.data.dirty |= Refresh::DATA,
                Ok(EngineEvent::SettingsChanged) => self.data.dirty |= Refresh::SETTINGS,
                Ok(EngineEvent::HostKeyPrompt(p)) => self.prompts.add_from(crate::prompts::from_event(&p, "host_key")),
                Ok(EngineEvent::AuthPrompt(p)) => self.prompts.add_from(crate::prompts::from_event(&p, "auth")),
                Ok(EngineEvent::PromptWithdrawn { prompt_id }) => self.prompts.withdraw(prompt_id),
                Ok(EngineEvent::SyncStatus { server, status }) => {
                    if let Ok(value) = serde_json::to_value(&status) {
                        self.data.sync_status.insert(server.0.to_string(), value);
                    }
                }
                Ok(EngineEvent::SyncWarning { detail, .. }) => self.toasts.error(&crate::widgets::visible(&detail)),
                Ok(EngineEvent::ForwardStatus(status)) => {
                    if let Ok(value) = serde_json::to_value(&status) {
                        self.data.forwards.insert(status.rule_id, value);
                    }
                }
                Ok(EngineEvent::Ui(sshcraft_engine::UiRequest::OpenUrl { url })) => self.urls.push(url),
                Ok(_) => {}
                // Events were lost: fetch everything again.
                Err(TryRecvError::Lagged(_)) => {
                    self.data.dirty |= Refresh::DATA | Refresh::SETTINGS;
                    self.prompts.stale = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }
    }

    /// A new-pane page with an id no other pane has.
    pub fn new_pane(&mut self) -> PaneSlot {
        self.ui.next_pane = self.ui.next_pane.wrapping_add(1);
        PaneSlot::New { id: self.ui.next_pane }
    }

    /// Opens the new-tab page as a tab and shows it.
    pub fn open_new_tab(&mut self) {
        let slot = self.new_pane();
        self.ui.tabs.push(TabState { title: tr("New tab").to_string(), panes: Panes::single(slot), named: false });
        self.ui.page = Page::Tab(self.ui.tabs.len() - 1);
    }

    /// Adds a new-pane page beside the focused pane of the shown tab.
    pub fn split(&mut self) {
        let Page::Tab(i) = self.ui.page else { return };
        let slot = self.new_pane();
        let Some(tab) = self.ui.tabs.get_mut(i) else { return };
        if !tab.panes.add(slot) {
            self.toasts.error(tr("A tab holds at most 16 panes."));
        }
    }

    /// Closes the focused pane of the shown tab (and its session).
    pub fn close_pane(&mut self) {
        if let Page::Tab(i) = self.ui.page
            && let Some(focused) = self.ui.tabs.get(i).map(|t| t.panes.focused)
        {
            self.close_pane_at(i, focused);
        }
    }

    /// Closes pane `pane` of tab `tab` (and its session); the last pane closes the tab.
    pub fn close_pane_at(&mut self, tab: usize, pane: usize) {
        let Some(t) = self.ui.tabs.get_mut(tab) else { return };
        if t.panes.panes.len() <= 1 {
            self.close_tab_at(tab);
            return;
        }
        if let Some(PaneSlot::Session { session }) = t.panes.remove(pane) {
            self.terminals.remove(&session);
            self.run_reported("session.close", serde_json::json!({ "id": session }));
        }
    }

    /// Moves pane `pane` of tab `tab` into a tab of its own, without reconnecting.
    pub fn move_pane_to_own_tab(&mut self, tab: usize, pane: usize) {
        let Some(t) = self.ui.tabs.get_mut(tab) else { return };
        if t.panes.panes.len() <= 1 {
            return;
        }
        let Some(slot) = t.panes.remove(pane) else { return };
        self.ui.tabs.push(TabState { title: String::new(), panes: Panes::single(slot), named: false });
        self.ui.page = Page::Tab(self.ui.tabs.len() - 1);
    }

    /// Moves the panes of tab `from` into tab `to` (up to 16; the rest stays in `from`).
    pub fn merge_tabs(&mut self, from: usize, to: usize) {
        if from == to || from >= self.ui.tabs.len() || to >= self.ui.tabs.len() {
            return;
        }
        let mut moving = std::mem::take(&mut self.ui.tabs[from].panes.panes);
        let room = crate::panes::MAX_PANES.saturating_sub(self.ui.tabs[to].panes.panes.len());
        let rest = moving.split_off(room.min(moving.len()));
        let target = &mut self.ui.tabs[to].panes;
        target.panes.extend(moving);
        target.reset_sizes();
        if rest.is_empty() {
            self.ui.tabs.remove(from);
            let to = if from < to { to - 1 } else { to };
            self.ui.page = Page::Tab(to);
        } else {
            let left = &mut self.ui.tabs[from].panes;
            left.panes = rest;
            left.focused = 0;
            left.reset_sizes();
            self.ui.page = Page::Tab(to);
        }
    }

    /// Moves the focus `step` panes on in the shown tab, wrapping around.
    pub fn focus_pane(&mut self, step: isize) {
        if let Page::Tab(i) = self.ui.page
            && let Some(t) = self.ui.tabs.get_mut(i)
        {
            let count = t.panes.panes.len().max(1) as isize;
            t.panes.focused = (t.panes.focused as isize + step).rem_euclid(count) as usize;
        }
    }

    /// Typing into one pane goes to every connected pane of the shown tab, or stops doing so.
    pub fn toggle_broadcast(&mut self) {
        if let Page::Tab(i) = self.ui.page
            && let Some(t) = self.ui.tabs.get_mut(i)
        {
            t.panes.broadcast = !t.panes.broadcast;
        }
    }

    /// One pane large or all in the grid, in the shown tab.
    pub fn toggle_layout(&mut self) {
        if let Page::Tab(i) = self.ui.page
            && let Some(t) = self.ui.tabs.get_mut(i)
        {
            t.panes.focus_layout = !t.panes.focus_layout;
        }
    }

    /// Closes the shown tab (and its session); the tab before it, or the hosts page, comes next.
    pub fn close_tab(&mut self) {
        if let Page::Tab(i) = self.ui.page {
            self.close_tab_at(i);
        }
    }

    /// Closes tab `i` (and its session). The shown page stays, one place to the left when it was
    /// after `i`; when `i` was shown, the tab before it, or the hosts page, comes next.
    pub fn close_tab_at(&mut self, i: usize) {
        if i >= self.ui.tabs.len() {
            return;
        }
        let tab = self.ui.tabs.remove(i);
        for session in tab.panes.sessions() {
            self.terminals.remove(&session);
            self.run_reported("session.close", serde_json::json!({ "id": session }));
        }
        self.ui.page = match self.ui.page {
            Page::Tab(shown) if shown == i => {
                if self.ui.tabs.is_empty() {
                    Page::Hosts
                } else {
                    Page::Tab(i.saturating_sub(1).min(self.ui.tabs.len().saturating_sub(1)))
                }
            }
            Page::Tab(shown) if shown > i => Page::Tab(shown - 1),
            other => other,
        };
    }

    /// Shows the tab `step` places further, wrapping around; from a page, the first or last tab.
    pub fn cycle_tab(&mut self, step: isize) {
        let count = self.ui.tabs.len();
        if count == 0 {
            return;
        }
        let next = match self.ui.page {
            Page::Tab(i) => (i as isize + step).rem_euclid(count as isize) as usize,
            _ if step > 0 => 0,
            _ => count - 1,
        };
        self.ui.page = Page::Tab(next);
    }

    /// Whether the shown page is a terminal (shortcuts then leave the shell its keys).
    pub fn in_terminal(&self) -> bool {
        matches!(self.ui.page, Page::Tab(i) if matches!(self.ui.tabs.get(i).and_then(|t| t.panes.focused_slot()), Some(PaneSlot::Session { .. })))
    }

    /// One frame: results that came back, the engine's events, then drawing.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        for (then, result) in self.runner.take_done() {
            then(self, result);
        }
        self.take_events();
        crate::pages::import_export::take_dialog_answers(self);
        data::refresh(self);
        crate::prompts::refresh(self);
        let ctx = ui.ctx().clone();
        for url in self.urls.drain(..) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        // A character no font has is drawn as a box, never a panic (tests switch egui to panic).
        ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
        crate::theme::apply(&ctx, &self.data.settings);
        crate::i18n::set_language(crate::i18n::Lang::resolve(self.data.settings.general.language));
        if self.bindings_from.as_ref() != Some(&self.data.settings.shortcuts) {
            self.bindings = Bindings::from_settings(&self.data.settings.shortcuts);
            self.bindings_from = Some(self.data.settings.shortcuts.clone());
        }
        // The palette's own keys (arrows, Enter, Escape) go to it while it is open.
        // A shortcut being recorded takes the next key itself.
        if !self.ui.palette.open && self.ui.settings.recording.is_none() {
            for id in self.bindings.dispatch(&ctx, self.in_terminal()) {
                crate::commands::run(self, id);
            }
        }
        crate::session_tab::pump_all(self, &ctx);
        crate::session_tab::refresh_titles(self);
        crate::session_tab::mark_looked_at(self);
        crate::shell::show(self, ui);
        crate::session_tab::take_request(self);
        crate::session_tab::show_picker(self, &ctx);
        crate::session_tab::show_clipboard_question(self, &ctx);
        crate::pages::templates::show_dialog(self, &ctx);
        crate::pages::snippets::dialogs(self, &ctx);
        crate::pages::sync::dialogs(self, &ctx);
        crate::prompts::show(self, &ctx);
        crate::log_viewer::show(self, &ctx);
        // The logs are fetched again on the next visit of their page.
        if self.ui.page != Page::Logs {
            self.ui.logs.fresh = false;
        }
        // A shortcut recording ends when the user leaves the settings.
        if self.ui.page != Page::Settings {
            self.ui.settings.recording = None;
        }
        if let Some((id, arg)) = crate::palette::show(self, &ctx) {
            match id {
                crate::palette::OPEN_WORKSPACE => {
                    let template = self.data.templates.iter().find(|t| t.get("id").and_then(serde_json::Value::as_str) == arg.as_deref()).cloned();
                    if let Some(template) = template {
                        crate::pages::templates::open(self, &template);
                    }
                }
                crate::palette::RUN_SNIPPET => {
                    if let Some(snippet) = arg {
                        crate::pages::snippets::open_run(self, &snippet);
                    }
                }
                _ => crate::commands::run(self, id),
            }
        }
        self.toasts.show(&ctx);
        self.forms.observe(crate::forms::shape(&self.ui));
    }

    /// Shows the file dialog asked for in this frame. Call it after [`Self::frame`], on the main
    /// thread, with the window as `parent`.
    pub fn show_dialogs<W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle>(&mut self, parent: Option<&W>) {
        self.dialogs.show_pending(parent);
    }
}
