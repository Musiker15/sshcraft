fn main() {
    ensure_cli_placeholder();
    tauri_build::build()
}

/// The `sshcraft` command ships next to the app (`bundle.externalBin`). A release build puts the
/// real program there first (`scripts/build-cli.mjs`); for every other build an empty stand-in
/// is enough, so tests and `cargo build` do not need it.
fn ensure_cli_placeholder() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::Path::new("binaries").join(format!("sshcraft-{target}{ext}"));
    if !path.exists() {
        let _ = std::fs::create_dir_all("binaries");
        let _ = std::fs::write(&path, b"");
    }
    println!("cargo:rerun-if-changed={}", path.display());
}
