//! Plays a session log back in a read-only terminal (the `term` crate), with its marks:
//! bookmarks and comments at byte positions of the recorded output. A log of more lines than
//! the terminal keeps at its width (at most [`MAX_LINES`]) shows only its end, and its marks are
//! off. The log is read and laid out off the window's thread.

use serde_json::{Value, json};
use sshcraft_term::{Direction, TermConfig, TermSize, Terminal};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::terminal::{TermPalette, TerminalView};
use crate::theme::Tokens;

/// Lines the viewer keeps.
pub const MAX_LINES: usize = 100_000;
/// Most characters of a bookmark's name and of a comment.
pub const MAX_BOOKMARK: usize = 100;
pub const MAX_COMMENT: usize = 2000;

pub struct LogViewer {
    id: String,
    /// The recorded output, to lay the marks out again when they change.
    bytes: Vec<u8>,
    term: Terminal,
    /// The output as plain text.
    plain: String,
    /// Where each line starts in the recorded bytes.
    line_starts: Vec<u64>,
    marks: Vec<Value>,
    /// The terminal could not keep every line, at some width it had.
    beginning_missing: bool,
    /// The comment being written.
    pub comment: Option<String>,
}

impl std::fmt::Debug for LogViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogViewer").field("id", &self.id).field("marks", &self.marks.len()).finish()
    }
}

fn offset_of(mark: &Value) -> u64 {
    mark.get("offset").and_then(Value::as_u64).unwrap_or(0)
}

impl LogViewer {
    /// The log `id` with its `bytes` and `marks` (as `log.list` gives them).
    pub fn from_bytes(id: String, bytes: &[u8], marks: &[Value]) -> Self {
        let mut line_starts = vec![0u64];
        line_starts.extend(bytes.iter().enumerate().filter(|(_, b)| **b == b'\n').map(|(i, _)| u64::try_from(i + 1).unwrap_or(u64::MAX)));
        let mut marks: Vec<Value> = marks.to_vec();
        marks.sort_by_key(offset_of);
        let term = Terminal::new(TermSize::clamped(160, 40), &TermConfig { scrollback: MAX_LINES, clipboard_write: false });
        let plain = sshcraft_model::logtext::plain_text(bytes);
        let mut viewer = Self { id, bytes: bytes.to_vec(), term, plain, line_starts, marks, beginning_missing: false, comment: None };
        viewer.lay_out();
        viewer
    }

    /// Lines of the output (by their line breaks).
    fn lines(&self) -> usize {
        self.line_starts.len().saturating_sub(1)
    }

    /// Notes when the terminal keeps fewer lines than the output has (a wider view keeps fewer);
    /// once lost, lines stay lost.
    fn check_missing(&mut self) {
        let keeps = self.term.effective_scrollback().saturating_add(usize::from(self.term.size().rows));
        self.beginning_missing |= self.lines() > keeps.min(MAX_LINES);
    }

    /// Feeds the output again into a fresh terminal of the same size, a mark on the line the
    /// output had reached at each mark's position.
    fn lay_out(&mut self) {
        self.term = Terminal::new(self.term.size(), &TermConfig { scrollback: MAX_LINES, clipboard_write: false });
        self.check_missing();
        let term = &mut self.term;
        let mut fed = 0usize;
        if !self.beginning_missing {
            for mark in &self.marks {
                let to = usize::try_from(offset_of(mark)).unwrap_or(usize::MAX).min(self.bytes.len());
                if to > fed {
                    term.feed(self.bytes.get(fed..to).unwrap_or_default());
                    fed = to;
                }
                term.add_mark();
            }
        }
        term.feed(self.bytes.get(fed..).unwrap_or_default());
        // A read-only viewer answers nothing and shows no title.
        let _ = term.take_notices();
    }

    pub fn text(&self) -> &str {
        &self.plain
    }

    pub fn marks(&self) -> &[Value] {
        &self.marks
    }

    pub fn beginning_missing(&self) -> bool {
        self.beginning_missing
    }

    /// The byte position of the top visible line (lines counted by their line breaks).
    fn top_offset(&self) -> u64 {
        let screen = self.term.screen();
        let below = usize::from(screen.rows).saturating_add(screen.display_offset);
        let top = self.line_starts.len().saturating_sub(below);
        self.line_starts.get(top).copied().unwrap_or(0)
    }

    /// The marks after one was added or removed; the terminal's own marks follow them.
    pub fn set_marks(&mut self, mut marks: Vec<Value>) {
        marks.sort_by_key(offset_of);
        self.marks = marks;
        self.lay_out();
    }

    /// The text on screen.
    pub fn screen_text(&self) -> String {
        self.term.visible_text()
    }

    /// Shows mark `index` (in position order).
    pub fn jump(&mut self, index: usize) {
        self.term.scroll_to_bottom();
        for _ in index..self.marks.len() {
            self.term.jump_to_mark(Direction::Up);
        }
    }
}

