//! `cargo xtask layers`: the dependency layering of the workspace (AGENTS.md, "Layering").
//!
//! A crate may depend only on crates in a lower layer, every workspace crate is in the table,
//! and nothing below L6 depends on a UI toolkit, so the UI crate stays swappable.

use serde_json::Value;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// A layered crate.
    Layer(u8),
    /// Programs and build tooling: nothing may depend on them, and they may depend on anything.
    Exempt,
}

/// The layer table. Names are package names without the `sshcraft-` prefix.
pub const TABLE: &[(&str, Class)] = &[
    ("proto", Class::Layer(0)),
    ("platform", Class::Layer(0)),
    ("model", Class::Layer(1)),
    ("link", Class::Layer(1)),
    ("store", Class::Layer(2)),
    ("term", Class::Layer(2)),
    ("import", Class::Layer(3)),
    ("ssh", Class::Layer(3)),
    ("local", Class::Layer(3)),
    ("telnet", Class::Layer(3)),
    ("sync", Class::Layer(3)),
    ("server-lib", Class::Layer(3)),
    ("fido", Class::Layer(4)),
    ("engine", Class::Layer(5)),
    ("ui-egui", Class::Layer(6)),
    ("mcp", Class::Layer(6)),
    // The sync server program (apps/sshcraft-server).
    ("server", Class::Exempt),
    // The desktop app (apps/sshcraft).
    ("sshcraft", Class::Exempt),
    // The command line and MCP server (apps/sshcraft-cli).
    ("cli", Class::Exempt),
    // The Android app (apps/android).
    ("android", Class::Exempt),
    ("xtask", Class::Exempt),
];

/// The first layer that may use a UI toolkit.
pub const UI_LAYER: u8 = 6;

/// One workspace crate and what it depends on: other workspace crates by short name, external
/// crates by package name. Only normal and build dependencies count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crate {
    pub name: String,
    pub deps: Vec<String>,
}

/// A broken rule: `from` depends on `to` (empty when the crate itself is the problem).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub from: String,
    pub to: String,
    pub why: &'static str,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.to.is_empty() { write!(f, "{}: {}", self.from, self.why) } else { write!(f, "{} -> {}: {}", self.from, self.to, self.why) }
    }
}

/// The package name without the `sshcraft-` prefix.
pub fn short_name(package: &str) -> &str {
    package.strip_prefix("sshcraft-").unwrap_or(package)
}

/// The class of a crate by short name; `None` when it is not in the table.
pub fn classify(name: &str) -> Option<Class> {
    TABLE.iter().find(|(n, _)| *n == name).map(|(_, class)| *class)
}

/// Whether an external crate is a UI toolkit or windowing crate.
pub fn is_ui_toolkit(name: &str) -> bool {
    name.starts_with("egui") || matches!(name, "eframe" | "winit" | "rfd")
}

/// Every broken rule in the given crates.
pub fn check(crates: &[Crate]) -> Vec<Violation> {
    let workspace: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let mut found = Vec::new();
    for krate in crates {
        let layer = match classify(&krate.name) {
            None => {
                found.push(Violation { from: krate.name.clone(), to: String::new(), why: "not in the layer table (xtask/src/layers.rs)" });
                continue;
            }
            Some(Class::Exempt) => continue,
            Some(Class::Layer(layer)) => layer,
        };
        for dep in &krate.deps {
            let why = if workspace.contains(&dep.as_str()) {
                match classify(dep) {
                    Some(Class::Layer(other)) if other >= layer => Some("depends on a crate in the same or a higher layer"),
                    Some(Class::Exempt) => Some("depends on a program or a tool"),
                    // A lower layer is fine; an unregistered crate is reported on its own.
                    _ => None,
                }
            } else if layer < UI_LAYER && is_ui_toolkit(dep) {
                Some("UI toolkits are only allowed from L6 on")
            } else {
                None
            };
            if let Some(why) = why {
                found.push(Violation { from: krate.name.clone(), to: dep.clone(), why });
            }
        }
    }
    found
}

