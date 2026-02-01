mod network;
mod android_storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let chat_connections: network::chat::ChatConnections = Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(android_storage::init())
        .manage(chat_connections)
        .invoke_handler(tauri::generate_handler![
            network::transfer::start_websocket_server,
            network::transfer::select_folder,
            network::transfer::pick_multiple_files,
            network::transfer::send_files_android,
            network::transfer::cancel_file_sending,
            network::transfer::cancel_file_receiving,
            network::transfer::get_local_ip,
            network::transfer::get_download_dir,
            network::transfer::start_discovery,
            network::chat::start_chat_server,
            network::chat::connect_to_chat,
            network::chat::send_chat_message,
            network::chat::disconnect_chat,
            network::chat::stop_chat_server,
            network::chat::disconnect_all_chats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
