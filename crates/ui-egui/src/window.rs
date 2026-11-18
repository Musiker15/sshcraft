//! The window's top strip: the menu bar and, where the app draws the frame, the title bar with
//! its window buttons. craft-window decides who draws the frame (the policy of the Craft apps):
//! the app by default, the desktop with "Use system title bar" (read when the app starts) and on
//! tiling window managers.

use craft_window::Button;
use craft_window::policy::{CaptionFamily, Captions, Frame, TitleBarPref};
use egui::{Color32, Rect, Sense, Stroke, Ui, vec2};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// The strip's height: one row of menu titles.
const STRIP_HEIGHT: f32 = 30.0;
/// A window button's width on Windows; the round buttons of the Linux desktops are square.
const WINDOWS_BUTTON_WIDTH: f32 = 46.0;
const ROUND_BUTTON: f32 = 24.0;
/// The menu titles take at most this share of the row; the rest moves the window.
const MENU_SHARE: f32 = 0.7;

/// The setting "Use system title bar" as craft-window's preference.
pub fn title_bar_pref(settings: &sshcraft_engine::settings::EngineSettings) -> TitleBarPref {
    if settings.general.system_title_bar { TitleBarPref::System } else { TitleBarPref::Integrated }
}

/// Whether the window needs the strip: menus in the window, or a frame the app draws.
fn strip_needed(app: &SshcraftApp) -> bool {
    // A touch screen has no menu bar: "Commands" in the bar or sidebar opens the palette, which
    // holds the same commands.
    if app.form == crate::app::Form::Tablet {
        return false;
    }
    !crate::menus::native(app) || !matches!(app.window_frame, Frame::Native)
}

/// The strip across the top of the window; call it first in the frame, before the other panels.
pub fn strip(app: &mut SshcraftApp, ui: &mut Ui) {
    if !strip_needed(app) {
        return;
    }
    let tokens = Tokens::get(ui.ctx());
    egui::Panel::top("sshcraft-title-bar").exact_size(STRIP_HEIGHT).frame(egui::Frame::new().fill(tokens.sidebar)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Under the controls: the strip's empty part moves the window and maximizes it on a
        // double click, the way the system's title bar does.
        if !matches!(app.window_frame, Frame::Native) {
            let _ = craft_window::title_bar::empty_area(ui, rect);
        }
        let frame = app.window_frame.clone();
        let (left, right) = match &frame {
            Frame::Integrated(captions) => (captions.layout.left.clone(), captions.layout.right.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if let Frame::Inset(inset) = &frame {
                ui.add_space(inset.leading);
            }
            if let Frame::Integrated(captions) = &frame {
                // Outermost last in the layout, outermost first from the edge here.
                let mut order = left.clone();
                order.reverse();
                buttons(ui, captions, &order);
            }
            ui.add_space(4.0);
            if !crate::menus::native(app) {
                let width = (ui.available_width() * MENU_SHARE).max(80.0);
                crate::menus::bar_ui(app, ui, width);
            }
            if let Frame::Integrated(captions) = &frame {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut order = right.clone();
                    order.reverse();
                    buttons(ui, captions, &order);
                });
            }
        });
    });
    #[cfg(target_os = "macos")]
    if matches!(app.window_frame, Frame::Inset(_)) {
        craft_window::title_bar::sync_movable(ui.ctx(), &mut app.movable);
    }
}

fn buttons(ui: &mut Ui, captions: &Captions, which: &[Button]) {
    for button in which {
        caption_button(ui, captions.family, *button);
    }
}

