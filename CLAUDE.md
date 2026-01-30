# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Tauri v2 application for LAN file transfer with a React + TypeScript frontend and Rust backend. The app allows users to send and receive files over a local network using UDP broadcast discovery and WebSocket file transfer.

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
- `App.tsx` - Main UI with mode selection (send/receive), device list, file transfer
- `index.css` - Tailwind CSS 4 entry point
- Uses browser's native WebSocket API for sending files

**Backend (`src-tauri/src/`)**
- `main.rs` / `lib.rs` - Entry point, registers Tauri commands and plugins
- `network/transfer.rs` - All network logic:
  - `start_discovery()` - UDP multicast device discovery
  - `start_websocket_server()` - File receiving server (supports both file path and SAF content:// URI)
  - `get_local_ip()` - Get local network IP
  - `get_download_dir()` - Get system download directory
  - `select_folder()` - Native folder picker dialog (desktop: tauri-plugin-dialog, Android: SAF)
- `android_storage.rs` - Android Storage Access Framework (SAF) plugin bridge:
  - Rust-side plugin that communicates with Kotlin `StoragePlugin` via `run_mobile_plugin`
  - Methods: `pick_folder()`, `open_writer()`, `write_chunk()`, `close_writer()`

**Android Plugin (`src-tauri/gen/android/app/src/main/java/`)**
- `app/tauri/storage/StoragePlugin.kt` - Kotlin-side SAF implementation:
  - `pickFolder` - Launches `ACTION_OPEN_DOCUMENT_TREE` via `registerForActivityResult`, returns `content://` URI
  - `openWriter` - Creates file via `DocumentsContract.createDocument`, returns handle
  - `writeChunk` - Writes base64-encoded data to the OutputStream for a given handle
  - `closeWriter` - Flushes and closes the OutputStream
- `com/tauri_app/app/MainActivity.kt` - Acquires `WifiManager.MulticastLock` for UDP multicast discovery

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
  2. Sends JSON: `{"name": "filename.ext"}`
  3. Sends binary data (file contents)
  4. Closes connection

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
// On Close: emit "file-received" event
```

## Network Ports

| Port  | Protocol | Purpose |
|-------|----------|---------|
| 37821 | UDP      | Device discovery broadcast |
| 7878  | TCP/WS   | File transfer |
| 1420  | TCP      | Vite dev server |
| 1421  | TCP      | Vite HMR |

## Key Dependencies

### Rust (`src-tauri/Cargo.toml`)
- `tokio` - Async runtime
- `tokio-tungstenite` - WebSocket server
- `socket2` - Low-level socket control (SO_REUSEADDR, multicast)
- `hostname` - Get device hostname (desktop only, not available on Android)
- `dirs` - Cross-platform directories
- `tauri-plugin-dialog` - Native file dialogs (desktop folder picker)
- `base64` - Base64 encoding for SAF write chunks (Android only)

### Frontend (`package.json`)
- `@tauri-apps/api` - Tauri IPC
- `tailwindcss` v4 - Styling
- `@tailwindcss/postcss` - PostCSS plugin

## Application Modes

1. **Select Mode** - Choose between send/receive
2. **Send Mode** - Select file, discover devices, send to target
3. **Receive Mode** - Start WebSocket server, wait for incoming files

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

## Important Notes

- WebSocket server only starts in Receive mode (prevents port conflict)
- UDP discovery uses multicast (239.255.77.88) instead of broadcast for Android compatibility
- Multiple instances on same machine work via `instance_id` differentiation
- Uses `SO_REUSEADDR` for UDP to allow multiple processes on same port
- File transfer uses browser WebSocket (frontend), not Rust (simpler, works cross-platform)
- Android file writing uses SAF (content:// URIs) via base64-encoded chunks through the plugin bridge
- Android requires `MulticastLock` for UDP multicast (acquired in `MainActivity.kt`)
- Desktop warnings about unused Android-specific code are expected and harmless
