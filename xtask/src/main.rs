//! SSHCraft workspace tooling: `cargo xtask <command>`.
//!
//! - `ci`: every gate below in order, with a summary (what CI and every commit run)
//! - `fmt`, `clippy`, `test`: formatting check, clippy with `-D warnings`, the tests
//! - `layers`: the dependency layering (`layers.rs`)
//! - `assets`: every asset attributed, no font files (`assets.rs`)
//! - `deny`: licences, bans, sources and advisories through cargo-deny (`deny.toml`)
//! - `wasm`: the crates that must build for the browser do
//! - `perf`: the performance budgets of the terminal and the window, in a release build
//!   (`crates/term/tests/budgets.rs`, `crates/ui-egui/tests/budgets.rs`)

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod assets;
mod layers;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// The crates `cargo xtask wasm` checks (AGENTS.md: only these build for the browser).
const WASM_PACKAGES: &[&str] = &["sshcraft-proto", "sshcraft-model"];

type Step = fn() -> Result<(), String>;

const CI: &[(&str, Step)] = &[
    ("fmt", cmd_fmt),
    ("clippy", cmd_clippy),
    ("test", cmd_test),
    ("layers", cmd_layers),
    ("assets", cmd_assets),
    ("deny", cmd_deny),
    ("wasm", cmd_wasm),
    ("perf", cmd_perf),
];

fn main() -> ExitCode {
    // Every command works on the whole workspace, wherever it is started from.
    if let Some(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent()
        && let Err(err) = std::env::set_current_dir(root)
    {
        eprintln!("xtask: cannot enter the workspace root {}: {err}", root.display());
        return ExitCode::FAILURE;
    }
    let arg = std::env::args().nth(1);
    let result = match arg.as_deref() {
        Some("ci") => cmd_ci(),
        Some(name) => match CI.iter().find(|(n, _)| *n == name) {
            Some((_, step)) => step(),
            None => Err(format!("unknown command `{name}`\n{}", usage())),
        },
        None => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    let names: Vec<&str> = CI.iter().map(|(n, _)| *n).collect();
    format!("usage: cargo xtask <ci|{}>", names.join("|"))
}

/// The cargo that runs this tool, so `cargo +toolchain xtask` stays on that toolchain.
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: could not start: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed ({status})")) }
}

fn cmd_ci() -> Result<(), String> {
    let mut failed = Vec::new();
    let mut summary = Vec::new();
    for (name, step) in CI {
        eprintln!("\n== xtask {name}");
        match step() {
            Ok(()) => summary.push(format!("ok    {name}")),
            Err(err) => {
                summary.push(format!("FAIL  {name}: {err}"));
                failed.push(*name);
            }
        }
    }
    eprintln!("\n== summary");
    for line in &summary {
        eprintln!("{line}");
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

fn cmd_fmt() -> Result<(), String> {
    let mut c = cargo();
    c.args(["fmt", "--all", "--", "--check"]);
    run(c, "cargo fmt --check")
}

fn cmd_clippy() -> Result<(), String> {
    let mut c = cargo();
    c.args(["clippy", "--workspace", "--all-targets", "--keep-going", "--", "-D", "warnings"]);
    run(c, "cargo clippy")
}

/// The tests through cargo-nextest when it is installed, else `cargo test`; then the doctests,
/// which nextest does not run.
fn cmd_test() -> Result<(), String> {
    let nextest = cargo().args(["nextest", "--version"]).output().is_ok_and(|out| out.status.success());
    let mut c = cargo();
    if nextest {
        c.args(["nextest", "run", "--workspace"]);
        run(c, "cargo nextest run")?;
    } else {
        c.args(["test", "--workspace"]);
        run(c, "cargo test")?;
    }
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --doc")
}

fn cmd_layers() -> Result<(), String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed ({})", out.status));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let crates = layers::from_metadata(&meta)?;
    let violations = layers::check(&crates);
    if violations.is_empty() {
        eprintln!("layers: {} crates, no violation", crates.len());
        return Ok(());
    }
    for v in &violations {
        eprintln!("layers: {v}");
    }
    Err(format!("{} layering violation(s)", violations.len()))
}

fn cmd_assets() -> Result<(), String> {
    let out = Command::new("git").args(["ls-files", "-z"]).output().map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed ({})", out.status));
    }
    let files: Vec<PathBuf> =
        out.stdout.split(|b| *b == 0).filter(|name| !name.is_empty()).map(|name| PathBuf::from(String::from_utf8_lossy(name).into_owned())).collect();
    let attribution = std::fs::read_to_string("ATTRIBUTION.md").map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let problems = assets::check(&files, &attribution);
    if problems.is_empty() {
        eprintln!("assets: {} files checked, every asset attributed", files.len());
        return Ok(());
    }
    for p in &problems {
        eprintln!("assets: {p}");
    }
    Err(format!("{} asset problem(s)", problems.len()))
}

fn cmd_deny() -> Result<(), String> {
    let mut c = cargo();
    c.args(["deny", "check", "licenses", "bans", "sources", "advisories"]);
    run(c, "cargo deny (install it: cargo install cargo-deny --locked)")
}

fn cmd_wasm() -> Result<(), String> {
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown"]);
    for package in WASM_PACKAGES {
        c.args(["-p", package]);
    }
    run(c, "cargo check --target wasm32-unknown-unknown")
}

/// Budgets are about the optimized program; a debug build would measure something else.
fn cmd_perf() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--release", "-p", "sshcraft-term", "--test", "budgets"]);
    run(c, "cargo test --release -p sshcraft-term --test budgets")?;
    let mut c = cargo();
    c.args(["test", "--release", "-p", "sshcraft-ui-egui", "--test", "budgets"]);
    run(c, "cargo test --release -p sshcraft-ui-egui --test budgets")
}
