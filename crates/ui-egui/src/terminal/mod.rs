//! The terminal in egui. [`layout`] turns a screen into primitives without touching egui's
//! painter, so it is tested on its own; [`paint`] draws them.

pub mod fonts;
mod keys;
pub mod layout;
pub mod paint;
pub mod palette;
pub mod pump;
mod view;

pub use fonts::install_terminal_font;
pub use layout::{CellMetrics, LineStyle, Primitive, layout};
pub use paint::paint;
pub use palette::TermPalette;
pub use view::{TerminalResponse, TerminalView};
