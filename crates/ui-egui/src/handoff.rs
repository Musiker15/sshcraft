//! An `ssh://` link or a desktop shortcut's `--open-host` given to a second start of SSHCraft
//! reaches the running one through a file in `config_dir()/handoff/`. The running app looks once
//! a second, takes at most [`MAX_FILES`] files of at most [`MAX_BYTES`] each and deletes them. A
//! link fills the quick connect dialog and never connects by itself; a shortcut opens a host
//! that is saved here, nothing else.

use std::path::{Path, PathBuf};

use sshcraft_model::model::HostId;
use sshcraft_model::ssh_url::{Target, parse_url};

/// What a second start handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handed {
    Link(Target),
    Host(HostId),
    /// A plain second start: the running window comes to the front.
    Reveal,
}

/// Files taken in one look.
pub const MAX_FILES: usize = 8;
/// The largest file read.
pub const MAX_BYTES: u64 = 4096;
/// Seconds between two looks.
pub const EVERY: f64 = 1.0;

/// The handoff folder inside SSHCraft's settings folder.
pub fn dir_in(config_dir: &Path) -> PathBuf {
    config_dir.join("handoff")
}

/// Leaves `link` for the running SSHCraft. Written under a temporary name and renamed, so the
/// reader never sees half a file.
pub fn write(dir: &Path, link: &str) -> std::io::Result<()> {
    write_value(dir, &serde_json::json!({ "link": link }))
}

/// Asks the running SSHCraft to come to the front.
pub fn write_reveal(dir: &Path) -> std::io::Result<()> {
    write_value(dir, &serde_json::json!({ "reveal": true }))
}

/// Leaves the host a desktop shortcut opens.
pub fn write_host(dir: &Path, id: HostId) -> std::io::Result<()> {
    write_value(dir, &serde_json::json!({ "open_host": id }))
}

fn write_value(dir: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = uuid::Uuid::new_v4().to_string();
    let temp = dir.join(format!("{name}.tmp"));
    std::fs::write(&temp, value.to_string())?;
    std::fs::rename(&temp, dir.join(format!("{name}.json")))
}

/// Takes what is waiting: each file is deleted, valid or not; only valid entries come back.
pub fn take(dir: &Path) -> Vec<Handed> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).take(MAX_FILES) {
        let path = entry.path();
        // Only plain files: a link to somewhere else is removed without being read.
        let plain = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
        let text = if plain { std::fs::read_to_string(&path).ok() } else { None };
        let _ = std::fs::remove_file(&path);
        let value = text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let link = value.as_ref().and_then(|v| v.get("link")).and_then(|l| l.as_str()).and_then(|l| parse_url(l).ok());
        let host = value.as_ref().and_then(|v| v.get("open_host")).and_then(|h| serde_json::from_value::<HostId>(h.clone()).ok());
        let reveal = value.as_ref().and_then(|v| v.get("reveal")).and_then(serde_json::Value::as_bool) == Some(true);
        match (link, host, reveal) {
            (Some(target), None, false) => out.push(Handed::Link(target)),
            (None, Some(id), false) => out.push(Handed::Host(id)),
            (None, None, true) => out.push(Handed::Reveal),
            _ => {}
        }
    }
    out
}

/// Looks for links once a second; the first valid one fills the quick connect dialog, unless
/// the dialog is already open (the user's own text is never replaced).
pub fn poll(app: &mut crate::app::SshcraftApp, ctx: &egui::Context) {
    let Some(dir) = app.handoff_dir.clone() else { return };
    let now = ctx.input(|i| i.time);
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(EVERY));
    if now < app.handoff_next {
        return;
    }
    app.handoff_next = now + EVERY;
    match take(&dir).into_iter().next() {
        Some(Handed::Link(target)) => {
            if app.ui.quick_connect.is_none() {
                crate::quick_connect::open(app, target.name());
            }
            crate::dropdown::reveal(app, ctx);
        }
        // Only a host saved here; an id from anywhere else opens nothing.
        Some(Handed::Host(id)) if app.data.hosts.iter().any(|r| r.host.id == id) => {
            app.ui.connect_request = Some(crate::state::ConnectRequest::Host(id));
            crate::dropdown::reveal(app, ctx);
        }
        Some(Handed::Reveal) => crate::dropdown::reveal(app, ctx),
        _ => {}
    }
}
