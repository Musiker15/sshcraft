//! Port forwarding: the rules of every host, started and stopped here (R5.6).

use egui::Ui;

use crate::app::SshcraftApp;
use crate::i18n::tr;

pub fn show(_app: &mut SshcraftApp, ui: &mut Ui) {
    ui.heading(tr("Port forwarding"));
}
