//! Listing a directory for path completion: `ls` over its own exec channel on a server, the file
//! system for a local shell.

/// At most this many names come back.
pub const MAX_NAMES: usize = 500;
/// At most this much `ls` output is read.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// The `ls` command for `dir`, safe for any name: POSIX single quotes, with a leading `~` or
/// `~/` left outside so the shell expands it.
pub fn list_dir_command(dir: &str) -> String {
    fn quote(text: &str) -> String {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
    let target = if dir == "~" {
        "~".to_owned()
    } else if let Some(rest) = dir.strip_prefix("~/") {
        format!("~/{}", quote(rest))
    } else {
        quote(dir)
    };
    format!("ls -1Ap -- {target}")
}

/// Names from `ls -1Ap` output: directories end in `/`; empty lines and `./`, `../` are left out.
pub fn parse_listing(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|name| !name.is_empty() && *name != "./" && *name != "../")
        .filter(|name| !name.chars().any(char::is_control))
        .take(MAX_NAMES)
        .map(str::to_owned)
        .collect()
}

/// Lists a local directory (absolute, or starting with `~`); directories end in `/`. Names with
/// control characters are left out.
pub fn list_local(dir: &str) -> Result<Vec<String>, String> {
    let path = match dir.strip_prefix('~') {
        Some(rest) => {
            let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .ok_or("no home directory")?;
            std::path::PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        None => std::path::PathBuf::from(dir),
    };
    if !path.is_absolute() {
        return Err("not an absolute path".into());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.chars().any(char::is_control) {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        names.push(if is_dir { format!("{name}/") } else { name });
        if names.len() >= MAX_NAMES {
            break;
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_quote_every_name() {
        assert_eq!(list_dir_command("/tmp"), "ls -1Ap -- '/tmp'");
        assert_eq!(list_dir_command("/srv/it's"), "ls -1Ap -- '/srv/it'\\''s'");
        assert_eq!(list_dir_command("$(reboot)"), "ls -1Ap -- '$(reboot)'");
        assert_eq!(list_dir_command("~/a b"), "ls -1Ap -- ~/'a b'");
        assert_eq!(list_dir_command("~"), "ls -1Ap -- ~");
        assert_eq!(list_dir_command("~/"), "ls -1Ap -- ~/''");
    }

    #[test]
    fn listings_are_cleaned_and_limited() {
        let mut out = b"./\n../\n\nbin/\nfile.txt\n".to_vec();
        for i in 0..600 {
            out.extend_from_slice(format!("f{i}\n").as_bytes());
        }
        let names = parse_listing(&out);
        assert_eq!(names[..2], ["bin/".to_owned(), "file.txt".to_owned()]);
        assert_eq!(names.len(), MAX_NAMES);
        assert!(
            !names
                .iter()
                .any(|n| n == "./" || n == "../" || n.is_empty())
        );
    }

    #[test]
    fn local_directories_are_listed() {
        let dir = std::env::temp_dir().join(format!("sshcraft-list-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("note.txt"), b"x").unwrap();
        let mut names = list_local(&dir.to_string_lossy()).unwrap();
        names.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names, vec!["note.txt".to_owned(), "sub/".to_owned()]);
    }

    #[test]
    fn relative_local_paths_are_refused() {
        assert!(list_local("relative/dir").is_err());
        assert!(list_local("").is_err());
    }
}
