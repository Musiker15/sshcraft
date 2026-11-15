//! `~/.ssh/config` kept in step: with `integrations.ssh_config_live` on, the hosts of the file
//! (and the files it includes) are hosts in SSHCraft, in a group of their own, and follow the
//! file when it changes. A host that leaves the file stays in SSHCraft; only its link goes
//! (local data stays authoritative). Writing back happens only when the user asks for it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::json;
use sshcraft_import::sshconfig::{self, ParsedConfig, RealFs};
use sshcraft_import::sshconfig_write::{WriteHost, write_host};
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, Proxy};
use sshcraft_store::store::ConfigLink;

use crate::caller::Caller;
use crate::cmd::CommandError;
use crate::error::ErrorCode;
use crate::{Session, Weak};

/// The name of the group the hosts of the file live in.
pub const GROUP_NAME: &str = "~/.ssh/config";

/// The setting row that remembers the group (it may be renamed).
const GROUP_SETTING: &str = "sshconfig.group";

/// Refreshes and write-backs one at a time.
pub(crate) static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How often the files are checked for changes.
const WATCH_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RefreshOutcome {
    pub added: usize,
    pub updated: usize,
    /// Hosts no longer in the file; they stay in SSHCraft.
    pub unlinked: usize,
    /// Lines and files that were not followed.
    pub skipped: Vec<String>,
}

fn failed(e: impl std::fmt::Display) -> CommandError {
    CommandError::failed(ErrorCode::Io, e)
}

fn engine(e: crate::error::EngineError) -> CommandError {
    CommandError::failed(ErrorCode::Internal, e)
}

/// The file `~/.ssh/config` under `home`, with its includes; empty when there is no file.
fn read(home: &Path) -> ParsedConfig {
    let root = home.join(".ssh").join("config");
    if root.exists() { sshconfig::parse_tree(&RealFs, &root, home) } else { ParsedConfig::default() }
}

/// The group of the file's hosts, made when it is not there.
async fn live_group(session: &Session) -> Result<GroupId, CommandError> {
    let remembered = session.store().get_setting(GROUP_SETTING)?.and_then(|id| uuid::Uuid::parse_str(&id).ok()).map(GroupId);
    if let Some(id) = remembered
        && session.store().list_groups()?.iter().any(|g| g.id == id)
    {
        return Ok(id);
    }
    let group = Group {
        id: GroupId(uuid::Uuid::new_v4()),
        name: GROUP_NAME.to_owned(),
        defaults: ConnectionSettings::default(),
        location: None,
        parent_id: None,
        tags: Vec::new(),
    };
    session.execute(Caller::User, "group.save", json!({ "group": group })).await.map_err(engine)?;
    session.store().put_setting(GROUP_SETTING, &group.id.0.to_string())?;
    Ok(group.id)
}

