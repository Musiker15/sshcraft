//! The window around the pages: the sidebar on the left, the tab bar above the main area, and
//! the page or tab shown.

use egui::{FontId, Sense, Ui, vec2};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::state::{Page, TabKind};
use crate::theme::Tokens;
use crate::widgets::icons::{Icon, draw};
use crate::widgets::{elided_label, visible};

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
    for (page, icon, label) in
        [(Page::Hosts, Icon::Host, tr("Hosts")), (Page::Keys, Icon::Key, tr("Keys")), (Page::Settings, Icon::Settings, tr("Settings"))]
    {
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
    ui.horizontal(|ui| {
        for (i, tab) in app.ui.tabs.iter().enumerate() {
            let selected = app.ui.page == Page::Tab(i);
            let frame =
                egui::Frame::new().fill(if selected { tokens.panel } else { tokens.bg }).corner_radius(tokens.radius).inner_margin(vec2(8.0, 4.0));
            frame.show(ui, |ui| {
                ui.set_max_width(MAX_TAB_WIDTH);
                let icon = match tab.kind {
                    TabKind::New => Icon::Plus,
                    TabKind::Session { .. } => Icon::Terminal,
                };
                let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                draw(ui.painter(), icon, icon_rect, tokens.text_dim);
                let color = if selected { tokens.text_strong } else { tokens.text };
                if elided_label(ui, &tab.title, MAX_TAB_WIDTH - 50.0, color, FontId::proportional(13.0)).clicked() {
                    select = Some(i);
                }
                let (close_rect, close_response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                draw(ui.painter(), Icon::Close, close_rect, if close_response.hovered() { tokens.text_strong } else { tokens.text_dim });
                close_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("Close tab")));
                if close_response.clicked() || (ui.rect_contains_pointer(ui.min_rect()) && ui.input(|i| i.pointer.middle_down())) {
                    close = Some(i);
                }
            });
        }
        let (plus, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
        draw(ui.painter(), Icon::Plus, plus.shrink(5.0), tokens.text_dim);
        if response.on_hover_text(tr("New tab")).clicked() {
            select = None;
            app.open_new_tab();
        }
    });
    if let Some(i) = select {
        app.ui.page = Page::Tab(i);
    }
    if let Some(i) = close {
        app.ui.page = Page::Tab(i);
        app.close_tab();
    }
}

/// The new-tab page: hosts to connect to, the local terminal and the serial port.
fn new_tab_page(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    ui.add_space(16.0);
    ui.heading(tr("New tab"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(tr("Local terminal")).clicked() {
            app.ui.connect_request = Some(crate::state::ConnectRequest::Local);
        }
        if ui.button(tr("Serial port")).clicked() {
            app.ui.connect_request = Some(crate::state::ConnectRequest::Serial);
        }
    });
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
    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in hosts {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                draw(ui.painter(), Icon::Host, icon, tokens.text_dim);
                if elided_label(ui, &row.host.name, 360.0, tokens.text, FontId::proportional(14.0)).clicked() {
                    app.ui.connect_request = Some(crate::state::ConnectRequest::Host(row.host.id));
                }
                if let Some(address) = &row.host.settings.address {
                    ui.label(egui::RichText::new(visible(address)).color(tokens.text_dim));
                }
            });
        }
    });
}

/// Draws the sidebar, the tab bar and the page.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
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
        Page::Settings => {
            ui.heading(tr("Settings"));
        }
        Page::Tab(i) => match app.ui.tabs.get(i).map(|t| t.kind.clone()) {
            Some(TabKind::New) | None => new_tab_page(app, ui),
            Some(TabKind::Session { session }) => crate::session_tab::show(app, ui, session, i),
        },
    });
}
