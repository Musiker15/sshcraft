//! Copy and paste in the desktop app reach the system's clipboard. Without egui-winit's
//! `clipboard` feature egui keeps a clipboard of its own: Ctrl+C, Ctrl+V and the context menu
//! then only work inside SSHCraft, and nothing copied elsewhere can be pasted into it.

use std::process::Command;

use serde_json::Value;

/// The features cargo resolves for `package` in the desktop app's build for `target`.
fn features(package: &str, target: &str) -> Vec<String> {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--filter-platform", target, "--manifest-path", manifest])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == package)
        .and_then(|p| p["id"].as_str())
        .unwrap_or_else(|| panic!("{package} is not in the build"))
        .to_owned();
    let node = metadata["resolve"]["nodes"].as_array().unwrap().iter().find(|n| n["id"] == id.as_str()).unwrap();
    node["features"].as_array().unwrap().iter().filter_map(|f| f.as_str().map(str::to_owned)).collect()
}

#[test]
fn the_desktop_app_uses_the_system_clipboard() {
    for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd", "aarch64-apple-darwin"]
    {
        let found = features("egui-winit", target);
        assert!(found.iter().any(|f| f == "clipboard"), "{target}: egui-winit without clipboard: {found:?}");
    }
    // On Wayland egui-winit reaches the clipboard only with its wayland feature; arboard alone
    // would need XWayland.
    for target in ["x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd"] {
        let found = features("egui-winit", target);
        assert!(found.iter().any(|f| f == "wayland"), "{target}: egui-winit without wayland: {found:?}");
    }
}
