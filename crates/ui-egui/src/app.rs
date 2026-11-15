//! The app: the engine session, the cached data, the UI state, and one frame at a time.

use serde_json::Value;
use sshcraft_engine::{EngineEvent, Session};
use tokio::sync::broadcast::error::TryRecvError;

use crate::data::{self, Data, Refresh};
use crate::i18n::tr;
use crate::runner::Runner;
use crate::state::{Page, UiState};
use crate::theme::Tokens;
use crate::toasts::Toasts;

pub struct SshcraftApp {
    pub session: Session,
    pub runner: Runner,
    pub data: Data,
    pub ui: UiState,
    pub toasts: Toasts,
    events: tokio::sync::broadcast::Receiver<EngineEvent>,
    /// Kept alive for the app when it owns the runtime (tests); the app binary owns it itself.
    _runtime: Option<std::sync::Arc<tokio::runtime::Runtime>>,
}

impl SshcraftApp {
    pub fn new(session: Session, ctx: &egui::Context) -> Self {
        let events = session.subscribe();
        let data = Data { dirty: Refresh::DATA | Refresh::SETTINGS, ..Data::default() };
        Self {
            runner: Runner::new(session.clone(), ctx.clone()),
            session,
            data,
            ui: UiState::default(),
            toasts: Toasts::default(),
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

    /// One frame: results that came back, the engine's events, then drawing.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        for (then, result) in self.runner.take_done() {
            then(self, result);
        }
        self.take_events();
        data::refresh(self);
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, &self.data.settings);
        crate::i18n::set_language(crate::i18n::Lang::resolve(self.data.settings.general.language));
        let tokens = Tokens::get(&ctx);
        egui::Panel::left("sshcraft-sidebar").exact_size(180.0).show(ui, |ui| {
            for (page, label) in [(Page::Hosts, tr("Hosts")), (Page::Keys, tr("Keys")), (Page::Settings, tr("Settings"))] {
                if ui.selectable_label(self.ui.page == page, label).clicked() {
                    self.ui.page = page;
                }
            }
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(tokens.bg)).show(ui, |ui| {
            ui.heading(match self.ui.page {
                Page::Hosts => tr("Hosts"),
                Page::Keys => tr("Keys"),
                Page::Settings => tr("Settings"),
                Page::Tab(_) => tr("Terminal"),
            });
        });
        self.toasts.show(&ctx);
    }
}
