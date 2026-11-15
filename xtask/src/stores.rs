//! `cargo xtask stores <SHA256SUMS.txt> [out-dir]`: the winget manifests and the Homebrew cask of
//! a release, filled in from the release's checksum file.
//!
//! The templates live in `packaging/winget/` and `packaging/homebrew/`. `{{version}}` is the
//! workspace version; `{{sha256:<file>}}` is that file's line in `SHA256SUMS.txt`. A template
//! naming a file the checksums do not list is an error, so a manifest never points at an artifact
//! the release does not have. Default output: `dist/stores/`. Submitting the results to the
//! stores is the maintainer's step (docs/releasing.md, "Stores").

use std::collections::BTreeMap;
use std::path::Path;

/// Template path (from the workspace root) and the output path below the out-dir.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("packaging/winget/MSKScripts.SSHCraft.yaml", "winget/MSKScripts.SSHCraft.yaml"),
    ("packaging/winget/MSKScripts.SSHCraft.installer.yaml", "winget/MSKScripts.SSHCraft.installer.yaml"),
    ("packaging/winget/MSKScripts.SSHCraft.locale.en-US.yaml", "winget/MSKScripts.SSHCraft.locale.en-US.yaml"),
    ("packaging/homebrew/sshcraft.rb", "homebrew/sshcraft.rb"),
];

/// The largest checksum file read; a real one has a few dozen lines.
const MAX_SUMS: u64 = 1 << 20;

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let (sums_path, out) = match args {
        [sums] => (root.join(sums), root.join("dist").join("stores")),
        [sums, out] => (root.join(sums), root.join(out)),
        _ => return Err("usage: cargo xtask stores <SHA256SUMS.txt> [out-dir]".into()),
    };
    let size = std::fs::metadata(&sums_path).map_err(|e| format!("{}: {e}", sums_path.display()))?.len();
    if size > MAX_SUMS {
        return Err(format!("{}: larger than {MAX_SUMS} bytes", sums_path.display()));
    }
    let sums = parse_sums(&std::fs::read_to_string(&sums_path).map_err(|e| format!("{}: {e}", sums_path.display()))?)?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    crate::version::validate(&version)?;
    for (template, target) in TEMPLATES {
        let text = std::fs::read_to_string(root.join(template)).map_err(|e| format!("{template}: {e}"))?;
        let filled = render(&text, &version, &sums).map_err(|e| format!("{template}: {e}"))?;
        let path = out.join(target);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, filled).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// `sha256sum` output: `<64 hex>  <file>` or `<64 hex> *<file>`, one per line.
pub fn parse_sums(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut sums = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let (hash, file) = line.split_once(' ').ok_or_else(|| format!("line {}: not `<hash>  <file>`", n + 1))?;
        let file = file.trim_start_matches(' ').trim_start_matches('*');
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("line {}: not a SHA-256 hash", n + 1));
        }
        if file.is_empty() || file.contains(['/', '\\']) {
            return Err(format!("line {}: not a plain file name", n + 1));
        }
        if sums.insert(file.to_string(), hash.to_ascii_lowercase()).is_some() {
            return Err(format!("line {}: {file} is listed twice", n + 1));
        }
    }
    Ok(sums)
}

/// `template` with `{{version}}` and then every `{{sha256:<file>}}` replaced.
pub fn render(template: &str, version: &str, sums: &BTreeMap<String, String>) -> Result<String, String> {
    let text = template.replace("{{version}}", version);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("an unclosed {{")?;
        let key = &after[..end];
        let file = key.strip_prefix("sha256:").ok_or_else(|| format!("unknown placeholder {{{{{key}}}}}"))?;
        let hash = sums.get(file).ok_or_else(|| format!("{file} is not in the checksums"))?;
        out.push_str(hash);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn sums() -> BTreeMap<String, String> {
        parse_sums(&format!("{A}  sshcraft-1.0.0-windows-x64.msi\n{B} *sshcraft-1.0.0-macos-universal.dmg\n")).unwrap()
    }

    #[test]
    fn fills_version_and_checksums() {
        let t = "v{{version}} {{sha256:sshcraft-{{version}}-windows-x64.msi}} {{sha256:sshcraft-{{version}}-macos-universal.dmg}}";
        assert_eq!(render(t, "1.0.0", &sums()).unwrap(), format!("v1.0.0 {A} {B}"));
    }

    #[test]
    fn a_file_missing_from_the_checksums_is_an_error() {
        let err = render("{{sha256:sshcraft-{{version}}-windows-arm64.msi}}", "1.0.0", &sums()).unwrap_err();
        assert!(err.contains("sshcraft-1.0.0-windows-arm64.msi"), "{err}");
    }

    #[test]
    fn an_unknown_placeholder_is_an_error() {
        assert!(render("{{name}}", "1.0.0", &sums()).is_err());
        assert!(render("{{sha256:x", "1.0.0", &sums()).is_err());
    }

    #[test]
    fn checksum_lines_must_be_well_formed() {
        assert!(parse_sums("abc  file.msi").is_err(), "short hash");
        assert!(parse_sums(&format!("{A}  ../file.msi")).is_err(), "path");
        assert!(parse_sums(&format!("{A}  a.msi\n{B}  a.msi")).is_err(), "twice");
        assert!(parse_sums("no-space-here").is_err());
        assert_eq!(parse_sums(&format!("{}  a.msi\r\n\n", A.to_uppercase())).unwrap()["a.msi"], A);
    }

    /// Every template renders with checksums for the artifacts the release workflow builds, and
    /// asks for no other file.
    #[test]
    fn the_shipped_templates_render() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let v = "1.0.0";
        let files = ["windows-x64.msi", "windows-x86.msi", "windows-arm64.msi", "macos-universal.dmg"];
        let text: String = files.iter().map(|f| format!("{A}  sshcraft-{v}-{f}\n")).collect();
        let sums = parse_sums(&text).unwrap();
        for (template, _) in TEMPLATES {
            let t = std::fs::read_to_string(root.join(template)).unwrap();
            let out = render(&t, v, &sums).unwrap();
            assert!(!out.contains("{{"), "{template}");
            assert!(out.contains(v), "{template}");
        }
    }
}
