//! Panes in a tab: up to [`MAX_PANES`] in a near-square grid, where the last pane of a row
//! reaches its right edge, or one pane large with the others listed beside it.

use egui::{Rect, Sense, Ui, pos2, vec2};
use sshcraft_engine::SessionId;

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::state::{PaneSlot, Panes};
use crate::theme::Tokens;
use crate::widgets::elided_label;
use crate::widgets::icons::{Icon, icon_button};

/// The most panes a tab holds.
pub const MAX_PANES: usize = 16;
/// The smallest share of the width or height a grid column or row keeps.
pub const MIN_SHARE: f32 = 0.1;
const TITLE_HEIGHT: f32 = 24.0;
const DIVIDER: f32 = 6.0;
const LIST_WIDTH: f32 = 200.0;

/// Columns and rows for `count` panes: as square as possible, wider than tall.
pub fn grid(count: usize) -> (usize, usize) {
    let count = count.clamp(1, MAX_PANES);
    let mut cols = 1;
    while cols * cols < count {
        cols += 1;
    }
    let rows = count.div_ceil(cols);
    (cols, rows)
}

fn equal(n: usize) -> Vec<f32> {
    vec![1.0 / n.max(1) as f32; n.max(1)]
}

impl Panes {
    pub fn single(slot: PaneSlot) -> Self {
        let mut panes = Self { panes: vec![slot], focused: 0, focus_layout: false, col_sizes: Vec::new(), row_sizes: Vec::new(), broadcast: false };
        panes.reset_sizes();
        panes
    }

    /// Equal columns and rows for the current count.
    pub fn reset_sizes(&mut self) {
        let (cols, rows) = grid(self.panes.len());
        self.col_sizes = equal(cols);
        self.row_sizes = equal(rows);
    }

    pub fn focused_slot(&self) -> Option<PaneSlot> {
        self.panes.get(self.focused).copied()
    }

    pub fn sessions(&self) -> impl Iterator<Item = SessionId> + '_ {
        self.panes.iter().filter_map(|p| match p {
            PaneSlot::Session { session } => Some(*session),
            PaneSlot::New { .. } => None,
        })
    }

    pub fn position(&self, slot: PaneSlot) -> Option<usize> {
        self.panes.iter().position(|p| *p == slot)
    }

    /// Adds `slot` after the focused pane and focuses it; false when the tab is full.
    pub fn add(&mut self, slot: PaneSlot) -> bool {
        if self.panes.len() >= MAX_PANES {
            return false;
        }
        let at = (self.focused + 1).min(self.panes.len());
        self.panes.insert(at, slot);
        self.focused = at;
        self.reset_sizes();
        true
    }

    /// Takes pane `i` out; the focus stays on the pane before it.
    pub fn remove(&mut self, i: usize) -> Option<PaneSlot> {
        if i >= self.panes.len() {
            return None;
        }
        let slot = self.panes.remove(i);
        if self.focused >= i && self.focused > 0 {
            self.focused -= 1;
        }
        self.focused = self.focused.min(self.panes.len().saturating_sub(1));
        self.reset_sizes();
        Some(slot)
    }

    /// The grid rectangle of every pane inside `area` (dividers between them).
    pub fn rects(&self, area: Rect) -> Vec<Rect> {
        let (cols, rows) = grid(self.panes.len());
        let sizes = |shares: &[f32], n: usize| if shares.len() == n { shares.to_vec() } else { equal(n) };
        let (cs, rs) = (sizes(&self.col_sizes, cols), sizes(&self.row_sizes, rows));
        let edges = |shares: &[f32], start: f32, length: f32| {
            let mut at = start;
            let mut out = vec![at];
            for s in shares {
                at += s * length;
                out.push(at);
            }
            out
        };
        let (xs, ys) = (edges(&cs, area.min.x, area.width()), edges(&rs, area.min.y, area.height()));
        let half = DIVIDER / 2.0;
        (0..self.panes.len())
            .map(|i| {
                let (row, col) = (i / cols, i % cols);
                let last_in_row = i + 1 == self.panes.len() || col + 1 == cols;
                let x0 = xs.get(col).copied().unwrap_or(area.min.x) + if col > 0 { half } else { 0.0 };
                let x1 = if last_in_row { area.max.x } else { xs.get(col + 1).copied().unwrap_or(area.max.x) - half };
                let y0 = ys.get(row).copied().unwrap_or(area.min.y) + if row > 0 { half } else { 0.0 };
                let y1 = if row + 1 == rows { area.max.y } else { ys.get(row + 1).copied().unwrap_or(area.max.y) - half };
                Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)))
            })
            .collect()
    }

    /// Moves the boundary after column (or row) `index` by `delta` of the whole; each keeps
    /// [`MIN_SHARE`].
    pub fn resize(&mut self, columns: bool, index: usize, delta: f32) {
        let shares = if columns { &mut self.col_sizes } else { &mut self.row_sizes };
        let (Some(a), Some(b)) = (shares.get(index).copied(), shares.get(index + 1).copied()) else { return };
        let delta = delta.clamp(MIN_SHARE - a, b - MIN_SHARE);
        if let Some(s) = shares.get_mut(index) {
            *s = a + delta;
        }
        if let Some(s) = shares.get_mut(index + 1) {
            *s = b - delta;
        }
    }
}

