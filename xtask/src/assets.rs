//! `cargo xtask assets`: every asset file has a row in `ATTRIBUTION.md`, and no font file is in
//! the repository (fonts live in storytold/craft-fonts).

use std::path::PathBuf;

/// File endings that count as assets.
pub const ASSET_EXTENSIONS: &[&str] = &["png", "svg", "ico", "icns", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "wav", "mp3", "ogg"];

/// File endings of fonts, which never belong in this repository.
pub const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "woff", "woff2"];

/// Every problem with the given repository files against the text of `ATTRIBUTION.md`.
pub fn check(files: &[PathBuf], attribution: &str) -> Vec<String> {
    let listed = listed_paths(attribution);
    let mut problems = Vec::new();
    for file in files {
        let Some(ext) = file.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        let path = file.to_string_lossy().replace('\\', "/");
        if FONT_EXTENSIONS.contains(&ext.as_str()) {
            problems.push(format!("{path}: font files belong in storytold/craft-fonts, not in this repository"));
        } else if ASSET_EXTENSIONS.contains(&ext.as_str()) && !listed.contains(&path) {
            problems.push(format!("{path}: no row in ATTRIBUTION.md"));
        }
    }
    problems
}

/// The paths in backticks in the first cell of every table row.
fn listed_paths(attribution: &str) -> Vec<String> {
    attribution
        .lines()
        .filter_map(|line| line.trim().strip_prefix('|')?.split('|').next())
        .flat_map(|cell| cell.split('`').skip(1).step_by(2).map(str::to_string).collect::<Vec<_>>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::check;
    use std::path::PathBuf;

    const ATTRIBUTION: &str = "\
| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `assets/app-icon/sshcraft.svg` | SSHCraft contributors | original | MIT OR Apache-2.0 | |
";

    #[test]
    fn unlisted_asset_fails() {
        let problems = check(&[PathBuf::from("assets/icons/lock.png")], ATTRIBUTION);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("assets/icons/lock.png"));
    }

    #[test]
    fn listed_asset_passes() {
        let files = [PathBuf::from("assets/app-icon/sshcraft.svg"), PathBuf::from("crates/model/src/lib.rs")];
        assert_eq!(check(&files, ATTRIBUTION), Vec::<String>::new());
    }

    #[test]
    fn font_file_always_fails() {
        let attribution = format!("{ATTRIBUTION}| `assets/fonts/Inter.ttf` | x | y | OFL-1.1 | |\n");
        let problems = check(&[PathBuf::from("assets/fonts/Inter.ttf")], &attribution);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("craft-fonts"));
    }
}
