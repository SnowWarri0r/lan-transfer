use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Window};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{accept_async, MaybeTlsStream, WebSocketStream};
use futures_util::{SinkExt, StreamExt};
use socket2::{Socket, Domain, Type, Protocol};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const CLIPBOARD_PORT: u16 = 7880;
const POLL_INTERVAL_MS: u64 = 500;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClipboardMessage {
    pub content: String,
    pub from_ip: String,
    pub timestamp: i64,
    pub hash: String,
}

pub(crate) enum ClipboardWsWriter {
    Plain(futures_util::stream::SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>),
    Tls(futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>),
}

impl ClipboardWsWriter {
    async fn send(&mut self, msg: Message) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        match self {
            ClipboardWsWriter::Plain(w) => w.send(msg).await,
            ClipboardWsWriter::Tls(w) => w.send(msg).await,
        }
    }
}

pub struct ClipboardConnection {
    #[allow(dead_code)]
    pub ip: String,
    pub writer: Arc<Mutex<ClipboardWsWriter>>,
}

pub type ClipboardConnections = Arc<Mutex<HashMap<String, ClipboardConnection>>>;

static CLIPBOARD_SERVER_RUNNING: AtomicBool = AtomicBool::new(false);
static CLIPBOARD_POLLING_RUNNING: AtomicBool = AtomicBool::new(false);

// Last known clipboard hash to prevent echo
static LAST_CLIPBOARD_HASH: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn compute_hash(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Get system clipboard content (desktop only, Android uses plugin)
#[cfg(not(target_os = "android"))]
fn get_clipboard_content() -> Result<String, String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard.get_text().map_err(|e| format!("Failed to get clipboard text: {}", e))
}

#[cfg(target_os = "android")]
fn get_clipboard_content() -> Result<String, String> {
    Err("Use get_system_clipboard command for Android".to_string())
}

/// Set system clipboard content (desktop only, Android uses plugin)
#[cfg(not(target_os = "android"))]
fn set_clipboard_content(content: &str) -> Result<(), String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard.set_text(content).map_err(|e| format!("Failed to set clipboard text: {}", e))
}

#[cfg(target_os = "android")]
fn set_clipboard_content(_content: &str) -> Result<(), String> {
    Err("Use set_system_clipboard command for Android".to_string())
}

#[tauri::command]
pub async fn get_system_clipboard(app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let storage = app.state::<crate::android_storage::AndroidStorage>();
        return storage.get_clipboard();
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        get_clipboard_content()
    }
}

#[tauri::command]
pub async fn set_system_clipboard(content: String, app: AppHandle) -> Result<(), String> {
    // Update last hash to prevent echo
    if let Ok(mut hash) = LAST_CLIPBOARD_HASH.lock() {
        *hash = compute_hash(&content);
    }

    #[cfg(target_os = "android")]
    {
        let storage = app.state::<crate::android_storage::AndroidStorage>();
        return storage.set_clipboard(content);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        set_clipboard_content(&content)
    }
}

