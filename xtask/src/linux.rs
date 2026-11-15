//! `cargo xtask linux`: clippy and the tests on Linux, in a Docker container, from a Windows or
//! macOS machine. The container gets the system packages the CI installs; its registry and target
//! folder live in Docker volumes of their own, so they never mix with the host's build.

use std::process::Command;

/// The Rust image: the toolchain the workspace builds with.
const IMAGE: &str = "rust:1.99";

/// What the CI installs on Ubuntu (gate.yml `packages`), plus libudev for the serial ports.
const PACKAGES: &str = "pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
libgtk-3-dev libfontconfig1-dev libfreetype-dev libudev-dev libdbus-1-dev mesa-vulkan-drivers";

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let script = format!(
        "set -e; apt-get update -qq >/dev/null; DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {PACKAGES} >/dev/null 2>&1; \
         rustup component add clippy >/dev/null 2>&1; \
         cargo clippy --workspace --all-targets --locked -- -D warnings; \
         cargo test --workspace --locked"
    );
    let status = Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/src", root.display()))
        .args([
            "-v",
            "sshcraft-cargo:/usr/local/cargo/registry",
            "-v",
            "sshcraft-target:/target",
            "-e",
            "CARGO_TARGET_DIR=/target",
            "-w",
            "/src",
            IMAGE,
            "bash",
            "-c",
        ])
        .arg(script)
        .status()
        .map_err(|e| format!("docker: could not start ({e}); is Docker running?"))?;
    if status.success() { Ok(()) } else { Err(format!("the Linux run failed ({status})")) }
}
