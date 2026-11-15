//! Snippets: text to paste or run in terminals, grouped in packages (R5.7).

use egui::Ui;

use crate::app::SshcraftApp;
use crate::i18n::tr;

pub fn show(_app: &mut SshcraftApp, ui: &mut Ui) {
    ui.heading(tr("Snippets"));
}
