pub mod commands;
pub mod sessions;
pub mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = state::AppState::init(app.handle())?;
            app.manage(state);
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
            commands::keys::vault_status,
            commands::keys::list_keys,
            commands::keys::import_key,
            commands::keys::import_key_file,
            commands::keys::generate_key,
            commands::keys::delete_key,
            commands::sessions::session_open,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_close,
            commands::sessions::answer_host_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
