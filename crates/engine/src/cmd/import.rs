//! Import of hosts, host keys and key files from other programs, and the export. A command that
//! needs a file and gets no path asks the user through a dialog; a program gets an error.

use std::path::{Path, PathBuf};

use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sshcraft_import::export::{ExportFilter, export_csv, export_json};
use sshcraft_import::import::keyfiles;
use sshcraft_import::import::knownhosts;
use sshcraft_import::import::{ImportCandidate, ImportOutcome, ImportPlan, MAX_FILE, apply_import, csv, mobaxterm, putty, securecrt};
use sshcraft_import::sshconfig;
use sshcraft_model::model::ServerId;
use sshcraft_store::store::{ItemKind, Store};

use super::{CommandError, CommandSpec, command, dialog_or_error, needs_vault, parse, query, sync_run, to_value};
use crate::error::ErrorCode;
use crate::sync::{note_change, now_ms};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("import.defaults", "Where import sources usually are", "none", run_defaults),
        query(
            "import.preview",
            "Preview an import",
            "{ source: { kind: ssh_config } | { kind: known_hosts|csv|putty_reg|moba_xterm, path } | { kind: putty_registry } | { kind: secure_crt, dir } }",
            run_preview,
        ),
        command("import.apply", "Import hosts", "{ candidates, location?: server id }", run_apply),
        command("import.known_hosts", "Import host keys", "{ path? }; without path the user is asked", run_known_hosts),
        query("import.scan_keys", "Find key files", "{ dir? }; without dir the user is asked", run_scan_keys),
        CommandSpec {
            enabled: needs_vault,
            ..command("import.key_file", "Import a key file", "{ path?, passphrase? }; without path the user is asked", run_key_file)
        },
        query("import.ssh_config_preview", "Preview ~/.ssh/config", "none", run_ssh_config_preview),
        command("import.ssh_config_apply", "Import from ~/.ssh/config", "{ candidates }", run_ssh_config_apply),
        command("host.export", "Export hosts", "{ format: csv|json, filter, path? }; without path the user is asked", run_export),
    ]
}

/// Where to import from.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportSource {
    SshConfig,
    KnownHosts { path: String },
    Csv { path: String },
    PuttyRegistry,
    PuttyReg { path: String },
    MobaXterm { path: String },
    SecureCrt { dir: String },
}

/// Where each source usually is on this computer, to fill in the dialog.
#[derive(Debug, Clone, Serialize)]
pub struct ImportDefaults {
    known_hosts: Option<String>,
    ssh_dir: Option<String>,
    mobaxterm: Option<String>,
    securecrt: Option<String>,
    /// PuTTY sessions can be read straight from the registry.
    putty_registry: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    plan: ImportPlan,
    /// For `known_hosts`: how many host keys the file holds.
    host_keys: Option<usize>,
}

#[derive(Serialize)]
pub struct SshConfigPreview {
    candidates: Vec<ImportCandidate>,
    skipped: Vec<String>,
}

pub(crate) fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

/// Reads an import file of at most `limit` bytes.
pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, CommandError> {
    let meta = std::fs::metadata(path).map_err(|_| CommandError::code(ErrorCode::ImportUnreadable))?;
    if meta.len() > limit {
        return Err(CommandError::code(ErrorCode::ImportTooLarge));
    }
    std::fs::read(path).map_err(|_| CommandError::code(ErrorCode::ImportUnreadable))
}

