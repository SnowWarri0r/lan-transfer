use std::path::PathBuf;
use tauri::{Window, AppHandle, Emitter};
#[cfg(target_os = "android")]
use crate::android_storage::AndroidStorage;
#[cfg(target_os = "android")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(target_os = "android")]
use tauri::Manager;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async_with_config};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use socket2::{Socket, Domain, Type, Protocol};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

#[derive(Deserialize)]
struct FileMeta {
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Device {
    ip: String,
    hostname: String,
    last_seen: u64,
}

type DeviceList = Arc<Mutex<HashMap<String, Device>>>;

// 全局状态：防止服务重复启动
static DISCOVERY_RUNNING: AtomicBool = AtomicBool::new(false);
static WEBSOCKET_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
/// 启动设备发现服务
pub fn start_discovery(window: Window) {
    // 防止重复启动
    if DISCOVERY_RUNNING.swap(true, Ordering::SeqCst) {
        println!("Discovery service already running");
        return;
    }

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_discovery_service(window).await {
                eprintln!("Discovery service error: {}", e);
                DISCOVERY_RUNNING.store(false, Ordering::SeqCst);
            }
        });
    });
}

async fn run_discovery_service(window: Window) -> Result<(), Box<dyn std::error::Error>> {
    let devices: DeviceList = Arc::new(Mutex::new(HashMap::new()));
    let local_ip = get_local_ip()?;
    let hostname = {
        #[cfg(not(target_os = "android"))]
        {
            hostname::get()
                .unwrap_or_else(|_| "Unknown".into())
                .to_string_lossy()
                .to_string()
        }
        #[cfg(target_os = "android")]
        {
            "Android".to_string()
        }
    };

    // 生成唯一实例 ID（用进程 ID�?
    let instance_id = std::process::id().to_string();

    // 组播地址�?39.x.x.x 为管理范围组播地址�?
    let multicast_addr: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);

    // 使用 socket2 创建可重用的 UDP socket
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.set_multicast_ttl_v4(255)?;

    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 37821);
    socket.bind(&addr.into())?;

    // 加入组播�?
    socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED)?;

    // 转换为标准库�?UdpSocket
    let socket: UdpSocket = socket.into();

    let multicast_target = SocketAddrV4::new(multicast_addr, 37821);

    // 克隆 socket 用于发�?
    let socket_send = socket.try_clone()?;
    let instance_id_clone = instance_id.clone();

    // 任务1：定期发送组�?(格式: FILETRANSFER:IP:HOSTNAME:INSTANCE_ID)
    tokio::spawn(async move {
        loop {
            let msg = format!("FILETRANSFER:{}:{}:{}", local_ip, hostname, instance_id_clone);
            let _ = socket_send.send_to(msg.as_bytes(), multicast_target);
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    // 任务2：接收组播并更新设备列表
    let devices_clone = devices.clone();
    let window_clone = window.clone();

    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, _)) => {
                    if let Ok(msg) = std::str::from_utf8(&buf[..len]) {
                        if msg.starts_with("FILETRANSFER:") {
                            let parts: Vec<&str> = msg.strip_prefix("FILETRANSFER:").unwrap().split(':').collect();
                            if parts.len() >= 3 {
                                let ip = parts[0].to_string();
                                let hostname = parts[1].to_string();
                                let remote_instance_id = parts[2];

                                // 用实�?ID 判断是否是自己（而不�?IP�?
                                if remote_instance_id != instance_id {
                                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                                    let device = Device {
                                        ip: ip.clone(),
                                        hostname,
                                        last_seen: now,
                                    };

                                    let mut devices = devices_clone.lock().unwrap();
                                    // �?IP+实例ID 作为 key，支持同机器多实�?
                                    let key = format!("{}:{}", ip, remote_instance_id);
                                    devices.insert(key, device);

                                    // 发送更新到前端
                                    let device_list: Vec<Device> = devices.values().cloned().collect();
                                    let _ = window_clone.emit("devices-updated", device_list);
                                }
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // 非阻塞模式下没有数据，正常情�?
                }
                Err(e) => {
                    eprintln!("UDP recv error: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // 任务3：定期清理过期设备（30秒未响应�?
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let mut devices = devices.lock().unwrap();
            devices.retain(|_, device| now - device.last_seen < 30);

            let device_list: Vec<Device> = devices.values().cloned().collect();
            let _ = window.emit("devices-updated", device_list);
        }
    });

    // 保持运行
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

#[tauri::command]
/// 获取系统下载目录
pub fn get_download_dir(app: AppHandle) -> Result<String, String> {
    // 桌面端：使用系统下载目录
    if let Some(dir) = dirs::download_dir().or_else(|| dirs::home_dir()) {
        return Ok(dir.to_string_lossy().to_string());
    }

    // Android 回退：使用应用数据目录下�?Downloads
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if let Ok(path) = app.path().app_data_dir() {
            let download_path = path.join("Downloads");
            let _ = std::fs::create_dir_all(&download_path);
            return Ok(download_path.to_string_lossy().to_string());
        }
    }

    let _ = app; // 桌面端避�?unused 警告
    Err("无法获取下载目录".to_string())
}

#[tauri::command]
/// 获取本机局域网IP地址
pub fn get_local_ip() -> Result<String, String> {
    // 通过连接到外部地址（不实际发送数据）来获取本机IP
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind socket: {}", e))?;

    socket.connect("8.8.8.8:80")
        .map_err(|e| format!("Failed to connect: {}", e))?;

    let local_addr = socket.local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?;

    Ok(local_addr.ip().to_string())
}

#[tauri::command]
/// 弹出文件夹选择对话框并返回路径字符�?
pub async fn select_folder(app: AppHandle) -> Result<Option<String>, String> {
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_dialog::DialogExt;

        // 使用 blocking 版本适合异步命令
        let folder_path = app.dialog().file().blocking_pick_folder();

        match folder_path {
            Some(fp) => {
                let pathbuf = fp.into_path().map_err(|e| e.to_string())?;
                Ok(Some(pathbuf.to_string_lossy().to_string()))
            }
            None => Ok(None),
        }
    }

    // Android: 使用 SAF 选择文件夹并返回 tree Uri
    #[cfg(target_os = "android")]
    {
        let storage = app.state::<AndroidStorage>();
        storage.pick_folder()
    }
}

#[tauri::command]
pub fn start_websocket_server(save_dir: String, window: Window, app: AppHandle) {
    // 防止重复启动
    if WEBSOCKET_RUNNING.swap(true, Ordering::SeqCst) {
        println!("WebSocket server already running");
        return;
    }

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_websocket_server(save_dir, window, app).await {
                eprintln!("WebSocket server error: {}", e);
                WEBSOCKET_RUNNING.store(false, Ordering::SeqCst);
            }
        });
    });
}

