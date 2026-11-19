//! Short messages at the bottom right: information fades, errors stay until closed.

use std::time::{Duration, Instant};

/// How long an information message stays.
const INFO_FOR: Duration = Duration::from_secs(4);
/// At most this many messages; the oldest goes first.
const MAX_TOASTS: usize = 5;
/// A message longer than this is cut (an engine error can quote server output).
const MAX_TOAST_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    until: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    fn push(&mut self, text: &str, kind: ToastKind, until: Option<Instant>) {
        let text: String = text.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_TOAST_CHARS).collect();
        self.items.push(Toast { text, kind, until });
        if self.items.len() > MAX_TOASTS {
            self.items.remove(0);
        }
    }

    pub fn info(&mut self, text: &str) {
        self.push(text, ToastKind::Info, Some(Instant::now() + INFO_FOR));
    }

    pub fn error(&mut self, text: &str) {
        self.push(text, ToastKind::Error, None);
    }

    pub fn texts(&self) -> Vec<String> {
        self.items.iter().map(|t| t.text.clone()).collect()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.items.retain(|t| t.until.is_none_or(|u| u > now));
        if self.items.is_empty() {
            return;
        }
        let tokens = crate::theme::Tokens::get(ctx);
        let mut close = None;
        // At most 360 points, and never wider than the window between its 12 point margins (a
        // phone): the text wraps instead of running off the screen.
        let room = (ctx.content_rect().width() - 24.0 - 16.0).clamp(120.0, 360.0);
        egui::Area::new(egui::Id::unique("sshcraft-toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0)).show(ctx, |ui| {
            for (i, toast) in self.items.iter().enumerate() {
                let stroke = if toast.kind == ToastKind::Error { tokens.danger } else { tokens.separator };
                egui::Frame::new().fill(tokens.panel).stroke(egui::Stroke::new(1.0, stroke)).inner_margin(8.0).corner_radius(tokens.radius).show(
                    ui,
                    |ui| {
                        ui.set_max_width(room);
                        ui.horizontal_top(|ui| {
                            // Beside the close button; a label wraps only in a column of its own.
                            ui.vertical(|ui| {
                                ui.set_max_width((room - 36.0).max(60.0));
                                ui.add(egui::Label::new(egui::RichText::new(&toast.text).color(tokens.text)).wrap());
                            });
                            if ui.small_button("×").on_hover_text(crate::i18n::tr("Close")).clicked() {
                                close = Some(i);
                            }
                        });
                    },
                );
            }
        });
        if let Some(i) = close {
            self.items.remove(i);
        }
        if let Some(next) = self.items.iter().filter_map(|t| t.until).min() {
            ctx.request_repaint_after(next.saturating_duration_since(now));
        }
    }
}
