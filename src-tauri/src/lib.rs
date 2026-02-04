mod network;
mod android_storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let chat_connections: network::chat::ChatConnections = Arc::new(Mutex::new(HashMap::new()));
    let clipboard_connections: network::clipboard::ClipboardConnections = Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(android_storage::init())
        .manage(chat_connections)
        .manage(clipboard_connections)
        .invoke_handler(tauri::generate_handler![
            network::transfer::start_websocket_server,
            network::transfer::select_folder,
            network::transfer::list_folder_files,
            network::transfer::pick_multiple_files,
            network::transfer::pick_folder_for_send,
            network::transfer::send_files_android,
            network::transfer::send_folder_android,
            network::transfer::send_folder_desktop,
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
            network::clipboard::start_clipboard_server,
            network::clipboard::stop_clipboard_server,
            network::clipboard::connect_to_clipboard,
            network::clipboard::disconnect_clipboard,
            network::clipboard::disconnect_all_clipboards,
            network::clipboard::start_clipboard_polling,
            network::clipboard::stop_clipboard_polling,
            network::clipboard::send_clipboard_content,
            network::clipboard::get_system_clipboard,
            network::clipboard::set_system_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
