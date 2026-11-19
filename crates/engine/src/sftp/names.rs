//! Names and paths in SFTP. A name from the server is hostile: before it becomes part of a local
//! path it must be one plain name that cannot climb out of the folder or mean a device. Remote
//! paths are POSIX paths, made canonical by text alone (the server is never asked to resolve
//! `..`, so a link cannot change what a path means between check and use).

/// The longest name, in bytes; what common file systems allow.
pub const MAX_NAME: usize = 255;
/// The longest remote path, in bytes.
pub const MAX_PATH: usize = 4096;

/// Why a name or path was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the name is empty")]
    Empty,
    #[error("`.` and `..` are not names")]
    Dots,
    #[error("the name contains a path separator")]
    Separator,
    #[error("the name contains a control character")]
    Control,
    #[error("the name is longer than {MAX_NAME} bytes")]
    TooLong,
    #[error("the name is reserved on Windows")]
    Reserved,
    #[error("the path is not absolute")]
    NotAbsolute,
    #[error("the path is longer than {MAX_PATH} bytes")]
    PathTooLong,
}

/// Names Windows keeps for devices, with or without an extension (`NUL.txt` is still `NUL`).
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³", "CONIN$", "CONOUT$",
];

/// What every name must be, here and on the server: not empty, not `.` or `..`, no `/`, no NUL
/// or other control character, at most [`MAX_NAME`] bytes.
pub fn check_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::Dots);
    }
    if name.contains('/') {
        return Err(NameError::Separator);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    if name.len() > MAX_NAME {
        return Err(NameError::TooLong);
    }
    Ok(())
}

/// A name from the server that may become a file or folder in a local folder: [`check_name`],
/// no `\`, and on Windows (`windows`) none of its reserved names, no `:` (a second data stream)
/// and no `.` or space at the end (Windows drops them, so two names would meet).
pub fn check_local_name(name: &str, windows: bool) -> Result<(), NameError> {
    check_name(name)?;
    if name.contains('\\') {
        return Err(NameError::Separator);
    }
    if windows {
        if name.contains(':') || name.contains(['<', '>', '"', '|', '?', '*']) {
            return Err(NameError::Reserved);
        }
        if name.ends_with('.') || name.ends_with(' ') {
            return Err(NameError::Reserved);
        }
        let stem = name.split('.').next().unwrap_or(name).trim_end();
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Err(NameError::Reserved);
        }
    }
    Ok(())
}

/// [`check_local_name`] for this computer.
pub fn check_local_name_here(name: &str) -> Result<(), NameError> {
    check_local_name(name, cfg!(windows))
}