/// The workspace crates and their normal and build dependencies from `cargo metadata`.
pub fn from_metadata(meta: &Value) -> Result<Vec<Crate>, String> {
    let members: Vec<&str> =
        meta["workspace_members"].as_array().ok_or("cargo metadata: no workspace_members")?.iter().filter_map(Value::as_str).collect();
    let packages = meta["packages"].as_array().ok_or("cargo metadata: no packages")?;
    let mut crates = Vec::new();
    for package in packages {
        let Some(id) = package["id"].as_str() else {
            continue;
        };
        if !members.contains(&id) {
            continue;
        }
        let name = package["name"].as_str().ok_or("cargo metadata: a package without a name")?;
        let deps = package["dependencies"]
            .as_array()
            .map(|deps| {
                deps.iter()
                    .filter(|dep| matches!(dep["kind"].as_str(), None | Some("build")))
                    .filter_map(|dep| dep["name"].as_str())
                    .map(|dep| short_name(dep).to_string())
                    .collect()
            })
            .unwrap_or_default();
        crates.push(Crate { name: short_name(name).to_string(), deps });
    }
    Ok(crates)
}

#[cfg(test)]
mod tests {
    use super::{Crate, check, from_metadata};

    fn krate(name: &str, deps: &[&str]) -> Crate {
        Crate { name: name.into(), deps: deps.iter().map(|d| (*d).into()).collect() }
    }

    /// The crates of the table with the dependencies they have today.
    fn current() -> Vec<Crate> {
        vec![
            krate("proto", &["serde", "rand"]),
            krate("platform", &["directories"]),
            krate("model", &["serde", "uuid"]),
            krate("store", &["model", "rusqlite"]),
            krate("import", &["model", "store", "csv"]),
            krate("ssh", &["model", "russh"]),
            krate("local", &["model", "serial2"]),
            krate("telnet", &["model", "tokio"]),
            krate("sync", &["proto", "model", "store", "reqwest"]),
            krate("server-lib", &["proto", "axum"]),
            krate("fido", &["ssh", "windows"]),
            krate("server", &["server-lib", "clap"]),
            krate("xtask", &["serde_json"]),
        ]
    }

    #[test]
    fn engine_is_layer_five() {
        assert_eq!(super::classify("engine"), Some(super::Class::Layer(5)));
    }

    #[test]
    fn upward_dependency_is_a_violation() {
        let mut crates = current();
        crates.push(krate("model", &["store"]));
        let found = check(&crates);
        assert!(found.iter().any(|v| v.from == "model" && v.to == "store"), "{found:?}");
    }

    #[test]
    fn same_layer_dependency_is_a_violation() {
        let mut crates = current();
        crates.push(krate("ssh", &["sync"]));
        let found = check(&crates);
        assert!(found.iter().any(|v| v.from == "ssh" && v.to == "sync"), "{found:?}");
    }

    #[test]
    fn unregistered_crate_is_a_violation() {
        let mut crates = current();
        crates.push(krate("foo", &["model"]));
        let found = check(&crates);
        assert!(found.iter().any(|v| v.from == "foo" && v.to.is_empty()), "{found:?}");
    }

    #[test]
    fn ui_toolkit_below_l6_is_a_violation() {
        let mut crates = current();
        crates.push(krate("store", &["egui"]));
        let found = check(&crates);
        assert!(found.iter().any(|v| v.from == "store" && v.to == "egui"), "{found:?}");
    }

    #[test]
    fn current_table_without_toolkit_has_no_violation() {
        assert_eq!(check(&current()), Vec::new());
    }

    /// Tests may use any crate; only normal and build dependencies are layered.
    #[test]
    fn dev_dependencies_are_ignored() {
        let meta = serde_json::json!({
            "workspace_members": ["path+file:///w/crates/model#sshcraft-model@0.1.0"],
            "packages": [{
                "id": "path+file:///w/crates/model#sshcraft-model@0.1.0",
                "name": "sshcraft-model",
                "dependencies": [
                    { "name": "serde", "kind": null },
                    { "name": "cc", "kind": "build" },
                    { "name": "sshcraft-store", "kind": "dev" }
                ]
            }]
        });
        let crates = from_metadata(&meta).unwrap();
        assert_eq!(crates, vec![krate("model", &["serde", "cc"])]);
    }
}
