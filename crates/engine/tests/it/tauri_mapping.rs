//! Every command of the Tauri version has a place: an engine command, the interface, or the
//! terminal stream. Nothing was dropped on the way.

use sshcraft_engine::cmd::{command_specs, find_command};

enum Place {
    Engine(&'static str),
    /// Pure presentation, done by the interface (R4, R5).
    Ui,
    /// Part of the terminal stream (`TerminalStream::ack`).
    Channel,
}

use Place::{Channel, Engine, Ui};

/// The `#[tauri::command]` functions registered at `tauri-final`.
const TAURI: &[(&str, Place)] = &[
    ("get_settings", Engine("settings.get")),
    ("save_settings", Engine("settings.save")),
    ("local_shell_get", Engine("local.default_shell")),
    ("local_shell_set", Engine("local.set_default_shell")),
    ("serial_last", Engine("serial.last")),
    ("serial_remember", Engine("serial.remember")),
    ("list_groups", Engine("group.list")),
    ("save_group", Engine("group.save")),
    ("delete_group", Engine("group.delete")),
    ("list_hosts", Engine("host.list")),
    ("save_host", Engine("host.save")),
    ("delete_host", Engine("host.delete")),
    ("list_recent", Engine("host.recent")),
    ("forget_host_key", Engine("host.forget_key")),
    ("identities_list", Engine("identity.list")),
    ("identity_save", Engine("identity.save")),
    ("identity_delete", Engine("identity.delete")),
    ("password_set", Engine("password.set")),
    ("password_clear", Engine("password.clear")),
    ("vault_status", Engine("vault.status")),
    ("hello_status", Engine("vault.hello")),
    ("list_keys", Engine("key.list")),
    ("import_key", Engine("key.import")),
    ("generate_key", Engine("key.generate")),
    ("delete_key", Engine("key.delete")),
    ("security_key_support", Engine("key.security_support")),
    ("create_security_key", Engine("key.create_security")),
    ("attach_certificate", Engine("key.attach_certificate")),
    ("detach_certificate", Engine("key.detach_certificate")),
    ("remember_passphrase", Engine("key.remember_passphrase")),
    ("protect_key", Engine("key.protect")),
    ("list_agent_keys", Engine("key.agent_list")),
    ("pass_cli_status", Engine("key.passcli_status")),
    ("pass_cli_list_keys", Engine("key.passcli_list")),
    ("import_from_pass_cli", Engine("key.passcli_import")),
    ("install_key", Engine("key.install")),
    ("session_open", Engine("session.open")),
    ("session_write", Engine("session.send")),
    ("session_close", Engine("session.close")),
    ("session_resize", Engine("session.resize")),
    ("session_ack", Channel),
    ("answer_host_key", Engine("prompt.host_key")),
    ("answer_auth", Engine("prompt.auth")),
    ("local_shells", Engine("local.shells")),
    ("serial_ports", Engine("serial.ports")),
    ("session_list_dir", Engine("session.list_dir")),
    ("session_bookmark", Engine("session.bookmark")),
    ("session_recording", Engine("session.recording")),
    ("agent_policy_get", Engine("agent.policy")),
    ("agent_set_hidden", Engine("agent.set_hidden")),
    ("agent_set_level", Engine("agent.set_level")),
    ("agent_log_list", Engine("agent.log")),
    ("agent_log_clear", Engine("agent.log_clear")),
    ("answer_agent", Engine("prompt.agent")),
    ("forward_start", Engine("forward.start")),
    ("forward_stop", Engine("forward.stop")),
    ("forward_statuses", Engine("forward.status")),
    ("workspace_templates", Engine("workspace.list")),
    ("save_workspace_template", Engine("workspace.save")),
    ("delete_workspace_template", Engine("workspace.delete")),
    ("open_link", Engine("link.open")),
    ("snippets_list", Engine("snippet.list")),
    ("snippet_save", Engine("snippet.save")),
    ("snippet_delete", Engine("snippet.delete")),
    ("history_record", Engine("history.record")),
    ("history_list", Engine("history.list")),
    ("history_clear", Engine("history.clear")),
    ("logs_list", Engine("log.list")),
    ("log_read", Engine("log.read")),
    ("log_add_mark", Engine("log.mark_add")),
    ("log_remove_mark", Engine("log.mark_remove")),
    ("logs_delete", Engine("log.delete")),
    ("logs_search", Engine("log.search")),
    ("log_export", Engine("log.export")),
    ("import_defaults", Engine("import.defaults")),
    ("import_preview", Engine("import.preview")),
    ("import_apply", Engine("import.apply")),
    ("import_known_hosts", Engine("import.known_hosts")),
    ("scan_key_files", Engine("import.scan_keys")),
    ("import_key_file", Engine("import.key_file")),
    ("export_hosts", Engine("host.export")),
    ("ssh_config_preview", Engine("import.ssh_config_preview")),
    ("ssh_config_import", Engine("import.ssh_config_apply")),
    ("list_servers", Engine("sync.servers")),
    ("set_server_sync", Engine("sync.set_server")),
    ("set_server_logs", Engine("sync.server_logs")),
    ("create_sync_account", Engine("sync.create_account")),
    ("join_sync_account", Engine("sync.join_account")),
    ("remove_server", Engine("sync.remove_server")),
    ("list_sync_devices", Engine("device.list")),
    ("revoke_sync_device", Engine("device.revoke")),
    ("move_item", Engine("item.move")),
    ("find_recipient", Engine("share.find_recipient")),
    ("share_items", Engine("share.items")),
    ("list_shares", Engine("share.list")),
    ("set_share_grants", Engine("share.set_grants")),
    ("unshare", Engine("share.end")),
    ("item_shares", Engine("share.of_items")),
    ("confirm_recipient", Engine("share.confirm_recipient")),
    ("system_locale", Ui),
    ("account_key_qr", Ui),
];

/// Engine commands with no Tauri command before them: the agent backend's actions and the list
/// of open questions.
const NEW: &[&str] = &["host.info", "host.exec", "session.read", "prompt.list", "prompt.proxy_command"];

#[test]
fn every_tauri_command_has_a_place() {
    assert_eq!(TAURI.len(), 101);
    let mut names: Vec<&str> = TAURI.iter().map(|(name, _)| *name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), TAURI.len(), "a Tauri command is listed twice");
    let mapped: Vec<&str> = TAURI
        .iter()
        .filter_map(|(name, place)| match place {
            Engine(id) => {
                assert!(find_command(id).is_some(), "{name} maps to {id}, which does not exist");
                Some(*id)
            }
            Ui | Channel => None,
        })
        .collect();
    assert_eq!(mapped.len(), 98);
    for spec in command_specs() {
        assert!(mapped.contains(&spec.id) || NEW.contains(&spec.id), "{} has no Tauri command and is not listed as new", spec.id);
    }
    assert_eq!(command_specs().len(), mapped.len() + NEW.len());
}
