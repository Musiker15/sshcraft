//! Portable mode's start: before the engine opens, a small window asks for the passphrase that
//! unlocks the vault's master key beside the data, or, on the first start, has one chosen. It
//! shows nothing of the data; the engine opens only with the key it hands back.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use sshcraft_engine::portable::{self, PassphraseMasterKey, PortableError};
use sshcraft_engine::settings::{AppTheme, EngineSettings};
use zeroize::Zeroizing;

use crate::i18n::{tr, trf};
use crate::theme::Tokens;

/// The unlock window's state.
pub struct Unlock {
    data_dir: PathBuf,
    first: bool,
    passphrase: Zeroizing<String>,
    again: Zeroizing<String>,
    error: Option<String>,
    working: Option<Receiver<Result<PassphraseMasterKey, PortableError>>>,
    /// The key, once the passphrase opened it; the caller takes it and starts the engine.
    pub opened: Option<PassphraseMasterKey>,
    /// *Quit* was chosen: SSHCraft does not start.
    pub quit: bool,
    fonts: bool,
    theme: AppTheme,
    /// The field that takes the keyboard next frame: the first at the start and after a wrong
    /// passphrase, the second after Enter in the first on the first start.
    focus: Option<Focus>,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Passphrase,
    Again,
}

impl Unlock {
    /// The window for the data in `data_dir`; the first start (no key file yet) chooses a
    /// passphrase.
    pub fn new(data_dir: PathBuf) -> Self {
        let first = portable::is_new(&data_dir);
        Self {
            data_dir,
            first,
            passphrase: Zeroizing::default(),
            again: Zeroizing::default(),
            error: None,
            working: None,
            opened: None,
            quit: false,
            fonts: false,
            theme: AppTheme::System,
            focus: Some(Focus::Passphrase),
        }
    }

    /// The window in `theme` instead of the system's (pictures of it).
    pub fn with_theme(mut self, theme: AppTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Whether a passphrase is being checked.
    pub fn working(&self) -> bool {
        self.working.is_some()
    }

    /// Whether the window has finished: the key is open or *Quit* was chosen.
    pub fn done(&self) -> bool {
        self.opened.is_some() || self.quit
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // The app's fonts and theme, so the window looks like SSHCraft and not like egui.
        if !self.fonts {
            self.fonts = true;
            crate::fonts::install_embedded(ui.ctx(), crate::fonts::CRAFT_FONTS);
            ui.ctx().add_font_provider(std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new()));
        }
        let mut settings = EngineSettings::default();
        settings.appearance.app_theme = self.theme;
        crate::theme::apply(ui.ctx(), &settings);
        self.poll(ui.ctx());
        let tokens = Tokens::get(ui.ctx());
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            crate::widgets::buttons::title(ui, if self.first { tr("Choose a passphrase") } else { tr("Unlock SSHCraft") });
            ui.add_space(6.0);
            let about = if self.first {
                tr("This copy of SSHCraft keeps its data beside the program. The passphrase protects your keys and passwords there; without it nobody can open them, you neither.")
            } else {
                tr("This copy of SSHCraft keeps its data beside the program. Enter the passphrase to open it.")
            };
            ui.label(egui::RichText::new(about).color(tokens.text_dim));
            ui.add_space(12.0);
            let busy = self.working.is_some();
            let mut submit = false;
            let label = ui.label(tr("Passphrase"));
            let field = ui.add_enabled(!busy, egui::TextEdit::singleline(&mut *self.passphrase).password(true).desired_width(f32::INFINITY)).labelled_by(label.id);
            if !busy && self.focus == Some(Focus::Passphrase) {
                field.request_focus();
                self.focus = None;
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            // On the first start Enter goes on to the second field; otherwise it unlocks.
            if enter && self.first {
                self.focus = Some(Focus::Again);
            } else {
                submit |= enter;
            }
            if self.first {
                let label = ui.label(tr("Passphrase again"));
                let field = ui.add_enabled(!busy, egui::TextEdit::singleline(&mut *self.again).password(true).desired_width(f32::INFINITY)).labelled_by(label.id);
                if !busy && self.focus == Some(Focus::Again) {
                    field.request_focus();
                    self.focus = None;
                }
                submit |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
            if let Some(error) = &self.error {
                ui.add_space(4.0);
                ui.colored_label(tokens.danger, error);
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if busy {
                    ui.add(egui::Spinner::new());
                    ui.label(egui::RichText::new(tr("Opening…")).color(tokens.text_dim));
                    return;
                }
                let action = if self.first { tr("Create") } else { tr("Unlock") };
                submit |= crate::widgets::buttons::primary(ui, None, action, 32.0).clicked();
                if crate::widgets::buttons::ghost(ui, tr("Quit"), 32.0).clicked() {
                    self.quit = true;
                }
            });
            if submit && !busy {
                self.submit(ui.ctx());
            }
        });
    }

    fn submit(&mut self, ctx: &egui::Context) {
        if self.first {
            if self.passphrase.chars().count() < portable::MIN_PASSPHRASE {
                self.error = Some(too_short());
                return;
            }
            if self.passphrase != self.again {
                self.error = Some(tr("The two passphrases differ.").to_owned());
                return;
            }
        } else if self.passphrase.is_empty() {
            return;
        }
        self.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = self.data_dir.clone();
        let passphrase = self.passphrase.clone();
        let ctx = ctx.clone();
        // Argon2id takes a moment on purpose; the window keeps drawing meanwhile.
        std::thread::spawn(move || {
            let _ = tx.send(PassphraseMasterKey::open(&dir, &passphrase));
            ctx.request_repaint();
        });
        self.working = Some(rx);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.working else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => Err(PortableError::Io(tr("The passphrase could not be checked.").to_owned())),
        };
        self.working = None;
        match result {
            Ok(key) => self.opened = Some(key),
            Err(PortableError::WrongPassphrase) => {
                self.passphrase.clear();
                self.focus = Some(Focus::Passphrase);
                self.error = Some(tr("The passphrase is wrong.").to_owned());
            }
            Err(PortableError::WeakPassphrase) => self.error = Some(too_short()),
            Err(PortableError::Damaged) => self.error = Some(tr("The master key file beside the data is damaged.").to_owned()),
            Err(PortableError::Io(e)) => self.error = Some(e),
        }
    }
}

fn too_short() -> String {
    trf("The passphrase needs at least {count} characters.", &[("count", &portable::MIN_PASSPHRASE.to_string())])
}
