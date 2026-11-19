//! The SFTP pane (S1): this computer on the left, the host on the right, transfers below. Under
//! [`TWO_SIDES_FROM`] points one side shows at a time, with a switch. On Android only the host's
//! side is there: the app cannot browse the device's folders. "Copy to…" sends the host's
//! selection into the folder another open pane shows (from server to server).
//!
//! Everything it does goes through the engine's `sftp.*` commands; names from the server are
//! shown through [`crate::widgets::visible`], so an escape sequence in one cannot do anything.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_model::model::HostId;

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::state::{Page, PaneSlot, Panes, TabState};
use crate::theme::Tokens;
use crate::widgets::buttons::{chip, ghost, primary, search, square};
use crate::widgets::icons::{Icon, draw};
use crate::widgets::{one_line, visible};

/// From this width both sides show beside each other.
pub const TWO_SIDES_FROM: f32 = 700.0;
const ROW: f32 = 30.0;
const BUTTON: f32 = 32.0;
/// One transfer in the strip, with the space after it.
const ROW_TRANSFER: f32 = 48.0;

/// Whether this build has the local side.
pub const LOCAL_SIDE: bool = !cfg!(target_os = "android");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideKind {
    Local,
    #[default]
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
    Modified,
}

/// One entry as `sftp.list` and `sftp.local_list` give it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub permissions: Option<u32>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == "dir"
    }
}

#[derive(Debug, Clone, Default)]
pub struct Side {
    pub path: String,
    pub entries: Vec<Entry>,
    pub truncated: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub selected: BTreeSet<String>,
    pub filter: String,
    pub show_hidden: bool,
    pub sort: SortBy,
    pub descending: bool,
}

impl Side {
    /// The entries as shown: filtered, hidden ones left out unless asked for, folders first.
    pub fn shown(&self) -> Vec<&Entry> {
        let filter = self.filter.to_lowercase();
        let mut shown: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter))
            .collect();
        shown.sort_by(|a, b| {
            let order = match self.sort {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
            };
            b.is_dir().cmp(&a.is_dir()).then(if self.descending { order.reverse() } else { order })
        });
        shown
    }
}

/// A question the pane asks before it acts.
#[derive(Debug, Clone)]
pub enum Dialog {
    NewFolder {
        name: String,
    },
    Rename {
        path: String,
        name: String,
    },
    Permissions {
        path: String,
        mode: String,
    },
    Delete {
        paths: Vec<String>,
        count: Option<Value>,
    },
    /// Which other pane's folder the selection is copied into.
    CopyTo {
        items: Vec<String>,
        names: Vec<String>,
    },
    /// Some names are already in the target folder.
    Conflict {
        transfer: PendingTransfer,
        taken: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Upload,
    Download,
    /// Into the remote folder of pane `to`, from server to server.
    Copy {
        to: u64,
    },
}

#[derive(Debug, Clone)]
pub struct PendingTransfer {
    pub kind: TransferKind,
    pub items: Vec<String>,
    pub target: String,
}

impl PendingTransfer {
    fn command(&self) -> &'static str {
        match self.kind {
            TransferKind::Upload => "sftp.upload",
            TransferKind::Download => "sftp.download",
            TransferKind::Copy { .. } => "sftp.copy",
        }
    }
}

/// Another open pane, as a target for "Copy to…": its id, host name, shown folder and names.
#[derive(Debug, Clone)]
pub struct OtherPane {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub names: BTreeSet<String>,
}

/// The state of one SFTP pane.
#[derive(Debug, Clone)]
pub struct SftpView {
    pub id: u64,
    pub host: Option<HostId>,
    /// The host's name, as the right side's heading.
    pub name: String,
    pub remote: Side,
    pub local: Side,
    /// The side shown when only one fits.
    pub narrow: SideKind,
    pub dialog: Option<Dialog>,
    /// The connection went away; the host's side offers to connect again.
    pub lost: bool,
}

impl SftpView {
    pub fn new(id: u64, host: Option<HostId>, name: String, home: String) -> Self {
        SftpView {
            id,
            host,
            name,
            remote: Side { path: home, loading: true, ..Side::default() },
            local: Side { loading: LOCAL_SIDE, ..Side::default() },
            narrow: SideKind::Remote,
            dialog: None,
            lost: false,
        }
    }

    pub fn side(&mut self, kind: SideKind) -> &mut Side {
        match kind {
            SideKind::Local => &mut self.local,
            SideKind::Remote => &mut self.remote,
        }
    }
}

