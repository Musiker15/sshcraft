//! Icons drawn in code: no image files, nothing taken from another product.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};

/// The icons of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Host,
    Group,
    Key,
    Identity,
    Settings,
    Terminal,
    Serial,
    Plus,
    Search,
    Close,
    Warning,
    Snippet,
    Forward,
    Log,
    Broadcast,
    Record,
    Maximize,
    Detach,
    Save,
    /// A prompt without a frame: connect over SSH.
    Prompt,
    /// A folder: files (SFTP).
    Folder,
    /// Three dots: more actions.
    More,
    /// A page with a folded corner: a file.
    File,
    /// An arrow up: the folder above.
    Up,
    /// A circle with an arrow: read again.
    Refresh,
    /// An arrow up out of a tray: send to the server.
    Upload,
    /// A bin: delete.
    Trash,
}

/// Draws `icon` into `rect` (square works best) with lines of `color`.
pub fn draw(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let p = |x: f32, y: f32| -> Pos2 { pos2(c.x + x * s / 2.0, c.y + y * s / 2.0) };
    let stroke = Stroke::new((s / 12.0).max(1.2), color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    match icon {
        Icon::Host => {
            painter.rect_stroke(Rect::from_min_max(p(-0.8, -0.7), p(0.8, 0.35)), 2.0, stroke, egui::StrokeKind::Inside);
            line(p(-0.35, 0.75), p(0.35, 0.75));
            line(p(0.0, 0.35), p(0.0, 0.75));
        }
        Icon::Group => {
            let points = vec![p(-0.85, -0.55), p(-0.3, -0.55), p(-0.15, -0.35), p(0.85, -0.35), p(0.85, 0.7), p(-0.85, 0.7)];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        Icon::Key => {
            painter.circle_stroke(p(-0.45, 0.0), s * 0.2, stroke);
            line(p(-0.2, 0.0), p(0.85, 0.0));
            line(p(0.55, 0.0), p(0.55, 0.35));
            line(p(0.8, 0.0), p(0.8, 0.3));
        }
        Icon::Identity => {
            painter.circle_stroke(p(0.0, -0.35), s * 0.18, stroke);
            painter.add(egui::Shape::line(vec![p(-0.65, 0.8), p(-0.5, 0.25), p(0.5, 0.25), p(0.65, 0.8)], stroke));
        }
        Icon::Settings => {
            painter.circle_stroke(c, s * 0.18, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (x, y) = (a.cos(), a.sin());
                line(p(x * 0.5, y * 0.5), p(x * 0.8, y * 0.8));
            }
        }
        Icon::Terminal => {
            painter.rect_stroke(Rect::from_min_max(p(-0.85, -0.7), p(0.85, 0.7)), 2.0, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::line(vec![p(-0.5, -0.25), p(-0.2, 0.0), p(-0.5, 0.25)], stroke));
            line(p(0.0, 0.3), p(0.45, 0.3));
        }
        Icon::Serial => {
            painter.rect_stroke(Rect::from_min_max(p(-0.7, -0.35), p(0.7, 0.35)), 2.0, stroke, egui::StrokeKind::Inside);
            for x in [-0.35, 0.0, 0.35] {
                painter.circle_filled(p(x, 0.0), s * 0.05, color);
            }
        }
        Icon::Prompt => {
            painter.add(egui::Shape::line(vec![p(-0.7, -0.45), p(-0.2, 0.0), p(-0.7, 0.45)], stroke));
            line(p(0.0, 0.5), p(0.7, 0.5));
        }
        Icon::Folder => {
            let points = vec![p(-0.85, -0.6), p(-0.25, -0.6), p(-0.05, -0.4), p(0.85, -0.4), p(0.85, 0.65), p(-0.85, 0.65)];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        Icon::More => {
            for x in [-0.55, 0.0, 0.55] {
                painter.circle_filled(p(x, 0.0), s * 0.07, color);
            }
        }
        Icon::Plus => {
            line(p(-0.6, 0.0), p(0.6, 0.0));
            line(p(0.0, -0.6), p(0.0, 0.6));
        }
        Icon::Search => {
            painter.circle_stroke(p(-0.15, -0.15), s * 0.25, stroke);
            line(p(0.2, 0.2), p(0.7, 0.7));
        }
        Icon::Close => {
            line(p(-0.5, -0.5), p(0.5, 0.5));
            line(p(-0.5, 0.5), p(0.5, -0.5));
        }
        Icon::Warning => {
            painter.add(egui::Shape::closed_line(vec![p(0.0, -0.8), p(0.85, 0.7), p(-0.85, 0.7)], stroke));
            line(p(0.0, -0.3), p(0.0, 0.2));
            painter.circle_filled(p(0.0, 0.45), s * 0.05, color);
        }
        Icon::Snippet => {
            // A page with a prompt on it.
            painter.rect_stroke(Rect::from_min_max(p(-0.7, -0.8), p(0.7, 0.8)), 2.0, stroke, egui::StrokeKind::Inside);
            line(p(-0.4, -0.35), p(-0.1, -0.1));
            line(p(-0.1, -0.1), p(-0.4, 0.15));
            line(p(0.05, 0.3), p(0.4, 0.3));
        }
        Icon::Forward => {
            // Two arrows, one each way.
            line(p(-0.8, -0.35), p(0.8, -0.35));
            line(p(0.5, -0.6), p(0.8, -0.35));
            line(p(0.5, -0.1), p(0.8, -0.35));
            line(p(0.8, 0.35), p(-0.8, 0.35));
            line(p(-0.5, 0.1), p(-0.8, 0.35));
            line(p(-0.5, 0.6), p(-0.8, 0.35));
        }
        Icon::Log => {
            // Lines of text with a fold.
            for y in [-0.45, -0.1, 0.25, 0.6] {
                line(p(-0.7, y), p(0.7, y));
            }
        }
        Icon::Broadcast => {
            // A mast with waves.
            line(p(0.0, -0.1), p(0.0, 0.85));
            painter.circle_filled(p(0.0, -0.2), s * 0.08, color);
            for r in [0.35, 0.65] {
                let arc = |sign: f32| {
                    (0..=8)
                        .map(|i| {
                            let a = std::f32::consts::FRAC_PI_4 * (i as f32 / 8.0 * 2.0 - 1.0);
                            p(sign * r * a.cos(), -0.2 + r * a.sin())
                        })
                        .collect::<Vec<_>>()
                };
                painter.add(egui::Shape::line(arc(1.0), stroke));
                painter.add(egui::Shape::line(arc(-1.0), stroke));
            }
        }
        Icon::Record => {
            painter.circle_filled(c, s * 0.3, color);
        }
        Icon::Maximize => {
            painter.rect_stroke(Rect::from_min_max(p(-0.7, -0.7), p(0.7, 0.7)), 1.0, stroke, egui::StrokeKind::Inside);
            line(p(-0.7, -0.4), p(0.7, -0.4));
        }
        Icon::Save => {
            // An arrow down into a tray.
            line(p(0.0, -0.8), p(0.0, 0.3));
            line(p(-0.35, -0.05), p(0.0, 0.3));
            line(p(0.35, -0.05), p(0.0, 0.3));
            line(p(-0.8, 0.2), p(-0.8, 0.75));
            line(p(-0.8, 0.75), p(0.8, 0.75));
            line(p(0.8, 0.75), p(0.8, 0.2));
        }
        Icon::File => {
            let points = vec![p(-0.6, -0.8), p(0.25, -0.8), p(0.6, -0.45), p(0.6, 0.8), p(-0.6, 0.8)];
            painter.add(egui::Shape::closed_line(points, stroke));
            painter.add(egui::Shape::line(vec![p(0.25, -0.8), p(0.25, -0.45), p(0.6, -0.45)], stroke));
        }
        Icon::Up => {
            line(p(0.0, 0.75), p(0.0, -0.7));
            line(p(-0.45, -0.25), p(0.0, -0.7));
            line(p(0.45, -0.25), p(0.0, -0.7));
        }
        Icon::Refresh => {
            let arc: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 20.0 * std::f32::consts::PI * 1.6;
                    p(0.65 * a.cos(), 0.65 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, stroke));
            line(p(0.0, -0.65), p(-0.35, -0.95));
            line(p(0.0, -0.65), p(-0.3, -0.35));
        }
        Icon::Upload => {
            line(p(0.0, 0.3), p(0.0, -0.8));
            line(p(-0.35, -0.45), p(0.0, -0.8));
            line(p(0.35, -0.45), p(0.0, -0.8));
            line(p(-0.8, 0.2), p(-0.8, 0.75));
            line(p(-0.8, 0.75), p(0.8, 0.75));
            line(p(0.8, 0.75), p(0.8, 0.2));
        }
        Icon::Trash => {
            line(p(-0.75, -0.55), p(0.75, -0.55));
            line(p(-0.25, -0.55), p(-0.2, -0.8));
            line(p(-0.2, -0.8), p(0.2, -0.8));
            line(p(0.2, -0.8), p(0.25, -0.55));
            painter.add(egui::Shape::closed_line(vec![p(-0.55, -0.55), p(0.55, -0.55), p(0.42, 0.8), p(-0.42, 0.8)], stroke));
        }
        Icon::Detach => {
            painter.rect_stroke(Rect::from_min_max(p(-0.8, -0.3), p(0.3, 0.8)), 1.0, stroke, egui::StrokeKind::Inside);
            line(p(-0.1, 0.1), p(0.8, -0.8));
            line(p(0.3, -0.8), p(0.8, -0.8));
            line(p(0.8, -0.8), p(0.8, -0.3));
        }
    }
}

/// A small button drawn as `icon`, named `label` for screen readers and in its tooltip.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> bool {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, tokens.row_hover);
    }
    draw(ui.painter(), icon, rect.shrink(3.0), if response.hovered() { tokens.text_strong } else { tokens.text_dim });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label).clicked()
}

