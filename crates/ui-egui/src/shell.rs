//! The window around the pages: the sidebar on the left, the tab bar above the main area, and
//! the page or tab shown.

use egui::{FontId, Sense, Ui, vec2};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::state::Page;
use crate::theme::Tokens;
use crate::widgets::elided_label;
use crate::widgets::icons::{Icon, draw};

/// The widest a tab may be; its title is cut to fit.
pub const MAX_TAB_WIDTH: f32 = 200.0;
const SIDEBAR_WIDTH: f32 = 188.0;

fn sidebar_entry(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, tokens.radius, if selected { tokens.row_selected } else { tokens.row_hover });
    }
    let color = if selected { tokens.text_strong } else { tokens.text };
    draw(ui.painter(), icon, egui::Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(16.0, 16.0)), color);
    ui.painter().text(rect.left_center() + vec2(36.0, 0.0), egui::Align2::LEFT_CENTER, label, FontId::proportional(14.0), color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response.clicked()
}

fn sidebar(app: &mut SshcraftApp, ui: &mut Ui) {
    ui.add_space(10.0);
    for (page, icon, label) in [
        (Page::Hosts, Icon::Host, tr("Hosts")),
        (Page::Keys, Icon::Key, tr("Keys")),
        (Page::Snippets, Icon::Snippet, tr("Snippets")),
        (Page::Forwarding, Icon::Forward, tr("Port forwarding")),
        (Page::Logs, Icon::Log, tr("Logs")),
        (Page::Settings, Icon::Settings, tr("Settings")),
    ] {
        if sidebar_entry(ui, icon, label, app.ui.page == page) {
            app.ui.page = page;
        }
    }
}

/// The tab bar: one entry per tab with its title cut to [`MAX_TAB_WIDTH`], a close button, and
/// a button for a new tab.
fn tab_bar(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    let mut select = None;
    let mut close = None;
    let mut merge = None;
    let mut save_tab = None;
    let mut rects = Vec::new();
    let mut dropped = None;
    // Last frame's places of the tabs: the drag areas lie under the tabs' own buttons.
    let places_id = egui::Id::unique("sshcraft-tab-places");
    let last: Vec<egui::Rect> = ui.ctx().data(|d| d.get_temp(places_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        for (i, place) in last.iter().enumerate().take(app.ui.tabs.len()) {
            let handle = ui.interact(*place, ui.scope_id().with(("sshcraft-tab", i)), Sense::drag());
            handle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, tr("Tab")));
            if handle.drag_stopped() {
                dropped = Some(i);
            }
        }
        for (i, tab) in app.ui.tabs.iter().enumerate() {
            let selected = app.ui.page == Page::Tab(i);
            // The look of the tab's first host, worked out once per tab and frame.
            let look = crate::session_tab::host_appearance(app, tab.panes.sessions().next().and_then(|s| app.terminals.get(&s)).and_then(|t| t.host));
            let frame =
                egui::Frame::new().fill(if selected { tokens.panel } else { tokens.bg }).corner_radius(tokens.radius).inner_margin(vec2(8.0, 4.0));
            let shown = frame.show(ui, |ui| {
                ui.set_max_width(MAX_TAB_WIDTH);
                let icon = if tab.panes.sessions().next().is_some() { Icon::Terminal } else { Icon::Plus };
                let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                draw(ui.painter(), icon, icon_rect, tokens.text_dim);
                if let Some(dot) = crate::status::Dot::most_urgent(tab.panes.panes.iter().filter_map(|s| crate::session_tab::slot_dot(app, *s))) {
                    let (dot_rect, dot_response) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
                    dot.draw(ui.painter(), dot_rect.center(), &tokens);
                    let program = tab.panes.sessions().filter_map(|s| app.terminals.get(&s)).find_map(crate::session_tab::SessionTab::program_text);
                    if let Some(text) = program {
                        dot_response.on_hover_text(text);
                    }
                }
                if tab.panes.broadcast {
                    let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    draw(ui.painter(), Icon::Broadcast, rect, tokens.danger);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, tr("Broadcast on")));
                }
                if let Some(badge) = &look.badge {
                    let [r, g, b] = look.color.unwrap_or([0x55, 0x5a, 0x66]);
                    egui::Frame::new().fill(egui::Color32::from_rgb(r, g, b)).corner_radius(3.0).inner_margin(vec2(4.0, 0.0)).show(ui, |ui| {
                        ui.label(egui::RichText::new(badge).size(11.0).color(egui::Color32::WHITE));
                    });
                }
                let color = if selected { tokens.text_strong } else { tokens.text };
                if elided_label(ui, &tab.title, MAX_TAB_WIDTH - 50.0, color, FontId::proportional(13.0)).clicked() {
                    select = Some(i);
                }
                let (close_rect, close_response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                draw(ui.painter(), Icon::Close, close_rect, if close_response.hovered() { tokens.text_strong } else { tokens.text_dim });
                close_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("Close tab")));
                // A middle click closes the tab under it once, on release.
                let middle = ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Middle)) && ui.rect_contains_pointer(ui.min_rect());
                if close_response.clicked() || middle {
                    close = Some(i);
                }
            });
            // The host's colour as a line under its tab.
            if let Some([r, g, b]) = look.color {
                let rect = shown.response.rect;
                ui.painter().line_segment(
                    [rect.left_bottom() + vec2(4.0, -1.0), rect.right_bottom() + vec2(-4.0, -1.0)],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(r, g, b)),
                );
            }
            // Dragging a tab onto another merges its panes into that one.
            rects.push(shown.response.rect);
        }
        // The shown tab can be kept as a workspace template.
        if let Page::Tab(i) = app.ui.page
            && app.ui.tabs.get(i).is_some_and(|t| t.panes.sessions().next().is_some())
            && crate::widgets::icons::icon_button(ui, Icon::Save, tr("Save as workspace"))
        {
            save_tab = Some(i);
        }
        let (plus, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
        draw(ui.painter(), Icon::Plus, plus.shrink(5.0), tokens.text_dim);
        if response.on_hover_text(tr("New tab")).clicked() {
            select = None;
            app.open_new_tab();
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(places_id, rects.clone()));
    if let Some(from) = dropped
        && let Some(at) = ui.ctx().pointer_interact_pos()
        && let Some(to) = rects.iter().position(|r| r.contains(at))
        && to != from
    {
        merge = Some((from, to));
    }
    if let Some(i) = select {
        app.ui.page = Page::Tab(i);
    }
    if let Some(i) = save_tab {
        crate::pages::templates::open_dialog(app, i);
    }
    if let Some((from, to)) = merge {
        app.merge_tabs(from, to);
    }
    if let Some(i) = close {
        app.close_tab_at(i);
    }
}