/// What a click asked for; done after the frame's drawing.
#[derive(Debug, Clone)]
enum Action {
    Open(SideKind, String),
    Up(SideKind),
    Refresh(SideKind),
    Transfer(PendingTransfer),
    Ask(Dialog),
    Count(Vec<String>),
    Run(&'static str, Value),
    Cancel(u64),
    Retry(u64),
    /// Pick a file to upload into the shown folder (Android).
    PickUpload,
    /// Pick where a remote file goes (Android).
    PickDownload {
        remote: String,
        name: String,
    },
    Reconnect,
    ClearTransfers,
}

/// The payload of a drag from one side to the other: the side and the names.
#[derive(Debug, Clone)]
struct Dragged {
    from: SideKind,
    names: Vec<String>,
}

// ---- Opening and closing ---------------------------------------------------------------------

/// Opens the files of `host` in a tab of its own.
pub fn open(app: &mut SshcraftApp, host: HostId) {
    let name = app.data.hosts.iter().find(|r| r.host.id == host).map(|r| r.host.name.clone()).unwrap_or_else(|| tr("Host").to_string());
    let asked_from = app.ui.page;
    app.runner.run(
        "sftp.open",
        json!({ "host_id": host }),
        Box::new(move |app, result| {
            let opened = match result {
                Ok(v) => v,
                Err(e) => {
                    app.toasts.error(&error_text(&e));
                    return;
                }
            };
            let (Some(id), Some(home)) = (opened["id"].as_u64(), opened["home"].as_str()) else { return };
            let title = tab_title(&name);
            app.sftp.insert(id, SftpView::new(id, Some(host), name.clone(), home.to_owned()));
            app.ui.tabs.push(TabState { title, panes: Panes::single(PaneSlot::Sftp { id }), named: true });
            if app.ui.page == asked_from {
                app.ui.page = Page::Tab(app.ui.tabs.len() - 1);
            }
            list(app, id, SideKind::Remote, home.to_owned());
            if LOCAL_SIDE {
                list(app, id, SideKind::Local, String::new());
            }
        }),
    );
}

/// Closes the pane's SFTP session.
pub fn close(app: &mut SshcraftApp, id: u64) {
    app.sftp.remove(&id);
    app.run_reported("sftp.close", json!({ "id": id }));
}

/// The pane's title, for the tab and the pane's bar.
pub fn title(app: &SshcraftApp, id: u64) -> String {
    app.sftp.get(&id).map(|v| tab_title(&v.name)).unwrap_or_default()
}

/// "web-01 (files)": a tab with a host's files.
pub fn tab_title(name: &str) -> String {
    trf("{name} (files)", &[("name", &visible(name))])
}

fn error_text(e: &sshcraft_engine::EngineError) -> String {
    use sshcraft_engine::{EngineError, ErrorCode};
    match e {
        EngineError::Failed { code, detail } => {
            let text = match code {
                ErrorCode::SftpUnavailable => tr("This server offers no SFTP."),
                ErrorCode::PermissionDenied => tr("Permission denied."),
                ErrorCode::NotFound => tr("It is not there (any more)."),
                ErrorCode::AlreadyExists => tr("Something of that name is already there."),
                ErrorCode::NameInvalid => tr("That name cannot be used."),
                ErrorCode::TooManyFiles => tr("Too many files or folders for one go."),
                ErrorCode::Network => tr("The connection was lost."),
                ErrorCode::SftpFailed => tr("The server refused."),
                // The fingerprints say nothing to most people; the explanation does.
                ErrorCode::HostKeyChanged => {
                    return tr(
                        "The host key of this server changed. Someone may be listening in, or the server was set up again. SSHCraft does not connect; if the change is expected, forget the old key in the host's settings.",
                    )
                    .to_string();
                }
                ErrorCode::HostKeyRejected => tr("The host key was not accepted."),
                ErrorCode::PasswordRejected => tr("The password was not accepted."),
                ErrorCode::SignInFailed => tr("Sign-in failed."),
                ErrorCode::SignInCancelled => tr("Sign-in was cancelled."),
                ErrorCode::Timeout => tr("The server did not answer in time."),
                _ => return detail.as_deref().map_or_else(|| visible(&format!("{code:?}")), visible),
            };
            match detail {
                Some(d) if !d.is_empty() => format!("{text} ({})", visible(d)),
                _ => text.to_string(),
            }
        }
        other => visible(&other.to_string()),
    }
}

/// Reads the folder `path` of one side (empty: the local home folder).
fn list(app: &mut SshcraftApp, id: u64, side: SideKind, path: String) {
    if let Some(view) = app.sftp.get_mut(&id) {
        let s = view.side(side);
        s.loading = true;
        s.error = None;
    }
    let (command, params) = match side {
        SideKind::Remote => ("sftp.list", json!({ "id": id, "path": path })),
        SideKind::Local if path.is_empty() => ("sftp.local_list", json!({})),
        SideKind::Local => ("sftp.local_list", json!({ "path": path })),
    };
    app.runner.run(
        command,
        params,
        Box::new(move |app, result| {
            let Some(view) = app.sftp.get_mut(&id) else { return };
            let s = view.side(side);
            s.loading = false;
            match result {
                Ok(v) => {
                    let new_path = v["path"].as_str().unwrap_or_default().to_owned();
                    if new_path != s.path {
                        s.selected.clear();
                        s.filter.clear();
                    }
                    s.path = new_path;
                    s.entries = serde_json::from_value(v["entries"].clone()).unwrap_or_default();
                    s.truncated = v["truncated"].as_bool().unwrap_or(false);
                    s.selected.retain(|n| s.entries.iter().any(|e| &e.name == n));
                }
                Err(e) => {
                    s.error = Some(error_text(&e));
                    view.lost |= lost_connection(&e);
                }
            }
        }),
    );
}

/// Whether `e` says the connection went away (then the pane offers to connect again).
fn lost_connection(e: &sshcraft_engine::EngineError) -> bool {
    use sshcraft_engine::{EngineError, ErrorCode};
    matches!(e, EngineError::Failed { code: ErrorCode::Network | ErrorCode::Timeout, .. })
}

/// The path of `name` in the folder `dir` on `side`.
fn child_path(side: SideKind, dir: &str, name: &str) -> String {
    match side {
        SideKind::Remote if dir.ends_with('/') => format!("{dir}{name}"),
        SideKind::Remote => format!("{dir}/{name}"),
        SideKind::Local => std::path::Path::new(dir).join(name).to_string_lossy().into_owned(),
    }
}

fn parent(side: SideKind, path: &str) -> Option<String> {
    match side {
        SideKind::Remote => {
            let trimmed = path.trim_end_matches('/');
            if trimmed.is_empty() {
                return None;
            }
            let cut = trimmed.rfind('/')?;
            Some(if cut == 0 { "/".to_owned() } else { trimmed[..cut].to_owned() })
        }
        SideKind::Local => std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()),
    }
}

// ---- Drawing ---------------------------------------------------------------------------------