/// One window button: minimize, maximize (restore when maximized) or close.
fn caption_button(ui: &mut Ui, family: CaptionFamily, button: Button) {
    let tokens = Tokens::get(ui.ctx());
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    let label = match button {
        Button::Minimize => tr("Minimize"),
        Button::Maximize if maximized => tr("Restore"),
        Button::Maximize => tr("Maximize"),
        Button::Close => tr("Close window"),
    };
    let round = family != CaptionFamily::Windows;
    let size = if round { vec2(ROUND_BUTTON + 8.0, STRIP_HEIGHT) } else { vec2(WINDOWS_BUTTON_WIDTH, STRIP_HEIGHT) };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let hovered = response.hovered();
    let painter = ui.painter_at(rect);
    let close = button == Button::Close;
    let fill = match (hovered, close) {
        (true, true) if !round => Color32::from_rgb(0xc4, 0x2b, 0x1c),
        (true, _) => tokens.row_hover,
        (false, _) if round => tokens.button,
        _ => Color32::TRANSPARENT,
    };
    let glyph = if hovered && close && !round { Color32::WHITE } else { tokens.text };
    if round {
        painter.circle_filled(rect.center(), ROUND_BUTTON / 2.0, fill);
    } else {
        painter.rect_filled(rect, 0.0, fill);
    }
    draw_glyph(&painter, rect, button, maximized, Stroke::new(1.0, glyph));
    if response.clicked() {
        let command = match button {
            Button::Minimize => egui::ViewportCommand::Minimized(true),
            Button::Maximize => egui::ViewportCommand::Maximized(!maximized),
            Button::Close => egui::ViewportCommand::Close,
        };
        ui.ctx().send_viewport_cmd(command);
    }
}

/// The button's sign, ten points across, in the middle of `rect`.
fn draw_glyph(painter: &egui::Painter, rect: Rect, button: Button, maximized: bool, stroke: Stroke) {
    let c = rect.center();
    let half = 5.0;
    match button {
        Button::Minimize => {
            painter.line_segment([c + vec2(-half, 0.0), c + vec2(half, 0.0)], stroke);
        }
        Button::Maximize if maximized => {
            let back = Rect::from_min_max(c + vec2(-half + 2.0, -half), c + vec2(half, half - 2.0));
            let front = Rect::from_min_max(c + vec2(-half, -half + 2.0), c + vec2(half - 2.0, half));
            painter.rect_stroke(back, 0.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(front, 0.0, Color32::TRANSPARENT);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
        }
        Button::Maximize => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(2.0 * half, 2.0 * half)), 0.0, stroke, egui::StrokeKind::Inside);
        }
        Button::Close => {
            painter.line_segment([c + vec2(-half, -half), c + vec2(half, half)], stroke);
            painter.line_segment([c + vec2(-half, half), c + vec2(half, -half)], stroke);
        }
    }
}

/// How far the resize bands reach into the window, and the corners.
const EDGE: f32 = 5.0;
const CORNER: f32 = 10.0;

/// Where the app draws the frame, the window has no edges of the system's: bands along them,
/// over everything else, start a resize as the system's edges would. None while maximized.
pub fn resize_edges(app: &SshcraftApp, ctx: &egui::Context) {
    if !matches!(app.window_frame, Frame::Integrated(_)) || ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let full = ctx.content_rect();
    let id = egui::Id::unique("sshcraft-resize-edges");
    egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(full.min).interactable(true).show(ctx, |ui| {
        for (i, (direction, band)) in bands(full).into_iter().enumerate() {
            let response = ui.interact(band, id.with(i), Sense::click_and_drag()).on_hover_cursor(cursor(direction));
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    });
}

/// The bands of `full`, corners last: egui gives a press to the widget added last, so the corners
/// win where they overlap the edges.
fn bands(full: Rect) -> [(egui::ResizeDirection, Rect); 8] {
    use egui::ResizeDirection::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
    let c = vec2(CORNER, CORNER);
    [
        (North, full.with_max_y(full.top() + EDGE)),
        (South, full.with_min_y(full.bottom() - EDGE)),
        (West, full.with_max_x(full.left() + EDGE)),
        (East, full.with_min_x(full.right() - EDGE)),
        (NorthWest, Rect::from_min_size(full.left_top(), c)),
        (NorthEast, Rect::from_min_size(full.right_top() - vec2(CORNER, 0.0), c)),
        (SouthWest, Rect::from_min_size(full.left_bottom() - vec2(0.0, CORNER), c)),
        (SouthEast, Rect::from_min_size(full.right_bottom() - c, c)),
    ]
}

fn cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::ResizeDirection as D;
    match direction {
        D::North => egui::CursorIcon::ResizeNorth,
        D::South => egui::CursorIcon::ResizeSouth,
        D::East => egui::CursorIcon::ResizeEast,
        D::West => egui::CursorIcon::ResizeWest,
        D::NorthEast => egui::CursorIcon::ResizeNorthEast,
        D::NorthWest => egui::CursorIcon::ResizeNorthWest,
        D::SouthEast => egui::CursorIcon::ResizeSouthEast,
        D::SouthWest => egui::CursorIcon::ResizeSouthWest,
    }
}