async fn run_websocket_server(save_dir: String, window: Window, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("0.0.0.0:7878").await?;
    println!("WebSocket server listening on ws://0.0.0.0:7878");

    while let Ok((stream, _)) = listener.accept().await {
        let save_dir = save_dir.clone();
        let window = window.clone();
        let app = app.clone();
        
        tokio::spawn(async move {
            if let Err(e) = handle_websocket_connection(stream, save_dir, window, app).await {
                eprintln!("WebSocket connection error: {}", e);
            }
        });
    }
    
    Ok(())
}

async fn handle_websocket_connection(
    stream: tokio::net::TcpStream,
    save_dir: String,
    window: Window,
    app: AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_config = WebSocketConfig {
        max_message_size: None,
        max_frame_size: None,
        ..Default::default()
    };
    let ws_stream = accept_async_with_config(stream, Some(ws_config)).await?;
    let (_write, mut read) = ws_stream.split();

    let mut file: Option<File> = None;
    #[cfg(target_os = "android")]
    let mut writer_handle: Option<i64> = None;
    let mut file_name: Option<String> = None;
    let mut bytes_received: u64 = 0;
    #[cfg(target_os = "android")]
    let is_content_uri = save_dir.starts_with("content://");

    while let Some(msg_result) = read.next().await {
        match msg_result? {
            Message::Text(json_str) => {
                if let Ok(meta) = serde_json::from_str::<FileMeta>(&json_str) {
                    file_name = Some(meta.name.clone());

                    #[cfg(target_os = "android")]
                    if is_content_uri {
                        let storage = app.state::<AndroidStorage>();
                        match storage.open_writer(save_dir.clone(), meta.name.clone()) {
                            Ok(handle) => {
                                writer_handle = Some(handle);
                                println!("Receiving file via SAF: {}", meta.name);
                                let _ = window.emit("file-receiving", &meta.name);
                            }
                            Err(e) => {
                                eprintln!("Failed to open SAF writer: {}", e);
                            }
                        }
                        continue;
                    }

                    let mut full_path = PathBuf::from(&save_dir);
                    full_path.push(&meta.name);

                    match File::create(&full_path).await {
                        Ok(f) => {
                            file = Some(f);
                            println!("Receiving file: {}", full_path.display());
                            // 
                            let _ = window.emit("file-receiving", &meta.name);
                        }
                        Err(e) => {
                            eprintln!("Failed to create file {}: {}", full_path.display(), e);
                        }
                    }
                }
            }
            Message::Binary(data) => {
                #[cfg(target_os = "android")]
                if is_content_uri {
                    if let Some(handle) = writer_handle {
                        let storage = app.state::<AndroidStorage>();
                        let encoded = general_purpose::STANDARD.encode(&data);
                        bytes_received += data.len() as u64;
                        if let Err(e) = storage.write_chunk(handle, encoded) {
                            eprintln!("Failed to write chunk via SAF: {}", e);
                        }
                        continue;
                    }
                }

                if let Some(f) = file.as_mut() {
                    bytes_received += data.len() as u64;
                    if let Err(e) = f.write_all(&data).await {
                        eprintln!("Failed to write to file: {}", e);
                    }
                }
            }
            Message::Close(_) => {
                println!("WebSocket connection closed");
                break;
            }
            _ => {}
        }
    }

    // 关闭 SAF writer
    #[cfg(target_os = "android")]
    if let Some(handle) = writer_handle {
        let storage = app.state::<AndroidStorage>();
        if let Err(e) = storage.close_writer(handle) {
            eprintln!("Failed to close SAF writer: {}", e);
        }
    }

    // 确保文件被正确关闭和刷新
    if let Some(mut f) = file {
        let _ = f.flush().await;
    }

    // 通知前端接收完成
    if let Some(name) = file_name {
        let _ = window.emit("file-received", serde_json::json!({
            "name": name,
            "size": bytes_received
        }));
        println!("File received: {} ({} bytes)", name, bytes_received);
    }

    Ok(())
}

