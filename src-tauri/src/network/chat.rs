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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub content: String,
    pub from_ip: String,
    pub timestamp: i64,
}

pub(crate) enum WsWriter {
    Plain(futures_util::stream::SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>),
    Tls(futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>),
}

impl WsWriter {
    async fn send(&mut self, msg: Message) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        match self {
            WsWriter::Plain(w) => w.send(msg).await,
            WsWriter::Tls(w) => w.send(msg).await,
        }
    }
}

pub struct ChatConnection {
    #[allow(dead_code)]
    pub ip: String,
    pub writer: Arc<Mutex<WsWriter>>,
}

pub type ChatConnections = Arc<Mutex<HashMap<String, ChatConnection>>>;

static CHAT_SERVER_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn start_chat_server(window: Window, app: AppHandle) -> Result<(), String> {
    if CHAT_SERVER_RUNNING.load(Ordering::Relaxed) {
        return Ok(());
    }

    CHAT_SERVER_RUNNING.store(true, Ordering::Relaxed);

    let connections: ChatConnections = app.state::<ChatConnections>().inner().clone();

    tokio::spawn(async move {
        let addr = "0.0.0.0:7879";

        // Create socket with SO_REUSEADDR to allow port reuse
        let socket = match Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create socket: {}", e);
                CHAT_SERVER_RUNNING.store(false, Ordering::Relaxed);
                let _ = window.emit("chat-server-error", format!("Failed to create socket: {}", e));
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
            eprintln!("Failed to bind chat server: {}", e);
            CHAT_SERVER_RUNNING.store(false, Ordering::Relaxed);
            let _ = window.emit("chat-server-error", format!("Failed to bind chat server: {}", e));
            return;
        }

        if let Err(e) = socket.listen(128) {
            eprintln!("Failed to listen: {}", e);
            CHAT_SERVER_RUNNING.store(false, Ordering::Relaxed);
            let _ = window.emit("chat-server-error", format!("Failed to listen: {}", e));
            return;
        }

        let std_listener: std::net::TcpListener = socket.into();
        let listener = match TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to convert listener: {}", e);
                CHAT_SERVER_RUNNING.store(false, Ordering::Relaxed);
                let _ = window.emit("chat-server-error", format!("Failed to convert listener: {}", e));
                return;
            }
        };

        println!("Chat server listening on {}", addr);

        while let Ok((stream, peer_addr)) = listener.accept().await {
            let mut peer_ip = peer_addr.ip().to_string();

            // 修复同机测试：将 127.0.0.1 替换为本机实际 IP
            if peer_ip == "127.0.0.1" || peer_ip == "::1" {
                if let Ok(local_ip) = crate::network::transfer::get_local_ip() {
                    peer_ip = local_ip;
                    println!("Normalized loopback address to local IP: {}", peer_ip);
                }
            }

            println!("New chat connection from {}", peer_ip);

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
                let writer = Arc::new(Mutex::new(WsWriter::Plain(writer)));

                // Store connection
                {
                    let mut conns = connections_clone.lock().await;
                    conns.insert(peer_ip.clone(), ChatConnection {
                        ip: peer_ip.clone(),
                        writer: writer.clone(),
                    });
                }

                let _ = window_clone.emit("chat-connected", &peer_ip);

                // Listen for messages
                while let Some(msg_result) = reader.next().await {
                    match msg_result {
                        Ok(Message::Text(text)) => {
                            match serde_json::from_str::<ChatMessage>(&text) {
                                Ok(chat_msg) => {
                                    let _ = window_clone.emit("chat-message-received", chat_msg);
                                }
                                Err(e) => {
                                    eprintln!("Failed to parse chat message: {}", e);
                                }
                            }
                        }
                        Ok(Message::Ping(data)) => {
                            // Respond to ping with pong, ignore errors (connection might be closing)
                            let _ = writer.lock().await.send(Message::Pong(data)).await;
                        }
                        Ok(Message::Pong(_)) => {
                            // Ignore pong messages
                        }
                        Ok(Message::Close(_)) => {
                            println!("Chat connection closed by {}", peer_ip);
                            break;
                        }
                        Ok(Message::Binary(_)) => {
                            // Ignore binary messages
                        }
                        Ok(Message::Frame(_)) => {
                            // Ignore raw frames
                        }
                        Err(e) => {
                            // Only log unexpected errors, not connection resets (which are normal)
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

                let _ = window_clone.emit("chat-disconnected", &peer_ip);
            });
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn connect_to_chat(target_ip: String, window: Window, app: AppHandle) -> Result<(), String> {
    let connections: ChatConnections = app.state::<ChatConnections>().inner().clone();

    // Check if already connected
    {
        let conns = connections.lock().await;
        if conns.contains_key(&target_ip) {
            return Ok(());
        }
    }

    let url = format!("ws://{}:7879", target_ip);

    let ws_stream = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("Failed to connect to chat: {}", e))?
        .0;

    let (writer, mut reader) = ws_stream.split();
    let writer = Arc::new(Mutex::new(WsWriter::Tls(writer)));

    // Store connection
    {
        let mut conns = connections.lock().await;
        conns.insert(target_ip.clone(), ChatConnection {
            ip: target_ip.clone(),
            writer: writer.clone(),
        });
    }

    let _ = window.emit("chat-connected", &target_ip);

    let connections_clone = connections.clone();
    let target_ip_clone = target_ip.clone();
    let window_clone = window.clone();

    // Spawn task to listen for messages
    let writer_clone = writer.clone();
    tokio::spawn(async move {
        while let Some(msg_result) = reader.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<ChatMessage>(&text) {
                        Ok(chat_msg) => {
                            let _ = window_clone.emit("chat-message-received", chat_msg);
                        }
                        Err(e) => {
                            eprintln!("Failed to parse chat message: {}", e);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    // Respond to ping with pong, ignore errors (connection might be closing)
                    let _ = writer_clone.lock().await.send(Message::Pong(data)).await;
                }
                Ok(Message::Pong(_)) => {
                    // Ignore pong messages
                }
                Ok(Message::Close(_)) => {
                    println!("Chat connection closed by {}", target_ip_clone);
                    break;
                }
                Ok(Message::Binary(_)) => {
                    // Ignore binary messages
                }
                Ok(Message::Frame(_)) => {
                    // Ignore raw frames
                }
                Err(e) => {
                    // Only log unexpected errors, not connection resets (which are normal)
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

        let _ = window_clone.emit("chat-disconnected", &target_ip_clone);
    });

    Ok(())
}

#[tauri::command]
pub async fn send_chat_message(target_ip: String, content: String, app: AppHandle) -> Result<(), String> {
    let connections: ChatConnections = app.state::<ChatConnections>().inner().clone();

    // Get writer Arc without holding the lock
    let writer = {
        let conns = connections.lock().await;
        let connection = conns.get(&target_ip)
            .ok_or_else(|| format!("Not connected to {}", target_ip))?;
        connection.writer.clone()
    };

    let local_ip = crate::network::transfer::get_local_ip()
        .unwrap_or_else(|_| "unknown".to_string());

    let message = ChatMessage {
        content,
        from_ip: local_ip,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    };

    let json = serde_json::to_string(&message)
        .map_err(|e| format!("Failed to serialize message: {}", e))?;

    let result = {
        let mut w = writer.lock().await;
        w.send(Message::Text(json)).await
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            // Remove dead connection
            let mut conns = connections.lock().await;
            conns.remove(&target_ip);
            Err(format!("连接已断开: {}", e))
        }
    }
}

#[tauri::command]
pub async fn disconnect_chat(target_ip: String, app: AppHandle) -> Result<(), String> {
    let connections: ChatConnections = app.state::<ChatConnections>().inner().clone();

    let mut conns = connections.lock().await;

    if let Some(connection) = conns.remove(&target_ip) {
        // Try to send close frame, but don't fail if it errors (connection might already be dead)
        let _ = connection.writer.lock().await.send(Message::Close(None)).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_chat_server() -> Result<(), String> {
    CHAT_SERVER_RUNNING.store(false, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn disconnect_all_chats(app: AppHandle) -> Result<(), String> {
    let connections: ChatConnections = app.state::<ChatConnections>().inner().clone();
    let mut conns = connections.lock().await;

    // Close all connections
    for (_ip, connection) in conns.drain() {
        let _ = connection.writer.lock().await.send(Message::Close(None)).await;
    }

    Ok(())
}