/// Draws pane `id` into `ui`.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui, id: u64) {
    let Some(mut view) = app.sftp.remove(&id) else {
        ui.centered_and_justified(|ui| ui.label(tr("These files are closed.")));
        return;
    };
    // A copy shows in the pane it writes to and in the one it reads from.
    let transfers: Vec<Value> =
        app.transfers.values().filter(|t| t["sftp"].as_u64() == Some(id) || t["source"].as_u64() == Some(id)).cloned().collect();
    let others: Vec<OtherPane> = app
        .sftp
        .values()
        .filter(|v| !v.lost && !v.remote.path.is_empty())
        .map(|v| OtherPane {
            id: v.id,
            name: v.name.clone(),
            path: v.remote.path.clone(),
            names: v.remote.entries.iter().map(|e| e.name.clone()).collect(),
        })
        .collect();
    let mut actions = Vec::new();
    let tokens = Tokens::get(ui.ctx());
    egui::Frame::new().fill(tokens.bg).inner_margin(12.0).show(ui, |ui| {
        let width = ui.available_width();
        let both = LOCAL_SIDE && width >= TWO_SIDES_FROM;
        // The strip: its frame and heading, then up to four rows; more scroll.
        let strip = if transfers.is_empty() { 0.0 } else { (transfers.len().min(4) as f32) * ROW_TRANSFER + 74.0 };
        let height = (ui.available_height() - strip).max(160.0);
        if both {
            let half = (width - 12.0) / 2.0;
            ui.horizontal_top(|ui| {
                side_panel(ui, &mut view, SideKind::Local, vec2(half, height), !others.is_empty(), &mut actions);
                ui.add_space(12.0);
                side_panel(ui, &mut view, SideKind::Remote, vec2(half, height), !others.is_empty(), &mut actions);
            });
        } else {
            if LOCAL_SIDE {
                ui.horizontal(|ui| {
                    if chip(ui, tr("This computer"), view.narrow == SideKind::Local).clicked() {
                        view.narrow = SideKind::Local;
                    }
                    if chip(ui, &visible(&view.name), view.narrow == SideKind::Remote).clicked() {
                        view.narrow = SideKind::Remote;
                    }
                });
                ui.add_space(8.0);
            }
            let kind = if LOCAL_SIDE { view.narrow } else { SideKind::Remote };
            let height = (height - if LOCAL_SIDE { 38.0 } else { 0.0 }).max(160.0);
            side_panel(ui, &mut view, kind, vec2(width, height), !others.is_empty(), &mut actions);
        }
        if !transfers.is_empty() {
            ui.add_space(10.0);
            transfer_strip(ui, &transfers, &mut actions);
        }
    });
    dialog(ui.ctx(), &mut view, &others, &mut actions);
    app.sftp.insert(id, view);
    for action in actions {
        act(app, id, action);
    }
}

fn side_panel(ui: &mut Ui, view: &mut SftpView, kind: SideKind, size: egui::Vec2, other_panes: bool, actions: &mut Vec<Action>) {
    let tokens = Tokens::get(ui.ctx());
    let has_other = LOCAL_SIDE;
    let other_path = match kind {
        SideKind::Local => view.remote.path.clone(),
        SideKind::Remote => view.local.path.clone(),
    };
    let other_names: BTreeSet<String> = match kind {
        SideKind::Local => view.remote.entries.iter().map(|e| e.name.clone()).collect(),
        SideKind::Remote => view.local.entries.iter().map(|e| e.name.clone()).collect(),
    };
    let heading = match kind {
        SideKind::Local => tr("This computer").to_string(),
        SideKind::Remote => view.name.clone(),
    };
    let lost = view.lost;
    let side = view.side(kind);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let frame = egui::Frame::new().fill(tokens.panel).stroke(Stroke::new(1.0, tokens.separator)).corner_radius(tokens.radius_card).inner_margin(12.0);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)).id_salt(("sftp-side", view_id_salt(kind))));
    let panel = frame.show(&mut child, |ui| {
        ui.set_min_size(size - vec2(24.0, 24.0));
        ui.set_max_size(size - vec2(24.0, 24.0));
        ui.horizontal(|ui| {
            crate::widgets::buttons::heading(ui, &visible(&heading), 16.0);
            if kind == SideKind::Remote && lost {
                ui.add_space(8.0);
                if primary(ui, Some(Icon::Refresh), tr("Reconnect"), 28.0).clicked() {
                    actions.push(Action::Reconnect);
                }
            }
        });
        ui.add_space(6.0);
        // The folder, and how to move.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if square(ui, Icon::Up, tr("Folder above"), BUTTON, false).clicked() {
                actions.push(Action::Up(kind));
            }
            if square(ui, Icon::Refresh, tr("Read again"), BUTTON, false).clicked() {
                actions.push(Action::Refresh(kind));
            }
            let room = (ui.available_width() - 4.0).max(40.0);
            crate::widgets::elided_label(ui, &visible(&side.path), room, tokens.text_dim, FontId::monospace(13.0));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let field = (ui.available_width() - 120.0).clamp(80.0, 260.0);
            search(ui, &mut side.filter, tr("Filter"), vec2(field, BUTTON));
            if chip(ui, tr("Hidden files"), side.show_hidden).clicked() {
                side.show_hidden = !side.show_hidden;
            }
        });
        ui.add_space(6.0);
        // What the selection can do.
        let selected: Vec<String> = side.selected.iter().cloned().collect();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if has_other && !selected.is_empty() {
                let (icon, label) = match kind {
                    SideKind::Local => (Icon::Upload, tr("Upload")),
                    SideKind::Remote => (Icon::Save, tr("Download")),
                };
                if primary(ui, Some(icon), label, BUTTON).clicked() {
                    let items = selected.iter().map(|n| child_path(kind, &side.path, n)).collect();
                    actions.push(transfer_action(kind, items, &selected, other_path.clone(), &other_names));
                }
            }
            // Without a local side (Android) files come and go through the system's picker.
            if kind == SideKind::Remote && !has_other {
                if primary(ui, Some(Icon::Upload), tr("Upload"), BUTTON).clicked() {
                    actions.push(Action::PickUpload);
                }
                let one_file = (selected.len() == 1).then(|| side.entries.iter().find(|e| e.name == selected[0] && e.kind == "file")).flatten();
                if let Some(file) = one_file
                    && ghost(ui, tr("Download"), BUTTON).clicked()
                {
                    actions.push(Action::PickDownload { remote: child_path(kind, &side.path, &file.name), name: file.name.clone() });
                }
            }
            // From server to server: into the folder another open pane shows.
            if kind == SideKind::Remote && other_panes && !selected.is_empty() && ghost(ui, tr("Copy to…"), BUTTON).clicked() {
                let items = selected.iter().map(|n| child_path(kind, &side.path, n)).collect();
                actions.push(Action::Ask(Dialog::CopyTo { items, names: selected.clone() }));
            }
            if kind == SideKind::Remote {
                if square(ui, Icon::Plus, tr("New folder"), BUTTON, false).clicked() {
                    actions.push(Action::Ask(Dialog::NewFolder { name: String::new() }));
                }
                if selected.len() == 1 {
                    let name = selected[0].clone();
                    let path = child_path(kind, &side.path, &name);
                    if ghost(ui, tr("Rename"), BUTTON).clicked() {
                        actions.push(Action::Ask(Dialog::Rename { path: path.clone(), name: name.clone() }));
                    }
                    let mode = side.entries.iter().find(|e| e.name == name).and_then(|e| e.permissions).map(|m| format!("{m:o}")).unwrap_or_default();
                    if ghost(ui, tr("Permissions"), BUTTON).clicked() {
                        actions.push(Action::Ask(Dialog::Permissions { path, mode }));
                    }
                }
                if !selected.is_empty() && square(ui, Icon::Trash, tr("Delete"), BUTTON, false).clicked() {
                    let paths: Vec<String> = selected.iter().map(|n| child_path(kind, &side.path, n)).collect();
                    actions.push(Action::Count(paths.clone()));
                    actions.push(Action::Ask(Dialog::Delete { paths, count: None }));
                }
            }
        });
        ui.add_space(8.0);
        column_header(ui, side, kind);
        if let Some(error) = &side.error {
            ui.add_space(8.0);
            ui.colored_label(tokens.danger, error);
        } else if side.loading && side.entries.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(tokens.text_dim, tr("Reading…"));
        }
        let path = side.path.clone();
        egui::ScrollArea::vertical().id_salt(("sftp-list", view_id_salt(kind))).auto_shrink([false, false]).show(ui, |ui| {
            let shown: Vec<Entry> = side.shown().into_iter().cloned().collect();
            if shown.is_empty() && !side.loading && side.error.is_none() {
                ui.add_space(12.0);
                ui.colored_label(tokens.text_dim, tr("This folder is empty."));
            }
            for entry in &shown {
                let selected = side.selected.contains(&entry.name);
                let response = entry_row(ui, entry, selected, kind);
                if response.clicked() {
                    let toggle = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    if toggle {
                        if !side.selected.remove(&entry.name) {
                            side.selected.insert(entry.name.clone());
                        }
                    } else {
                        side.selected.clear();
                        side.selected.insert(entry.name.clone());
                    }
                }
                if response.double_clicked() {
                    if entry.is_dir() || entry.kind == "symlink" {
                        actions.push(Action::Open(kind, child_path(kind, &path, &entry.name)));
                    } else if has_other {
                        let names = vec![entry.name.clone()];
                        actions.push(transfer_action(kind, vec![child_path(kind, &path, &entry.name)], &names, other_path.clone(), &other_names));
                    }
                }
                if has_other && response.drag_started() {
                    let names = if selected { side.selected.iter().cloned().collect() } else { vec![entry.name.clone()] };
                    response.dnd_set_drag_payload(Dragged { from: kind, names });
                }
            }
            if side.truncated {
                ui.add_space(6.0);
                ui.colored_label(tokens.warning, trf("Only the first {count} entries are shown.", &[("count", "100000")]));
            }
        });
    });
    // Dropped here from the other side: a transfer into this side's folder.
    let drop_area = panel.response.rect;
    if let Some(dragged) = egui::DragAndDrop::payload::<Dragged>(ui.ctx())
        && dragged.from != kind
        && ui.rect_contains_pointer(drop_area)
    {
        ui.painter().rect_stroke(drop_area, f32::from(tokens.radius_card), Stroke::new(2.0, tokens.accent), StrokeKind::Inside);
        if ui.input(|i| i.pointer.any_released())
            && let Some(dragged) = egui::DragAndDrop::take_payload::<Dragged>(ui.ctx())
        {
            let from_path = match dragged.from {
                SideKind::Local => view.local.path.clone(),
                SideKind::Remote => view.remote.path.clone(),
            };
            let here = view.side(kind);
            let names_here: BTreeSet<String> = here.entries.iter().map(|e| e.name.clone()).collect();
            let items = dragged.names.iter().map(|n| child_path(dragged.from, &from_path, n)).collect();
            actions.push(transfer_action(dragged.from, items, &dragged.names, here.path.clone(), &names_here));
        }
    }
}

