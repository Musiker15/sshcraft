//! A desktop shortcut that opens one host: a `.lnk` file on Windows, a `.desktop` file
//! elsewhere. It starts `sshcraft --open-host <id>`; the id is all it carries, so a shortcut
//! never holds an address or a password.

use std::path::{Path, PathBuf};

use crate::PlatformError;

/// The longest file name, without its extension.
const MAX_NAME: usize = 64;
/// Names Windows keeps for devices.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
    "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A file name from a host name: no path separators, control or reserved characters, no
/// leading or trailing dots and spaces, at most [`MAX_NAME`] characters, never a device name.
pub fn clean_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%' | '$' | '`'))
        .take(MAX_NAME)
        .collect();
    let trimmed = kept.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return "SSHCraft host".into();
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(trimmed.split('.').next().unwrap_or_default())) {
        return format!("{trimmed} (SSHCraft)");
    }
    trimmed.to_string()
}

/// The arguments the shortcut starts SSHCraft with.
pub fn arguments(host_id: &uuid::Uuid) -> [String; 2] {
    ["--open-host".into(), host_id.to_string()]
}

/// One argument as the desktop entry spec wants it in `Exec`: in double quotes, with `"`, `` ` ``,
/// `$` and `\` escaped, and `%` doubled.
fn exec_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `.desktop` file for `host_id`, named `name`, starting `exe`.
pub fn desktop_entry(name: &str, exe: &Path, host_id: &uuid::Uuid) -> String {
    let exe = exe.to_string_lossy();
    let args = arguments(host_id);
    let exec = std::iter::once(exe.as_ref()).chain(args.iter().map(String::as_str)).map(exec_arg).collect::<Vec<_>>().join(" ");
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment=SSHCraft\nExec={exec}\nIcon=de.musiker15.sshcraft\nTerminal=false\n",
        clean_name(name)
    )
}

/// The user's desktop folder.
pub fn desktop_dir() -> Result<PathBuf, PlatformError> {
    directories::UserDirs::new().and_then(|d| d.desktop_dir().map(Path::to_path_buf)).ok_or(PlatformError::NoHome)
}

/// The longest suffix tried for a free file name.
const MAX_SUFFIX: u32 = 99;

/// `base.ext` in `dir`, or `base (2).ext` and so on: a file that is there is never replaced.
fn free_path(dir: &Path, base: &str, ext: &str) -> Result<PathBuf, PlatformError> {
    let first = dir.join(format!("{base}.{ext}"));
    if !first.exists() {
        return Ok(first);
    }
    (2..=MAX_SUFFIX)
        .map(|n| dir.join(format!("{base} ({n}).{ext}")))
        .find(|p| !p.exists())
        .ok_or_else(|| PlatformError::Io(format!("too many shortcuts named {base}")))
}

/// Writes the shortcut for `host_id` into `dir`, under a name no file there has yet.
pub fn create(dir: &Path, name: &str, exe: &Path, host_id: &uuid::Uuid) -> Result<PathBuf, PlatformError> {
    let base = clean_name(name);
    #[cfg(windows)]
    {
        let path = free_path(dir, &base, "lnk")?;
        let mut link = mslnk::ShellLink::new(exe).map_err(|e| PlatformError::Io(e.to_string()))?;
        link.set_arguments(Some(arguments(host_id).join(" ")));
        link.set_name(Some(base.clone()));
        link.create_lnk(&path).map_err(|e| PlatformError::Io(e.to_string()))?;
        Ok(path)
    }
    #[cfg(not(windows))]
    {
        let path = free_path(dir, &base, "desktop")?;
        std::fs::write(&path, desktop_entry(name, exe, host_id)).map_err(|e| PlatformError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A launcher on the desktop must be executable to start.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| PlatformError::Io(e.to_string()))?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("web/prod: \"main\""), "webprod main");
        assert_eq!(clean_name("..\\..\\evil"), "evil");
        assert_eq!(clean_name("a\nb"), "ab");
        assert_eq!(clean_name("  ...  "), "SSHCraft host");
        assert_eq!(clean_name("con"), "con (SSHCraft)");
        assert_eq!(clean_name("COM1.example"), "COM1.example (SSHCraft)");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), MAX_NAME);
        assert_eq!(clean_name("$(rm -rf ~)"), "(rm -rf ~)");
    }

    #[test]
    fn the_desktop_entry_quotes_every_argument() {
        let id = uuid::Uuid::nil();
        let entry = desktop_entry("db \"prod\"", Path::new("/opt/SSH Craft/bin/ssh$craft"), &id);
        assert!(entry.contains("Exec=\"/opt/SSH Craft/bin/ssh\\$craft\" \"--open-host\" \"00000000-0000-0000-0000-000000000000\"\n"), "{entry}");
        assert!(entry.contains("Name=db prod\n"), "{entry}");
        assert_eq!(entry.lines().filter(|l| l.starts_with("Exec=")).count(), 1);
    }

    #[test]
    fn a_shortcut_file_is_written_into_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let exe = std::env::current_exe().unwrap();
        let path = create(dir.path(), "web", &exe, &id).unwrap();
        assert_eq!(path.parent(), Some(dir.path()));
        let bytes = std::fs::read(&path).unwrap();
        let id_text = id.to_string();
        let found = if cfg!(windows) {
            let wide: Vec<u8> = id_text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            bytes.windows(wide.len()).any(|w| w == wide.as_slice()) || bytes.windows(id_text.len()).any(|w| w == id_text.as_bytes())
        } else {
            bytes.windows(id_text.len()).any(|w| w == id_text.as_bytes())
        };
        assert!(found, "the shortcut carries the host id");
    }

    #[test]
    fn a_file_on_the_desktop_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let ext = if cfg!(windows) { "lnk" } else { "desktop" };
        let theirs = dir.path().join(format!("Firefox.{ext}"));
        std::fs::write(&theirs, b"the user's own").unwrap();
        let path = create(dir.path(), "Firefox", &exe, &uuid::Uuid::new_v4()).unwrap();
        assert_eq!(path, dir.path().join(format!("Firefox (2).{ext}")));
        assert_eq!(std::fs::read(&theirs).unwrap(), b"the user's own");
    }
}