/// Reads the file and brings the linked hosts in step with it.
pub async fn refresh_from(session: &Session, home: &Path) -> Result<(RefreshOutcome, Vec<PathBuf>), CommandError> {
    // One refresh or write-back at a time (the watcher, the command, a write-back).
    let _one = BUSY.lock().await;
    let agent: Option<Vec<String>> = session.sessions().agent_keys().await.ok().map(|keys| keys.into_iter().map(|k| k.fingerprint).collect());
    // Reading files and key files blocks: off the runtime's workers.
    let home_owned = home.to_path_buf();
    let (parsed, candidates) = tokio::task::spawn_blocking(move || {
        let parsed = read(&home_owned);
        let candidates = sshconfig::plan_import(&parsed.hosts, &crate::cmd::import::fingerprint_of, agent.as_deref());
        (parsed, candidates)
    })
    .await
    .map_err(|e| CommandError::failed(ErrorCode::Internal, e))?;
    let root = home.join(".ssh").join("config");
    // A file that is gone or half written says nothing about its hosts: links stay.
    let root_read = parsed.files.first() == Some(&root) && !parsed.hosts.is_empty();
    let mut outcome = RefreshOutcome { skipped: parsed.skipped.clone(), ..RefreshOutcome::default() };
    let links = session.store().config_links()?;
    let hosts = session.store().list_hosts()?;
    let group = if candidates.is_empty() { None } else { Some(live_group(session).await?) };

    // Every alias gets its host first, so jump hosts can point at each other. An alias without
    // a link takes back a host of the same name in the group that lost its link (the entry was
    // commented out for a while), instead of making a second one.
    let mut ids: Vec<(String, HostId, Option<ConfigLink>)> = Vec::new();
    for (candidate, config) in candidates.iter().zip(&parsed.hosts) {
        let link = links.iter().find(|l| l.alias == candidate.name && hosts.iter().any(|h| h.id == l.host_id)).cloned();
        let unlinked_twin = || {
            hosts
                .iter()
                .find(|h| h.name == candidate.name && h.group_id.is_some() && h.group_id == group && !links.iter().any(|l| l.host_id == h.id))
                .map(|h| h.id)
        };
        let id = link.as_ref().map(|l| l.host_id).or_else(unlinked_twin).unwrap_or_else(HostId::new);
        let file = config.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        ids.push((candidate.name.clone(), id, link.map(|l| ConfigLink { file, ..l })));
    }
    for (candidate, (alias, id, link)) in candidates.iter().zip(&ids) {
        let digest = sshcraft_store::store::command_digest(&[serde_json::to_string(candidate).unwrap_or_default()]);
        if link.as_ref().is_some_and(|l| l.digest == digest) {
            continue;
        }
        let jumps: Vec<HostId> =
            candidate.jump_aliases.iter().filter_map(|a| ids.iter().find(|(name, ..)| name == a).map(|(_, id, _)| *id)).collect();
        let mut settings = candidate.settings.clone();
        settings.address = settings.address.or_else(|| candidate.group_defaults.address.clone());
        settings.jump_hosts = (!jumps.is_empty()).then_some(jumps);
        let host = match hosts.iter().find(|h| h.id == *id) {
            Some(existing) => {
                // What the file says replaces the connection values; the rest (password, tags,
                // rules, where it syncs) stays as set in SSHCraft.
                let mut host = existing.clone();
                host.settings.address = settings.address;
                host.settings.port = settings.port;
                host.settings.username = settings.username;
                if settings.key.is_some() {
                    host.settings.key = settings.key;
                }
                host.settings.jump_hosts = settings.jump_hosts;
                host.settings.proxy = settings.proxy;
                host.settings.agent_forwarding = settings.agent_forwarding;
                host.settings.env = settings.env;
                outcome.updated += 1;
                host
            }
            None => {
                outcome.added += 1;
                Host { id: *id, name: alias.clone(), group_id: group, settings, location: None, tags: Vec::new(), os: None, forwards: Vec::new() }
            }
        };
        // Saved as the user: the file is the user's own, its proxy commands are theirs.
        session.execute(Caller::User, "host.save", json!({ "host": host, "approve_command": true })).await.map_err(engine)?;
        let file = link.as_ref().map(|l| l.file.clone()).unwrap_or_else(|| {
            parsed
                .hosts
                .iter()
                .find(|h| h.alias == *alias)
                .and_then(|h| h.file.as_ref())
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        session.store().set_config_link(&ConfigLink { host_id: *id, alias: alias.clone(), file, digest })?;
    }
    for link in links.iter().filter(|l| root_read && !candidates.iter().any(|c| c.name == l.alias)) {
        session.store().remove_config_link(link.host_id)?;
        outcome.unlinked += 1;
    }
    if outcome.unlinked > 0 {
        session.announce(&[]);
    }
    Ok((outcome, parsed.files))
}

/// A host as it goes back into the file: its own values, else the nearest group's. A user
/// name may be missing (`ssh` then takes the local one), unlike when connecting.
fn write_entry(session: &Session, host_id: HostId, links: &[ConfigLink]) -> Result<WriteHost, CommandError> {
    let store = session.store();
    let (hosts, groups) = (store.list_hosts()?, store.list_groups()?);
    drop(store);
    let find = |id: HostId| hosts.iter().find(|h| h.id == id).ok_or_else(|| CommandError::NotFound(format!("host {}", id.0)));
    let host = find(host_id)?;
    let chain = host.group_id.map(|g| sshcraft_model::model::group_chain(&groups, g)).unwrap_or_default();
    let chain_refs: Vec<&Group> = chain.iter().collect();
    // The nearest value: the host's own, then its groups from the nearest up.
    let pick = |get: &dyn Fn(&ConnectionSettings) -> Option<String>| get(&host.settings).or_else(|| chain.iter().find_map(|g| get(&g.defaults)));
    let (address, port) = sshcraft_model::model::effective_endpoint(host, &chain_refs)
        .map_err(|e| CommandError::failed(ErrorCode::from(&e), "the host has no address"))?;
    let user = pick(&|s| s.username.clone());
    // Written when set in SSHCraft, even when it is 22, so a later `Host *` does not take over.
    let explicit_port = host.settings.port.or_else(|| chain.iter().find_map(|g| g.defaults.port));
    let jumps = host.settings.jump_hosts.clone().or_else(|| chain.iter().find_map(|g| g.defaults.jump_hosts.clone())).unwrap_or_default();
    let proxy = host.settings.proxy.clone().or_else(|| chain.iter().find_map(|g| g.defaults.proxy.clone()));
    let agent = host.settings.agent_forwarding.or_else(|| chain.iter().find_map(|g| g.defaults.agent_forwarding));
    let mut jump = Vec::new();
    for id in jumps {
        let hop = find(id)?;
        let alias = match links.iter().find(|l| l.host_id == id) {
            Some(link) => link.alias.clone(),
            None => {
                let hop_chain = hop.group_id.map(|g| sshcraft_model::model::group_chain(&groups, g)).unwrap_or_default();
                let refs: Vec<&Group> = hop_chain.iter().collect();
                let (a, p) = sshcraft_model::model::effective_endpoint(hop, &refs)
                    .map_err(|e| CommandError::failed(ErrorCode::from(&e), "a jump host has no address"))?;
                let a = if a.contains(':') { format!("[{a}]") } else { a };
                let u = hop.settings.username.clone().or_else(|| hop_chain.iter().find_map(|g| g.defaults.username.clone()));
                match u {
                    Some(u) => format!("{u}@{a}:{p}"),
                    None => format!("{a}:{p}"),
                }
            }
        };
        jump.push(alias);
    }
    Ok(WriteHost {
        alias: links.iter().find(|l| l.host_id == host_id).map_or_else(|| host.name.clone(), |l| l.alias.clone()),
        hostname: Some(address),
        port: explicit_port.or((port != sshcraft_model::model::DEFAULT_PORT).then_some(port)),
        user,
        jump,
        proxy_command: match proxy {
            Some(Proxy::Command { argv }) => Some(argv),
            _ => None,
        },
        forward_agent: agent,
    })
}

/// Where the copies of files before a write-back go: a folder of its own, so no `Include` glob
/// (`config.d/*`) reads them.
pub fn backup_dir(home: &Path) -> PathBuf {
    home.join(".ssh").join(".sshcraft-backup")
}

/// Writes `path` in one step: a temporary file next to it (a dot file, which `*` does not
/// match), then renamed over it, keeping its permissions (`~/.ssh/config` must not be writable
/// by others).
fn replace_file(path: &Path, text: &str) -> Result<(), CommandError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.sshcraft-tmp"));
    std::fs::write(&temp, text).map_err(failed)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        failed(e)
    })
}