/// What a pane's title bar asked for.
enum PaneAction {
    Focus,
    Maximize,
    OwnTab,
    Close,
    /// Dropped onto pane `index`.
    SwapWith(usize),
}

fn slot_title(app: &SshcraftApp, slot: PaneSlot) -> String {
    match slot {
        PaneSlot::New { .. } => tr("New pane").to_string(),
        PaneSlot::Session { session } => app.terminals.get(&session).map(|t| t.title.clone()).unwrap_or_default(),
    }
}

/// The title bar of pane `index` (shown when the tab has more than one).
fn title_bar(app: &SshcraftApp, ui: &mut Ui, rect: Rect, index: usize, slot: PaneSlot, focused: bool, rects: &[Rect]) -> Option<PaneAction> {
    let tokens = Tokens::get(ui.ctx());
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_HEIGHT));
    let response = ui.interact(bar, ui.scope_id().with(("sshcraft-pane-title", index)), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, tr("Pane title")));
    ui.painter().rect_filled(bar, 0.0, if focused { tokens.row_selected } else { tokens.panel });
    let mut action = None;
    if response.clicked() {
        action = Some(PaneAction::Focus);
    }
    if response.drag_stopped()
        && let Some(at) = ui.ctx().pointer_interact_pos()
        && let Some(target) = rects.iter().position(|r| r.contains(at))
        && target != index
    {
        action = Some(PaneAction::SwapWith(target));
    }
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    elided_label(
        &mut child,
        &slot_title(app, slot),
        (bar.width() - 120.0).max(40.0),
        if focused { tokens.text_strong } else { tokens.text },
        egui::FontId::proportional(13.0),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if icon_button(ui, Icon::Close, tr("Close pane")) {
            action = Some(PaneAction::Close);
        }
        if icon_button(ui, Icon::Detach, tr("Move to its own tab")) {
            action = Some(PaneAction::OwnTab);
        }
        if icon_button(ui, Icon::Maximize, tr("Maximize")) {
            action = Some(PaneAction::Maximize);
        }
    });
    action
}

/// The dividers between grid columns and rows; dragging one moves it.
fn dividers(app: &mut SshcraftApp, ui: &mut Ui, tab: usize, area: Rect) {
    let Some(panes) = app.ui.tabs.get(tab).map(|t| t.panes.clone()) else { return };
    let (cols, rows) = grid(panes.panes.len());
    let mut x = area.min.x;
    for c in 0..cols.saturating_sub(1) {
        x += panes.col_sizes.get(c).copied().unwrap_or(0.0) * area.width();
        let strip = Rect::from_center_size(pos2(x, area.center().y), vec2(DIVIDER, area.height()));
        let response = ui.interact(strip, ui.scope_id().with(("sshcraft-col-divider", c)), Sense::drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, tr("Resize columns")));
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            let delta = response.drag_delta().x / area.width().max(1.0);
            if let Some(t) = app.ui.tabs.get_mut(tab) {
                t.panes.resize(true, c, delta);
            }
        }
    }
    let mut y = area.min.y;
    for r in 0..rows.saturating_sub(1) {
        y += panes.row_sizes.get(r).copied().unwrap_or(0.0) * area.height();
        let strip = Rect::from_center_size(pos2(area.center().x, y), vec2(area.width(), DIVIDER));
        let response = ui.interact(strip, ui.scope_id().with(("sshcraft-row-divider", r)), Sense::drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, tr("Resize rows")));
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            let delta = response.drag_delta().y / area.height().max(1.0);
            if let Some(t) = app.ui.tabs.get_mut(tab) {
                t.panes.resize(false, r, delta);
            }
        }
    }
}

