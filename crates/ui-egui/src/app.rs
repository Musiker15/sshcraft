//! The app: the engine session, the cached data, the UI state, and one frame at a time.

use serde_json::Value;
use sshcraft_engine::{EngineEvent, Session};
use tokio::sync::broadcast::error::TryRecvError;

use crate::data::{self, Data, Refresh};
use crate::i18n::tr;
use crate::keymap::Bindings;
use crate::runner::Runner;
use crate::state::{Page, TabKind, TabState, UiState};
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
                Ok(_) => {}
                // Events were lost: fetch everything again.
                Err(TryRecvError::Lagged(_)) => self.data.dirty |= Refresh::DATA | Refresh::SETTINGS,
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }
    }

    /// Opens the new-tab page as a tab and shows it.
    pub fn open_new_tab(&mut self) {
        self.ui.tabs.push(TabState { title: tr("New tab").to_string(), kind: TabKind::New });
        self.ui.page = Page::Tab(self.ui.tabs.len() - 1);
    }

    /// Closes the shown tab (and its session); the tab before it, or the hosts page, comes next.
    pub fn close_tab(&mut self) {
        let Page::Tab(i) = self.ui.page else { return };
        if i >= self.ui.tabs.len() {
            return;
        }
        let tab = self.ui.tabs.remove(i);
        if let TabKind::Session { session } = tab.kind {
            self.run_reported("session.close", serde_json::json!({ "id": session }));
        }
        self.ui.page = if self.ui.tabs.is_empty() { Page::Hosts } else { Page::Tab(i.saturating_sub(1).min(self.ui.tabs.len() - 1)) };
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
        matches!(self.ui.page, Page::Tab(i) if matches!(self.ui.tabs.get(i).map(|t| &t.kind), Some(TabKind::Session { .. })))
    }

    /// One frame: results that came back, the engine's events, then drawing.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        for (then, result) in self.runner.take_done() {
            then(self, result);
        }
        self.take_events();
        crate::pages::import_export::take_dialog_answers(self);
        data::refresh(self);
        let ctx = ui.ctx().clone();
        // A character no font has is drawn as a box, never a panic (tests switch egui to panic).
        ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
        crate::theme::apply(&ctx, &self.data.settings);
        crate::i18n::set_language(crate::i18n::Lang::resolve(self.data.settings.general.language));
        if self.bindings_from.as_ref() != Some(&self.data.settings.shortcuts) {
            self.bindings = Bindings::from_settings(&self.data.settings.shortcuts);
            self.bindings_from = Some(self.data.settings.shortcuts.clone());
        }
        // The palette's own keys (arrows, Enter, Escape) go to it while it is open.
        if !self.ui.palette.open {
            for id in self.bindings.dispatch(&ctx, self.in_terminal()) {
                crate::commands::run(self, id);
            }
        }
        crate::shell::show(self, ui);
        if let Some(id) = crate::palette::show(self, &ctx) {
            crate::commands::run(self, id);
        }
        self.toasts.show(&ctx);
    }

    /// Shows the file dialog asked for in this frame. Call it after [`Self::frame`], on the main
    /// thread, with the window as `parent`.
    pub fn show_dialogs<W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle>(&mut self, parent: Option<&W>) {
        self.dialogs.show_pending(parent);
    }
}
