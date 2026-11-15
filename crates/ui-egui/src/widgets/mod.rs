//! Shared widgets: icons drawn in code, labels that never run over their line.

pub mod elided;
pub mod icons;

pub use elided::{elided_label, forget_undo, secret_field, visible, visible_lines};