/// Writes hosts into the file their link names, or into `~/.ssh/config`. Each file changed is
/// copied into `~/.ssh/.sshcraft-backup/` before.
pub fn write_back(session: &Session, host_ids: &[HostId], home: &Path) -> Result<Vec<String>, CommandError> {
    let links = session.store().config_links()?;
    let root = home.join(".ssh").join("config");
    let mut written: Vec<String> = Vec::new();
    for id in host_ids {
        let entry = write_entry(session, *id, &links)?;
        // `ssh` runs a proxy command through a shell, and the next refresh would take it as the
        // user's own: only a command allowed on this device goes into the file.
        if let Some(argv) = &entry.proxy_command
            && !session.store().command_approved(&sshcraft_store::store::command_digest(argv))?
        {
            return Err(CommandError::Denied(format!(
                "the proxy command of {} is not allowed on this device yet; connect once to allow it",
                entry.alias
            )));
        }
        let file = links.iter().find(|l| l.host_id == *id).map(|l| PathBuf::from(&l.file)).filter(|f| f.is_file()).unwrap_or_else(|| root.clone());
        // A linked config (dotfile managers) is written where it points, the link stays.
        let file = std::fs::canonicalize(&file).unwrap_or(file);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(failed(e)),
        };
        let shown = file.to_string_lossy().into_owned();
        if !written.contains(&shown) && !text.is_empty() {
            // One copy per write-back, by time, so the original is never overwritten.
            let dir = backup_dir(home);
            std::fs::create_dir_all(&dir).map_err(failed)?;
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "config".into());
            std::fs::write(dir.join(format!("{name}.{}", crate::now_ms())), &text).map_err(failed)?;
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(failed)?;
        }
        let new = write_host(&text, &entry).map_err(|e| CommandError::bad_params("sshconfig.write", e.to_string()))?;
        replace_file(&file, &new)?;
        // Linked from now on; an empty digest makes the next refresh read it back once.
        session.store().set_config_link(&ConfigLink { host_id: *id, alias: entry.alias, file: shown.clone(), digest: String::new() })?;
        if !written.contains(&shown) {
            written.push(shown);
        }
    }
    Ok(written)
}