/// `path` made canonical: absolute, single `/` between names, no `.`, `..` taken away by text
/// (never above `/`), no `/` at the end. Refuses NUL and paths over [`MAX_PATH`] bytes.
pub fn normalize_remote(path: &str) -> Result<String, NameError> {
    if path.len() > MAX_PATH {
        return Err(NameError::PathTooLong);
    }
    if !path.starts_with('/') {
        return Err(NameError::NotAbsolute);
    }
    if path.contains('\0') {
        return Err(NameError::Control);
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

/// `name` in the folder `dir` (both on the server); `name` must pass [`check_name`].
pub fn remote_join(dir: &str, name: &str) -> Result<String, NameError> {
    check_name(name)?;
    let dir = normalize_remote(dir)?;
    let joined = if dir == "/" { format!("/{name}") } else { format!("{dir}/{name}") };
    if joined.len() > MAX_PATH {
        return Err(NameError::PathTooLong);
    }
    Ok(joined)
}

/// The folder above `path`, `None` for `/`.
pub fn remote_parent(path: &str) -> Option<String> {
    let path = normalize_remote(path).ok()?;
    if path == "/" {
        return None;
    }
    let cut = path.rfind('/').unwrap_or(0);
    Some(if cut == 0 { "/".to_owned() } else { path[..cut].to_owned() })
}

/// The last name of `path`, `None` for `/`.
pub fn remote_name(path: &str) -> Option<String> {
    let path = normalize_remote(path).ok()?;
    path.rsplit('/').next().filter(|n| !n.is_empty()).map(str::to_owned)
}

/// The name a file is written under until it is complete: `.<name>.sshcraft-part`, with `name`
/// cut (at a character boundary) so the whole stays within [`MAX_NAME`].
pub fn part_name(name: &str) -> String {
    const PREFIX: &str = ".";
    const SUFFIX: &str = ".sshcraft-part";
    let room = MAX_NAME - PREFIX.len() - SUFFIX.len();
    let mut end = name.len().min(room);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{PREFIX}{}{SUFFIX}", &name[..end])
}

/// A free name for `name` in a folder that holds `taken`: `name`, else `name (2).ext`, `name
/// (3).ext` and so on; `None` after 999 tries or when no try fits [`MAX_NAME`].
pub fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    if !taken(name) {
        return Some(name.to_owned());
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (2..1000).map(|n| format!("{stem} ({n}){ext}")).find(|candidate| candidate.len() <= MAX_NAME && !taken(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_could_climb_out_or_hide_are_refused() {
        for bad in ["", ".", "..", "a/b", "/", "a\0b", "a\nb", "\u{1b}[31m"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(check_name(&"x".repeat(256)), Err(NameError::TooLong));
        assert!(check_name("ganz normal.txt").is_ok());
        assert!(check_name("...").is_ok(), "three dots are a name");
    }

    #[test]
    fn local_names_follow_the_rules_of_windows_there() {
        assert_eq!(check_local_name("a\\b", false), Err(NameError::Separator));
        for bad in ["CON", "nul.txt", "Com1", "LPT9.log", "a:b", "x.", "x ", "a?b", "CONIN$"] {
            assert_eq!(check_local_name(bad, true), Err(NameError::Reserved), "{bad}");
            assert!(check_local_name(bad, false).is_ok() || bad.contains('\\'), "{bad} is fine elsewhere");
        }
        assert!(check_local_name("console.txt", true).is_ok());
        assert!(check_local_name("Ärger über Größe.md", true).is_ok());
    }

    #[test]
    fn remote_paths_are_made_canonical_by_text() {
        assert_eq!(normalize_remote("/").unwrap(), "/");
        assert_eq!(normalize_remote("/home//me/./docs/").unwrap(), "/home/me/docs");
        assert_eq!(normalize_remote("/a/../../etc").unwrap(), "/etc", "never above /");
        assert_eq!(normalize_remote("relative"), Err(NameError::NotAbsolute));
        assert_eq!(normalize_remote("/a\0b"), Err(NameError::Control));
        assert_eq!(normalize_remote(&format!("/{}", "a".repeat(MAX_PATH))), Err(NameError::PathTooLong));
    }

    #[test]
    fn joining_parents_and_names() {
        assert_eq!(remote_join("/", "a").unwrap(), "/a");
        assert_eq!(remote_join("/home/me/", "b c").unwrap(), "/home/me/b c");
        assert_eq!(remote_join("/home", ".."), Err(NameError::Dots));
        assert_eq!(remote_join("/home", "x/y"), Err(NameError::Separator));
        assert_eq!(remote_parent("/home/me").as_deref(), Some("/home"));
        assert_eq!(remote_parent("/home").as_deref(), Some("/"));
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_name("/home/me/").as_deref(), Some("me"));
        assert_eq!(remote_name("/"), None);
    }

    #[test]
    fn part_names_stay_within_the_limit() {
        assert_eq!(part_name("report.pdf"), ".report.pdf.sshcraft-part");
        let long = "ö".repeat(200);
        let part = part_name(&long);
        assert!(part.len() <= MAX_NAME && check_name(&part).is_ok());
    }

    #[test]
    fn a_free_name_counts_up_before_the_extension() {
        let taken = ["a.txt", "a (2).txt"];
        assert_eq!(free_name("a.txt", |n| taken.contains(&n)).as_deref(), Some("a (3).txt"));
        assert_eq!(free_name("b", |_| false).as_deref(), Some("b"));
        assert_eq!(free_name(".bashrc", |n| n == ".bashrc").as_deref(), Some(".bashrc (2)"));
        assert_eq!(free_name("x", |_| true), None);
    }
}
