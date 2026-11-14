//! Icons drawn in code: no image files, nothing taken from another product.

/// The app icon: a prompt (`>_`) in SSHCraft's green on a dark rounded square, 64 by 64 pixels.
pub fn app_icon() -> egui::IconData {
    const SIZE: usize = 64;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let bg = [0x0f, 0x11, 0x15, 0xff];
    let fg = [0x00, 0xe6, 0x76, 0xff];
    let radius = 12.0_f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance outside the rounded square's inner rectangle.
            let dx = (radius - fx).max(fx - (SIZE as f32 - radius)).max(0.0);
            let dy = (radius - fy).max(fy - (SIZE as f32 - radius)).max(0.0);
            let inside = dx * dx + dy * dy <= radius * radius;
            // `>`: two strokes meeting at (30, 32); `_`: a bar from x 34 to 50 at y 44 to 48.
            let chevron = |x0: f32, y0: f32, x1: f32, y1: f32| {
                let (vx, vy) = (x1 - x0, y1 - y0);
                let t = (((fx - x0) * vx + (fy - y0) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
                let (px, py) = (x0 + t * vx, y0 + t * vy);
                (fx - px).powi(2) + (fy - py).powi(2) <= 3.2_f32.powi(2)
            };
            let mark =
                chevron(16.0, 20.0, 30.0, 32.0) || chevron(30.0, 32.0, 16.0, 44.0) || ((34.0..=50.0).contains(&fx) && (42.0..=47.0).contains(&fy));
            let color = if !inside {
                [0, 0, 0, 0]
            } else if mark {
                fg
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
    }
}
