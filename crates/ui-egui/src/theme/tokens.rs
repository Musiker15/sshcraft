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
    /// Blue: something new to look at (unseen output).
    pub info: Color32,
    /// Corners of controls.
    pub radius: u8,
    /// Corners of cards: hosts, groups, sections.
    pub radius_card: u8,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Tokens {
    pub const fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x18191e),
            panel: hex(0x24252b),
            sidebar: hex(0x24252b),
            row: hex(0x24252b),
            row_hover: hex(0x2d2e35),
            row_selected: hex(0x3f3f3f),
            separator: hex(0x3f3f3f),
            text: hex(0xededef),
            text_dim: hex(0x949494),
            text_strong: hex(0xffffff),
            accent: hex(0x0071ff),
            accent_strong: hex(0x005acc),
            accent_dim: hex(0x0a2a55),
            field: hex(0x24252b),
            // #3f3f3f would be 1.45:1 on the field; borders need 1.6:1 (contrast_tests).
            field_border: hex(0x4a4b52),
            button: hex(0x2d2e35),
            button_border: hex(0x4a4b52),
            danger: hex(0xff6b6b),
            warning: hex(0xe5b44b),
            success: hex(0x3fcf8e),
            info: hex(0x5aa9ff),
            radius: 10,
            radius_card: 16,
        }
    }

    pub const fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xffffff),
            panel: hex(0xffffff),
            sidebar: hex(0xefeff2),
            row: hex(0xffffff),
            row_hover: hex(0xe0e0e0),
            row_selected: hex(0xe0e0e0),
            separator: hex(0xcccccc),
            text: hex(0x18191e),
            text_dim: hex(0x666666),
            text_strong: hex(0x000000),
            accent: hex(0x0071ff),
            accent_strong: hex(0x005acc),
            accent_dim: hex(0xdcebff),
            field: hex(0xffffff),
            field_border: hex(0xcccccc),
            button: hex(0xefeff2),
            button_border: hex(0xcccccc),
            danger: hex(0xc4302b),
            warning: hex(0x9a6b00),
            success: hex(0x12a150),
            info: hex(0x1f5fbf),
            radius: 10,
            radius_card: 16,
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
