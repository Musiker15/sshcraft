//! Listing a directory for path completion: `ls` over its own exec channel on a server, the file
//! system for a local shell.
//!
//! On a server the command is always the same string, so no shell (POSIX, fish, csh, cmd) ever
//! parses a typed name: the directory goes in on standard input.

/// At most this many names come back.
pub const MAX_NAMES: usize = 500;
/// At most this much `ls` output is read.
pub const MAX_OUTPUT: usize = 64 * 1024;
/// Printed before the listing, so whatever a shell start-up file prints is not taken for names.
pub const MARKER: &str = "SSHCRAFT-LS";

/// Reads the directory from standard input; a leading `~` means the home directory. `-L` gives
/// symlinked directories their `/`.
pub const LIST_COMMAND: &str =
    "sh -c 'IFS= read -r d; case \"$d\" in \"~\"|\"~/\"*) d=\"$HOME${d#?}\";; esac; printf \"%s\\n\" SSHCRAFT-LS; ls -1ApL -- \"$d\"'";

/// What goes to the command's standard input for `dir`; `None` for a name that cannot be one
/// line.
pub fn listing_input(dir: &str) -> Option<String> {
    (!dir.is_empty() && !dir.chars().any(char::is_control)).then(|| format!("{dir}\n"))
}

/// Names from `ls -1ApL` output after the marker that start with `prefix`: directories end in
/// `/`; empty lines, `./`, `../` and names with control characters are left out.
pub fn parse_listing(output: &[u8], prefix: &str) -> Vec<String> {
    let text = String::from_utf8_lossy(output);
    let mut lines = text.split('\n');
    // Everything up to the marker came from start-up files, not from ls.
    if !lines.any(|line| line == MARKER) {
        return Vec::new();
    }
    lines
        .filter(|name| !name.is_empty() && *name != "./" && *name != "../")
        .filter(|name| !name.chars().any(char::is_control))
        .filter(|name| name.starts_with(prefix))
        .take(MAX_NAMES)
        .map(str::to_owned)
        .collect()
}

/// Lists a local directory (absolute, `~` or `~/…`); directories (also through a link) end in
/// `/`. Names with control characters are left out; sorted.
pub fn list_local(dir: &str, prefix: &str) -> Result<Vec<String>, String> {
    let path = if dir == "~" || dir.starts_with("~/") || dir.starts_with("~\\") {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok_or("no home directory")?;
        std::path::PathBuf::from(home).join(dir[1..].trim_start_matches(['/', '\\']))
    } else {
        std::path::PathBuf::from(dir)
    };
    if !path.is_absolute() {
        return Err("not an absolute path".into());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.chars().any(char::is_control) || !name.starts_with(prefix) {
            continue;
        }
        // Follows a link, as `ls -L` does.
        let is_dir = std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir());
        names.push(if is_dir { format!("{name}/") } else { name });
    }
    names.sort();
    names.truncate(MAX_NAMES);
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_never_holds_a_typed_name() {
        assert!(!LIST_COMMAND.contains("/tmp"));
        assert_eq!(listing_input("/srv/it's").as_deref(), Some("/srv/it's\n"));
        assert_eq!(listing_input("/tmp/a\\';{rm,-rf,$HOME};#/").as_deref(), Some("/tmp/a\\';{rm,-rf,$HOME};#/\n"));
        assert_eq!(listing_input("a\nb"), None);
        assert_eq!(listing_input("a\0b"), None);
        assert_eq!(listing_input(""), None);
    }

    fn output(names: &[&str]) -> Vec<u8> {
        let mut out = b"Welcome from .bashrc\n".to_vec();
        out.extend_from_slice(format!("{MARKER}\n").as_bytes());
        for name in names {
            out.extend_from_slice(name.as_bytes());
            out.push(b'\n');
        }
        out
    }

    #[test]
    fn only_names_after_the_marker_count() {
        let names = parse_listing(&output(&["./", "../", "", "bin/", "file.txt"]), "");
        assert_eq!(names, vec!["bin/".to_owned(), "file.txt".to_owned()]);
        assert!(parse_listing(b"no marker here\nbin/\n", "").is_empty());
    }

    #[test]
    fn the_prefix_is_applied_before_the_limit() {
        let many: Vec<String> = (0..2000).map(|i| format!("a{i:04}")).collect();
        let mut names: Vec<&str> = many.iter().map(String::as_str).collect();
        names.push("systemctl");
        let found = parse_listing(&output(&names), "sy");
        assert_eq!(found, vec!["systemctl".to_owned()]);
        assert_eq!(parse_listing(&output(&names), "").len(), MAX_NAMES);
    }

    #[test]
    fn names_with_control_characters_are_left_out() {
        let names = parse_listing(&output(&["foo\r", "ok"]), "");
        assert_eq!(names, vec!["ok".to_owned()]);
    }

    #[test]
    fn local_directories_are_listed_sorted_with_prefix() {
        let dir = std::env::temp_dir().join(format!("sshcraft-list-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("note.txt"), b"x").unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        let all = list_local(&dir.to_string_lossy(), "").unwrap();
        let some = list_local(&dir.to_string_lossy(), "n").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(all, vec!["a.txt".to_owned(), "note.txt".to_owned(), "sub/".to_owned()]);
        assert_eq!(some, vec!["note.txt".to_owned()]);
    }

    #[test]
    fn relative_and_other_users_home_are_refused() {
        assert!(list_local("relative/dir", "").is_err());
        assert!(list_local("", "").is_err());
        assert!(list_local("~other/x", "").is_err());
    }
}