/// Opens log `id`: reads its output and shows it.
pub fn open(app: &mut SshcraftApp, id: String) {
    let marks = app
        .data
        .logs
        .iter()
        .find(|l| l.get("id").and_then(Value::as_str) == Some(id.as_str()))
        .and_then(|l| l.get("marks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Decoding and laying out a large log takes a while: it happens off the window's thread,
    // and the finished viewer is handed over.
    let ready: std::sync::Arc<std::sync::Mutex<Option<LogViewer>>> = std::sync::Arc::default();
    let slot = ready.clone();
    let session = app.session.clone();
    app.runner.run_future(
        async move {
            let v = session.execute(sshcraft_engine::Caller::User, "log.read", json!({ "id": id })).await?;
            let viewer = tokio::task::spawn_blocking(move || {
                let bytes =
                    v.get("data_base64").and_then(Value::as_str).and_then(|b| data_encoding::BASE64.decode(b.as_bytes()).ok()).unwrap_or_default();
                LogViewer::from_bytes(id, &bytes, &marks)
            })
            .await
            .map_err(|_| sshcraft_engine::EngineError::Internal { id: "log.read".into() })?;
            *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(viewer);
            Ok(Value::Null)
        },
        Box::new(move |app, result| match result {
            Ok(_) => app.log_viewer = ready.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take(),
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn add_mark(app: &mut SshcraftApp, kind: &str, text: String) {
    let Some(viewer) = app.log_viewer.as_ref() else { return };
    let mark = json!({ "id": uuid::Uuid::new_v4(), "offset": viewer.top_offset(), "kind": kind, "text": text, "created_at": now_ms() });
    let log = viewer.id.clone();
    app.runner.run(
        "log.mark_add",
        json!({ "id": log, "mark": mark }),
        Box::new(move |app, result| match result {
            Ok(_) => {
                if let Some(v) = app.log_viewer.as_mut().filter(|v| v.id == log) {
                    let mut marks = v.marks.clone();
                    marks.push(mark);
                    v.set_marks(marks);
                }
                app.data.dirty |= crate::data::Refresh::LOGS;
            }
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

enum ViewerAction {
    Close,
    Bookmark,
    Comment,
    Jump(usize),
    Remove(Value),
}

pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut viewer) = app.log_viewer.take() else { return };
    let tokens = Tokens::get(ctx);
    let palette = TermPalette::dark();
    let font = egui::FontId::monospace(13.0);
    let mut action = None;
    let size = ctx.content_rect().size();
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-log-viewer")).show(ctx, |ui| {
        ui.set_width((size.x - 120.0).max(400.0));
        ui.horizontal(|ui| {
            ui.heading(tr("Session log"));
            ui.add_space(12.0);
            let marks_on = !viewer.beginning_missing;
            if ui.add_enabled(marks_on, egui::Button::new(tr("Bookmark"))).clicked() {
                action = Some(ViewerAction::Bookmark);
            }
            if ui.add_enabled(marks_on, egui::Button::new(tr("Comment"))).clicked() {
                viewer.comment = Some(String::new());
            }
            if ui.button(tr("Close")).clicked() {
                action = Some(ViewerAction::Close);
            }
        });
        if viewer.beginning_missing {
            ui.label(
                egui::RichText::new(tr("This log is longer than the viewer keeps: its beginning is not shown, and marks are off."))
                    .color(tokens.warning),
            );
        }
        if let Some(comment) = viewer.comment.as_mut() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::multiline(comment).desired_rows(2).desired_width(400.0).char_limit(MAX_COMMENT));
                if ui.add_enabled(!comment.trim().is_empty(), egui::Button::new(tr("Add comment"))).clicked() {
                    action = Some(ViewerAction::Comment);
                }
            });
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                ui.label(egui::RichText::new(tr("Marks")).strong());
                for (i, mark) in viewer.marks.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let text = mark.get("text").and_then(Value::as_str).unwrap_or_default();
                        if crate::widgets::elided_label(ui, text, 170.0, tokens.text, egui::FontId::proportional(13.0)).clicked() {
                            action = Some(ViewerAction::Jump(i));
                        }
                        if crate::widgets::icons::icon_button(ui, crate::widgets::icons::Icon::Close, tr("Remove mark")) {
                            action = Some(ViewerAction::Remove(mark.clone()));
                        }
                    });
                }
            });
            let height = (size.y - 220.0).max(200.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                // Read-only: what the view would send goes nowhere.
                let _ = TerminalView { term: &mut viewer.term, palette: &palette, font: font.clone(), ligatures: true }.show(ui);
            });
            // A wider view keeps fewer lines; nothing the log prints is answered.
            viewer.check_missing();
            let _ = viewer.term.take_notices();
        });
    });
    if modal.should_close() {
        action = action.or(Some(ViewerAction::Close));
    }
    match action {
        Some(ViewerAction::Close) => return,
        Some(ViewerAction::Jump(i)) => viewer.jump(i),
        Some(ViewerAction::Bookmark) => {
            let name: String = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string().chars().take(MAX_BOOKMARK).collect();
            app.log_viewer = Some(viewer);
            add_mark(app, "bookmark", name);
            return;
        }
        Some(ViewerAction::Comment) => {
            let text = viewer.comment.take().unwrap_or_default();
            app.log_viewer = Some(viewer);
            add_mark(app, "comment", text);
            return;
        }
        Some(ViewerAction::Remove(mark)) => {
            let mark_id = mark.get("id").cloned().unwrap_or_default();
            let log = viewer.id.clone();
            app.runner.run(
                "log.mark_remove",
                json!({ "id": log, "mark": mark_id }),
                Box::new(move |app, result| match result {
                    Ok(_) => {
                        if let Some(v) = app.log_viewer.as_mut().filter(|v| v.id == log) {
                            let marks = v.marks.iter().filter(|m| m.get("id") != Some(&mark_id)).cloned().collect();
                            v.set_marks(marks);
                        }
                        app.data.dirty |= crate::data::Refresh::LOGS;
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }),
            );
        }
        None => {}
    }
    app.log_viewer = Some(viewer);
}
