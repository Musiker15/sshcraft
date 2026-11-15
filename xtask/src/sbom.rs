//! `cargo xtask sbom [out.json]`: a CycloneDX 1.6 software bill of materials for a release.
//!
//! It lists every crate that goes into the shipped programs (`sshcraft`, `sshcraft-cli`,
//! `sshcraft-server`) on any platform: the normal and build dependencies reached from them in
//! `cargo metadata`, never dev-dependencies. `cargo metadata` unifies features across the whole
//! workspace, so a crate a test enables through a feature would look shipped; only crates that
//! `cargo tree -e normal,build` also lists, which resolves features as a release build does, are
//! kept. Each component carries its version, its licence
//! expression and a package URL; git dependencies also name their repository and commit. Default
//! output: `dist/release/sshcraft-<version>.cdx.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// The packages whose binaries a release ships.
pub const ROOTS: &[&str] = &["sshcraft", "sshcraft-cli", "sshcraft-server"];

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let out = match args {
        [] => root.join("dist").join("release").join(format!("sshcraft-{version}.cdx.json")),
        [path] => root.join(path),
        _ => return Err("usage: cargo xtask sbom [out.json]".into()),
    };
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let meta: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    let mut tree = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    tree.args(["tree", "--locked", "-e", "normal,build", "--target", "all", "--prefix", "none", "-f", "{p}"]);
    for name in ROOTS {
        tree.args(["-p", name]);
    }
    let output = tree.current_dir(root).output().map_err(|e| format!("cargo tree: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo tree failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let shipped = parse_tree(&String::from_utf8_lossy(&output.stdout));
    let bom = build(&meta, ROOTS, &version, &shipped)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&bom).map_err(|e| e.to_string())?;
    std::fs::write(&out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
    let count = bom.get("components").and_then(Value::as_array).map_or(0, Vec::len);
    println!("{} ({count} components)", out.display());
    Ok(())
}

/// `(name, version)` of every line of `cargo tree --prefix none -f "{p}"` (`name vX.Y.Z ...`).
pub fn parse_tree(text: &str) -> BTreeSet<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let version = words.next()?.strip_prefix('v')?;
            Some((name.to_string(), version.to_string()))
        })
        .collect()
}

struct Package<'a> {
    name: &'a str,
    version: &'a str,
    license: Option<&'a str>,
    source: Option<&'a str>,
    workspace: bool,
}

