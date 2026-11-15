//! Session logs: recorded output to search, play back and export (R5.9).

use egui::Ui;

use crate::app::SshcraftApp;
use crate::i18n::tr;

pub fn show(_app: &mut SshcraftApp, ui: &mut Ui) {
    ui.heading(tr("Logs"));
}
