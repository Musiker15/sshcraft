//! A host as a row (in lists, groups and "Without a group") and as a card ("Recently
//! connected"): its state, name and address cut to fit, and a button for SSH and one for SFTP.
//! SFTP is there but disabled until S1 brings it.

use egui::{Align, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use sshcraft_model::model::Group;

use crate::app::SshcraftApp;
use crate::data::HostRow;
use crate::i18n::tr;
use crate::state::ConnectRequest;
use crate::theme::Tokens;
use crate::widgets::elided_label;
use crate::widgets::icons::{Icon, draw};

/// Height of a host row: name and address stacked, with room for the buttons.
pub const ROW_HEIGHT: f32 = 48.0;
/// Height of a host row on a touch screen, where the buttons are 44 points.
pub const TOUCH_ROW_HEIGHT: f32 = 56.0;

/// Whether the window is a touch screen (a tablet or phone): bigger targets.
pub fn touch(app: &SshcraftApp) -> bool {
    app.form == crate::app::Form::Tablet
}

/// The height of a host row for this form.
pub fn row_height(app: &SshcraftApp) -> f32 {
    if touch(app) { TOUCH_ROW_HEIGHT } else { ROW_HEIGHT }
}

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
        ui.painter().circle_filled(rect.center(), 5.0, color);
        response.on_hover_text(text);
    }
}

/// A square button with only a symbol: SSH (the accent outline) or SFTP (grey), `size` points.
/// A disabled one still shows, named for screen readers, with the reason on hover.
fn action(ui: &mut Ui, icon: Icon, label: &str, accent: bool, enabled: bool, size: f32, bordered: bool) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), sense);
    let radius = (size * 0.26).round();
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, radius, tokens.row_hover);
    }
    if bordered {
        let border = if accent { tokens.accent } else { tokens.button_border };
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    }
    let color = if accent { tokens.accent } else { tokens.text_dim };
    draw(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled {
        response.on_hover_text(label).clicked()
    } else {
        response.on_hover_text(tr("SFTP comes with S1"));
        false
    }
}

/// A wide button with a symbol and a word, as on the cards: SSH filled with the accent, SFTP with
/// an outline. `icon` is left out on narrow cards.
fn wide_button(ui: &mut Ui, icon: Option<Icon>, label: &str, filled: bool, enabled: bool, size: egui::Vec2) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let radius = (size.y * 0.28).round();
    let text_color = if filled { egui::Color32::WHITE } else { tokens.text };
    if filled {
        ui.painter().rect_filled(rect, radius, if response.hovered() { tokens.accent_strong } else { tokens.accent });
    } else {
        if enabled && response.hovered() {
            ui.painter().rect_filled(rect, radius, tokens.row_hover);
        }
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, tokens.button_border), StrokeKind::Inside);
    }
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.5), text_color);
    let icon_width = if icon.is_some() { 21.0 } else { 0.0 };
    let start = rect.center().x - (galley.size().x + icon_width) / 2.0;
    if let Some(icon) = icon {
        draw(ui.painter(), icon, Rect::from_center_size(egui::pos2(start + 7.5, rect.center().y), vec2(15.0, 15.0)), text_color);
    }
    ui.painter().galley(egui::pos2(start + icon_width, rect.center().y - galley.size().y / 2.0), galley, text_color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled {
        response.clicked()
    } else {
        response.on_hover_text(tr("SFTP comes with S1"));
        false
    }
}