/// The bill of materials for `roots` out of `cargo metadata` output, keeping only the packages in
/// `shipped` (see the module comment).
pub fn build(meta: &Value, roots: &[&str], version: &str, shipped: &BTreeSet<(String, String)>) -> Result<Value, String> {
    let members: BTreeSet<&str> = meta.get("workspace_members").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut packages = BTreeMap::new();
    for p in meta.get("packages").and_then(Value::as_array).ok_or("cargo metadata has no packages")? {
        let id = p.get("id").and_then(Value::as_str).ok_or("a package without id")?;
        packages.insert(
            id,
            Package {
                name: p.get("name").and_then(Value::as_str).ok_or("a package without name")?,
                version: p.get("version").and_then(Value::as_str).ok_or("a package without version")?,
                license: p.get("license").and_then(Value::as_str),
                source: p.get("source").and_then(Value::as_str),
                workspace: members.contains(id),
            },
        );
    }
    // Shipped edges only: a dependency counts when one of its kinds is normal (null) or build.
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let nodes = meta.get("resolve").and_then(|r| r.get("nodes")).and_then(Value::as_array).ok_or("cargo metadata has no resolve graph")?;
    for node in nodes {
        let id = node.get("id").and_then(Value::as_str).ok_or("a node without id")?;
        let mut out = Vec::new();
        for dep in node.get("deps").and_then(Value::as_array).into_iter().flatten() {
            let shipped = dep
                .get("dep_kinds")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|k| matches!(k.get("kind"), None | Some(Value::Null)) || k.get("kind").and_then(Value::as_str) == Some("build"));
            if shipped && let Some(pkg) = dep.get("pkg").and_then(Value::as_str) {
                out.push(pkg);
            }
        }
        edges.insert(id, out);
    }
    let mut start = Vec::new();
    for name in roots {
        let id = packages.iter().find(|(_, p)| p.workspace && p.name == *name).map(|(id, _)| *id);
        start.push(id.ok_or_else(|| format!("workspace package `{name}` not found"))?);
    }
    let mut reached = BTreeSet::new();
    let mut todo = start.clone();
    while let Some(id) = todo.pop() {
        let in_tree = packages.get(id).is_some_and(|p| shipped.contains(&(p.name.to_string(), p.version.to_string())));
        if in_tree && reached.insert(id) {
            todo.extend(edges.get(id).into_iter().flatten().copied());
        }
    }

    let mut components = Vec::new();
    let mut dependencies = Vec::new();
    for id in &reached {
        let p = packages.get(id).ok_or_else(|| format!("resolved package {id} has no metadata"))?;
        components.push(component(id, p));
        let mut on: Vec<&str> = edges.get(id).into_iter().flatten().copied().filter(|d| reached.contains(d)).collect();
        on.sort_unstable();
        on.dedup();
        dependencies.push(json!({ "ref": id, "dependsOn": on }));
    }
    Ok(json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": "sshcraft-release",
                "name": "SSHCraft",
                "version": version,
                "components": start.iter().map(|id| json!({ "type": "application", "bom-ref": format!("{id}#root"), "name": packages.get(id).map_or("", |p| p.name) })).collect::<Vec<_>>(),
            },
            "tools": { "components": [{ "type": "application", "name": "cargo xtask sbom" }] },
        },
        "components": components,
        "dependencies": dependencies,
    }))
}