fn view_id_salt(kind: SideKind) -> &'static str {
    match kind {
        SideKind::Local => "local",
        SideKind::Remote => "remote",
    }
}

/// A transfer of `items` (named `names`) from side `from` into `target`; asks first when a name
/// is taken there.
fn transfer_action(from: SideKind, items: Vec<String>, names: &[String], target: String, taken_there: &BTreeSet<String>) -> Action {
    let kind = if from == SideKind::Local { TransferKind::Upload } else { TransferKind::Download };
    let transfer = PendingTransfer { kind, items, target };
    let taken: Vec<String> = names.iter().filter(|n| taken_there.contains(*n)).cloned().collect();
    if taken.is_empty() { Action::Transfer(transfer) } else { Action::Ask(Dialog::Conflict { transfer, taken }) }
}

fn column_header(ui: &mut Ui, side: &mut Side, kind: SideKind) {
    let tokens = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let columns = columns(rect, kind);
    for (sort, label, area) in
        [(SortBy::Name, tr("Name"), columns.name), (SortBy::Size, tr("Size"), columns.size), (SortBy::Modified, tr("Modified"), columns.modified)]
    {
        if area.width() < 30.0 {
            continue;
        }
        let response = ui.interact(area, ui.scope_id().with(("sftp-sort", label)), Sense::click());
        let on = side.sort == sort;
        let arrow = if on { if side.descending { " ▼" } else { " ▲" } } else { "" };
        let color = if on || response.hovered() { tokens.text_strong } else { tokens.text_dim };
        // Sizes stand right-aligned, so their heading does too.
        let (at, align) = if sort == SortBy::Size {
            (pos2(area.right() - 8.0, area.center().y), egui::Align2::RIGHT_CENTER)
        } else {
            (pos2(area.left(), area.center().y), egui::Align2::LEFT_CENTER)
        };
        ui.painter().text(at, align, format!("{label}{arrow}"), FontId::proportional(12.0), color);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if response.clicked() {
            if on {
                side.descending = !side.descending;
            } else {
                side.sort = sort;
                side.descending = false;
            }
        }
    }
    if let Some(area) = columns.permissions {
        ui.painter().text(
            pos2(area.left(), area.center().y),
            egui::Align2::LEFT_CENTER,
            tr("Permissions"),
            FontId::proportional(12.0),
            tokens.text_dim,
        );
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, tokens.separator));
}

