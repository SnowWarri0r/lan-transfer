// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod network;

fn main() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
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
