mod network;
mod android_storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(android_storage::init())
        .invoke_handler(tauri::generate_handler![
            network::transfer::start_websocket_server,
            network::transfer::select_folder,
            network::transfer::get_local_ip,
            network::transfer::get_download_dir,
            network::transfer::start_discovery,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