#[tauri::command]
pub async fn start_clipboard_server(window: Window, app: AppHandle) -> Result<(), String> {
    if CLIPBOARD_SERVER_RUNNING.load(Ordering::Relaxed) {
        return Ok(());
    }

    CLIPBOARD_SERVER_RUNNING.store(true, Ordering::Relaxed);

    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();

    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", CLIPBOARD_PORT);

        let socket = match Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create clipboard socket: {}", e);
                CLIPBOARD_SERVER_RUNNING.store(false, Ordering::Relaxed);
                let _ = window.emit("clipboard-server-error", format!("Failed to create socket: {}", e));
                return;
            }
        };

        if let Err(e) = socket.set_reuse_address(true) {
            eprintln!("Failed to set SO_REUSEADDR: {}", e);
        }

        #[cfg(not(windows))]
        if let Err(e) = socket.set_reuse_port(true) {
            eprintln!("Failed to set SO_REUSEPORT: {}", e);
        }

        if let Err(e) = socket.set_nonblocking(true) {
            eprintln!("Failed to set nonblocking: {}", e);
        }

        let sock_addr: std::net::SocketAddr = addr.parse().unwrap();
        if let Err(e) = socket.bind(&sock_addr.into()) {
            eprintln!("Failed to bind clipboard server: {}", e);
            CLIPBOARD_SERVER_RUNNING.store(false, Ordering::Relaxed);
            let _ = window.emit("clipboard-server-error", format!("Failed to bind: {}", e));
            return;
        }

        if let Err(e) = socket.listen(128) {
            eprintln!("Failed to listen: {}", e);
            CLIPBOARD_SERVER_RUNNING.store(false, Ordering::Relaxed);
            let _ = window.emit("clipboard-server-error", format!("Failed to listen: {}", e));
            return;
        }

        let std_listener: std::net::TcpListener = socket.into();
        let listener = match TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to convert listener: {}", e);
                CLIPBOARD_SERVER_RUNNING.store(false, Ordering::Relaxed);
                let _ = window.emit("clipboard-server-error", format!("Failed to convert listener: {}", e));
                return;
            }
        };

        println!("Clipboard server listening on {}", addr);

        while CLIPBOARD_SERVER_RUNNING.load(Ordering::Relaxed) {
            tokio::select! {
                result = listener.accept() => {
                    if let Ok((stream, peer_addr)) = result {
                        let mut peer_ip = peer_addr.ip().to_string();

                        // Normalize loopback to local IP
                        if peer_ip == "127.0.0.1" || peer_ip == "::1" {
                            if let Ok(local_ip) = crate::network::transfer::get_local_ip() {
                                peer_ip = local_ip;
                            }
                        }

                        println!("New clipboard connection from {}", peer_ip);

                        let connections_clone = connections.clone();
                        let window_clone = window.clone();

                        tokio::spawn(async move {
                            let ws_stream = match accept_async(stream).await {
                                Ok(ws) => ws,
                                Err(e) => {
                                    eprintln!("WebSocket handshake failed: {}", e);
                                    return;
                                }
                            };

                            let (writer, mut reader) = ws_stream.split();
                            let writer = Arc::new(Mutex::new(ClipboardWsWriter::Plain(writer)));

                            // Store connection
                            {
                                let mut conns = connections_clone.lock().await;
                                conns.insert(peer_ip.clone(), ClipboardConnection {
                                    ip: peer_ip.clone(),
                                    writer: writer.clone(),
                                });
                            }

                            let _ = window_clone.emit("clipboard-connected", &peer_ip);

                            // Listen for messages
                            while let Some(msg_result) = reader.next().await {
                                match msg_result {
                                    Ok(Message::Text(text)) => {
                                        match serde_json::from_str::<ClipboardMessage>(&text) {
                                            Ok(clip_msg) => {
                                                // Update last hash to prevent echo
                                                if let Ok(mut hash) = LAST_CLIPBOARD_HASH.lock() {
                                                    *hash = clip_msg.hash.clone();
                                                }

                                                // Set local clipboard
                                                #[cfg(not(target_os = "android"))]
                                                {
                                                    let _ = set_clipboard_content(&clip_msg.content);
                                                }

                                                let _ = window_clone.emit("clipboard-received", clip_msg);
                                            }
                                            Err(e) => {
                                                eprintln!("Failed to parse clipboard message: {}", e);
                                            }
                                        }
                                    }
                                    Ok(Message::Ping(data)) => {
                                        let _ = writer.lock().await.send(Message::Pong(data)).await;
                                    }
                                    Ok(Message::Pong(_)) => {}
                                    Ok(Message::Close(_)) => {
                                        println!("Clipboard connection closed by {}", peer_ip);
                                        break;
                                    }
                                    Ok(Message::Binary(_)) => {}
                                    Ok(Message::Frame(_)) => {}
                                    Err(e) => {
                                        let error_msg = e.to_string();
                                        if !error_msg.contains("Connection reset") && !error_msg.contains("Broken pipe") {
                                            eprintln!("WebSocket error from {}: {}", peer_ip, e);
                                        }
                                        break;
                                    }
                                }
                            }

                            // Remove connection
                            {
                                let mut conns = connections_clone.lock().await;
                                conns.remove(&peer_ip);
                            }

                            let _ = window_clone.emit("clipboard-disconnected", &peer_ip);
                        });
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    // Check if server should stop
                    if !CLIPBOARD_SERVER_RUNNING.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }

        println!("Clipboard server stopped");
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_clipboard_server() -> Result<(), String> {
    CLIPBOARD_SERVER_RUNNING.store(false, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn connect_to_clipboard(target_ip: String, window: Window, app: AppHandle) -> Result<(), String> {
    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();

    // Check if already connected
    {
        let conns = connections.lock().await;
        if conns.contains_key(&target_ip) {
            return Ok(());
        }
    }

    let url = format!("ws://{}:{}", target_ip, CLIPBOARD_PORT);

    let ws_stream = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("Failed to connect to clipboard: {}", e))?
        .0;

    let (writer, mut reader) = ws_stream.split();
    let writer = Arc::new(Mutex::new(ClipboardWsWriter::Tls(writer)));

    // Store connection
    {
        let mut conns = connections.lock().await;
        conns.insert(target_ip.clone(), ClipboardConnection {
            ip: target_ip.clone(),
            writer: writer.clone(),
        });
    }

    let _ = window.emit("clipboard-connected", &target_ip);

    let connections_clone = connections.clone();
    let target_ip_clone = target_ip.clone();
    let window_clone = window.clone();

    let writer_clone = writer.clone();
    tokio::spawn(async move {
        while let Some(msg_result) = reader.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<ClipboardMessage>(&text) {
                        Ok(clip_msg) => {
                            // Update last hash to prevent echo
                            if let Ok(mut hash) = LAST_CLIPBOARD_HASH.lock() {
                                *hash = clip_msg.hash.clone();
                            }

                            // Set local clipboard
                            #[cfg(not(target_os = "android"))]
                            {
                                let _ = set_clipboard_content(&clip_msg.content);
                            }

                            let _ = window_clone.emit("clipboard-received", clip_msg);
                        }
                        Err(e) => {
                            eprintln!("Failed to parse clipboard message: {}", e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = writer_clone.lock().await.send(Message::Pong(data)).await;
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => {
                    println!("Clipboard connection closed by {}", target_ip_clone);
                    break;
                }
                Ok(Message::Binary(_)) => {}
                Ok(Message::Frame(_)) => {}
                Err(e) => {
                    let error_msg = e.to_string();
                    if !error_msg.contains("Connection reset") && !error_msg.contains("Broken pipe") {
                        eprintln!("WebSocket error from {}: {}", target_ip_clone, e);
                    }
                    break;
                }
            }
        }

        // Remove connection
        {
            let mut conns = connections_clone.lock().await;
            conns.remove(&target_ip_clone);
        }

        let _ = window_clone.emit("clipboard-disconnected", &target_ip_clone);
    });

    Ok(())
}

#[tauri::command]
pub async fn disconnect_clipboard(target_ip: String, app: AppHandle) -> Result<(), String> {
    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();

    let mut conns = connections.lock().await;

    if let Some(connection) = conns.remove(&target_ip) {
        let _ = connection.writer.lock().await.send(Message::Close(None)).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn disconnect_all_clipboards(app: AppHandle) -> Result<(), String> {
    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();
    let mut conns = connections.lock().await;

    for (_ip, connection) in conns.drain() {
        let _ = connection.writer.lock().await.send(Message::Close(None)).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn send_clipboard_content(app: AppHandle) -> Result<(), String> {
    let content = get_system_clipboard(app.clone()).await?;

    if content.is_empty() {
        return Err("Clipboard is empty".to_string());
    }

    let hash = compute_hash(&content);

    // Update last hash to prevent echo
    if let Ok(mut last_hash) = LAST_CLIPBOARD_HASH.lock() {
        *last_hash = hash.clone();
    }

    let local_ip = crate::network::transfer::get_local_ip()
        .unwrap_or_else(|_| "unknown".to_string());

    let message = ClipboardMessage {
        content,
        from_ip: local_ip,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        hash,
    };

    let json = serde_json::to_string(&message)
        .map_err(|e| format!("Failed to serialize message: {}", e))?;

    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();
    let conns = connections.lock().await;

    for (ip, connection) in conns.iter() {
        let mut w = connection.writer.lock().await;
        if let Err(e) = w.send(Message::Text(json.clone())).await {
            eprintln!("Failed to send clipboard to {}: {}", ip, e);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn start_clipboard_polling(window: Window, app: AppHandle) -> Result<(), String> {
    if CLIPBOARD_POLLING_RUNNING.load(Ordering::Relaxed) {
        return Ok(());
    }

    CLIPBOARD_POLLING_RUNNING.store(true, Ordering::Relaxed);

    let connections: ClipboardConnections = app.state::<ClipboardConnections>().inner().clone();

    #[cfg(target_os = "android")]
    let storage = app.state::<crate::android_storage::AndroidStorage>().inner().clone();

    tokio::spawn(async move {
        let mut last_content_hash = String::new();

        while CLIPBOARD_POLLING_RUNNING.load(Ordering::Relaxed) {
            tokio::time::sleep(tokio::time::Duration::from_millis(POLL_INTERVAL_MS)).await;

            // Get current clipboard content
            #[cfg(not(target_os = "android"))]
            let content_result = get_clipboard_content();

            #[cfg(target_os = "android")]
            let content_result = storage.get_clipboard();

            let content = match content_result {
                Ok(c) => c,
                Err(_) => continue,
            };

            if content.is_empty() {
                continue;
            }

            let current_hash = compute_hash(&content);

            // Skip if same as last known hash (to prevent echo)
            if let Ok(last_hash) = LAST_CLIPBOARD_HASH.lock() {
                if *last_hash == current_hash {
                    continue;
                }
            }

            // Skip if same as last polled content
            if current_hash == last_content_hash {
                continue;
            }

            last_content_hash = current_hash.clone();

            // Update last hash
            if let Ok(mut hash) = LAST_CLIPBOARD_HASH.lock() {
                *hash = current_hash.clone();
            }

            // Broadcast to all connections
            let local_ip = crate::network::transfer::get_local_ip()
                .unwrap_or_else(|_| "unknown".to_string());

            let message = ClipboardMessage {
                content: content.clone(),
                from_ip: local_ip,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                hash: current_hash,
            };

            let json = match serde_json::to_string(&message) {
                Ok(j) => j,
                Err(_) => continue,
            };

            let conns = connections.lock().await;
            for (ip, connection) in conns.iter() {
                let mut w = connection.writer.lock().await;
                if let Err(e) = w.send(Message::Text(json.clone())).await {
                    eprintln!("Failed to broadcast clipboard to {}: {}", ip, e);
                }
            }

            // Emit local event for UI update
            let _ = window.emit("clipboard-sent", &message);
        }

        println!("Clipboard polling stopped");
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_clipboard_polling() -> Result<(), String> {
    CLIPBOARD_POLLING_RUNNING.store(false, Ordering::Relaxed);
    Ok(())
}