/// The app icon: the SSHCraft mark (logo V3) in white on a blue rounded square, 64 by 64
/// pixels, as in `assets/app-icon/sshcraft.svg`.
pub fn app_icon() -> egui::IconData {
    const SIZE: usize = 64;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let bg = [0x00, 0x71, 0xff, 0xff];
    let radius = 12.0_f32;
    // The mark on its 64-unit grid, drawn at 75 % with 8 pixels around it (as the SVG does).
    let at = |[x, y]: [f32; 2]| [8.0 + x * 0.75, 8.0 + y * 0.75];
    let top = [[45.0, 13.0], [25.0, 13.0], [19.0, 19.0], [19.0, 26.0], [25.0, 32.0], [32.0, 32.0]].map(at);
    let bottom = [[32.0, 32.0], [39.0, 32.0], [45.0, 38.0], [45.0, 45.0], [39.0, 51.0], [22.0, 51.0]].map(at);
    let half = 3.0_f32;
    // Inside a stroke of width 6 along the line: square ends, joins filled by a disc.
    let on_line = |points: &[[f32; 2]], fx: f32, fy: f32| {
        points.windows(2).any(|w| {
            let ([x0, y0], [x1, y1]) = (w[0], w[1]);
            let (vx, vy) = (x1 - x0, y1 - y0);
            let len2 = vx * vx + vy * vy;
            let t = ((fx - x0) * vx + (fy - y0) * vy) / len2;
            let (px, py) = (x0 + t * vx, y0 + t * vy);
            (0.0..=1.0).contains(&t) && (fx - px).powi(2) + (fy - py).powi(2) <= half * half
        }) || points[1..points.len() - 1].iter().any(|[x, y]| (fx - x).powi(2) + (fy - y).powi(2) <= half * half)
    };
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance outside the rounded square's inner rectangle.
            let dx = (radius - fx).max(fx - (SIZE as f32 - radius)).max(0.0);
            let dy = (radius - fy).max(fy - (SIZE as f32 - radius)).max(0.0);
            let inside = dx * dx + dy * dy <= radius * radius;
            let cursor = (16.25..=22.25).contains(&fx) && (43.25..=49.25).contains(&fy);
            let color = if !inside {
                [0, 0, 0, 0]
            } else if cursor || on_line(&top, fx, fy) {
                [0xff, 0xff, 0xff, 0xff]
            } else if on_line(&bottom, fx, fy) {
                // White at 55 % over the blue.
                [0x8c, 0xbe, 0xff, 0xff]
            } else {
                bg
            };
            let i = (y * SIZE + x) * 4;
            if let Some(px) = rgba.get_mut(i..i + 4) {
                px.copy_from_slice(&color);
            }
        }
    }
    egui::IconData { rgba, width: SIZE as u32, height: SIZE as u32 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_app_icon_is_square_and_has_transparent_corners() {
        let icon = super::app_icon();
        assert_eq!((icon.width, icon.height), (64, 64));
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        assert_eq!(icon.rgba[3], 0, "the corner is transparent");
        let px = |x: usize, y: usize| &icon.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(px(19, 46), [0xff, 0xff, 0xff, 0xff], "the cursor square is white");
        assert_eq!(px(32, 25), [0x00, 0x71, 0xff, 0xff], "inside the S the tile is blue");
        assert_eq!(px(30, 17), [0xff, 0xff, 0xff, 0xff], "the top bar is white");
    }
}