fn read_text(path: &str) -> Result<String, CommandError> {
    let bytes = read_limited(Path::new(path), MAX_FILE)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<String> {
    paths.into_iter().find(|p| p.exists()).map(|p| p.to_string_lossy().into_owned())
}

fn defaults() -> ImportDefaults {
    let home = home();
    let in_home = |rel: &[&str]| {
        home.as_ref().map(|h| {
            let mut path = h.clone();
            path.extend(rel);
            path
        })
    };
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    ImportDefaults {
        known_hosts: first_existing(in_home(&[".ssh", "known_hosts"])),
        ssh_dir: first_existing(in_home(&[".ssh"])),
        mobaxterm: first_existing(
            [in_home(&["Documents", "MobaXterm", "MobaXterm.ini"]), app_data.as_ref().map(|a| a.join("MobaXterm").join("MobaXterm.ini"))]
                .into_iter()
                .flatten(),
        ),
        securecrt: first_existing(
            [
                app_data.as_ref().map(|a| a.join("VanDyke").join("Config").join("Sessions")),
                in_home(&["Library", "Application Support", "VanDyke", "SecureCRT", "Config", "Sessions"]),
                in_home(&[".vandyke", "SecureCRT", "Config", "Sessions"]),
            ]
            .into_iter()
            .flatten(),
        ),
        putty_registry: cfg!(windows),
    }
}

fn run_defaults(_: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&defaults()))
}

/// PuTTY's sessions straight from the Windows registry.
#[cfg(windows)]
fn putty_registry_sessions() -> Vec<putty::Session> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, REG_DWORD, REG_SZ};
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(putty::SESSIONS_KEY) else {
        return Vec::new();
    };
    root.enum_keys()
        .filter_map(Result::ok)
        .filter_map(|name| {
            let key = root.open_subkey(&name).ok()?;
            let values = key
                .enum_values()
                .filter_map(Result::ok)
                .filter_map(|(value_name, value)| {
                    let parsed = match value.vtype {
                        REG_SZ => putty::PuttyValue::Text(
                            String::from_utf16_lossy(&value.bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
                                .trim_end_matches('\0')
                                .to_owned(),
                        ),
                        REG_DWORD => {
                            let bytes: [u8; 4] = value.bytes.get(..4)?.try_into().ok()?;
                            putty::PuttyValue::Number(u32::from_le_bytes(bytes))
                        }
                        _ => return None,
                    };
                    Some((value_name, parsed))
                })
                .collect();
            Some(putty::Session { name: putty::decode_name(&name), values })
        })
        .collect()
}

#[cfg(not(windows))]
fn putty_registry_sessions() -> Vec<putty::Session> {
    Vec::new()
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home().map_or_else(|| PathBuf::from(path), |h| h.join(rest)),
        None => PathBuf::from(path),
    }
}

/// SHA256 fingerprint of the public key that belongs to an `IdentityFile` entry.
fn fingerprint_of(identity_file: &str) -> Option<String> {
    let path = expand(identity_file);
    let public = if path.extension().is_some_and(|e| e == "pub") {
        path
    } else {
        let mut with_pub = path.into_os_string();
        with_pub.push(".pub");
        PathBuf::from(with_pub)
    };
    let text = std::fs::read_to_string(public).ok()?;
    let key = russh::keys::PublicKey::from_openssh(text.trim()).ok()?;
    Some(key.fingerprint(russh::keys::HashAlg::Sha256).to_string())
}