/// The new-tab page: hosts to connect to, the local terminal and the serial port.
pub fn new_tab_page(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    ui.add_space(16.0);
    ui.heading(tr("New tab"));
    ui.add_space(8.0);
    // A tablet has neither a local shell nor serial ports to open.
    if app.form == crate::app::Form::Desktop {
        ui.horizontal(|ui| {
            if ui.button(tr("Local terminal")).clicked() {
                app.ui.connect_request = Some(crate::state::ConnectRequest::Local);
            }
            if ui.button(tr("Serial port")).clicked() {
                app.ui.connect_request = Some(crate::state::ConnectRequest::Serial);
            }
        });
        ui.add_space(8.0);
    }
    ui.label(tr("Quick connect"));
    let mut text = std::mem::take(&mut app.ui.quick_text);
    if let Some(problem) = crate::quick_connect::field(app, ui, &mut text) {
        app.toasts.error(&problem);
    }
    app.ui.quick_text = text;
    crate::pages::templates::list(app, ui);
    ui.add_space(8.0);
    ui.add(egui::TextEdit::singleline(&mut app.ui.new_tab_query).hint_text(tr("Search hosts")).desired_width(320.0));
    ui.add_space(8.0);
    let query = app.ui.new_tab_query.to_lowercase();
    let mut hosts: Vec<_> = app
        .data
        .hosts
        .iter()
        .filter(|row| {
            query.is_empty()
                || row.host.name.to_lowercase().contains(&query)
                || row.host.settings.address.as_deref().is_some_and(|a| a.contains(&query))
        })
        .collect();
    hosts.sort_by_key(|row| row.host.name.to_lowercase());
    let field_scroll = app.field_paste.scroll;
    egui::ScrollArea::vertical().show(ui, |ui| {
        crate::field_paste::scroll_into_view(ui, field_scroll);
        for row in hosts {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                draw(ui.painter(), Icon::Host, icon, tokens.text_dim);
                if elided_label(ui, &row.host.name, 360.0, tokens.text, FontId::proportional(14.0)).clicked() {
                    app.ui.connect_request = Some(crate::state::ConnectRequest::Host(row.host.id));
                }
                if let Some(address) = &row.host.settings.address {
                    elided_label(ui, address, 240.0, tokens.text_dim, FontId::proportional(13.0));
                }
            });
        }
    });
}

/// Draws the sidebar, the tab bar and the page.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    // The menus and the title bar span the window, above the sidebar.
    crate::window::strip(app, ui);
    egui::Panel::left("sshcraft-sidebar")
        .exact_size(SIDEBAR_WIDTH)
        .frame(egui::Frame::new().fill(tokens.sidebar).inner_margin(vec2(8.0, 0.0)))
        .show(ui, |ui| sidebar(app, ui));
    if !app.ui.tabs.is_empty() {
        egui::Panel::top("sshcraft-tabs").frame(egui::Frame::new().fill(tokens.bg).inner_margin(vec2(8.0, 6.0))).show(ui, |ui| tab_bar(app, ui));
    }
    egui::CentralPanel::default().frame(egui::Frame::new().fill(tokens.bg).inner_margin(12.0)).show(ui, |ui| match app.ui.page {
        Page::Hosts => crate::pages::hosts::show(app, ui),
        Page::Keys => crate::pages::keys::show(app, ui),
        Page::Snippets => crate::pages::snippets::show(app, ui),
        Page::Forwarding => crate::pages::forwarding::show(app, ui),
        Page::Logs => crate::pages::logs::show(app, ui),
        Page::Settings => crate::pages::settings::show(app, ui),
        Page::Tab(i) if i < app.ui.tabs.len() => crate::panes::show(app, ui, i),
        Page::Tab(_) => new_tab_page(app, ui),
    });
}