struct Columns {
    icon: Rect,
    name: Rect,
    size: Rect,
    modified: Rect,
    permissions: Option<Rect>,
}

/// Where the columns of a row go; narrow rows drop the date, then the size.
fn columns(rect: Rect, kind: SideKind) -> Columns {
    let w = rect.width();
    let perm_w = if kind == SideKind::Remote && w > 520.0 { 64.0 } else { 0.0 };
    let date_w = if w > 380.0 { 118.0 } else { 0.0 };
    let size_w = if w > 240.0 { 70.0 } else { 0.0 };
    let icon = Rect::from_min_size(rect.min, vec2(26.0, rect.height()));
    let right = rect.right();
    let permissions = (perm_w > 0.0).then(|| Rect::from_min_max(pos2(right - perm_w, rect.top()), pos2(right, rect.bottom())));
    let modified = Rect::from_min_max(pos2(right - perm_w - date_w, rect.top()), pos2(right - perm_w, rect.bottom()));
    let size = Rect::from_min_max(pos2(modified.left() - size_w, rect.top()), pos2(modified.left(), rect.bottom()));
    let name = Rect::from_min_max(pos2(icon.right() + 4.0, rect.top()), pos2((size.left() - 8.0).max(icon.right() + 4.0), rect.bottom()));
    Columns { icon, name, size, modified, permissions }
}

fn entry_row(ui: &mut Ui, entry: &Entry, selected: bool, kind: SideKind) -> egui::Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
    let fill = if selected {
        tokens.row_selected
    } else if response.hovered() {
        tokens.row_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, f32::from(tokens.radius), fill);
    let c = columns(rect, kind);
    let icon = if entry.is_dir() { Icon::Folder } else { Icon::File };
    let icon_color = if entry.is_dir() { tokens.accent } else { tokens.text_dim };
    draw(ui.painter(), icon, Rect::from_center_size(c.icon.center(), vec2(16.0, 16.0)), icon_color);
    let name = visible(&entry.name);
    // Cut with "…" to the column (a phone leaves little room); the whole name is in the tooltip.
    let galley = one_line(ui, &entry.name, c.name.width(), tokens.text, FontId::proportional(14.0));
    let cut = galley.elided;
    ui.painter().galley(pos2(c.name.left(), c.name.center().y - galley.size().y / 2.0), galley, tokens.text);
    if c.size.width() > 0.0 && !entry.is_dir() {
        ui.painter().text(
            pos2(c.size.right() - 8.0, c.size.center().y),
            egui::Align2::RIGHT_CENTER,
            human_size(entry.size),
            FontId::proportional(12.0),
            tokens.text_dim,
        );
    }
    if c.modified.width() > 0.0
        && let Some(when) = entry.modified.and_then(date)
    {
        ui.painter().text(
            pos2(c.modified.left(), c.modified.center().y),
            egui::Align2::LEFT_CENTER,
            when,
            FontId::proportional(12.0),
            tokens.text_dim,
        );
    }
    if let (Some(area), Some(mode)) = (c.permissions, entry.permissions) {
        ui.painter().text(
            pos2(area.left(), area.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{mode:04o}"),
            FontId::monospace(12.0),
            tokens.text_dim,
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    if cut { response.on_hover_text(name) } else { response }
}

/// `bytes` in the largest unit that keeps a number of at least 1.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// Seconds since 1970 as a local date and time.
fn date(seconds: u64) -> Option<String> {
    let utc = chrono::DateTime::from_timestamp(i64::try_from(seconds).ok()?, 0)?;
    Some(utc.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
}

fn transfer_strip(ui: &mut Ui, transfers: &[Value], actions: &mut Vec<Action>) {
    let tokens = Tokens::get(ui.ctx());
    egui::Frame::new().fill(tokens.panel).stroke(Stroke::new(1.0, tokens.separator)).corner_radius(tokens.radius_card).inner_margin(10.0).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                crate::widgets::buttons::heading(ui, tr("Transfers"), 14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ended = transfers.iter().any(|t| !matches!(t["state"]["kind"].as_str(), Some("queued" | "running")));
                    if ended && ghost(ui, tr("Clear finished"), 28.0).clicked() {
                        actions.push(Action::ClearTransfers);
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("sftp-transfers").max_height(4.0 * ROW_TRANSFER).show(ui, |ui| {
                for t in transfers.iter().rev() {
                    transfer_row(ui, t, actions);
                }
            });
        },
    );
}

fn transfer_row(ui: &mut Ui, t: &Value, actions: &mut Vec<Action>) {
    let tokens = Tokens::get(ui.ctx());
    let id = t["id"].as_u64().unwrap_or_default();
    let state = t["state"]["kind"].as_str().unwrap_or_default();
    let done = t["bytes_done"].as_u64().unwrap_or(0);
    let total = t["bytes_total"].as_u64().unwrap_or(0);
    let icon = match t["direction"].as_str() {
        Some("upload") => Icon::Upload,
        Some("copy") => Icon::Forward,
        _ => Icon::Save,
    };
    let label = visible(t["label"].as_str().unwrap_or_default());
    let (text, color) = match state {
        "queued" => (tr("waiting").to_string(), tokens.text_dim),
        "running" => (format!("{} / {}", human_size(done), human_size(total)), tokens.text),
        "done" => {
            let skipped = t["skipped"].as_u64().unwrap_or(0);
            let errors = t["errors"].as_array().map_or(0, Vec::len);
            if errors > 0 {
                (trf("done, {count} failed", &[("count", &errors.to_string())]), tokens.warning)
            } else if skipped > 0 {
                (trf("done, {count} skipped", &[("count", &skipped.to_string())]), tokens.text_dim)
            } else {
                (tr("done").to_string(), tokens.success)
            }
        }
        "cancelled" => (tr("cancelled").to_string(), tokens.text_dim),
        _ => (visible(t["state"]["detail"].as_str().unwrap_or(tr("failed"))), tokens.danger),
    };
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        draw(ui.painter(), icon, Rect::from_min_size(ui.cursor().min + vec2(0.0, 10.0), vec2(18.0, 18.0)), tokens.text_dim);
        ui.add_space(26.0);
        ui.vertical(|ui| {
            let width = (ui.available_width() - 48.0).max(60.0);
            ui.horizontal(|ui| {
                crate::widgets::elided_label(ui, &label, width * 0.6, tokens.text, FontId::proportional(13.0));
                ui.colored_label(color, text);
            });
            let (bar, _) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
            ui.painter().rect_filled(bar, 2.0, tokens.field);
            let share = if total == 0 { if state == "done" { 1.0 } else { 0.0 } } else { (done as f32 / total as f32).clamp(0.0, 1.0) };
            ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height())), 2.0, tokens.accent);
            // Why a file failed: the first reason, the rest on hover.
            if let Some(errors) = t["errors"].as_array().filter(|e| !e.is_empty()) {
                let first = visible(errors[0].as_str().unwrap_or_default());
                let all: Vec<String> = errors.iter().filter_map(Value::as_str).map(visible).collect();
                crate::widgets::elided_label(ui, &first, width, tokens.warning, FontId::proportional(12.0)).on_hover_text(all.join("\n"));
            }
        });
        let unfinished = matches!(state, "failed" | "cancelled") || (state == "done" && t["errors"].as_array().is_some_and(|e| !e.is_empty()));
        if matches!(state, "queued" | "running") {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if square(ui, Icon::Close, tr("Cancel"), 28.0, false).clicked() {
                    actions.push(Action::Cancel(id));
                }
            });
        } else if unfinished {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ghost(ui, tr("Try again"), 28.0).clicked() {
                    actions.push(Action::Retry(id));
                }
            });
        }
    });
}

