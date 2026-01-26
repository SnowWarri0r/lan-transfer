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
- `main.rs` - Entry point, registers Tauri commands and plugins
- `network/transfer.rs` - All network logic:
  - `start_discovery()` - UDP broadcast device discovery
  - `start_websocket_server()` - File receiving server
  - `get_local_ip()` - Get local network IP
  - `get_download_dir()` - Get system download directory
  - `select_folder()` - Native folder picker dialog

## Device Discovery (UDP Broadcast)

### Protocol
- **Port:** 37821 (UDP)
- **Address:** 255.255.255.255 (broadcast)
- **Message Format:** `FILETRANSFER:IP:HOSTNAME:INSTANCE_ID`
- **Example:** `FILETRANSFER:192.168.1.10:MyPC:12345`

### How It Works
1. Each app instance binds to UDP port 37821 with `SO_REUSEADDR` (allows multiple processes)
2. Every 3 seconds, broadcasts its presence to `255.255.255.255:37821`
3. Listens for broadcasts from other devices
4. Filters out self using `instance_id` (process ID), not IP (same machine can have multiple instances)
5. Removes devices not seen for 30 seconds
6. Emits `devices-updated` event to frontend when list changes

### Key Implementation Details
```rust
// Enable port reuse for multiple instances
let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
socket.set_reuse_address(true)?;  // Critical for multi-instance
socket.set_broadcast(true)?;
socket.set_nonblocking(true)?;
socket.bind("0.0.0.0:37821")?;
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
- `socket2` - Low-level socket control (SO_REUSEADDR)
- `hostname` - Get device hostname
- `dirs` - Cross-platform directories
- `tauri-plugin-dialog` - Native file dialogs

### Frontend (`package.json`)
- `@tauri-apps/api` - Tauri IPC
- `tailwindcss` v4 - Styling
- `@tailwindcss/postcss` - PostCSS plugin

## Application Modes

1. **Select Mode** - Choose between send/receive
2. **Send Mode** - Select file, discover devices, send to target
3. **Receive Mode** - Start WebSocket server, wait for incoming files

## Important Notes

- WebSocket server only starts in Receive mode (prevents port conflict)
- UDP discovery runs in both modes (for device visibility)
- Multiple instances on same machine work via `instance_id` differentiation
- Uses `SO_REUSEADDR` for UDP to allow multiple processes on same port
- File transfer uses browser WebSocket (frontend), not Rust (simpler, works cross-platform)