async fn ssh_config_preview(session: &Session) -> Result<SshConfigPreview, CommandError> {
    let path = home().map(|h| h.join(".ssh").join("config")).ok_or_else(|| CommandError::failed(ErrorCode::Io, "no home directory"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(CommandError::failed(ErrorCode::Io, e)),
    };
    let parsed = sshconfig::parse(&text);
    // Fingerprints the agent holds, `None` if it cannot be reached or asked.
    let agent: Option<Vec<String>> = session.sessions().agent_keys().await.ok().map(|keys| keys.into_iter().map(|k| k.fingerprint).collect());
    Ok(SshConfigPreview { candidates: sshconfig::plan_import(&parsed.hosts, &fingerprint_of, agent.as_deref()), skipped: parsed.skipped })
}

#[derive(Deserialize)]
struct PreviewParams {
    source: ImportSource,
}

fn run_preview(ctx: Ctx, params: Value) -> Out {
    async move {
        let PreviewParams { source } = parse("import.preview", params)?;
        let mut host_keys = None;
        let plan = match source {
            ImportSource::SshConfig => {
                let preview = ssh_config_preview(&ctx.session).await?;
                ImportPlan { candidates: preview.candidates, skipped: preview.skipped }
            }
            ImportSource::KnownHosts { path } => {
                let parsed = knownhosts::parse(&read_text(&path)?);
                host_keys = Some(parsed.entries.len());
                ImportPlan { candidates: parsed.candidates, skipped: parsed.skipped }
            }
            ImportSource::Csv { path } => csv::parse(&read_text(&path)?),
            ImportSource::PuttyRegistry => putty::candidates(&putty_registry_sessions()),
            ImportSource::PuttyReg { path } => putty::candidates(&putty::parse_reg(&read_limited(Path::new(&path), MAX_FILE)?)),
            ImportSource::MobaXterm { path } => mobaxterm::parse(&read_limited(Path::new(&path), MAX_FILE)?),
            ImportSource::SecureCrt { dir } => securecrt::parse_dir(Path::new(&dir)),
        };
        to_value(&ImportPreview { plan, host_keys })
    }
    .boxed()
}

/// Creates the confirmed candidates at `location` and queues what was created for that
/// server. Returns the outcome and the servers to nudge.
pub fn apply_and_queue(
    store: &Store,
    candidates: &[ImportCandidate],
    location: Option<ServerId>,
) -> Result<(ImportOutcome, Vec<ServerId>), CommandError> {
    let outcome = apply_import(store, candidates, location)?;
    let mut servers = Vec::new();
    let now = now_ms();
    for group in &outcome.groups {
        servers.extend(note_change(store, ItemKind::Group, &group.0.to_string(), None, now)?);
    }
    for host in &outcome.hosts {
        servers.extend(note_change(store, ItemKind::Host, &host.0.to_string(), None, now)?);
    }
    servers.sort_by_key(|s| s.0);
    servers.dedup();
    Ok((outcome, servers))
}

#[derive(Deserialize)]
struct ApplyParams {
    candidates: Vec<ImportCandidate>,
    #[serde(default)]
    location: Option<ServerId>,
}

fn run_apply(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ApplyParams { candidates, location } = parse("import.apply", params)?;
        let (outcome, servers) = apply_and_queue(&ctx.session.store(), &candidates, location)?;
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        to_value(&outcome)
    })
}

#[derive(Deserialize)]
struct PathParams {
    #[serde(default)]
    path: Option<String>,
}

/// Takes over the host keys of a `known_hosts` file; differing known keys stay.
fn run_known_hosts(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let PathParams { path } = parse("import.known_hosts", params.clone())?;
        let Some(path) = path else {
            return dialog_or_error(&ctx, "importKnownHosts", &params);
        };
        let parsed = knownhosts::parse(&read_text(&path)?);
        to_value(&knownhosts::apply(&ctx.session.store(), &parsed.entries)?)
    })
}

#[derive(Deserialize)]
struct DirParams {
    #[serde(default)]
    dir: Option<String>,
}

fn run_scan_keys(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let DirParams { dir } = parse("import.scan_keys", params.clone())?;
        let Some(dir) = dir else {
            return dialog_or_error(&ctx, "scanKeys", &params);
        };
        to_value(&keyfiles::scan(Path::new(&dir)))
    })
}

#[derive(Deserialize)]
struct KeyFileParams {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    passphrase: Option<zeroize::Zeroizing<String>>,
}

