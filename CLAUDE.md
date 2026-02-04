# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Tauri v2 application for LAN file transfer, real-time chat, and clipboard sync with a React + TypeScript frontend and Rust backend. The app allows users to send and receive files over a local network using UDP multicast discovery and WebSocket transfer, bidirectional text messaging via WebSocket chat, and cross-device clipboard synchronization.

## Development Commands

### Frontend (React + Vite + Tailwind CSS 4)
- `yarn dev` - Start Vite dev server (runs on port 1420)
- `yarn build` - Build frontend for production (outputs to `dist/`)
- `yarn preview` - Preview production build

### Tauri (Desktop App)
- `yarn tauri dev` - Run app in development mode (starts both Vite dev server and Tauri)
- `yarn tauri build` - Build production app bundle
- `cargo build` - Build Rust backend only (from `src-tauri/` directory)
- `cargo check` - Quick check Rust code without building
- `cargo clippy` - Run Rust linter

### Android
- `yarn tauri android dev` - Run on connected device/emulator
- `yarn tauri android build` - Build release APK/AAB
- `yarn tauri android build --target aarch64` - Build for arm64 only (faster)
- APK output: `src-tauri/gen/android/app/build/outputs/apk/universal/release/`
- AAB output: `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/`

Note: No test suite is currently configured in this project.

## Architecture

### Communication Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Application                     │
│  ┌───────────────────┐      ┌───────────────────────┐  │
│  │   Frontend        │ IPC  │    Backend (Rust)      │  │
│  │   (React)         │◄────►│                       │  │
│  │  - UI rendering   │      │  - System calls       │  │
│  │  - User input     │      │  - Network ops        │  │
│  │  - WebSocket send │      │  - File I/O           │  │
│  └───────────────────┘      └───────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Tauri IPC (Inter-Process Communication)

**Frontend → Backend (Commands):**
```typescript
// Call Rust functions from React
invoke('start_discovery');
invoke<string>('get_local_ip');
invoke('start_websocket_server', { saveDir: '/path' });
```

**Backend → Frontend (Events):**
```rust
// Rust sends events to React
window.emit("devices-updated", device_list);
window.emit("file-received", { name, size });
```

```typescript
// React listens for events
listen<Device[]>('devices-updated', (event) => {
  setDevices(event.payload);
});
```

### Key Components

**Frontend (`src/`)**
- `App.tsx` - Main UI with mode selection (send/receive/chat/clipboard), device list, file transfer, chat interface, clipboard sync
- `index.css` - Tailwind CSS 4 entry point
- Uses browser's native WebSocket API for sending files (file transfer mode)