// ---- Dialogs ---------------------------------------------------------------------------------

fn dialog(ctx: &egui::Context, view: &mut SftpView, others: &[OtherPane], actions: &mut Vec<Action>) {
    let Some(current) = view.dialog.clone() else { return };
    let remote_dir = view.remote.path.clone();
    let mut close = false;
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sftp-dialog").with(view.id)).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 420.0));
        match current {
            Dialog::NewFolder { mut name } => {
                crate::widgets::buttons::title(ui, tr("New folder"));
                ui.add_space(8.0);
                let field = ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("Name")).desired_width(f32::INFINITY));
                field.request_focus();
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(12.0);
                let (cancel, go) = buttons(ui, tr("Create"), !name.trim().is_empty());
                if (go || enter) && !name.trim().is_empty() {
                    actions.push(Action::Run("sftp.mkdir", json!({ "id": view.id, "dir": remote_dir, "name": name.trim() })));
                    close = true;
                } else if cancel {
                    close = true;
                } else {
                    view.dialog = Some(Dialog::NewFolder { name });
                }
            }
            Dialog::Rename { path, mut name } => {
                crate::widgets::buttons::title(ui, tr("Rename"));
                ui.add_space(8.0);
                let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(12.0);
                let (cancel, go) = buttons(ui, tr("Rename"), !name.trim().is_empty());
                if (go || enter) && !name.trim().is_empty() {
                    actions.push(Action::Run("sftp.rename", json!({ "id": view.id, "path": path, "name": name.trim() })));
                    close = true;
                } else if cancel {
                    close = true;
                } else {
                    view.dialog = Some(Dialog::Rename { path, name });
                }
            }
            Dialog::Permissions { path, mut mode } => {
                crate::widgets::buttons::title(ui, tr("Permissions"));
                ui.add_space(4.0);
                ui.colored_label(Tokens::get(ui.ctx()).text_dim, visible(&path));
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(&mut mode).hint_text("644").font(FontId::monospace(15.0)).desired_width(120.0));
                let parsed = u32::from_str_radix(mode.trim(), 8).ok().filter(|m| *m <= 0o7777);
                if parsed.is_none() && !mode.trim().is_empty() {
                    ui.colored_label(Tokens::get(ui.ctx()).danger, tr("Octal digits, at most 7777."));
                }
                ui.add_space(12.0);
                let (cancel, go) = buttons(ui, tr("Change"), parsed.is_some());
                if go && let Some(m) = parsed {
                    actions.push(Action::Run("sftp.chmod", json!({ "id": view.id, "path": path, "mode": m })));
                    close = true;
                } else if cancel {
                    close = true;
                } else {
                    view.dialog = Some(Dialog::Permissions { path, mode });
                }
            }
            Dialog::Delete { paths, count } => {
                crate::widgets::buttons::title(ui, tr("Delete for good?"));
                ui.add_space(8.0);
                let what = match &count {
                    Some(c) => trf(
                        "{files} files and {dirs} folders on the server are deleted. This cannot be undone.",
                        &[("files", &c["files"].as_u64().unwrap_or(0).to_string()), ("dirs", &c["dirs"].as_u64().unwrap_or(0).to_string())],
                    ),
                    None => tr("Counting what goes…").to_string(),
                };
                ui.label(what);
                ui.add_space(12.0);
                let (cancel, go) = buttons(ui, tr("Delete"), count.is_some());
                if go {
                    actions.push(Action::Run("sftp.delete", json!({ "id": view.id, "paths": paths })));
                    close = true;
                } else if cancel {
                    close = true;
                }
            }
            Dialog::CopyTo { items, names } => {
                crate::widgets::buttons::title(ui, tr("Copy to…"));
                ui.add_space(4.0);
                ui.colored_label(Tokens::get(ui.ctx()).text_dim, tr("Into the folder another open host's files show:"));
                ui.add_space(8.0);
                let targets: Vec<&OtherPane> = others.iter().filter(|o| o.id != view.id).collect();
                if targets.is_empty() {
                    ui.label(tr("Open the files of another host first."));
                }
                for other in targets {
                    // A deep folder would push the button past the dialog; its end says most.
                    let label = format!("{}: {}", visible(&other.name), short_path(&visible(&other.path), 48));
                    if ghost(ui, &label, BUTTON).clicked() {
                        let transfer =
                            PendingTransfer { kind: TransferKind::Copy { to: other.id }, items: items.clone(), target: other.path.clone() };
                        let taken: Vec<String> = names.iter().filter(|n| other.names.contains(*n)).cloned().collect();
                        actions.push(if taken.is_empty() { Action::Transfer(transfer) } else { Action::Ask(Dialog::Conflict { transfer, taken }) });
                        close = true;
                    }
                }
                ui.add_space(12.0);
                if ghost(ui, tr("Cancel"), BUTTON).clicked() {
                    close = true;
                }
            }
            Dialog::Conflict { transfer, taken } => {
                crate::widgets::buttons::title(ui, tr("Already there"));
                ui.add_space(8.0);
                let names: Vec<String> = taken.iter().take(5).map(|n| visible(n)).collect();
                let mut text = trf("In the target folder: {names}", &[("names", &names.join(", "))]);
                if taken.len() > 5 {
                    text.push_str(&trf(" and {count} more", &[("count", &(taken.len() - 5).to_string())]));
                }
                ui.label(text);
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (label, conflict) in [(tr("Skip"), "skip"), (tr("Keep both"), "rename"), (tr("Replace"), "overwrite")] {
                        if ghost(ui, label, BUTTON).clicked() {
                            actions.push(Action::Run(transfer.command(), transfer_params(view.id, &transfer, conflict)));
                            close = true;
                        }
                    }
                    if ghost(ui, tr("Cancel"), BUTTON).clicked() {
                        close = true;
                    }
                });
            }
        }
    });
    if close || modal.should_close() {
        view.dialog = None;
    }
}

