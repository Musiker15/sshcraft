//! Shared widgets: icons drawn in code, labels that never run over their line.

pub mod buttons;
pub mod elided;
pub mod icons;
pub mod logo;
pub mod qr;

pub use elided::{elided_label, forget_undo, secret_field, visible, visible_lines};