fn component(id: &str, p: &Package<'_>) -> Value {
    let mut c = json!({
        "type": if p.workspace { "application" } else { "library" },
        "bom-ref": id,
        "name": p.name,
        "version": p.version,
        "purl": format!("pkg:cargo/{}@{}", p.name, p.version),
    });
    if let Some(obj) = c.as_object_mut() {
        if let Some(license) = p.license {
            obj.insert("licenses".into(), json!([{ "expression": license }]));
        }
        if let Some(src) = p.source.and_then(|s| s.strip_prefix("git+")) {
            obj.insert("externalReferences".into(), json!([{ "type": "vcs", "url": src }]));
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(kind: Option<&str>) -> Value {
        json!([{ "kind": kind, "target": null }])
    }

    /// app -> lib (normal) -> helper (build); app -> testkit (dev only); app -> both (dev and normal).
    fn fixture() -> Value {
        json!({
            "workspace_members": ["app 0.1.0 (path+file:///w)"],
            "packages": [
                { "id": "app 0.1.0 (path+file:///w)", "name": "app", "version": "0.1.0", "license": "MIT OR Apache-2.0", "source": null },
                { "id": "lib 1.2.3 (registry)", "name": "lib", "version": "1.2.3", "license": "MIT", "source": "registry+https://github.com/rust-lang/crates.io-index" },
                { "id": "helper 0.4.0 (registry)", "name": "helper", "version": "0.4.0", "license": "Apache-2.0", "source": "registry+https://github.com/rust-lang/crates.io-index" },
                { "id": "testkit 2.0.0 (registry)", "name": "testkit", "version": "2.0.0", "license": "MIT", "source": "registry+https://github.com/rust-lang/crates.io-index" },
                { "id": "both 0.9.0 (git)", "name": "both", "version": "0.9.0", "license": null, "source": "git+https://github.com/storytold/craft-libs?rev=abc#abc" }
            ],
            "resolve": { "nodes": [
                { "id": "app 0.1.0 (path+file:///w)", "deps": [
                    { "pkg": "lib 1.2.3 (registry)", "dep_kinds": kinds(None) },
                    { "pkg": "testkit 2.0.0 (registry)", "dep_kinds": kinds(Some("dev")) },
                    { "pkg": "both 0.9.0 (git)", "dep_kinds": [{ "kind": "dev", "target": null }, { "kind": null, "target": "cfg(windows)" }] }
                ]},
                { "id": "lib 1.2.3 (registry)", "deps": [{ "pkg": "helper 0.4.0 (registry)", "dep_kinds": kinds(Some("build")) }] },
                { "id": "helper 0.4.0 (registry)", "deps": [] },
                { "id": "testkit 2.0.0 (registry)", "deps": [] },
                { "id": "both 0.9.0 (git)", "deps": [] }
            ]}
        })
    }

    /// What `cargo tree` would list for the fixture: everything but the dev-only crate.
    fn shipped() -> BTreeSet<(String, String)> {
        parse_tree(
            "app v0.1.0 (/w)
lib v1.2.3
helper v0.4.0 (*)
both v0.9.0 (https://github.com/storytold/craft-libs?rev=abc#abc)
",
        )
    }

    fn names(bom: &Value) -> Vec<String> {
        let mut n: Vec<String> = bom["components"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
        n.sort();
        n
    }

    #[test]
    fn lists_shipped_crates_and_leaves_dev_only_ones_out() {
        let bom = build(&fixture(), &["app"], "0.1.0", &shipped()).unwrap();
        assert_eq!(names(&bom), ["app", "both", "helper", "lib"]);
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["specVersion"], "1.6");
        assert_eq!(bom["metadata"]["component"]["version"], "0.1.0");
    }

    #[test]
    fn components_carry_purl_licence_and_repository() {
        let bom = build(&fixture(), &["app"], "0.1.0", &shipped()).unwrap();
        let by_name = |n: &str| bom["components"].as_array().unwrap().iter().find(|c| c["name"] == n).unwrap().clone();
        let lib = by_name("lib");
        assert_eq!(lib["purl"], "pkg:cargo/lib@1.2.3");
        assert_eq!(lib["type"], "library");
        assert_eq!(lib["licenses"][0]["expression"], "MIT");
        let app = by_name("app");
        assert_eq!(app["type"], "application");
        assert_eq!(app["licenses"][0]["expression"], "MIT OR Apache-2.0");
        let both = by_name("both");
        assert!(both.get("licenses").is_none());
        assert_eq!(both["externalReferences"][0]["url"], "https://github.com/storytold/craft-libs?rev=abc#abc");
    }

    #[test]
    fn dependency_graph_names_only_listed_components() {
        let bom = build(&fixture(), &["app"], "0.1.0", &shipped()).unwrap();
        let deps = bom["dependencies"].as_array().unwrap();
        let app = deps.iter().find(|d| d["ref"] == "app 0.1.0 (path+file:///w)").unwrap();
        assert_eq!(app["dependsOn"], json!(["both 0.9.0 (git)", "lib 1.2.3 (registry)"]));
    }

    /// A crate `cargo metadata` reaches through a workspace-wide feature, but a release build does
    /// not, stays out, and so does what only it pulls in.
    #[test]
    fn crates_a_release_build_does_not_resolve_stay_out() {
        let mut only = shipped();
        only.remove(&("lib".to_string(), "1.2.3".to_string()));
        let bom = build(&fixture(), &["app"], "0.1.0", &only).unwrap();
        assert_eq!(names(&bom), ["app", "both"]);
    }

    #[test]
    fn tree_lines_give_name_and_version() {
        let set = parse_tree(
            "tempfile v3.27.0
async-trait v0.1.92 (proc-macro)
x v1.0.0 (https://g/x?rev=1#1) (*)

noversion
",
        );
        assert!(set.contains(&("tempfile".into(), "3.27.0".into())));
        assert!(set.contains(&("async-trait".into(), "0.1.92".into())));
        assert!(set.contains(&("x".into(), "1.0.0".into())));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn an_unknown_root_is_an_error() {
        let err = build(&fixture(), &["missing"], "0.1.0", &shipped()).unwrap_err();
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn a_registry_crate_named_like_a_root_is_not_a_root() {
        let err = build(&fixture(), &["lib"], "0.1.0", &shipped()).unwrap_err();
        assert!(err.contains("lib"), "{err}");
    }
}