/// One host as a row: selection on click (with the modifiers of the click), connect on a double
/// click or the SSH button, the host's menu on a right click. The caller sets the row's size.
pub fn host_row(app: &mut SshcraftApp, ui: &mut Ui, pos: usize, row: &HostRow, all_groups: &[Group]) {
    let tokens = Tokens::get(ui.ctx());
    let id = row.host.id;
    let rect = ui.max_rect();
    if app.ui.hosts.selection.hosts.contains(&id) {
        ui.painter().rect_filled(rect, 12.0, tokens.row_selected);
    } else if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 12.0, tokens.row_hover);
    }
    let state = reach(app, &tokens, row);
    let address = shown_address(app, row);
    let touch = touch(app);
    let size = if touch { 44.0 } else { 34.0 };
    // A phone draws the symbols without a frame, as in its own lists.
    let bordered = crate::shell::layout_for(ui.ctx().content_rect().width()) != crate::shell::Layout::Narrow;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(8.0);
        action(ui, Icon::Folder, tr("SFTP"), false, false, size, bordered);
        if action(ui, Icon::Prompt, tr("SSH"), true, true, size, bordered) {
            app.ui.connect_request = Some(ConnectRequest::Host(id));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add_space(12.0);
            dot(ui, state);
            let width = (ui.available_width() - 8.0).max(24.0);
            let response = text_block(ui, row, address.as_deref(), width, if touch { 15.5 } else { 15.0 });
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

/// The address the host connects to: its own, or the one it inherits from its groups.
pub fn shown_address(app: &SshcraftApp, row: &HostRow) -> Option<String> {
    let chain = crate::pages::host_form::chain(row.host.group_id, app.ui.hosts.cache.groups());
    sshcraft_model::model::effective_endpoint(&row.host, &chain).ok().map(|(address, _)| address).or_else(|| row.host.settings.address.clone())
}

/// `text` laid out on one line at most `width` wide, cut with "…"; true when it was cut.
fn one_line(ui: &Ui, text: &str, width: f32, font: FontId, color: egui::Color32) -> (std::sync::Arc<egui::Galley>, bool) {
    let mut job = egui::text::LayoutJob::single_section(crate::widgets::visible(text), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let cut = galley.elided;
    (galley, cut)
}

/// The host's name over its address (and, for a host shared without sign-in, a warning),
/// centred in the row's height; the whole block takes clicks and is named by the host.
fn text_block(ui: &mut Ui, row: &HostRow, address: Option<&str>, width: f32, size: f32) -> egui::Response {
    let tokens = Tokens::get(ui.ctx());
    // Tags take at most 45 % of the line after the name; the name is cut first to its share.
    let pills = tag_pills(ui, &row.host.tags, width * 0.45);
    let pills_width: f32 = pills.iter().map(|(g, _)| g.size().x + PILL_PAD * 2.0 + PILL_GAP).sum();
    let (name, cut) = one_line(ui, &row.host.name, (width - pills_width).max(24.0), FontId::proportional(size), tokens.text_strong);
    let address = address.map(|a| (one_line(ui, a, width, FontId::monospace(12.0), tokens.text_dim).0, a.to_string()));
    let warn = crate::pages::hosts::needs_own_sign_in(&row.host)
        .then(|| one_line(ui, tr("Add your own sign-in"), width, FontId::proportional(12.0), tokens.warning).0);
    let height = ui.available_height().max(20.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let total = name.size().y + address.as_ref().map_or(0.0, |(g, _)| g.size().y + 1.0) + warn.as_ref().map_or(0.0, |g| g.size().y + 1.0);
    let mut y = rect.center().y - total / 2.0;
    let h = name.size().y;
    let mut x = rect.left() + name.size().x + PILL_GAP + 2.0;
    ui.painter().galley(egui::pos2(rect.left(), y), name, tokens.text_strong);
    for (galley, tag) in pills {
        let at = egui::Rect::from_min_size(egui::pos2(x, y + (h - 18.0) / 2.0), vec2(galley.size().x + PILL_PAD * 2.0, 18.0));
        draw_pill(ui, at, galley, &tag, row.host.id);
        x = at.right() + PILL_GAP;
    }
    y += h + 1.0;
    if let Some((galley, text)) = address {
        // Its own node too, so screen readers read the address.
        let at = egui::Rect::from_min_size(egui::pos2(rect.left(), y), galley.size());
        ui.painter().galley(at.min, galley, tokens.text_dim);
        let node = ui.interact(at, ui.scope_id().with(("address", row.host.id)), Sense::hover());
        node.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        y += at.height() + 1.0;
    }
    if let Some(galley) = warn {
        // Its own node, so screen readers (and tests) find the warning with its explanation.
        let at = egui::Rect::from_min_size(egui::pos2(rect.left(), y), galley.size());
        ui.painter().galley(at.min, galley, tokens.warning);
        let note = ui.interact(at, ui.scope_id().with(("sign-in", row.host.id)), Sense::hover());
        note.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tr("Add your own sign-in")));
        note.on_hover_text(tr(
            "This host was shared without its key or password. Edit it and choose a key or password of your own; it stays on your devices.",
        ));
    }
    let label = row.host.name.clone();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    if cut { response.on_hover_text(crate::widgets::visible(&row.host.name)) } else { response }
}

const PILL_PAD: f32 = 6.0;
const PILL_GAP: f32 = 4.0;

/// The host's tags laid out as pills, as many as fit in `max_width`.
fn tag_pills(ui: &Ui, tags: &[String], max_width: f32) -> Vec<(std::sync::Arc<egui::Galley>, String)> {
    let tokens = Tokens::get(ui.ctx());
    let mut used = 0.0;
    let mut out = Vec::new();
    for tag in tags.iter().take(4) {
        let galley = one_line(ui, tag, 90.0, FontId::proportional(11.5), tokens.text_dim).0;
        let w = galley.size().x + PILL_PAD * 2.0 + PILL_GAP;
        if used + w > max_width {
            break;
        }
        used += w;
        out.push((galley, tag.clone()));
    }
    out
}

/// One tag pill at `at`, named by the tag for screen readers.
fn draw_pill(ui: &mut Ui, at: Rect, galley: std::sync::Arc<egui::Galley>, tag: &str, host: sshcraft_model::model::HostId) {
    let tokens = Tokens::get(ui.ctx());
    ui.painter().rect(at, 9.0, tokens.button, Stroke::new(1.0, tokens.separator), StrokeKind::Inside);
    ui.painter().galley(egui::pos2(at.left() + PILL_PAD, at.center().y - galley.size().y / 2.0), galley, tokens.text_dim);
    let node = ui.interact(at, ui.scope_id().with(("tag", host, tag)), Sense::hover());
    let tag = crate::widgets::visible(tag);
    node.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &tag));
}

