//! An `ssh://` link given to a second start of SSHCraft reaches the running one through a file
//! in `config_dir()/handoff/`. The running app looks once a second, takes at most
//! [`MAX_FILES`] files of at most [`MAX_BYTES`] each, deletes them, and fills the quick connect
//! dialog with the first valid link. A link never connects by itself.

use std::path::{Path, PathBuf};

use sshcraft_model::ssh_url::{Target, parse_url};

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
    std::fs::create_dir_all(dir)?;
    let name = uuid::Uuid::new_v4().to_string();
    let temp = dir.join(format!("{name}.tmp"));
    std::fs::write(&temp, serde_json::json!({ "link": link }).to_string())?;
    std::fs::rename(&temp, dir.join(format!("{name}.json")))
}

/// Takes the waiting links: each file is deleted, valid or not; only valid links come back.
pub fn take(dir: &Path) -> Vec<Target> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).take(MAX_FILES) {
        let path = entry.path();
        // Only plain files: a link to somewhere else is removed without being read.
        let plain = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
        let text = if plain { std::fs::read_to_string(&path).ok() } else { None };
        let _ = std::fs::remove_file(&path);
        let link = text
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("link").and_then(|l| l.as_str()).map(str::to_string));
        if let Some(target) = link.and_then(|l| parse_url(&l).ok()) {
            out.push(target);
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
    if let Some(target) = take(&dir).into_iter().next()
        && app.ui.quick_connect.is_none()
    {
        crate::quick_connect::open(app, target.name());
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}