**Backend (`src-tauri/src/`)**
- `main.rs` / `lib.rs` - Entry point, registers Tauri commands and plugins
- `network/transfer.rs` - File transfer network logic:
  - `start_discovery()` - UDP multicast device discovery
  - `start_websocket_server()` - File receiving server (supports both file path and SAF content:// URI)
  - `send_files_android()` - Android-only: send multiple files from content:// URIs with progress tracking
  - `cancel_file_sending()` - Set global cancel flag to abort ongoing sender transfers
  - `cancel_file_receiving()` - Set global cancel flag to abort ongoing receiver transfers
  - `pick_multiple_files()` - Android-only: launch native file picker, returns content:// URIs
  - `get_local_ip()` - Get local network IP
  - `get_download_dir()` - Get system download directory
  - `select_folder()` - Native folder picker dialog (desktop: tauri-plugin-dialog, Android: SAF)
- `network/chat.rs` - Chat network logic:
  - `start_chat_server()` - WebSocket chat server (dual server/client architecture)
  - `connect_to_chat()` - Connect to remote chat server
  - `send_chat_message()` - Send text message to connected peer
  - `disconnect_chat()` - Close chat connection
  - `stop_chat_server()` - Stop chat server
  - `disconnect_all_chats()` - Close all active connections
- `network/clipboard.rs` - Clipboard sync network logic:
  - `start_clipboard_server()` - WebSocket server for clipboard sync (port 7880)
  - `stop_clipboard_server()` - Stop clipboard server
  - `connect_to_clipboard()` - Connect to remote clipboard server
  - `disconnect_clipboard()` - Close clipboard connection
  - `disconnect_all_clipboards()` - Close all clipboard connections
  - `start_clipboard_polling()` - Start 500ms polling for clipboard changes
  - `stop_clipboard_polling()` - Stop clipboard polling
  - `send_clipboard_content()` - Manually broadcast clipboard to all peers
  - `get_system_clipboard()` - Read system clipboard (uses arboard on desktop, plugin on Android)
  - `set_system_clipboard()` - Write to system clipboard
- `android_storage.rs` - Android Storage Access Framework (SAF) plugin bridge:
  - Rust-side plugin that communicates with Kotlin `StoragePlugin` via `run_mobile_plugin`
  - Writing methods: `pick_folder()`, `open_writer()`, `write_chunk()`, `close_writer()`, `delete_document()`
  - Reading methods: `pick_multiple_files()`, `get_file_info()`, `read_uri_chunk()`
  - Clipboard methods: `get_clipboard()`, `set_clipboard()`

**Android Plugin (`src-tauri/gen/android/app/src/main/java/`)**
- `app/tauri/storage/StoragePlugin.kt` - Kotlin-side SAF implementation:
  - `pickFolder` - Launches `ACTION_OPEN_DOCUMENT_TREE` via `registerForActivityResult`, returns `content://` URI
  - `pickMultipleFiles` - Launches `ACTION_OPEN_DOCUMENT` with `EXTRA_ALLOW_MULTIPLE`, returns array of `content://` URIs
  - `getFileInfo` - Queries file name and size from content URI
  - `readUriChunk` - Reads file chunk from content URI, returns base64-encoded data
  - `openWriter` - Creates file via `DocumentsContract.createDocument`, returns handle + document URI
  - `writeChunk` - Writes base64-encoded data to the OutputStream for a given handle
  - `closeWriter` - Flushes and closes the OutputStream
  - `deleteDocument` - Deletes a document by URI via `DocumentsContract.deleteDocument` (used for incomplete transfer cleanup)
- `com/tauri_app/app/MainActivity.kt` - Acquires `WifiManager.MulticastLock` for UDP multicast discovery

**Important:** Use `ACTION_OPEN_DOCUMENT` instead of `ACTION_GET_CONTENT` for multi-select - better device compatibility and doesn't require persistable permissions.

## Device Discovery (UDP Multicast)

### Protocol
- **Port:** 37821 (UDP)
- **Address:** 239.255.77.88 (multicast, replaces broadcast for Android compatibility)
- **Message Format:** `FILETRANSFER:IP:HOSTNAME:INSTANCE_ID`
- **Example:** `FILETRANSFER:192.168.1.10:MyPC:12345`
- Android hostname is hardcoded to `"Android"` (hostname crate not available on Android)

### How It Works
1. Each app instance binds to UDP port 37821 with `SO_REUSEADDR` / `SO_REUSEPORT`
2. Joins multicast group `239.255.77.88`
3. Every 3 seconds, sends presence to multicast group
4. Listens for multicast messages from other devices
5. Filters out self using `instance_id` (process ID), not IP (same machine can have multiple instances)
6. Removes devices not seen for 30 seconds
7. Emits `devices-updated` event to frontend when list changes
8. Android requires `WifiManager.MulticastLock` (acquired in `MainActivity.kt`)

### Key Implementation Details
```rust
let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
socket.set_reuse_address(true)?;
socket.set_nonblocking(true)?;
socket.set_multicast_ttl_v4(255)?;
socket.bind("0.0.0.0:37821")?;
socket.join_multicast_v4(&Ipv4Addr::new(239, 255, 77, 88), &Ipv4Addr::UNSPECIFIED)?;
```

## File Transfer (WebSocket)

### Protocol
- **Port:** 7878 (TCP/WebSocket)
- **Flow:**
  1. Sender connects to `ws://{receiver_ip}:7878`
  2. Sends JSON metadata: `{"name": "filename.ext", "size": 1048576, "index": 0, "total": 3}`
  3. Sends binary data (file contents)
  4. Closes connection
  5. Repeat for next file (serial transfer)

### Protocol Extensions

**Multi-File Transfer (v2):**
- Added `size: u64` field for progress tracking
- Added `index: u32` and `total: u32` for queue position
- All fields use `#[serde(default)]` for backward compatibility
- Files are sent serially (one after another) to avoid network congestion

**Progress Tracking:**
- Backend emits `file-transfer-progress` event every 100KB or 10% progress
- Event payload: `{file_name, bytes_received, total_bytes, percentage}`
- Frontend displays progress bars on both sender and receiver sides

**Cancellation (Bidirectional):**
- **Sender cancel:** Sender can cancel at any time via cancel flag (desktop) or Rust command (Android). Sends WebSocket Close to receiver.
- **Receiver cancel:** Receiver can cancel via `cancel_file_receiving()` command. Sets `CANCEL_RECEIVING` AtomicBool flag checked in Binary message handler.
- **Close code 4001:** Receiver sends `Close(4001)` to notify sender that receiver cancelled. Sender detects this via `onclose` event (desktop) or `read.next()` after write error (Android).
- **Incomplete file cleanup:**
  - Desktop: Auto-deletes incomplete files via `tokio::fs::remove_file`
  - Android SAF: Deletes incomplete files via `DocumentsContract.deleteDocument` (through `delete_document()` plugin method)
- **Frontend notifications:**
  - Sender receives "Cancelled by receiver" error → shows "对方已取消接收" (with Broken pipe fallback)
  - Receiver receives `file-receive-cancelled` event → shows amber notification bar (auto-dismiss 3s)

### Sender (Frontend - Browser WebSocket)
```typescript
const socket = new WebSocket(`ws://${ip}:7878`);
socket.onopen = () => {
  socket.send(JSON.stringify({ name: file.name }));
  socket.send(arrayBuffer);  // File contents
  socket.close();
};
```

### Receiver (Backend - Rust Server)
```rust
// Listens on 0.0.0.0:7878
// On Text message: parse JSON, create file
// On Binary message: write to file
// On Close: check if transfer complete (bytes_received == total_bytes)
//   - Complete: emit "file-received" event
//   - Incomplete: delete file (desktop), emit "file-receive-cancelled"
```

### Multi-File Transfer Implementation

**Desktop (Frontend WebSocket):**
- User selects files via HTML input (multiple) or drag-and-drop
- Files stored in `FileQueueItem[]` state with status tracking
- Serial transfer: `sendFiles()` loops through queue, calls `sendSingleFile()` for each
- Cancel flag stored in `cancelSendingRef` (useRef to avoid closure issues)
- Progress updated in `updateFileProgress()` every 100ms during file stream read
- Cancel button in overall progress bar, delete button for pending files

**Android (Rust Backend):**
- User taps device "Send" button → launches `pickMultipleFiles()` native picker
- Returns `content://` URIs → passed to `send_files_android()` Rust command
- Rust reads files via `read_uri_chunk()` (256KB chunks, base64 encoded)
- Progress emitted via `file-transfer-progress` event
- Global `CANCEL_SENDING` AtomicBool flag for cancellation
- On cancel: sends Close message, resets flag, returns error

**Key Differences:**
| Aspect | Desktop | Android |
|--------|---------|---------|
| File Selection | HTML input / drag-drop | Native `ACTION_OPEN_DOCUMENT` |
| File Access | Direct File API | SAF content:// URIs |
| Transfer Logic | Frontend (sendSingleFile) | Backend (send_files_android) |
| Progress Tracking | Frontend updateFileProgress | Backend emit events |
| Cancellation | cancelSendingRef (React) | CANCEL_SENDING (Rust) |

## Chat (WebSocket Bidirectional Messaging)

### Protocol
- **Port:** 7879 (TCP/WebSocket)
- **Architecture:** Dual server/client pattern (each device runs both)
- **Message Format:** JSON-encoded `ChatMessage`
```json
{
  "content": "Hello",
  "from_ip": "192.168.1.10",
  "timestamp": 1706745600000
}
```

### Dual Server/Client Pattern
Every device in chat mode runs both:
1. **WebSocket Server** on port 7879 - accepts incoming connections
2. **WebSocket Client** - initiates outgoing connections to peers

This enables:
- Auto-accept: Device B's server accepts when A connects
- Auto-reconnect: If connection drops, either side can reconnect to the other's server
- Symmetric architecture: No dedicated "host" device

### Key Implementation Details

**Connection Management:**
```rust
pub struct ChatConnection {
    pub ip: String,
    pub writer: Arc<Mutex<WsWriter>>,
}

pub(crate) enum WsWriter {
    Plain(futures_util::stream::SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>),
    Tls(futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>),
}

pub type ChatConnections = Arc<Mutex<HashMap<String, ChatConnection>>>;
```

- Global `ChatConnections` state managed via Tauri `.manage()`
- Each connection stored by peer IP as key
- `WsWriter` enum handles both server-accepted (Plain) and client-initiated (Tls) streams
- Writer stored in `Arc<Mutex<>>` for concurrent access from message sender

**Localhost Normalization:**
When testing on same machine, peer IP shows as `127.0.0.1`. The server normalizes this to the actual local IP to ensure proper connection tracking:

```rust
let mut peer_ip = peer_addr.ip().to_string();
if peer_ip == "127.0.0.1" || peer_ip == "::1" {
    if let Ok(local_ip) = crate::network::transfer::get_local_ip() {
        peer_ip = local_ip;
    }
}
```

**WebSocket Message Handling:**
- Text messages: Parsed as `ChatMessage`, emitted to frontend
- Ping/Pong: Automatically responded to maintain connection
- Close: Gracefully removes connection from HashMap
- Connection reset/Broken pipe errors: Filtered from logs (expected during disconnect)

**Auto-Reconnect (Frontend):**
Uses `useRef` to avoid closure staleness in event listeners:

```typescript
const chatConnectedRef = useRef<boolean>(false);
const activeChatIpRef = useRef<string | null>(null);

const unlistenDisconnected = listen<string>('chat-disconnected', (event) => {
  const peerIp = event.payload;
  if (peerIp === activeChatIpRef.current) {
    setChatConnected(false);
    chatConnectedRef.current = false;
  }
});

const unlistenConnected = listen<string>('chat-connected', (event) => {
  const peerIp = event.payload;
  if (peerIp === activeChatIpRef.current && !chatConnectedRef.current) {
    // Reconnected to current peer
    setTimeout(async () => {
      await invoke('connect_to_chat', { targetIp: peerIp });
    }, 300);
    setChatConnected(true);
    chatConnectedRef.current = true;
  }
});
```

Key insight: Event listeners cannot depend on state variables (they capture stale closures). Use `useRef` and check `.current` value inside listener.

**Auto-Accept (Frontend):**
```typescript
if (!activeChatIpRef.current) {
  // Not in a chat session, auto-accept incoming connection
  setActiveChatIp(peerIp);
  activeChatIpRef.current = peerIp;
  setTimeout(async () => {
    await invoke('connect_to_chat', { targetIp: peerIp });
  }, 300);
}
```

### Flow Example
1. User A enters Chat mode → `start_chat_server()` starts server on 7879
2. User A clicks Device B → `connect_to_chat(B_IP)` creates client connection
3. Device B's server accepts connection → emits `chat-connected` event
4. Device B auto-accepts → calls `connect_to_chat(A_IP)` back to A's server
5. Both sides now have bidirectional connections in `ChatConnections` HashMap
6. Either side sends messages via `send_chat_message()` → writes to stored `WsWriter`
7. If connection drops → `chat-disconnected` event → auto-reconnect via `chat-connected` listener

### Limitations (MVP)
- **Single session:** Can only chat with one device at a time
- **No persistence:** Messages stored in React state, cleared on refresh
- **No typing indicators:** Future enhancement
- **Android background:** Chat server stops when app backgrounded (Tauri limitation)

## Clipboard Sync (WebSocket)

### Protocol
- **Port:** 7880 (TCP/WebSocket)
- **Architecture:** Same dual server/client pattern as chat
- **Message Format:** JSON-encoded `ClipboardMessage`
```json
{
  "content": "clipboard text",
  "from_ip": "192.168.1.10",
  "timestamp": 1706745600000,
  "hash": "a1b2c3d4e5f6"
}
```

### Key Features
- **Multi-device:** Can connect to multiple devices simultaneously (unlike chat)
- **Auto-sync:** Optional 500ms polling to detect and broadcast clipboard changes
- **Manual sync:** Button to immediately sync current clipboard
- **Anti-echo:** Content hash prevents infinite loops when receiving synced content
- **History:** Last 50 sync events displayed in UI

### Implementation Details

**Desktop Clipboard Access:**
- Uses `arboard` crate for cross-platform clipboard access
- Conditional compilation: `#[cfg(not(target_os = "android"))]`

**Android Clipboard Access:**
- Uses `ClipboardManager` system service via Kotlin plugin
- `getClipboard()` - reads primary clip as text
- `setClipboard()` - sets primary clip with plain text

**Polling Mechanism:**
```rust
static CLIPBOARD_POLLING_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_CLIPBOARD_HASH: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

// 500ms interval polling
// Computes content hash, compares with last known hash
// Broadcasts to all connections if changed
```

**Anti-Echo Logic:**
1. When receiving clipboard, update `LAST_CLIPBOARD_HASH` before setting system clipboard
2. When polling, skip if current hash matches `LAST_CLIPBOARD_HASH`
3. Prevents received content from being immediately broadcast back

### Events
| Event | Payload | Description |
|-------|---------|-------------|
| clipboard-connected | string (IP) | Device connected |
| clipboard-disconnected | string (IP) | Device disconnected |
| clipboard-received | ClipboardMessage | Received clipboard from peer |
| clipboard-sent | ClipboardMessage | Local clipboard broadcast |
| clipboard-server-error | string | Server error |

## Network Ports

| Port  | Protocol | Purpose |
|-------|----------|---------|
| 37821 | UDP      | Device discovery (multicast) |
| 7878  | TCP/WS   | File transfer |
| 7879  | TCP/WS   | Chat (bidirectional messaging) |
| 1420  | TCP      | Vite dev server |
| 1421  | TCP      | Vite HMR |

## Key Dependencies

### Rust (`src-tauri/Cargo.toml`)
- `tokio` - Async runtime
- `tokio-tungstenite` - WebSocket server (file transfer + chat)
- `futures-util` - Stream utilities (WebSocket split for chat)
- `socket2` - Low-level socket control (SO_REUSEADDR, multicast)
- `hostname` - Get device hostname (desktop only, not available on Android)
- `dirs` - Cross-platform directories
- `tauri-plugin-dialog` - Native file dialogs (desktop folder picker)
- `base64` - Base64 encoding for SAF write chunks (Android only)
- `serde` / `serde_json` - Serialization for ChatMessage protocol

### Frontend (`package.json`)
- `@tauri-apps/api` - Tauri IPC
- `tailwindcss` v4 - Styling
- `@tailwindcss/postcss` - PostCSS plugin

## Application Modes

1. **Select Mode** - Choose between send/receive/chat
2. **Send Mode** - Select file, discover devices, send to target
3. **Receive Mode** - Start WebSocket server, wait for incoming files
4. **Chat Mode** - Real-time text messaging with auto-accept and auto-reconnect

## Android Storage Access Framework (SAF)

### Architecture
Android 11+ enforces scoped storage, so direct file paths like `/storage/emulated/0/Download` may not be writable. The app uses SAF via a custom Tauri plugin:

```
Rust (android_storage.rs)  ──run_mobile_plugin──>  Kotlin (StoragePlugin.kt)
         │                                                    │
   PluginHandle<Wry>                                  DocumentsContract API
   Payload structs (serde)                            OutputStream management
```

### Plugin Registration
- **Plugin name:** `"android-storage"` (Rust builder)
- **Plugin identifier:** `"app.tauri.storage"` (Android package for `register_android_plugin`)
- **Kotlin class:** `app.tauri.storage.StoragePlugin`
- **Kotlin file path:** `src-tauri/gen/android/app/src/main/java/app/tauri/storage/StoragePlugin.kt`

### File Receive Flow on Android (SAF path)
1. User picks folder via SAF -> returns `content://` URI
2. WebSocket server starts with `content://` URI as `save_dir`
3. On Text message (file metadata): `open_writer(tree_uri, file_name)` creates file via `DocumentsContract.createDocument`, returns handle
4. On Binary message: `write_chunk(handle, base64_data)` writes data to OutputStream
5. On connection close: `close_writer(handle)` flushes and closes stream

### Tauri v2 Android Plugin API Notes
- Use `@InvokeArg` annotated classes + `invoke.parseArgs(Class)` to read arguments (NOT `invoke.getString`)
- Field names in `@InvokeArg` classes must match the Rust serde field names exactly (snake_case)
- Primitive types like `Long` cannot use `lateinit`; use `var handle: Long = 0` instead
- Use `invoke.resolve(JSObject)` / `invoke.reject(String)` to return results
- **`invoke.resolve()` 必须传 JSObject**：不带参数的 `invoke.resolve()` 会返回空 `{}`，如果 Rust 端 `run_mobile_plugin::<T>()` 期望反序列化具体字段（如 `{ ok: bool }`）会失败。即使 Kotlin 端操作已完成，Rust 端也会收到错误。始终构造 `JSObject` 并 `put` 字段后再 `resolve`。
- Tauri 插件初始化晚于 Activity STARTED，**不能**用 `activity.registerForActivityResult`（会抛 IllegalStateException）
- 必须用 `(activity as ComponentActivity).activityResultRegistry.register(key, contract, callback)` 代替（不检查 lifecycle 状态）
- Plugin constructor receives `Activity`, cast to `ComponentActivity` for `activityResultRegistry`

### WebSocket Server Save Directory Hot-Update

WebSocket 服务器启动后保存目录可动态更新，无需重启服务器：

```rust
// 全局保存目录，每次新连接时读取最新值
static CURRENT_SAVE_DIR: Mutex<String> = Mutex::new(String::new());
```

- `start_websocket_server()` 始终更新 `CURRENT_SAVE_DIR`，仅在服务器未运行时启动新服务器
- 每次新的 WebSocket 连接到来时，从 `CURRENT_SAVE_DIR` 读取最新目录
- 解决了用户在接收模式下切换保存目录后文件仍存到旧目录的 bug

### Android content:// URI 显示

前端 `formatSaveDir()` 函数将 Android SAF 的 `content://` URI 转换为可读路径：
- `content://com.android.externalstorage.documents/tree/primary%3ADownload` → `内部存储/Download`
- 普通文件路径保持原样

## App Icon

- **源文件:** `src-tauri/icons/icon.png` (1024x1024)
- **生成命令:** `yarn tauri icon src-tauri/icons/icon.png` — 自动生成所有平台所需的尺寸
- **桌面端窗口图标:** 由 `tauri::generate_context!()` 宏在编译时从 `icons/icon.ico` 自动嵌入，修改后需 `cargo clean` 再构建才能生效
- **Tauri v2 不支持** `Builder::default_window_icon()` 或 `tauri.conf.json` 中 `windows[].icon` 字段

## CI/CD

- **配置文件:** `.github/workflows/release.yml`
- **触发条件:** 推送 `v*` 格式的 git tag（如 `v0.1.0`）
- **构建产物:** Windows 桌面安装包 + Android 通用 APK
- **发布方式:** 自动发布到 GitHub Releases（非草稿）
- **重新发布同版本:** 删除旧 tag 后在新 commit 上重建，推送触发 CI：
  ```bash
  git tag -d v0.1.0
  git push origin :refs/tags/v0.1.0
  git tag v0.1.0
  git push origin v0.1.0
  ```

## Testing

### Chat Feature Testing

**Basic Flow:**
1. Start two app instances (can be on same machine or different devices)
2. Both enter Chat mode
3. Device A clicks on Device B in device list
4. Verify both sides show "已连接" (Connected) status
5. Send messages from both sides, verify real-time delivery
6. Verify message bubbles show correct alignment (own messages right, peer left)
7. Click disconnect, verify both return to device selection screen

**Auto-Reconnect:**
1. Establish chat connection
2. Kill one app instance
3. Restart killed instance, enter Chat mode
4. Verify connection automatically restores within 300ms
5. Send messages to confirm bidirectional communication restored

**Auto-Accept:**
1. Device A in Chat mode (not connected to anyone)
2. Device B enters Chat mode, clicks on Device A
3. Verify Device A automatically enters chat session with B (no manual accept needed)
4. Verify connection is bidirectional

**Same-Machine Testing:**
- Run multiple instances on localhost
- Verify `127.0.0.1` is normalized to actual local IP
- Verify messages route correctly between instances

**Edge Cases:**
- Send empty message → input button should be disabled
- Send very long message → verify text wraps in bubble
- Send emoji and special characters → verify correct rendering
- Rapid message sending → verify order preserved
- Click copy button → verify "✓ 已复制" feedback shows for 1.5s

**Cross-Platform:**
- Desktop ↔ Desktop
- Desktop ↔ Android
- Android ↔ Android

## Important Notes

- WebSocket servers only start in Receive/Chat modes (prevents port conflicts)
- Chat server (7879) and file transfer server (7878) can run simultaneously
- UDP discovery uses multicast (239.255.77.88) instead of broadcast for Android compatibility
- Multiple instances on same machine work via `instance_id` differentiation
- Uses `SO_REUSEADDR` for UDP and chat WebSocket to allow multiple processes on same port
- File transfer uses browser WebSocket (frontend), not Rust (simpler, works cross-platform)
- Chat uses Rust WebSocket server for bidirectional communication (tokio-tungstenite)
- Android file writing uses SAF (content:// URIs) via base64-encoded chunks through the plugin bridge
- Android file reading uses SAF with `read_uri_chunk()` for sending (256KB chunks)
- Android requires `MulticastLock` for UDP multicast (acquired in `MainActivity.kt`)
- Desktop warnings about unused Android-specific code are expected and harmless
- Chat messages are not persisted - cleared on app close/refresh

### Multi-File Transfer & Progress

- **Multi-file support:** Desktop uses HTML input multiple + drag-drop, Android uses native `ACTION_OPEN_DOCUMENT` with `EXTRA_ALLOW_MULTIPLE`
- **Serial transfer:** Files sent one by one to avoid network congestion and simplify implementation
- **Progress bars:** Real-time progress on both sender (green) and receiver (blue) sides
- **Cancellation:** Cancel button in progress bar, cancels current file and stops queue
- **Auto-cleanup:** Incomplete files automatically deleted on desktop (receiver checks `bytes_received == total_bytes`)
- **File queue UI:** Shows all selected files with individual status (pending/sending/completed/failed)
- **Delete from queue:** Trash icon on pending files allows removal before sending
- **Protocol backward compatible:** Old clients ignore new `size`, `index`, `total` fields (via `#[serde(default)]`)

### Receiver Cancel & Transfer Notification (Latest Update)

- **Receiver cancel:** Receiver can cancel ongoing file transfer via cancel button in progress bar
- **Bidirectional cancel notification:** Both sender and receiver are notified when the other side cancels
  - Receiver cancel → sends Close(4001) → sender shows "对方已取消接收"
  - Sender cancel → receiver shows amber notification "传输已取消: filename" (auto-dismiss 3s)
- **Android SAF file deletion:** Incomplete files on Android are properly deleted via `DocumentsContract.deleteDocument` instead of just closing the writer
- **`open_writer()` returns document URI:** `OpenWriterResponse` now includes `document_uri` field alongside `handle`, stored in `documentUris` HashMap in Kotlin plugin
- **Android default save directory:** Changed from app-internal directory to `/storage/emulated/0/Download`
- **Broken pipe handling:** Android sender detects receiver cancel by reading Close(4001) from buffer after write error, with frontend fallback for "Broken pipe" / "Connection reset"
- **Simplified send UI:** Merged overall progress bar and "Sending to IP" status into one section, removed redundant status bar during desktop multi-file sending
