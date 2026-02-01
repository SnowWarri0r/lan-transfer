<div align="center">

<img src="src-tauri/icons/128x128@2x.png" alt="LAN Transfer" width="128" />

# LAN Transfer

**Lightweight cross-platform LAN file transfer app built with Tauri v2**

[![Stars](https://img.shields.io/github/stars/SnowWarri0r/lan-transfer?style=flat-square&logo=github&color=yellow)](https://github.com/SnowWarri0r/lan-transfer/stargazers)
[![License](https://img.shields.io/github/license/SnowWarri0r/lan-transfer?style=flat-square&color=blue)](LICENSE)
[![Release](https://img.shields.io/github/v/release/SnowWarri0r/lan-transfer?style=flat-square&color=green&label=release)](https://github.com/SnowWarri0r/lan-transfer/releases)
[![Rust](https://img.shields.io/badge/Rust-1.70+-f74c00?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24c8db?style=flat-square&logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux%20%7C%20Android-brightgreen?style=flat-square)]()

A fast, zero-config file transfer tool for local networks.
Select a file, pick a device, and send — no cloud, no account, no internet required.

</div>

---

## Features

- **Cross-Platform** — Windows, macOS, Linux desktop + Android
- **Auto Discovery** — Devices find each other automatically via UDP multicast
- **Real-Time Chat** — Text messaging between devices with auto-reconnect
- **Multi-File Transfer** — Send multiple files in one batch with queue management
- **Progress Tracking** — Real-time progress bars on both sender and receiver sides
- **Cancellable Transfers** — Both sender and receiver can cancel anytime, with cross-device notification and auto-cleanup
- **Drag & Drop** — Drag files directly to the app (desktop)
- **Android Multi-Select** — Native file picker with multi-select support
- **No Setup** — No server, no registration, just open and go
- **Large File Support** — Streaming transfer with backpressure, handles files of any size
- **Android SAF** — Full Storage Access Framework support for modern Android scoped storage
- **Lightweight** — Small binary, minimal resource usage

## How It Works

### File Transfer

```
┌──────────────┐  UDP Multicast   ┌──────────────┐
│   Device A   │◄────────────────►│   Device B   │
│  (Sender)    │                  │  (Receiver)  │
│              │   WebSocket      │              │
│  Select File ├─────────────────►│  Save File   │
└──────────────┘   ws://ip:7878   └──────────────┘
```

1. Both devices open the app on the same LAN
2. Receiver enters **Receive Mode**, starts listening
3. Sender enters **Send Mode**, selects a file
4. Sender picks a discovered device (or enters IP manually)
5. File transfers directly over WebSocket

### Chat

```
┌──────────────┐  Bidirectional   ┌──────────────┐
│   Device A   │  WebSocket Chat  │   Device B   │
│              │◄────────────────►│              │
│  Chat Server │   ws://ip:7879   │  Chat Server │
└──────────────┘                  └──────────────┘
```

1. Enter **Chat Mode** on both devices
2. Pick a device to chat with (or auto-accept incoming connection)
3. Send messages in real-time with auto-reconnect
4. Each device runs both server and client for bidirectional communication

## Screenshots

| Select Mode | Send Mode | Receive Mode | Chat Mode |
|:-----------:|:---------:|:------------:|:---------:|
| Choose role | Pick device & send | Wait for files | Real-time messaging |

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) >= 1.70
- [Yarn](https://yarnpkg.com/) v1
- For Android: [Android Studio](https://developer.android.com/studio) + NDK

### Install & Run

```bash
# Clone
git clone https://github.com/SnowWarri0r/lan-transfer.git
cd lan-transfer

# Install dependencies
yarn install

# Run desktop dev
yarn tauri dev

# Run on Android device (USB debugging)
yarn tauri android dev
```

### Build

```bash
# Desktop (produces .exe / .dmg / .AppImage)
yarn tauri build

# Android (produces APK + AAB)
yarn tauri android build
```

## Tech Stack

| Layer    | Technology |
|----------|------------|
| Frontend | React 18 + TypeScript + Tailwind CSS v4 |
| Backend  | Rust + Tokio async runtime |
| Framework | Tauri v2 |
| Transfer | WebSocket (tokio-tungstenite) |
| Discovery | UDP Multicast (socket2) |
| Android Storage | SAF via custom Tauri plugin |

## Architecture

```
src/
  App.tsx              # React UI — mode selection, device list, file transfer, chat

src-tauri/src/
  lib.rs               # Entry point, plugin & command registration
  android_storage.rs   # Rust ↔ Kotlin plugin bridge for Android SAF
  network/
    transfer.rs        # Discovery, WebSocket server, file I/O
    chat.rs            # Bidirectional WebSocket chat (dual server/client)

src-tauri/gen/android/.../
  app/tauri/storage/
    StoragePlugin.kt   # Android SAF: folder picker, file write via DocumentsContract
  com/tauri_app/app/
    MainActivity.kt    # Multicast lock for UDP discovery on Android
```

## Network Ports

| Port  | Protocol | Purpose |
|-------|----------|---------|
| 37821 | UDP      | Device discovery (multicast 239.255.77.88) |
| 7878  | TCP/WS   | File transfer |
| 7879  | TCP/WS   | Chat (bidirectional messaging) |

## Contributing

Issues and PRs are welcome! Feel free to open an [issue](https://github.com/SnowWarri0r/lan-transfer/issues) for bug reports or feature requests.

## License

[MIT](LICENSE) &copy; [SnowWarri0r](https://github.com/SnowWarri0r)

---

<div align="center">

If this project helps you, consider giving it a :star:

</div>