/// `path` at most `max` characters long: a longer one keeps its end, after "…".
fn short_path(path: &str, max: usize) -> String {
    let count = path.chars().count();
    if count <= max {
        return path.to_owned();
    }
    let tail: String = path.chars().skip(count - max.saturating_sub(1)).collect();
    format!("…{tail}")
}

/// Cancel and the action, as the pane's own buttons; the action only when `ready`.
fn buttons(ui: &mut Ui, action: &str, ready: bool) -> (bool, bool) {
    let mut out = (false, false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        out.0 = ghost(ui, tr("Cancel"), BUTTON).clicked();
        out.1 = ui.add_enabled_ui(ready, |ui| primary(ui, None, action, BUTTON)).inner.clicked() && ready;
    });
    out
}

fn transfer_params(id: u64, t: &PendingTransfer, conflict: &str) -> Value {
    match t.kind {
        TransferKind::Upload => json!({ "id": id, "local": t.items, "remote_dir": t.target, "conflict": conflict }),
        TransferKind::Download => json!({ "id": id, "remote": t.items, "local_dir": t.target, "conflict": conflict }),
        TransferKind::Copy { to } => json!({ "from": id, "remote": t.items, "to": to, "remote_dir": t.target, "conflict": conflict }),
    }
}

// ---- Acting ----------------------------------------------------------------------------------

fn act(app: &mut SshcraftApp, id: u64, action: Action) {
    match action {
        Action::Open(side, path) => list(app, id, side, path),
        Action::Up(side) => {
            let path = app.sftp.get_mut(&id).map(|v| v.side(side).path.clone()).unwrap_or_default();
            if let Some(up) = parent(side, &path) {
                list(app, id, side, up);
            }
        }
        Action::Refresh(side) => {
            let path = app.sftp.get_mut(&id).map(|v| v.side(side).path.clone()).unwrap_or_default();
            list(app, id, side, path);
        }
        Action::Ask(dialog) => {
            if let Some(view) = app.sftp.get_mut(&id) {
                view.dialog = Some(dialog);
            }
        }
        Action::Count(paths) => app.runner.run(
            "sftp.count",
            json!({ "id": id, "paths": paths }),
            Box::new(move |app, result| {
                let Some(view) = app.sftp.get_mut(&id) else { return };
                match result {
                    Ok(count) => {
                        if let Some(Dialog::Delete { count: slot, .. }) = &mut view.dialog {
                            *slot = Some(count);
                        }
                    }
                    Err(e) => {
                        view.dialog = None;
                        app.toasts.error(&error_text(&e));
                    }
                }
            }),
        ),
        Action::Transfer(t) => run_and_refresh(app, id, t.command(), transfer_params(id, &t, "skip")),
        Action::Run(command, params) => run_and_refresh(app, id, command, params),
        Action::Cancel(transfer) => app.run_reported("sftp.cancel", json!({ "transfer": transfer })),
        Action::PickUpload => {
            let dir = app.sftp.get(&id).map(|v| v.remote.path.clone()).unwrap_or_default();
            let dialog = crate::file_dialogs::FileDialog::open().title(tr("Upload"));
            app.dialogs.ask(dialog, crate::pages::import_export::DialogFor::SftpUpload { id, dir });
        }
        Action::PickDownload { remote, name } => {
            let dialog = crate::file_dialogs::FileDialog::save().title(tr("Download")).file_name(visible(&name));
            app.dialogs.ask(dialog, crate::pages::import_export::DialogFor::SftpDownload { id, remote });
        }
        Action::Retry(transfer) => app.runner.run(
            "sftp.retry",
            json!({ "transfer": transfer }),
            Box::new(move |app, result| {
                if let Err(e) = result {
                    if let Some(view) = app.sftp.get_mut(&id) {
                        view.lost |= lost_connection(&e);
                    }
                    app.toasts.error(&error_text(&e));
                }
            }),
        ),
        Action::Reconnect => app.runner.run(
            "sftp.reconnect",
            json!({ "id": id }),
            Box::new(move |app, result| match result {
                Ok(_) => {
                    let Some(view) = app.sftp.get_mut(&id) else { return };
                    view.lost = false;
                    let (remote, local) = (view.remote.path.clone(), view.local.path.clone());
                    list(app, id, SideKind::Remote, remote);
                    if LOCAL_SIDE {
                        list(app, id, SideKind::Local, local);
                    }
                }
                Err(e) => app.toasts.error(&error_text(&e)),
            }),
        ),
        Action::ClearTransfers => {
            app.transfers.retain(|_, t| matches!(t["state"]["kind"].as_str(), Some("queued" | "running")));
            app.run_reported("sftp.clear_transfers", json!({}));
        }
    }
}

