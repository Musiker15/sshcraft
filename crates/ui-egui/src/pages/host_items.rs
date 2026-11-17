//! A host as a row (in lists, groups and "Without a group") and as a card ("Recently
//! connected"): its state, name and address cut to fit, and a button for SSH and one for SFTP.
//! SFTP is there but disabled until S1 brings it.

use egui::{Align, Button, FontId, Layout, RichText, Sense, Stroke, Ui, vec2};
use sshcraft_model::model::Group;

use crate::app::SshcraftApp;
use crate::data::HostRow;
use crate::i18n::tr;
use crate::state::ConnectRequest;
use crate::theme::Tokens;
use crate::widgets::elided_label;

/// Height of a host row: name and address stacked, with room for the buttons.
pub const ROW_HEIGHT: f32 = 48.0;

/// The colour and the words for the host's reachability, when the setting shows it.
fn reach(app: &SshcraftApp, tokens: &Tokens, row: &HostRow) -> Option<(egui::Color32, &'static str)> {
    if !app.data.settings.appearance.show_reachability {
        return None;
    }
    Some(match app.ui.hosts.reachability.hosts.get(&row.host.id) {
        Some(Some(true)) => (tokens.success, tr("Answers")),
        Some(Some(false)) => (tokens.danger, tr("Does not answer")),
        Some(None) => (tokens.text_dim, tr("Behind a jump host or proxy: not checked")),
        None => (tokens.text_dim, tr("Not checked yet")),
    })
}

fn dot(ui: &mut Ui, state: Option<(egui::Color32, &'static str)>) {
    if let Some((color, text)) = state {
        let (rect, response) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.5, color);
        response.on_hover_text(text);
    }
}

/// The SSH button: the accent as outline in rows, filled on cards.
fn ssh_button(ui: &mut Ui, tokens: &Tokens, filled: bool, width: f32) -> bool {
    let text = RichText::new(tr("SSH")).strong().color(if filled { egui::Color32::WHITE } else { tokens.accent });
    let button = Button::new(text)
        .fill(if filled { tokens.accent } else { egui::Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0, tokens.accent))
        .corner_radius(tokens.radius)
        .min_size(vec2(width, 32.0));
    ui.add(button).clicked()
}

/// The SFTP button, disabled until S1.
fn sftp_button(ui: &mut Ui, tokens: &Tokens, width: f32) {
    let button = Button::new(tr("SFTP")).stroke(Stroke::new(1.0, tokens.button_border)).corner_radius(tokens.radius).min_size(vec2(width, 32.0));
    ui.add_enabled(false, button).on_disabled_hover_text(tr("SFTP comes with S1"));
}

/// One host as a row: selection on click (with the modifiers of the click), connect on a double
/// click or the SSH button, the host's menu on a right click.
pub fn host_row(app: &mut SshcraftApp, ui: &mut Ui, pos: usize, row: &HostRow, all_groups: &[Group]) {
    let tokens = Tokens::get(ui.ctx());
    let id = row.host.id;
    if app.ui.hosts.selection.hosts.contains(&id) {
        ui.painter().rect_filled(ui.max_rect(), f32::from(tokens.radius), tokens.row_selected);
    }
    let state = reach(app, &tokens, row);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(6.0);
        sftp_button(ui, &tokens, 56.0);
        if ssh_button(ui, &tokens, false, 52.0) {
            app.ui.connect_request = Some(ConnectRequest::Host(id));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(8.0);
            dot(ui, state);
            let width = (ui.available_width() - 8.0).max(24.0);
            let response = ui
                .vertical(|ui| {
                    let response = elided_label(ui, &row.host.name, width, tokens.text, FontId::proportional(14.0));
                    if let Some(address) = &row.host.settings.address {
                        elided_label(ui, address, width, tokens.text_dim, FontId::monospace(12.0));
                    }
                    if crate::pages::hosts::needs_own_sign_in(&row.host) {
                        ui.label(RichText::new(tr("Add your own sign-in")).color(tokens.warning).small()).on_hover_text(tr(
                            "This host was shared without its key or password. Edit it and choose a key or password of your own; it stays on your devices.",
                        ));
                    }
                    response
                })
                .inner;
            if response.clicked() {
                // The modifiers of the release that made the click; held keys as a fallback.
                let modifiers = ui.input(|i| {
                    i.events
                        .iter()
                        .rev()
                        .find_map(|e| match e {
                            egui::Event::PointerButton { pressed: false, modifiers, .. } => Some(*modifiers),
                            _ => None,
                        })
                        .unwrap_or(i.modifiers)
                });
                crate::pages::hosts::select(app, pos, id, modifiers);
            }
            if response.double_clicked() {
                app.ui.connect_request = Some(ConnectRequest::Host(id));
            }
            response.context_menu(|ui| crate::pages::hosts::host_menu(app, ui, row, all_groups));
        });
    });
}

/// One host as a card: a tile with the system, name and address, its state, and the SSH and
/// SFTP buttons side by side. The caller sets the width.
pub fn host_card(app: &mut SshcraftApp, ui: &mut Ui, row: &HostRow) {
    let tokens = Tokens::get(ui.ctx());
    let state = reach(app, &tokens, row);
    egui::Frame::new().fill(tokens.panel).stroke(Stroke::new(1.0, tokens.separator)).corner_radius(tokens.radius_card).inner_margin(14.0).show(
        ui,
        |ui| {
            let inner = ui.available_width();
            ui.set_width(inner);
            ui.horizontal(|ui| {
                let label: String = row
                    .host
                    .os
                    .as_ref()
                    .filter(|os| !matches!(os, sshcraft_model::model::HostOs::Other))
                    .map(|os| format!("{os:?}").chars().take(3).collect::<String>().to_uppercase())
                    .unwrap_or_else(|| "SSH".into());
                let (tile, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                ui.painter().rect_filled(tile, tokens.radius_card.saturating_sub(4), tokens.row_hover);
                ui.painter().text(tile.center(), egui::Align2::CENTER_CENTER, label, FontId::monospace(12.0), tokens.text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dot(ui, state);
                    let width = (ui.available_width() - 8.0).max(24.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        elided_label(ui, &row.host.name, width, tokens.text_strong, FontId::proportional(15.0));
                        if let Some(address) = &row.host.settings.address {
                            elided_label(ui, address, width, tokens.text_dim, FontId::monospace(12.0));
                        }
                    });
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let half = ((inner - ui.spacing().item_spacing.x) / 2.0).max(40.0);
                if ssh_button(ui, &tokens, true, half) {
                    app.ui.connect_request = Some(ConnectRequest::Host(row.host.id));
                }
                sftp_button(ui, &tokens, half);
            });
        },
    );
}
