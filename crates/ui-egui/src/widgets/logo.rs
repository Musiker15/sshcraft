//! The SSHCraft mark (logo V3): a faceted S, its upper half in full colour, its lower half at
//! 55 %, with a square cursor at the end of the bottom bar. Drawn on a 64-unit grid; the app icons
//! come from the same drawing in `assets/app-icon/sshcraft.svg`.

use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, vec2};

/// The upper half of the S, on the 64-unit grid.
const TOP: [[f32; 2]; 6] = [[45.0, 13.0], [25.0, 13.0], [19.0, 19.0], [19.0, 26.0], [25.0, 32.0], [32.0, 32.0]];
/// The lower half, drawn at 55 %.
const BOTTOM: [[f32; 2]; 6] = [[32.0, 32.0], [39.0, 32.0], [45.0, 38.0], [45.0, 45.0], [39.0, 51.0], [22.0, 51.0]];
/// The cursor square at the end of the bottom bar: corner and side.
const CURSOR: ([f32; 2], f32) = ([11.0, 47.0], 8.0);
/// The width of both strokes.
const WIDTH: f32 = 8.0;

/// The shapes of the mark filling `rect` (square), in `color`.
pub fn logo_shapes(rect: Rect, color: Color32) -> Vec<Shape> {
    let scale = rect.width() / 64.0;
    let at = |[x, y]: [f32; 2]| -> Pos2 { rect.min + vec2(x, y) * scale };
    let line = |points: &[[f32; 2]], color: Color32| Shape::line(points.iter().copied().map(at).collect(), Stroke::new(WIDTH * scale, color));
    let (corner, side) = CURSOR;
    vec![
        line(&TOP, color),
        line(&BOTTOM, color.gamma_multiply(0.55)),
        Shape::rect_filled(Rect::from_min_size(at(corner), vec2(side, side) * scale), 0.0, color),
    ]
}

/// The app's tile: the white mark on an accent square of `size` points.
pub fn logo_tile(ui: &mut Ui, size: f32) -> egui::Response {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, size * 0.22, tokens.accent);
    let mark = Rect::from_center_size(rect.center(), vec2(size, size) * 0.72);
    ui.painter().extend(logo_shapes(mark, Color32::WHITE));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn the_cursor_sits_at_the_end_of_the_bottom_bar() {
        let shapes = logo_shapes(Rect::from_min_size(pos2(0.0, 0.0), vec2(64.0, 64.0)), Color32::WHITE);
        assert_eq!(shapes.len(), 3);
        let Shape::Rect(cursor) = &shapes[2] else { panic!("the third shape is the cursor square") };
        assert_eq!((cursor.rect.min, cursor.rect.size()), (pos2(11.0, 47.0), vec2(8.0, 8.0)));
        let shapes = logo_shapes(Rect::from_min_size(pos2(10.0, 10.0), vec2(128.0, 128.0)), Color32::WHITE);
        let Shape::Rect(cursor) = &shapes[2] else { panic!("the third shape is the cursor square") };
        assert_eq!(cursor.rect.min, pos2(32.0, 104.0));
    }
}
