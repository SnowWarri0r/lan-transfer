# LAN Transfer

> Lightweight cross-platform LAN file transfer app built with Tauri v2

A fast, zero-config file transfer tool for local networks. Select a file, pick a device, and send — no cloud, no account, no internet required.

## Features

- **Cross-Platform** — Windows, macOS, Linux desktop + Android
- **Auto Discovery** — Devices find each other automatically via UDP multicast
- **No Setup** — No server, no registration, just open and go
- **Large File Support** — Streaming transfer with backpressure, handles files of any size
- **Android SAF** — Full Storage Access Framework support for modern Android scoped storage
- **Lightweight** — Small binary, minimal resource usage

## How It Works

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

## Screenshots

| Select Mode | Send Mode | Receive Mode |
|:-----------:|:---------:|:------------:|
| Choose role | Pick device & send | Wait for files |

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
  App.tsx              # React UI — mode selection, device list, file transfer

src-tauri/src/
  lib.rs               # Entry point, plugin & command registration
  android_storage.rs   # Rust ↔ Kotlin plugin bridge for Android SAF
  network/
    transfer.rs        # Discovery, WebSocket server, file I/O

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

## License

MIT