/// When any of these files, or the folders they are in, last changed (missing ones count as
/// never). A folder changes when a file is added to it, so a new file in an `Include` glob is
/// seen too.
fn stamp(files: &[PathBuf]) -> Vec<Option<SystemTime>> {
    let mut paths: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    paths.extend(files.iter().filter_map(|f| f.parent()));
    paths.sort();
    paths.dedup();
    paths.iter().map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).collect()
}

/// Checks the files every few seconds while the setting is on and refreshes when one changed.
pub(crate) fn spawn_watch(session: Weak) {
    let Some(runtime) = session.upgrade().map(|s| s.runtime()) else {
        return;
    };
    runtime.spawn(async move {
        let mut last: Option<(Vec<PathBuf>, Vec<Option<SystemTime>>)> = None;
        loop {
            {
                let Some(session) = session.upgrade() else {
                    return;
                };
                let on = session.settings().get().integrations.ssh_config_live;
                match (on, sshcraft_platform::home_dir()) {
                    (true, Some(home)) => {
                        let root = home.join(".ssh").join("config");
                        let files = last.as_ref().map_or_else(|| vec![root.clone()], |(f, _)| f.clone());
                        let changed = last.as_ref().is_none_or(|(f, s)| stamp(f) != *s);
                        if changed && let Ok((_, mut read)) = refresh_from(&session, &home).await {
                            if read.is_empty() {
                                read.push(root);
                            }
                            let stamps = stamp(&read);
                            last = Some((read, stamps));
                        } else if changed {
                            let stamps = stamp(&files);
                            last = Some((files, stamps));
                        }
                    }
                    _ => last = None,
                }
            }
            tokio::time::sleep(WATCH_EVERY).await;
        }
    });
}

#[cfg(test)]
#[path = "ssh_config_tests.rs"]
mod tests;
