//! The colours of SSHCraft, dark and light. Its own values; nothing taken from another product.

use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub sidebar: Color32,
    pub row: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_strong: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub accent_dim: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub button: Color32,
    pub button_border: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub radius: u8,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Tokens {
    pub const fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x0f1115),
            panel: hex(0x15181d),
            sidebar: hex(0x111318),
            row: hex(0x1a1d23),
            row_hover: hex(0x22262e),
            row_selected: hex(0x1f3a2c),
            separator: hex(0x2a2e36),
            text: hex(0xeceae6),
            text_dim: hex(0xa3a7b0),
            text_strong: hex(0xffffff),
            accent: hex(0x00e676),
            accent_strong: hex(0x00b85c),
            accent_dim: hex(0x0f3d27),
            field: hex(0x1c1f25),
            field_border: hex(0x3a3f49),
            button: hex(0x242830),
            button_border: hex(0x3a3f49),
            danger: hex(0xff6b6b),
            warning: hex(0xf2c14e),
            success: hex(0x4ccf86),
            radius: 6,
        }
    }

    pub const fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xf7f6f3),
            panel: hex(0xffffff),
            sidebar: hex(0xefeee9),
            row: hex(0xfbfaf8),
            row_hover: hex(0xf0efea),
            row_selected: hex(0xd8f3e3),
            separator: hex(0xdedcd5),
            text: hex(0x1b1d21),
            text_dim: hex(0x575c66),
            text_strong: hex(0x000000),
            accent: hex(0x00873e),
            accent_strong: hex(0x006b31),
            accent_dim: hex(0xd8f3e3),
            field: hex(0xffffff),
            field_border: hex(0xb9bcc3),
            button: hex(0xf2f1ec),
            button_border: hex(0xb9bcc3),
            danger: hex(0xc4302b),
            warning: hex(0x9a6b00),
            success: hex(0x1f7a3a),
            radius: 6,
        }
    }

    /// The tokens in use, as [`Tokens::set`] stored them; dark before the first frame sets them.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::NULL)).unwrap_or(Self::dark())
    }

    pub fn set(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, self));
    }
}
