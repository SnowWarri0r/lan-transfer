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
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig, CloseFrame};
use futures_util::{StreamExt, SinkExt};
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
    #[serde(default)]
    size: u64,
    #[serde(default)]
    index: u32,
    #[serde(default)]
    total: u32,
    #[serde(default)]
    relative_path: Option<String>,
}

/// Sanitize relative path to prevent path traversal attacks.
/// Returns None if the path is invalid or attempts directory traversal.
fn sanitize_relative_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    // Normalize path separators
    let normalized = path.replace('\\', "/");

    // Split into components and validate each
    let mut components: Vec<&str> = Vec::new();
    for component in normalized.split('/') {
        match component {
            "" | "." => continue,  // Skip empty and current dir
            ".." => return None,   // Reject parent dir traversal
            c if c.contains('\0') => return None,  // Reject null bytes
            c => components.push(c),
        }
    }

    if components.is_empty() {
        return None;
    }

    Some(components.join("/"))
}

#[derive(Serialize, Clone, Debug)]
struct FileProgress {
    file_name: String,
    bytes_received: u64,
    total_bytes: u64,
    percentage: f64,
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
// 取消发送标志
static CANCEL_SENDING: AtomicBool = AtomicBool::new(false);
// 取消接收标志
static CANCEL_RECEIVING: AtomicBool = AtomicBool::new(false);
// 当前保存目录（可在服务器运行期间更新）
static CURRENT_SAVE_DIR: Mutex<String> = Mutex::new(String::new());

#[tauri::command]
/// 取消正在进行的文件发送
pub fn cancel_file_sending() {
    CANCEL_SENDING.store(true, Ordering::SeqCst);
}

#[tauri::command]
/// 取消正在进行的文件接收
pub fn cancel_file_receiving() {
    CANCEL_RECEIVING.store(true, Ordering::SeqCst);
}

#[tauri::command]
/// 启动设备发现服务
pub fn start_discovery(window: Window, app: tauri::AppHandle) {
    // 防止重复启动
    if DISCOVERY_RUNNING.swap(true, Ordering::SeqCst) {
        println!("Discovery service already running");
        return;
    }

    // Get device name before spawning thread
    #[cfg(target_os = "android")]
    let device_name = {
        use tauri::Manager;
        app.state::<crate::android_storage::AndroidStorage>()
            .get_device_name()
            .unwrap_or_else(|_| "Android".to_string())
    };
    #[cfg(not(target_os = "android"))]
    let device_name = {
        let _ = app;
        hostname::get()
            .unwrap_or_else(|_| "Unknown".into())
            .to_string_lossy()
            .to_string()
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_discovery_service(window, device_name).await {
                eprintln!("Discovery service error: {}", e);
                DISCOVERY_RUNNING.store(false, Ordering::SeqCst);
            }
        });
    });
}