/// Runs `command`; on success the remote side is read again (a transfer refreshes when it ends).
fn run_and_refresh(app: &mut SshcraftApp, id: u64, command: &'static str, params: Value) {
    app.runner.run(
        command,
        params,
        Box::new(move |app, result| match result {
            Ok(_) => {
                if !matches!(command, "sftp.upload" | "sftp.download" | "sftp.copy") {
                    let path = app.sftp.get(&id).map(|v| v.remote.path.clone()).unwrap_or_default();
                    list(app, id, SideKind::Remote, path);
                }
            }
            Err(e) => app.toasts.error(&error_text(&e)),
        }),
    );
}

/// A transfer's news from the engine; when one ends, the side it wrote to is read again.
pub fn on_transfer(app: &mut SshcraftApp, view: Value) {
    let Some(id) = view["id"].as_u64() else { return };
    let ended = !matches!(view["state"]["kind"].as_str(), Some("queued" | "running"));
    let was_open = app.transfers.get(&id).is_some_and(|t| matches!(t["state"]["kind"].as_str(), Some("queued" | "running")));
    let sftp = view["sftp"].as_u64();
    // An upload and a copy wrote to the remote side of pane `sftp`, a download to its local side.
    let upload = view["direction"] != "download";
    // A job that ended because the connection went away: the pane offers to connect again.
    if view["state"]["kind"] == "failed"
        && matches!(view["state"]["code"].as_str(), Some("network" | "timeout"))
        && let Some(pane) = sftp.and_then(|s| app.sftp.get_mut(&s))
    {
        pane.lost = true;
    }
    app.transfers.insert(id, view);
    // At most this many kept for the strip.
    while app.transfers.len() > 50 {
        let Some(oldest) = app.transfers.keys().next().copied() else { break };
        app.transfers.remove(&oldest);
    }
    if ended
        && was_open
        && let Some(sftp) = sftp
    {
        let side = if upload { SideKind::Remote } else { SideKind::Local };
        // Without a local side (Android) a download has nothing here to read again.
        if (side == SideKind::Remote || LOCAL_SIDE)
            && let Some(path) = app.sftp.get_mut(&sftp).map(|v| v.side(side).path.clone())
        {
            list(app, sftp, side, path);
        }
    }
}

/// The picker handed over the file to upload (Android): it goes into `dir` under the name the
/// picker showed, beside anything of that name.
pub fn picked_upload(app: &mut SshcraftApp, id: u64, dir: String, path: std::path::PathBuf) {
    #[cfg(target_os = "android")]
    let name = app.dialogs.picked_name(&path);
    #[cfg(not(target_os = "android"))]
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let Some(name) = name else {
        app.toasts.error(tr("The picker gave no name for the file."));
        return;
    };
    let params = json!({ "id": id, "local": [path], "remote_dir": dir, "conflict": "rename", "name": name });
    run_and_refresh(app, id, "sftp.upload", params);
}

/// The picker made the file to download into (Android).
pub fn picked_download(app: &mut SshcraftApp, id: u64, remote: String, path: std::path::PathBuf) {
    run_and_refresh(app, id, "sftp.download_file", json!({ "id": id, "remote": remote, "local_file": path }));
}

/// The transfers the app knows, by id.
pub type Known = BTreeMap<u64, Value>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Codes from signing in read as sentences, never as the code's own name.
    #[test]
    fn sign_in_failures_read_as_sentences() {
        use sshcraft_engine::{EngineError, ErrorCode};
        for code in [ErrorCode::PasswordRejected, ErrorCode::SignInFailed, ErrorCode::SignInCancelled, ErrorCode::Timeout] {
            let text = error_text(&EngineError::Failed { code, detail: None });
            assert!(text.ends_with('.') && !text.contains(&format!("{code:?}")), "{code:?}: {text}");
        }
    }

    /// A long path keeps its end, where the folder's own name is.
    #[test]
    fn a_long_path_is_cut_at_its_start() {
        assert_eq!(short_path("/srv/www", 40), "/srv/www");
        let long = format!("/{}/ziel", "a".repeat(100));
        let short = short_path(&long, 20);
        assert_eq!(short.chars().count(), 20);
        assert!(short.starts_with('…') && short.ends_with("/ziel"));
    }

    /// A copy reads from this pane's session and writes into the other pane's folder.
    #[test]
    fn a_copy_goes_from_this_pane_into_the_other() {
        let t = PendingTransfer { kind: TransferKind::Copy { to: 7 }, items: vec!["/a".into()], target: "/in".into() };
        assert_eq!(t.command(), "sftp.copy");
        assert_eq!(transfer_params(3, &t, "skip"), json!({ "from": 3, "remote": ["/a"], "to": 7, "remote_dir": "/in", "conflict": "skip" }));
    }

    fn entry(name: &str, kind: &str, size: u64) -> Entry {
        Entry { name: name.into(), kind: kind.into(), size, modified: None, permissions: None }
    }

    #[test]
    fn folders_come_first_and_hidden_files_stay_hidden() {
        let side = Side {
            entries: vec![entry("b.txt", "file", 1), entry(".env", "file", 1), entry("zeta", "dir", 0), entry("A.md", "file", 9)],
            ..Side::default()
        };
        let names: Vec<&str> = side.shown().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zeta", "A.md", "b.txt"]);
        let side = Side { show_hidden: true, filter: "ENV".into(), ..side };
        assert_eq!(side.shown().len(), 1);
    }

    #[test]
    fn parents_of_remote_and_local_paths() {
        assert_eq!(parent(SideKind::Remote, "/home/me").as_deref(), Some("/home"));
        assert_eq!(parent(SideKind::Remote, "/home").as_deref(), Some("/"));
        assert_eq!(parent(SideKind::Remote, "/"), None);
        assert_eq!(child_path(SideKind::Remote, "/", "a"), "/a");
        assert_eq!(child_path(SideKind::Remote, "/x", "a"), "/x/a");
    }

    #[test]
    fn sizes_read_like_people_say_them() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn a_taken_name_asks_first() {
        let taken: BTreeSet<String> = ["a".to_string()].into();
        assert!(matches!(
            transfer_action(SideKind::Local, vec!["/l/a".into()], &["a".into()], "/r".into(), &taken),
            Action::Ask(Dialog::Conflict { .. })
        ));
        assert!(matches!(transfer_action(SideKind::Local, vec!["/l/b".into()], &["b".into()], "/r".into(), &taken), Action::Transfer(_)));
    }
}
