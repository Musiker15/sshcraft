pub mod commands;
pub mod errors;
pub mod hello;
pub mod passcli;
pub mod prompts;
pub mod sessions;
pub mod settings;
pub mod state;
pub mod sync;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // A second start must not open the database and the vault a second time: it brings the
    // running window to the front and exits.
    #[cfg(desktop)]
    let builder =
        builder
            .plugin(tauri_plugin_dialog::init())
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
            app.manage(state);
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