async fn run_discovery_service(window: Window, hostname: String) -> Result<(), Box<dyn std::error::Error>> {
    let devices: DeviceList = Arc::new(Mutex::new(HashMap::new()));
    let local_ip = get_local_ip()?;

    // 生成唯一实例 ID（用进程 ID�?
    let instance_id = std::process::id().to_string();

    // 组播地址�?39.x.x.x 为管理范围组播地址�?
    let multicast_addr: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);

    // 解析本机 IP 为 Ipv4Addr，用于指定组播发送接口
    let local_ipv4: Ipv4Addr = local_ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);

    // 使用 socket2 创建可重用的 UDP socket
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.set_multicast_ttl_v4(255)?;
    // 显式指定组播发送接口，避免 Windows 多网卡时发到错误接口
    socket.set_multicast_if_v4(&local_ipv4)?;

    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 37821);
    socket.bind(&addr.into())?;

    // 加入组播�?
    socket.join_multicast_v4(&multicast_addr, &local_ipv4)?;

    // 转换为标准库�?UdpSocket
    let socket: UdpSocket = socket.into();

    let multicast_target = SocketAddrV4::new(multicast_addr, 37821);

    // 克隆 socket 用于发�?
    let socket_send = socket.try_clone()?;
    let instance_id_clone = instance_id.clone();
    let devices_for_send = devices.clone();

    // 任务1：定期发送组播 + 单播回复已知设备
    // (格式: FILETRANSFER:IP:HOSTNAME:INSTANCE_ID)
    tokio::spawn(async move {
        loop {
            let msg = format!("FILETRANSFER:{}:{}:{}", local_ip, hostname, instance_id_clone);
            // 组播发送
            let _ = socket_send.send_to(msg.as_bytes(), multicast_target);
            // 单播发送给所有已知设备（解决路由器组播单向不通的问题）
            if let Ok(known) = devices_for_send.lock() {
                for device in known.values() {
                    if let Ok(ip) = device.ip.parse::<Ipv4Addr>() {
                        let target = SocketAddrV4::new(ip, 37821);
                        let _ = socket_send.send_to(msg.as_bytes(), target);
                    }
                }
            }
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
                                    devices.insert(ip.clone(), device);

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
    // Android: 直接使用标准下载目录
    #[cfg(target_os = "android")]
    {
        return Ok("/storage/emulated/0/Download".to_string());
    }

    // 桌面端：使用系统下载目录
    #[allow(unreachable_code)]
    if let Some(dir) = dirs::download_dir().or_else(|| dirs::home_dir()) {
        return Ok(dir.to_string_lossy().to_string());
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
/// Android 原生多文件选择器
pub async fn pick_multiple_files(app: AppHandle) -> Result<Vec<String>, String> {
    #[cfg(target_os = "android")]
    {
        let storage = app.state::<AndroidStorage>();
        storage.pick_multiple_files()
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("pick_multiple_files is only supported on Android".to_string())
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct FolderFile {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    pub size: u64,
}

#[tauri::command]
/// 桌面端：读取文件夹内所有文件
pub async fn list_folder_files(folder_path: String) -> Result<Vec<FolderFile>, String> {
    use std::path::Path;

    let root = Path::new(&folder_path);
    if !root.is_dir() {
        return Err("Not a directory".to_string());
    }

    let root_name = root.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut files = Vec::new();
    collect_files(root, &root_name, &mut files)?;
    Ok(files)
}

fn collect_files(dir: &std::path::Path, relative_base: &str, files: &mut Vec<FolderFile>) -> Result<(), String> {
    use std::fs;

    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.is_dir() {
            let dir_name = path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let new_base = format!("{}/{}", relative_base, dir_name);
            collect_files(&path, &new_base, files)?;
        } else if path.is_file() {
            let file_name = path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;

            files.push(FolderFile {
                path: path.to_string_lossy().to_string(),
                name: file_name.clone(),
                relative_path: format!("{}/{}", relative_base, file_name),
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct AndroidFolderFile {
    pub uri: String,
    pub name: String,
    pub relative_path: String,
    pub size: u64,
}

#[tauri::command]
/// Android: 选择文件夹并列出所有文件（用于发送）
pub async fn pick_folder_for_send(app: AppHandle) -> Result<Vec<AndroidFolderFile>, String> {
    #[cfg(target_os = "android")]
    {
        let storage = app.state::<AndroidStorage>();

        // First pick a folder
        let folder_uri = storage.pick_folder()?;
        let folder_uri = folder_uri.ok_or("No folder selected")?;

        // Then list all files recursively
        let files = storage.list_folder_contents(folder_uri)?;

        Ok(files.into_iter().map(|f| AndroidFolderFile {
            uri: f.uri,
            name: f.name,
            relative_path: f.relative_path,
            size: f.size,
        }).collect())
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("pick_folder_for_send is only supported on Android".to_string())
    }
}

#[tauri::command]
/// Android: 从 content:// URI 发送多个文件
pub async fn send_files_android(
    uris: Vec<String>,
    target_ip: String,
    window: Window,
    app: AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::connect_async;
        use base64::{engine::general_purpose, Engine as _};

        // Reset cancel flag at start
        CANCEL_SENDING.store(false, Ordering::SeqCst);

        let storage = app.state::<AndroidStorage>();
        let total = uris.len() as u32;

        for (index, uri) in uris.iter().enumerate() {
            // Check if cancelled before starting next file
            if CANCEL_SENDING.load(Ordering::SeqCst) {
                CANCEL_SENDING.store(false, Ordering::SeqCst);
                return Err("Cancelled by user".to_string());
            }
            // 1. 获取文件信息
            let (file_name, file_size) = storage.get_file_info(uri.clone())
                .map_err(|e| format!("Failed to get file info for {}: {}", uri, e))?;

            window.emit("file-sending", &file_name)
                .map_err(|e| format!("Failed to emit event: {}", e))?;

            // 2. 建立 WebSocket 连接
            let ws_url = format!("ws://{}:7878", target_ip);
            let request = ws_url.into_client_request()
                .map_err(|e| format!("Failed to create request: {}", e))?;

            let (ws_stream, _) = connect_async(request).await
                .map_err(|e| format!("Failed to connect to {}: {}", target_ip, e))?;

            let (mut write, mut read) = ws_stream.split();

            // 3. 发送文件元数据
            let meta = serde_json::json!({
                "name": file_name,
                "size": file_size,
                "index": index,
                "total": total,
            });
            let meta_str = serde_json::to_string(&meta)
                .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

            write.send(Message::Text(meta_str)).await
                .map_err(|e| format!("Failed to send metadata: {}", e))?;

            // 4. 分块读取并发送文件内容
            const CHUNK_SIZE: i32 = 256 * 1024; // 256KB chunks
            let mut offset: u64 = 0;
            let mut bytes_sent: u64 = 0;

            loop {
                // Check if cancelled during file transfer
                if CANCEL_SENDING.load(Ordering::SeqCst) {
                    // Close the connection gracefully
                    let _ = write.send(Message::Close(None)).await;
                    CANCEL_SENDING.store(false, Ordering::SeqCst);
                    return Err("Cancelled by user".to_string());
                }

                let (base64_data, bytes_read) = storage.read_uri_chunk(
                    uri.clone(),
                    offset,
                    CHUNK_SIZE
                ).map_err(|e| format!("Failed to read chunk: {}", e))?;

                if bytes_read == 0 {
                    break;
                }

                // Decode base64 to binary
                let binary_data = general_purpose::STANDARD.decode(&base64_data)
                    .map_err(|e| format!("Failed to decode base64: {}", e))?;

                // Send binary chunk
                if let Err(e) = write.send(Message::Binary(binary_data)).await {
                    // 连接断开，检查是否是接收端取消（Close 4001 可能在接收缓冲区中）
                    if let Ok(Some(Ok(Message::Close(Some(frame))))) =
                        tokio::time::timeout(Duration::from_millis(500), read.next()).await
                    {
                        let code: u16 = frame.code.into();
                        if code == 4001 {
                            return Err("Cancelled by receiver".to_string());
                        }
                    }
                    return Err(format!("Failed to send chunk: {}", e));
                }

                offset += bytes_read as u64;
                bytes_sent += bytes_read as u64;

                // Emit progress
                let percentage = (bytes_sent as f64 / file_size as f64) * 100.0;
                let _ = window.emit("file-transfer-progress", FileProgress {
                    file_name: file_name.clone(),
                    bytes_received: bytes_sent,
                    total_bytes: file_size,
                    percentage,
                });

                if bytes_sent >= file_size {
                    break;
                }
            }

            // 5. 关闭连接
            write.send(Message::Close(None)).await
                .map_err(|e| format!("Failed to close connection: {}", e))?;

            // 6. 等待接收端关闭响应，检测是否被取消
            if let Some(Ok(Message::Close(Some(frame)))) = read.next().await {
                let code: u16 = frame.code.into();
                if code == 4001 {
                    return Err("Cancelled by receiver".to_string());
                }
            }

            window.emit("file-sent", &file_name)
                .map_err(|e| format!("Failed to emit event: {}", e))?;
        }

        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (uris, target_ip, window, app);
        Err("send_files_android is only supported on Android".to_string())
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct FolderFileToSend {
    pub uri: String,
    pub name: String,
    pub relative_path: String,
    pub size: u64,
}

#[tauri::command]
/// Android: 发送文件夹中的文件（带相对路径）
pub async fn send_folder_android(
    files: Vec<FolderFileToSend>,
    target_ip: String,
    window: Window,
    app: AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::connect_async;
        use base64::{engine::general_purpose, Engine as _};

        CANCEL_SENDING.store(false, Ordering::SeqCst);

        let storage = app.state::<AndroidStorage>();
        let total = files.len() as u32;

        for (index, file_info) in files.iter().enumerate() {
            if CANCEL_SENDING.load(Ordering::SeqCst) {
                CANCEL_SENDING.store(false, Ordering::SeqCst);
                return Err("Cancelled by user".to_string());
            }

            window.emit("file-sending", &file_info.name)
                .map_err(|e| format!("Failed to emit event: {}", e))?;

            let ws_url = format!("ws://{}:7878", target_ip);
            let request = ws_url.into_client_request()
                .map_err(|e| format!("Failed to create request: {}", e))?;

            let (ws_stream, _) = connect_async(request).await
                .map_err(|e| format!("Failed to connect to {}: {}", target_ip, e))?;

            let (mut write, mut read) = ws_stream.split();

            // Send metadata with relative_path
            let meta = serde_json::json!({
                "name": file_info.name,
                "size": file_info.size,
                "index": index,
                "total": total,
                "relative_path": file_info.relative_path,
            });
            let meta_str = serde_json::to_string(&meta)
                .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

            write.send(Message::Text(meta_str)).await
                .map_err(|e| format!("Failed to send metadata: {}", e))?;

            const CHUNK_SIZE: i32 = 256 * 1024;
            let mut offset: u64 = 0;
            let mut bytes_sent: u64 = 0;

            loop {
                if CANCEL_SENDING.load(Ordering::SeqCst) {
                    let _ = write.send(Message::Close(None)).await;
                    CANCEL_SENDING.store(false, Ordering::SeqCst);
                    return Err("Cancelled by user".to_string());
                }

                let (base64_data, bytes_read) = storage.read_uri_chunk(
                    file_info.uri.clone(),
                    offset,
                    CHUNK_SIZE
                ).map_err(|e| format!("Failed to read chunk: {}", e))?;

                if bytes_read == 0 {
                    break;
                }

                let binary_data = general_purpose::STANDARD.decode(&base64_data)
                    .map_err(|e| format!("Failed to decode base64: {}", e))?;

                if let Err(e) = write.send(Message::Binary(binary_data)).await {
                    if let Ok(Some(Ok(Message::Close(Some(frame))))) =
                        tokio::time::timeout(Duration::from_millis(500), read.next()).await
                    {
                        let code: u16 = frame.code.into();
                        if code == 4001 {
                            return Err("Cancelled by receiver".to_string());
                        }
                    }
                    return Err(format!("Failed to send chunk: {}", e));
                }

                offset += bytes_read as u64;
                bytes_sent += bytes_read as u64;

                let percentage = (bytes_sent as f64 / file_info.size as f64) * 100.0;
                let _ = window.emit("file-transfer-progress", FileProgress {
                    file_name: file_info.name.clone(),
                    bytes_received: bytes_sent,
                    total_bytes: file_info.size,
                    percentage,
                });

                if bytes_sent >= file_info.size {
                    break;
                }
            }

            write.send(Message::Close(None)).await
                .map_err(|e| format!("Failed to close connection: {}", e))?;

            if let Some(Ok(Message::Close(Some(frame)))) = read.next().await {
                let code: u16 = frame.code.into();
                if code == 4001 {
                    return Err("Cancelled by receiver".to_string());
                }
            }

            window.emit("file-sent", &file_info.name)
                .map_err(|e| format!("Failed to emit event: {}", e))?;
        }

        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (files, target_ip, window, app);
        Err("send_folder_android is only supported on Android".to_string())
    }
}

#[tauri::command]
/// 桌面端：发送文件夹
pub async fn send_folder_desktop(
    folder_path: String,
    target_ip: String,
    window: Window,
) -> Result<(), String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::connect_async;
    use tokio::fs::File as TokioFile;
    use tokio::io::AsyncReadExt;

    // Reset cancel flag
    CANCEL_SENDING.store(false, Ordering::SeqCst);

    // Get file list
    let files = list_folder_files(folder_path).await?;
    if files.is_empty() {
        return Err("Empty folder".to_string());
    }

    let total = files.len() as u32;

    for (index, file_info) in files.iter().enumerate() {
        if CANCEL_SENDING.load(Ordering::SeqCst) {
            CANCEL_SENDING.store(false, Ordering::SeqCst);
            return Err("Cancelled by user".to_string());
        }

        window.emit("file-sending", &file_info.name)
            .map_err(|e| format!("Failed to emit event: {}", e))?;

        let ws_url = format!("ws://{}:7878", target_ip);
        let request = ws_url.into_client_request()
            .map_err(|e| format!("Failed to create request: {}", e))?;

        let (ws_stream, _) = connect_async(request).await
            .map_err(|e| format!("Failed to connect to {}: {}", target_ip, e))?;

        let (mut write, mut read) = ws_stream.split();

        // Send metadata
        let meta = serde_json::json!({
            "name": file_info.name,
            "size": file_info.size,
            "index": index,
            "total": total,
            "relative_path": file_info.relative_path,
        });
        let meta_str = serde_json::to_string(&meta)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        write.send(Message::Text(meta_str)).await
            .map_err(|e| format!("Failed to send metadata: {}", e))?;

        // Read and send file
        let mut file = TokioFile::open(&file_info.path).await
            .map_err(|e| format!("Failed to open file: {}", e))?;

        let mut buffer = vec![0u8; 256 * 1024];
        let mut bytes_sent: u64 = 0;

        loop {
            if CANCEL_SENDING.load(Ordering::SeqCst) {
                let _ = write.send(Message::Close(None)).await;
                CANCEL_SENDING.store(false, Ordering::SeqCst);
                return Err("Cancelled by user".to_string());
            }

            let n = file.read(&mut buffer).await
                .map_err(|e| format!("Failed to read file: {}", e))?;

            if n == 0 {
                break;
            }

            write.send(Message::Binary(buffer[..n].to_vec())).await
                .map_err(|e| format!("Failed to send chunk: {}", e))?;

            bytes_sent += n as u64;

            let percentage = (bytes_sent as f64 / file_info.size as f64) * 100.0;
            let _ = window.emit("file-transfer-progress", FileProgress {
                file_name: file_info.name.clone(),
                bytes_received: bytes_sent,
                total_bytes: file_info.size,
                percentage,
            });
        }

        write.send(Message::Close(None)).await
            .map_err(|e| format!("Failed to close connection: {}", e))?;

        // Check for receiver cancel
        if let Some(Ok(Message::Close(Some(frame)))) = read.next().await {
            let code: u16 = frame.code.into();
            if code == 4001 {
                return Err("Cancelled by receiver".to_string());
            }
        }

        window.emit("file-sent", &file_info.name)
            .map_err(|e| format!("Failed to emit event: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
pub fn start_websocket_server(save_dir: String, window: Window, app: AppHandle) {
    // 始终更新保存目录（即使服务器已在运行）
    *CURRENT_SAVE_DIR.lock().unwrap() = save_dir;

    // 仅在服务器未运行时启动
    if WEBSOCKET_RUNNING.swap(true, Ordering::SeqCst) {
        println!("WebSocket server already running, save directory updated");
        return;
    }

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_websocket_server(window, app).await {
                eprintln!("WebSocket server error: {}", e);
                WEBSOCKET_RUNNING.store(false, Ordering::SeqCst);
            }
        });
    });
}

async fn run_websocket_server(window: Window, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("0.0.0.0:7878").await?;
    println!("WebSocket server listening on ws://0.0.0.0:7878");

    while let Ok((stream, _)) = listener.accept().await {
        // 每次新连接时读取最新的保存目录
        let save_dir = CURRENT_SAVE_DIR.lock().unwrap().clone();
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
    #[allow(unused_variables)] app: AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_config = WebSocketConfig {
        max_message_size: None,
        max_frame_size: None,
        ..Default::default()
    };
    let ws_stream = accept_async_with_config(stream, Some(ws_config)).await?;
    let (mut write, mut read) = ws_stream.split();

    // Reset cancel receiving flag at start of each connection
    CANCEL_RECEIVING.store(false, Ordering::SeqCst);

    let mut file: Option<File> = None;
    #[cfg(target_os = "android")]
    let mut writer_handle: Option<i64> = None;
    #[cfg(target_os = "android")]
    let mut document_uri: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut bytes_received: u64 = 0;
    let mut total_bytes: Option<u64> = None;
    let mut last_progress_emit: u64 = 0;
    const PROGRESS_INTERVAL: u64 = 100 * 1024; // 100KB
    #[cfg(target_os = "android")]
    let is_content_uri = save_dir.starts_with("content://");

    while let Some(msg_result) = read.next().await {
        match msg_result? {
            Message::Text(json_str) => {
                if let Ok(meta) = serde_json::from_str::<FileMeta>(&json_str) {
                    // Use relative_path for display if available, otherwise use name
                    file_name = Some(meta.relative_path.clone().unwrap_or_else(|| meta.name.clone()));
                    total_bytes = Some(meta.size);
                    bytes_received = 0;
                    last_progress_emit = 0;

                    // Log file info with index/total if available
                    if meta.total > 0 {
                        println!("Receiving file {} ({}/{}) - {} bytes",
                                 meta.name, meta.index + 1, meta.total, meta.size);
                    } else {
                        println!("Receiving file {} - {} bytes", meta.name, meta.size);
                    }

                    #[cfg(target_os = "android")]
                    if is_content_uri {
                        let storage = app.state::<AndroidStorage>();

                        // Handle relative_path for Android SAF
                        let target_tree_uri = if let Some(ref rel_path) = meta.relative_path {
                            if let Some(sanitized) = sanitize_relative_path(rel_path) {
                                // Extract parent directory from relative path
                                let path = std::path::Path::new(&sanitized);
                                if let Some(parent) = path.parent() {
                                    let parent_str = parent.to_string_lossy();
                                    if !parent_str.is_empty() {
                                        // Create subdirectories via SAF
                                        match storage.find_or_create_subdirectory(save_dir.clone(), parent_str.to_string()) {
                                            Ok(sub_uri) => sub_uri,
                                            Err(e) => {
                                                eprintln!("Failed to create subdirectory {}: {}", parent_str, e);
                                                save_dir.clone()
                                            }
                                        }
                                    } else {
                                        save_dir.clone()
                                    }
                                } else {
                                    save_dir.clone()
                                }
                            } else {
                                eprintln!("Invalid relative path: {}", rel_path);
                                save_dir.clone()
                            }
                        } else {
                            save_dir.clone()
                        };

                        match storage.open_writer(target_tree_uri, meta.name.clone()) {
                            Ok((handle, uri)) => {
                                writer_handle = Some(handle);
                                document_uri = Some(uri);
                                let _ = window.emit("file-receiving", &meta.name);
                            }
                            Err(e) => {
                                eprintln!("Failed to open SAF writer: {}", e);
                            }
                        }
                        continue;
                    }

                    // Desktop/Android non-SAF: handle relative_path by creating parent directories
                    let mut full_path = PathBuf::from(&save_dir);
                    if let Some(ref rel_path) = meta.relative_path {
                        if let Some(sanitized) = sanitize_relative_path(rel_path) {
                            full_path.push(&sanitized);
                            // Create parent directories if needed
                            if let Some(parent) = full_path.parent() {
                                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                                    eprintln!("Failed to create directory {}: {}", parent.display(), e);
                                }
                            }
                        } else {
                            eprintln!("Invalid relative path: {}, saving to root", rel_path);
                            full_path.push(&meta.name);
                        }
                    } else {
                        full_path.push(&meta.name);
                    }

                    match File::create(&full_path).await {
                        Ok(f) => {
                            file = Some(f);
                            let _ = window.emit("file-receiving", &meta.name);
                        }
                        Err(e) => {
                            eprintln!("Failed to create file {}: {}", full_path.display(), e);
                        }
                    }
                }
            }
            Message::Binary(data) => {
                // Check if receiving was cancelled
                if CANCEL_RECEIVING.load(Ordering::SeqCst) {
                    println!("File receiving cancelled by user");
                    // 立即发送 Close(4001) 通知发送端，此时连接仍然存活
                    let _ = write.send(Message::Close(Some(CloseFrame {
                        code: 4001u16.into(),
                        reason: "Cancelled by receiver".into(),
                    }))).await;
                    break;
                }

                #[cfg(target_os = "android")]
                if is_content_uri {
                    if let Some(handle) = writer_handle {
                        let storage = app.state::<AndroidStorage>();
                        let encoded = general_purpose::STANDARD.encode(&data);
                        bytes_received += data.len() as u64;
                        if let Err(e) = storage.write_chunk(handle, encoded) {
                            eprintln!("Failed to write chunk via SAF: {}", e);
                        }

                        // Emit progress for Android SAF
                        if let Some(total) = total_bytes {
                            let should_emit = bytes_received - last_progress_emit >= PROGRESS_INTERVAL
                                           || bytes_received >= total;
                            if should_emit {
                                let percentage = (bytes_received as f64 / total as f64) * 100.0;
                                let _ = window.emit("file-transfer-progress", FileProgress {
                                    file_name: file_name.clone().unwrap_or_default(),
                                    bytes_received,
                                    total_bytes: total,
                                    percentage,
                                });
                                last_progress_emit = bytes_received;
                            }
                        }
                        continue;
                    }
                }

                if let Some(f) = file.as_mut() {
                    bytes_received += data.len() as u64;
                    if let Err(e) = f.write_all(&data).await {
                        eprintln!("Failed to write to file: {}", e);
                    }

                    // Emit progress for regular file write
                    if let Some(total) = total_bytes {
                        let should_emit = bytes_received - last_progress_emit >= PROGRESS_INTERVAL
                                       || bytes_received >= total;
                        if should_emit {
                            let percentage = (bytes_received as f64 / total as f64) * 100.0;
                            let _ = window.emit("file-transfer-progress", FileProgress {
                                file_name: file_name.clone().unwrap_or_default(),
                                bytes_received,
                                total_bytes: total,
                                percentage,
                            });
                            last_progress_emit = bytes_received;
                        }
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

    // 检查文件是否完整接收
    let was_cancelled = CANCEL_RECEIVING.load(Ordering::SeqCst);
    CANCEL_RECEIVING.store(false, Ordering::SeqCst);

    let transfer_complete = if was_cancelled {
        false // 用户主动取消，即使数据已全部接收也视为未完成
    } else if let Some(expected_size) = total_bytes {
        bytes_received >= expected_size
    } else {
        true // 旧协议没有size字段，假设完整
    };

    if !transfer_complete {
        println!("Transfer incomplete: received {} of {} bytes",
                 bytes_received, total_bytes.unwrap_or(0));

        // 通知发送端：接收方已取消（Close code 4001）
        let _ = write.send(Message::Close(Some(CloseFrame {
            code: 4001u16.into(),
            reason: "Cancelled by receiver".into(),
        }))).await;

        // Android: 关闭并删除不完整的 SAF 文件
        #[cfg(target_os = "android")]
        if let Some(_handle) = writer_handle {
            let storage = app.state::<AndroidStorage>();
            if let Some(uri) = &document_uri {
                // delete_document will close the output stream and delete the file
                if let Err(e) = storage.delete_document(uri.clone()) {
                    eprintln!("Failed to delete incomplete SAF file: {}", e);
                } else {
                    println!("Deleted incomplete SAF file");
                }
            } else {
                let _ = storage.close_writer(_handle);
            }
        }

        // 桌面端：删除不完整的文件
        if let Some(f) = file {
            drop(f); // 关闭文件
            if let Some(name) = &file_name {
                let mut path = PathBuf::from(&save_dir);
                path.push(name);
                let _ = tokio::fs::remove_file(path).await;
                println!("Removed incomplete file: {}", name);
            }
        }

        // 通知前端传输取消
        if let Some(name) = file_name {
            let _ = window.emit("file-receive-cancelled", name);
        }

        return Ok(());
    }

    // 传输完整，正常关闭
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

