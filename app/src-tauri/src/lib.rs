pub mod agent_sessions;
pub mod agents;
pub mod commands;
pub mod errors;
pub mod forwards;
pub mod hello;
pub mod links;
pub mod listing;
pub mod passcli;
pub mod prompts;
pub mod recording;
pub mod sessions;
pub mod settings;
pub mod state;
pub mod sync;
pub mod workspaces;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // A second start must not open the database and the vault a second time: it brings the
    // running window to the front and exits.
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    builder
        .setup(|app| {
            let state = state::AppState::init(app.handle())?;
            // Before any tab records: logs a stopped app left open are ended here.
            if let Ok(vault) = state.vault.clone() {
                let store = state.store.clone();
                let days = settings::load(&store.lock().expect("store lock")).log_retention_days;
                let _ = recording::housekeeping(&store, &vault, days, recording::now_ms(), true);
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(recording::tidy_regularly(
                    store,
                    vault,
                    move |servers| {
                        if let Some(state) = handle.try_state::<state::AppState>() {
                            state.sync.nudge(servers);
                        }
                    },
                ));
            }
            #[cfg(desktop)]
            let confirmer = state.agent_confirms.clone();
            app.manage(state);
            // AI agents reach the app through `sshcraft` (MCP and command line).
            #[cfg(desktop)]
            agents::server::start(std::sync::Arc::new(agents::gate::Gate::new(
                std::sync::Arc::new(agents::tauri::TauriBackend {
                    app: app.handle().clone(),
                }),
                confirmer,
            )));
            // Runtimes need the async runtime; start them on it.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<state::AppState>().sync.start_all();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::data::list_groups,
            commands::data::save_group,
            commands::data::delete_group,
            commands::data::list_hosts,
            commands::data::save_host,
            commands::data::delete_host,
            commands::data::forget_host_key,
            commands::data::get_settings,
            commands::agents::agent_policy_get,
            commands::agents::agent_set_hidden,
            commands::agents::agent_set_level,
            commands::agents::agent_log_list,
            commands::agents::agent_log_clear,
            commands::agents::answer_agent,
            commands::logs::logs_list,
            commands::logs::log_read,
            commands::logs::log_add_mark,
            commands::logs::log_remove_mark,
            commands::logs::logs_delete,
            commands::logs::logs_search,
            commands::logs::log_export,
            commands::logs::session_bookmark,
            commands::logs::session_recording,
            commands::sync::set_server_logs,
            commands::snippets::snippets_list,
            commands::snippets::snippet_save,
            commands::snippets::snippet_delete,
            commands::snippets::history_record,
            commands::snippets::history_list,
            commands::snippets::history_clear,
            commands::snippets::session_list_dir,
            commands::forwards::forward_start,
            commands::forwards::forward_stop,
            commands::forwards::forward_statuses,
            commands::workspaces::workspace_templates,
            commands::workspaces::save_workspace_template,
            commands::workspaces::delete_workspace_template,
            commands::workspaces::open_link,
            commands::data::save_settings,
            commands::data::system_locale,
            commands::data::list_recent,
            commands::sshconfig::ssh_config_preview,
            commands::sshconfig::ssh_config_import,
            commands::import::import_defaults,
            commands::import::import_preview,
            commands::import::import_apply,
            commands::import::import_known_hosts,
            commands::import::scan_key_files,
            commands::import::import_key_file,
            commands::import::export_hosts,
            commands::keys::vault_status,
            commands::keys::hello_status,
            commands::keys::list_keys,
            commands::keys::import_key,
            commands::keys::generate_key,
            commands::keys::security_key_support,
            commands::keys::create_security_key,
            commands::keys::attach_certificate,
            commands::keys::detach_certificate,
            commands::keys::remember_passphrase,
            commands::keys::protect_key,
            commands::keys::install_key,
            commands::keys::delete_key,
            commands::keys::list_agent_keys,
            commands::keys::pass_cli_status,
            commands::keys::pass_cli_list_keys,
            commands::keys::import_from_pass_cli,
            commands::sessions::session_open,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_ack,
            commands::sessions::session_close,
            commands::sessions::answer_host_key,
            commands::sessions::answer_auth,
            commands::local::local_shells,
            commands::local::local_shell_get,
            commands::local::local_shell_set,
            commands::local::serial_ports,
            commands::local::serial_last,
            commands::local::serial_remember,
            commands::identities::identities_list,
            commands::identities::identity_save,
            commands::identities::identity_delete,
            commands::passwords::password_set,
            commands::passwords::password_clear,
            commands::sync::list_servers,
            commands::sync::create_sync_account,
            commands::sync::join_sync_account,
            commands::sync::set_server_sync,
            commands::sync::remove_server,
            commands::sync::list_sync_devices,
            commands::sync::revoke_sync_device,
            commands::shares::find_recipient,
            commands::shares::confirm_recipient,
            commands::shares::share_items,
            commands::shares::list_shares,
            commands::shares::set_share_grants,
            commands::shares::unshare,
            commands::shares::item_shares,
            commands::sync::move_item,
            commands::sync::account_key_qr,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<state::AppState>()
            {
                state.sessions.finish_recordings();
            }
        });
}