/// The short label of the host's system for its tile: three letters, or SSH when unknown.
fn os_label(row: &HostRow) -> String {
    row.host
        .os
        .as_ref()
        .filter(|os| !matches!(os, sshcraft_model::model::HostOs::Other))
        .map(|os| format!("{os:?}").chars().take(3).collect::<String>().to_uppercase())
        .unwrap_or_else(|| "SSH".into())
}

fn os_tile(ui: &mut Ui, row: &HostRow, side: f32) {
    let tokens = Tokens::get(ui.ctx());
    let (tile, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    ui.painter().rect_filled(tile, (side * 0.28).round(), tokens.button);
    ui.painter().text(tile.center(), egui::Align2::CENTER_CENTER, os_label(row), FontId::monospace(12.0), tokens.text_strong);
}

/// One host as a card: a tile with the system, name and address, its state, and the SSH and
/// SFTP buttons side by side. The caller sets the width; `compact` is the narrow card of a phone
/// (tile and state on top, the name below, the buttons without symbols).
pub fn host_card(app: &mut SshcraftApp, ui: &mut Ui, row: &HostRow, compact: bool) {
    let tokens = Tokens::get(ui.ctx());
    let state = reach(app, &tokens, row);
    let address = shown_address(app, row);
    let margin = if compact { 14.0 } else { 16.0 };
    egui::Frame::new().fill(tokens.panel).stroke(Stroke::new(1.0, tokens.separator)).corner_radius(16).inner_margin(margin).show(ui, |ui| {
        let inner = ui.available_width();
        ui.set_width(inner);
        if compact {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                os_tile(ui, row, 36.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| dot(ui, state));
            });
            elided_label(ui, &row.host.name, inner, tokens.text_strong, FontId::proportional(15.0));
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                os_tile(ui, row, 40.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    dot(ui, state);
                    let width = (ui.available_width() - 8.0).max(24.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        elided_label(ui, &row.host.name, width, tokens.text_strong, FontId::proportional(15.0));
                        if let Some(address) = &address {
                            elided_label(ui, address, width, tokens.text_dim, FontId::monospace(12.5));
                        }
                    });
                });
            });
            let pills = tag_pills(ui, &row.host.tags, inner);
            if !pills.is_empty() {
                ui.add_space(10.0);
                let (line, _) = ui.allocate_exact_size(vec2(inner, 18.0), Sense::hover());
                let mut x = line.left();
                for (galley, tag) in pills {
                    let at = egui::Rect::from_min_size(egui::pos2(x, line.top()), vec2(galley.size().x + PILL_PAD * 2.0, 18.0));
                    draw_pill(ui, at, galley, &tag, row.host.id);
                    x = at.right() + PILL_GAP;
                }
            }
            ui.add_space(14.0);
        }
        ui.horizontal(|ui| {
            let gap = if compact { 6.0 } else { 8.0 };
            ui.spacing_mut().item_spacing.x = gap;
            let height = if compact {
                34.0
            } else if touch(app) {
                40.0
            } else {
                36.0
            };
            let half = vec2(((inner - gap) / 2.0).max(40.0), height);
            let (ssh, sftp) = if compact { (None, None) } else { (Some(Icon::Prompt), Some(Icon::Folder)) };
            if wide_button(ui, ssh, tr("SSH"), true, true, half) {
                app.ui.connect_request = Some(ConnectRequest::Host(row.host.id));
            }
            wide_button(ui, sftp, tr("SFTP"), false, false, half);
        });
    });
}