/// Draws the panes of tab `tab` in the rest of `ui`.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui, tab: usize) {
    let Some(panes) = app.ui.tabs.get(tab).map(|t| t.panes.clone()) else { return };
    let area = ui.available_rect_before_wrap();
    let multi = panes.panes.len() > 1;
    let mut action: Option<(usize, PaneAction)> = None;
    let layout: Vec<(usize, Rect)> = if multi && panes.focus_layout {
        // The others as a list on the left; a click brings one to the front.
        let list = Rect::from_min_size(area.min, vec2(LIST_WIDTH, area.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink(4.0)));
        for (i, slot) in panes.panes.iter().enumerate() {
            let tokens = Tokens::get(child.ctx());
            let color = if i == panes.focused { tokens.text_strong } else { tokens.text };
            if let Some(dot) = crate::session_tab::slot_dot(app, *slot) {
                let at = child.cursor().min + vec2(-6.0 + 4.0, 9.0);
                dot.draw(child.painter(), at, &tokens);
            }
            if elided_label(&mut child, &slot_title(app, *slot), LIST_WIDTH - 12.0, color, egui::FontId::proportional(13.0)).clicked() {
                action = Some((i, PaneAction::Focus));
            }
        }
        vec![(panes.focused, Rect::from_min_max(pos2(list.max.x + DIVIDER, area.min.y), area.max))]
    } else {
        panes.rects(area).into_iter().enumerate().collect()
    };
    let rects: Vec<Rect> = layout.iter().map(|(_, r)| *r).collect();
    let grid_rects = panes.rects(area);
    for (i, rect) in &layout {
        let Some(slot) = panes.panes.get(*i).copied() else { continue };
        let focused = *i == panes.focused;
        let mut content = *rect;
        if multi {
            let targets = if panes.focus_layout { &rects } else { &grid_rects };
            if let Some(a) = title_bar(app, ui, *rect, *i, slot, focused, targets) {
                action = Some((*i, a));
            }
            content.min.y += TITLE_HEIGHT;
        }
        // A click into a pane gives it the focus.
        if ui.input(|inp| inp.pointer.primary_pressed()) && ui.ctx().pointer_interact_pos().is_some_and(|p| content.contains(p)) && !focused {
            action = Some((*i, PaneAction::Focus));
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content).id_salt(("sshcraft-pane", tab, *i)));
        match slot {
            PaneSlot::Session { session } => crate::session_tab::show(app, &mut child, session, focused),
            PaneSlot::New { .. } => crate::shell::new_tab_page(app, &mut child),
        }
        let tokens = Tokens::get(ui.ctx());
        if panes.broadcast {
            // Every pane typing goes to wears red.
            ui.painter().rect_stroke(*rect, 0.0, egui::Stroke::new(2.0, tokens.danger), egui::StrokeKind::Inside);
        } else if multi && focused {
            ui.painter().rect_stroke(*rect, 0.0, egui::Stroke::new(1.0, tokens.accent_dim), egui::StrokeKind::Inside);
        }
    }
    if multi && !panes.focus_layout {
        dividers(app, ui, tab, area);
    }
    ui.allocate_rect(area, Sense::hover());
    if let Some((i, action)) = action {
        apply(app, tab, i, action);
    }
}

fn apply(app: &mut SshcraftApp, tab: usize, i: usize, action: PaneAction) {
    match action {
        PaneAction::Focus => {
            if let Some(t) = app.ui.tabs.get_mut(tab) {
                t.panes.focused = i.min(t.panes.panes.len().saturating_sub(1));
            }
        }
        PaneAction::Maximize => {
            if let Some(t) = app.ui.tabs.get_mut(tab) {
                t.panes.focused = i;
                t.panes.focus_layout = !t.panes.focus_layout;
            }
        }
        PaneAction::Close => app.close_pane_at(tab, i),
        PaneAction::OwnTab => app.move_pane_to_own_tab(tab, i),
        PaneAction::SwapWith(j) => {
            if let Some(t) = app.ui.tabs.get_mut(tab)
                && i < t.panes.panes.len()
                && j < t.panes.panes.len()
            {
                t.panes.panes.swap(i, j);
                t.panes.focused = j;
            }
        }
    }
}
