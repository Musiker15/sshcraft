//! The colours of the terminal. The sixteen colours and both palettes are SSHCraft's own; the
//! 256-colour cube and grey ramp follow xterm's public formula.

use egui::Color32;
use sshcraft_term::{CellFlags, TermColor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermPalette {
    /// Black, red, green, yellow, blue, magenta, cyan, white, then the bright ones.
    pub ansi: [Color32; 16],
    pub fg: Color32,
    pub bg: Color32,
    pub cursor: Color32,
    pub selection_bg: Color32,
    pub selection_fg: Color32,
    pub link: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl TermPalette {
    pub fn dark() -> Self {
        Self {
            ansi: [
                hex(0x1c1f24),
                hex(0xe5534b),
                hex(0x4ac26b),
                hex(0xd9b44a),
                hex(0x4d8ef7),
                hex(0xc47fd5),
                hex(0x3fb9c6),
                hex(0xc9cdd3),
                hex(0x5c636e),
                hex(0xff7b72),
                hex(0x6fdd8b),
                hex(0xf2cc60),
                hex(0x79b8ff),
                hex(0xd2a8ff),
                hex(0x6ed7e0),
                hex(0xf5f7fa),
            ],
            fg: hex(0xe6e3dd),
            bg: hex(0x0f1115),
            cursor: hex(0x00e676),
            selection_bg: hex(0x2f4a6d),
            selection_fg: hex(0xf5f7fa),
            link: hex(0x79b8ff),
        }
    }

    pub fn light() -> Self {
        Self {
            ansi: [
                hex(0x24292f),
                hex(0xc4302b),
                hex(0x1f7a3a),
                hex(0x8a6400),
                hex(0x1f5fbf),
                hex(0x8a3fa0),
                hex(0x0f7480),
                hex(0x6e7781),
                hex(0x57606a),
                hex(0xa40e26),
                hex(0x116329),
                hex(0x6b4b00),
                hex(0x0a4aa6),
                hex(0x6e2b85),
                hex(0x0b5c66),
                hex(0x8c959f),
            ],
            fg: hex(0x1d2125),
            bg: hex(0xfbfaf8),
            cursor: hex(0x00a152),
            selection_bg: hex(0xb6d4fe),
            selection_fg: hex(0x1d2125),
            link: hex(0x0a4aa6),
        }
    }

    /// Index 0 to 255: the sixteen colours, the 6×6×6 cube, the 24 greys.
    pub fn indexed(&self, index: u8) -> Color32 {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
                let i = usize::from(index - 16);
                Color32::from_rgb(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
            }
            232..=255 => {
                let grey = 8 + 10 * (index - 232);
                Color32::from_rgb(grey, grey, grey)
            }
        }
    }

    /// The colour `color` stands for; `DIM` halves its strength.
    pub fn resolve(&self, color: TermColor, flags: CellFlags) -> Color32 {
        let c = match color {
            TermColor::Default => self.fg,
            TermColor::DefaultBg => self.bg,
            TermColor::Indexed(i) => self.indexed(i),
            TermColor::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        };
        if flags.contains(CellFlags::DIM) { c.gamma_multiply(0.5) } else { c }
    }
}

/// The contrast ratio of two opaque colours after WCAG 2.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn channel(v: u8) -> f32 {
        let c = f32::from(v) / 255.0;
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    fn luminance(c: Color32) -> f32 {
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