fn run_key_file(ctx: Ctx, params: Value) -> Out {
    super::blocking_run("import.key_file", move || {
        let KeyFileParams { path, passphrase } = parse("import.key_file", params.clone())?;
        let Some(path) = path else {
            return dialog_or_error(&ctx, "importKeyFile", &json!({}));
        };
        let vault = ctx.session.vault()?;
        let (info, servers) = {
            let store = ctx.session.store();
            let had_certificate: std::collections::HashSet<_> =
                store.list_key_records()?.into_iter().filter(|r| r.certificate.is_some()).map(|r| r.id).collect();
            let info = keyfiles::import(&vault, &store, Path::new(&path), passphrase.as_deref().map(String::as_str))?;
            // A certificate next to a key that is already synced goes to the other devices too,
            // unless the key came in a share (then it stays here).
            let servers = match info.location {
                Some(server)
                    if info.certificate.is_some()
                        && !had_certificate.contains(&info.id)
                        && crate::sync::share_rights_for_key(&store, server, info.id).is_ok() =>
                {
                    super::key::key_changed(&store, info.id)?
                }
                _ => Vec::new(),
            };
            (info, servers)
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        to_value(&info)
    })
}

fn run_ssh_config_preview(ctx: Ctx, _: Value) -> Out {
    async move { to_value(&ssh_config_preview(&ctx.session).await?) }.boxed()
}

#[derive(Deserialize)]
struct SshConfigApplyParams {
    candidates: Vec<ImportCandidate>,
}

/// Creates exactly the candidates the user confirmed in the preview, nothing re-read.
fn run_ssh_config_apply(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SshConfigApplyParams { candidates } = parse("import.ssh_config_apply", params)?;
        let created = apply_import(&ctx.session.store(), &candidates, None)?.created;
        ctx.session.announce(&[]);
        Ok(json!({ "created": created }))
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Deserialize)]
struct ExportParams {
    format: ExportFormat,
    filter: ExportFilter,
    #[serde(default)]
    path: Option<PathBuf>,
}

/// Writes the export (it holds no secrets) to `path`; without one the user picks the file.
fn run_export(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ExportParams { format, filter, path } = parse("host.export", params.clone())?;
        let Some(path) = path else {
            return dialog_or_error(&ctx, "exportHosts", &params);
        };
        let text = {
            let store = ctx.session.store();
            match format {
                ExportFormat::Csv => export_csv(&store, &filter)?,
                ExportFormat::Json => export_json(&store, &filter)?,
            }
        };
        std::fs::write(&path, text).map_err(|e| CommandError::failed(ErrorCode::Io, e))?;
        Ok(json!({ "path": path }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_model::model::ConnectionSettings;
    use sshcraft_store::store::ServerRow;
    use uuid::Uuid;

    fn store_with_server() -> (Store, ServerId) {
        let store = Store::open_in_memory().unwrap();
        let id = ServerId::new();
        store
            .add_server(&ServerRow {
                id,
                name: "home".into(),
                origin: "https://sync.example.org".into(),
                account: [1; 32],
                device_id: Uuid::new_v4(),
                sealed_device: vec![0],
                sync_enabled: true,
                sync_keys: false,
                revoked: false,
                log_head: None,
                tracker: None,
                created_at: 1,
            })
            .unwrap();
        (store, id)
    }

    fn candidate(name: &str, path: &[&str]) -> ImportCandidate {
        ImportCandidate {
            name: name.into(),
            group_path: path.iter().map(|s| s.to_string()).collect(),
            settings: ConnectionSettings { address: Some(format!("{name}.example.org")), ..Default::default() },
            ..ImportCandidate::default()
        }
    }

    #[test]
    fn import_at_a_server_is_queued() {
        let (store, server) = store_with_server();
        let (outcome, servers) = apply_and_queue(&store, &[candidate("a", &["G", "H"]), candidate("b", &[])], Some(server)).unwrap();
        assert_eq!(outcome.created, 2);
        assert_eq!(servers, vec![server]);
        let outbox = store.outbox(server).unwrap();
        assert_eq!(outbox.iter().filter(|(k, _)| *k == ItemKind::Group).count(), 2);
        assert_eq!(outbox.iter().filter(|(k, _)| *k == ItemKind::Host).count(), 2);
    }

    #[test]
    fn local_import_is_not_queued() {
        let (store, server) = store_with_server();
        let (_, servers) = apply_and_queue(&store, &[candidate("a", &["G"])], None).unwrap();
        assert!(servers.is_empty());
        assert!(store.outbox(server).unwrap().is_empty());
    }

    #[test]
    fn too_large_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.csv");
        std::fs::write(&path, "0123456789").unwrap();
        assert_eq!(read_limited(&path, 4).unwrap_err().failure(), ErrorCode::ImportTooLarge);
        assert!(read_limited(&path, 100).is_ok());
        assert_eq!(read_limited(&dir.path().join("missing"), 100).unwrap_err().failure(), ErrorCode::ImportUnreadable);
    }
}
