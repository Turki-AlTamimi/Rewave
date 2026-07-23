# Rewave — Rewrite Design Document

**Date:** 2026-07-23
**Status:** Design — awaiting approval

This document defines the architecture and component design for rewriting the Rewave Windows sender as a Tauri desktop application with a shared web UI, while preserving the Android receiver (Kotlin + C++ Oboe) and wrapping it in a WebView for a unified cross-platform UI.

---

## Table of contents

1. [Architecture overview](#1-architecture-overview)
2. [Shared web UI](#2-shared-web-ui)
3. [Windows: Rust backend](#3-windows-rust-backend)
4. [Android: WebView wrapper](#4-android-webview-wrapper)
5. [Communication contracts](#5-communication-contracts)
6. [Connection orchestration](#6-connection-orchestration)
7. [Audio pipeline (Windows)](#7-audio-pipeline-windows)
8. [Crypto & protocol (Rust port)](#8-crypto--protocol-rust-port)
9. [Discovery](#9-discovery)
10. [Pairing flow](#10-pairing-flow)
11. [Wi-Fi Aware implementation](#11-wi-fi-aware-implementation)
12. [Theming](#12-theming)
13. [Project structure](#13-project-structure)
14. [Known risks & mitigations](#14-known-risks--mitigations)

---

## 1. Architecture overview

```
┌──────────────────────────────────────────────────────────────┐
│                   SHARED WEB UI                              │
│             React 18 + HeroUI + Vite + Tailwind CSS          │
│                                                              │
│  ┌──────────┐ ┌───────────┐ ┌──────────┐ ┌──────────────┐   │
│  │ Devices  │ │ Dashboard │ │ Settings │ │ Pair New     │   │
│  └──────────┘ └───────────┘ └──────────┘ └──────────────┘   │
│                                                              │
│  Monorepo package: rewave-ui/                                │
│  Builds to static assets consumed by both platform shells    │
└────────────┬─────────────────────────┬───────────────────────┘
             │                         │
  ┌──────────▼──────────────┐  ┌───────▼───────────────────────┐
  │  WINDOWS — Tauri Shell  │  │  ANDROID — WebView Shell      │
  │  (Rust backend)         │  │  (Kotlin + existing NDK)       │
  │                         │  │                                │
  │  Crate: rewave-core     │  │  Kotlin layer:                 │
  │  ├─ audio (WASAPI)      │  │  ├─ WebView hosts rewave-ui    │
  │  ├─ stream (UDP pacing) │  │  ├─ JS bridge                  │
  │  ├─ crypto (HKDF/ECDH)  │  │  │   (@JavascriptInterface)    │
  │  ├─ protocol (M1/M6)    │  │  ├─ delegates to existing:     │
  │  ├─ discovery           │  │  │   ├─ AudioReceiver          │
  │  │   ├─ mDNS            │  │  │   ├─ AuthSession            │
  │  │   ├─ broadcast       │  │  │   ├─ Native Oboe engine     │
  │  │   └─ Wi-Fi Aware     │  │  │   ├─ RewaveLink             │
  │  ├─ connection orch.    │  │  │   ├─ PairingStore           │
  │  ├─ pairing store       │  │  │   └─ RewaveDiscovery        │
  │  ├─ WebSocket server    │  │  ├─ NAN publisher (new)        │
  │  └─ Tauri IPC handlers  │  │  └─ AudioStreamService         │
  │                         │  │                                │
  │  Binary: rewave-app     │  │  (UI assets bundled in assets/) │
  └─────────────────────────┘  └────────────────────────────────┘
```

### Design principles

- **One UI, two platforms.** The `rewave-ui/` React package compiles to static assets. Windows loads it via Tauri webview. Android loads it from `assets/` into a system WebView.
- **Keep the Android receiver intact.** The existing Kotlin/C++ audio engine, crypto, and networking are proven and tested. The WebView is a UI-only wrapper.
- **Rust monolith on Windows.** Single crate (`rewave-core`) with clearly separated modules. Not a workspace yet — can be split later if the crate grows unwieldy.
- **Hybrid IPC.** Windows uses Tauri IPC for commands (start/stop, pair/unpair) and WebSocket for real-time streaming stats. Android uses `@JavascriptInterface` bridge for both.

---

## 2. Shared web UI

### Technology stack

| Layer | Choice | Rationale |
|---|---|---|
| Framework | React 18 | Best Tauri support, ecosystem, HeroUI compatibility |
| UI library | HeroUI (NextUI v2) | Polished components, dark mode, Tailwind-native |
| Build tool | Vite | Fast HMR, static export, Tauri plugin |
| Styling | Tailwind CSS 4 | HeroUI requirement, utility-first |
| Routing | React Router v7 | Standard, file-based optional |
| State management | Zustand | Lightweight, no boilerplate |
| Real-time comms | WebSocket (native) | Browser-native, no library needed |

### Pages

| Route | Purpose |
|---|---|
| `/devices` | Device discovery, connect/disconnect, connection mode indicator |
| `/devices/:id` | Per-device dashboard: stats, waveform, spectral display, buffer health |
| `/settings` | Audio format, power mode (low latency / battery saver), Wi-Fi credentials, theme toggle |
| `/pair` | Pairing flow: PIN entry (sender) or PIN display + Confirm (receiver) |

### Component tree (key components)

```
App
├─ Layout
│   ├─ Sidebar
│   │   ├─ Logo + EQ animation
│   │   ├─ NavItem (Devices, Dashboard, Settings)
│   │   ├─ NavSection ("Pairing")
│   │   ├─ NavItem (Paired Devices, Pair New)
│   │   └─ StreamBadge (live indicator + latency)
│   └─ Content (React Router Outlet)
├─ DevicesPage
│   ├─ DiscoveryList (scan results with signal strength)
│   ├─ DeviceCard (name, icon, connection layer, actions)
│   └─ EmptyState ("Scan for devices")
├─ DashboardPage
│   ├─ StatusBadge (Connected / Streaming / Disconnected)
│   ├─ MetricGrid (latency, bitrate, packets, uptime)
│   ├─ WaveformCard (animated L+R bars)
│   ├─ SpectralDisplay (frequency bars)
│   ├─ SessionInfo (link type, format, encryption, pairing, buffer)
│   └─ SourceBar (WASAPI → Oboe, uptime, actions)
├─ SettingsPage
│   ├─ ThemeToggle (dark / light / system)
│   ├─ PowerModeToggle (low latency / battery saver)
│   ├─ AudioSettings (sample rate display, format info)
│   └─ ConnectionSettings (Wi-Fi Direct SSID/pass display)
├─ PairPage
│   ├─ PairMethodSelector (PIN / Confirm)
│   ├─ PinEntry (sender: 4-digit input)
│   ├─ PinDisplay (receiver: shows PIN, copy button)
│   └─ ConfirmCard (ECDH: accept/deny buttons, 30s timeout)
└─ TrayMenu (Windows only: Show Window, Quit)
```

### UI state machine

```
App Launch
  └─ Auto-scan for devices (3s broadcast + 1.5s mDNS + Wi-Fi Aware subscribe)
      ├─ Devices found → show list, user selects one
      │   ├─ Already paired → silent TOFU resume → Dashboard (streaming)
      │   └─ Not paired → PairPage
      │       ├─ ECDH Confirm (default) → Dashboard
      │       └─ PIN fallback → Dashboard
      └─ No devices found → EmptyState with "Scan Again" button
```

### Theming (light + dark)

| Variable | Dark | Light |
|---|---|---|
| Background | `#0b0b10` with indigo radial gradients | `#f8f8fb` with subtle indigo radial gradients |
| Glass surfaces | `rgba(255,255,255,0.03)` + `blur(24px)` | `rgba(255,255,255,0.65)` + `blur(24px)` |
| Glass border | `rgba(255,255,255,0.06)` | `rgba(0,0,0,0.06)` |
| Primary text | `#e8e8ed` | `#1a1a24` |
| Secondary text | `rgba(255,255,255,0.4)` | `rgba(0,0,0,0.35)` |
| Muted text | `rgba(255,255,255,0.2)` | `rgba(0,0,0,0.15)` |
| Accent | `#818cf8` (indigo-400) | `#6366f1` (indigo-500) |
| Accent gradient | `#818cf8 → #a78bfa` | `#6366f1 → #8b5cf6` |
| Success | `#34d399` (emerald-400) | `#059669` (emerald-600) |

Typography:
- Display/headlines: **Instrument Serif** (italic, serif)
- Body/UI: **DM Sans** (sans-serif, variable weight)

Built using HeroUI's theming system — a single Tailwind config with CSS custom properties, toggled via `data-theme` attribute or system `prefers-color-scheme`.

---

## 3. Windows: Rust backend

### Crate: `rewave-core`

Single crate, modules separated by domain:

```
rewave-core/
├── Cargo.toml
├── src/
│   ├── lib.rs              // public API: start/stop/discover/pair
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── capture.rs      // WASAPI loopback via `windows` crate
│   │   └── framer.rs       // downmix → resample → int16 → 960B chunks
│   ├── stream/
│   │   ├── mod.rs
│   │   ├── engine.rs       // 200pps pacing loop, high-res timer
│   │   └── pacing.rs       // wall-clock anchor, resync, PLC-inject
│   ├── crypto/
│   │   ├── mod.rs
│   │   ├── hkdf.rs         // HKDF-SHA256 (RFC 5869)
│   │   ├── ecdh.rs         // ECDH P-256 (secp256r1)
│   │   ├── session.rs      // M6 session key derivation + HMAC tags
│   │   └── constants.rs    // info strings, size constants
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── datagram.rs     // M1/M6 encode/decode, endianness
│   │   ├── control.rs      // REWAVE magic header, 11 message types
│   │   └── dispatch.rs     // length-based dispatch
│   ├── discovery/
│   │   ├── mod.rs
│   │   ├── mdns.rs         // Zeroconf browse + announce
│   │   ├── broadcast.rs    // subnet-directed HELLO broadcast
│   │   └── wifi_aware.rs   // NAN subscribe + data path (Windows)
│   ├── connection/
│   │   ├── mod.rs
│   │   └── orchestrator.rs // 3-layer state machine
│   ├── wifi/
│   │   ├── mod.rs
│   │   ├── direct.rs       // WLANProfile XML + netsh
│   │   └── aware.rs        // WiFiDirectAdvertisementPublisher
│   ├── pairing/
│   │   ├── mod.rs
│   │   └── store.rs        // TOFU store (pairings.json read/write)
│   ├── server/
│   │   ├── mod.rs
│   │   ├── ws.rs           // WebSocket server for UI stats
│   │   └── ipc.rs          // Tauri command handlers
│   └── stats.rs            // Snapshot struct, stat collection
```

### Dependencies (Cargo.toml)

```toml
[dependencies]
# Windows APIs
windows = { version = "0.58", features = [
    "Media_Audio",
    "Media_Devices",
    "Devices_WiFiDirect",
    "Networking_Sockets",
    "System_Threading",
] }

# Crypto
hmac = "0.12"
sha2 = "0.10"
hkdf = "0.12"
p256 = { version = "0.13", features = ["ecdh"] }
rand = "0.8"

# Networking
tokio = { version = "1", features = ["full"] }
socket2 = "0.5"
mdns-sd = "0.10"           # mDNS browse + announce
tokio-tungstenite = "0.24" # WebSocket server

# Tauri
tauri = "2"
tauri-plugin-shell = "2"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bincode = "1"              # fast PCM serialization (optional)

# Utilities
log = "0.4"
env_logger = "0.11"
thiserror = "2"
parking_lot = "0.12"       # faster mutexes
ringbuf = "0.4"            # SPSC ring buffer for audio
```

### Threading model

| Thread | Role |
|---|---|
| **Main (Tauri)** | Window event loop, IPC handlers |
| **Capture** (OS callback) | WASAPI → framer → SPSC ring |
| **Send** (dedicated) | Pop ring, 200pps pacing, UDP send |
| **WebSocket** (tokio task) | Push stats to UI at ~10 Hz |
| **Discovery** (tokio tasks) | mDNS browse/announce, broadcast, Wi-Fi Aware |
| **Connection** (tokio task) | Orchestrator state machine |

### Platform note

The Tauri app targets Windows only (not cross-platform for MVP). `rewave-core` could be split into platform-agnostic + Windows-specific in a future workspace refactor for Linux/macOS support.

---

## 4. Android: WebView wrapper

### Changes to existing receiver

The existing Android receiver (Kotlin + C++ Oboe) gets a **thin WebView wrapper** — no audio path changes:

```
AudioStreamService (existing, modified)
├─ WebView (new)
│   └─ loads rewave-ui/ assets
├─ JS Bridge (new class: RewaveJsBridge)
│   ├─ @JavascriptInterface methods:
│   │   ├─ getDevices(): List<DiscoveredDevice>
│   │   ├─ getThisDevice(): DeviceInfo
│   │   ├─ getPairingCode(): String (PIN)
│   │   ├─ getPairingState(): PairingState
│   │   ├─ confirmPair(peerId: String)
│   │   ├─ denyPair(peerId: String)
│   │   ├─ getStats(): StatsSnapshot
│   │   ├─ getConnectionMode(): ConnectionMode
│   │   ├─ startWifiAware(): Boolean
│   │   ├─ stopWifiAware()
│   │   ├─ disconnect()
│   │   └─ setPowerMode(mode: String)
│   └─ callbacks to JS (evaluateJavascript):
│       ├─ onDeviceDiscovered(device)
│       ├─ onPairRequest(info)
│       ├─ onPairComplete(success)
│       ├─ onStatsUpdate(stats)
│       ├─ onConnectionModeChanged(mode)
│       └─ onLinkLost()
├─ AudioReceiver (unchanged)
├─ AuthSession (unchanged)
├─ Native Oboe engine (unchanged)
├─ RewaveDiscovery (unchanged)
├─ PairingStore (unchanged)
├─ RewaveLink (unchanged)
├─ NAN publisher (new: RewaveAwarePublisher)
│   └─ WifiAwareManager.publish() for discovery
└─ Foreground service notification (unchanged)
```

### New: RewaveAwarePublisher (Wi-Fi Aware on Android)

```kotlin
class RewaveAwarePublisher(
    private val wifiAwareManager: WifiAwareManager,
    private val config: RewaveLinkConfig
) {
    // Publish a NAN service so nearby Windows devices can discover us
    fun publish(): Boolean
    fun createDataPath(peerHandle: PeerHandle): Network
    fun close()
}
```

Uses `WifiAwareManager.attach()` → `PublishConfig.Builder` with service name `"rewave"`. Creates a `Network` via `WifiAwareNetworkSpecifier` for the data path when a peer subscribes.

### Activity changes

`MainActivity` now hosts a single `WebView` instead of Compose UI. The service communication (previously via Compose ViewModel) now goes through the JS bridge.

### Build integration

The Vite build output lands in `app/src/main/assets/rewave-ui/`. A Gradle task copies it during the build.

---

## 5. Communication contracts

### Windows: Tauri IPC commands

All invoked from the web UI via `@tauri-apps/api`:

```typescript
// Commands (request → response)
invoke('start_stream', { host: string, port: number }) → void
invoke('stop_stream') → void
invoke('discover') → DiscoveredDevice[]
invoke('pair_pin', { pin: string, host: string, port: number }) → PairResult
invoke('pair_confirm', { host: string, port: number }) → PairResult
invoke('unpair', { peerId: string }) → void
invoke('set_power_mode', { mode: 'low_latency' | 'battery_saver' }) → void
invoke('connect_wifi', { ssid: string, passphrase: string }) → void
invoke('disconnect_wifi') → void
invoke('get_connection_mode') → ConnectionMode
```

### Windows: WebSocket events

```typescript
// Events (server → UI, ~10 Hz)
interface StatsEvent {
  type: 'stats'
  latency: number           // ms
  bitrate: number           // bps
  packets_sent: number
  uptime_seconds: number
  buffer_depth: number      // packets
  buffer_target: number     // packets
  waveform: number[]        // 28 amplitude values for visualization
  connection_mode: 'aware' | 'lan' | 'direct' | 'none'
  link_lost: boolean
}

interface DiscoveryEvent {
  type: 'discovery'
  devices: DiscoveredDevice[]
}

interface PairingEvent {
  type: 'pairing'
  state: 'requested' | 'confirmed' | 'denied' | 'completed' | 'failed'
  method: 'pin' | 'confirm'
  pin?: string              // only for PIN method
  peer_name?: string        // only for Confirm method
}
```

### Android: JS Bridge

The WebView calls `window.Android.*` (injected by `@JavascriptInterface`):

```typescript
// UI → Android
window.Android.getDevices(): DiscoveredDevice[]
window.Android.getStats(): StatsSnapshot
window.Android.confirmPair(peerId: string)
window.Android.denyPair(peerId: string)
window.Android.disconnect()
window.Android.setPowerMode(mode: 'low_latency' | 'battery_saver')

// Android → UI (via evaluateJavascript)
window.__rewave_onDeviceDiscovered(device: DiscoveredDevice)
window.__rewave_onPairRequest(info: PairRequestInfo)
window.__rewave_onStatsUpdate(stats: StatsSnapshot)
window.__rewave_onConnectionModeChanged(mode: ConnectionMode)
window.__rewave_onLinkLost()
```

---

## 6. Connection orchestration

### State machine

```
CONNECTING
  ├─ Try Wi-Fi Aware (NAN) → subscribe, await peer
  │   ├─ Success → CONNECTED (mode: aware)
  │   └─ Timeout/unsupported → fall through
  │
  ├─ Try Same-LAN → mDNS browse + broadcast
  │   ├─ Found → CONNECTED (mode: lan)
  │   └─ Timeout → fall through
  │
  └─ Try Wi-Fi Direct AP → auto-join via WLANProfile
      ├─ Success → CONNECTED (mode: direct)
      └─ Fail → DISCONNECTED (show manual help)

CONNECTED
  ├─ Link quality monitor (packet recv rate)
  ├─ Degraded → attempt upgrade (aware > lan > direct)
  └─ Lost > 5s → DISCONNECTED (auto-retry)

DISCONNECTED
  └─ Auto-retry with backoff (1s, 2s, 4s, 8s... max 30s)
```

### Priority order

| Priority | Mode | Internet | Latency | Discovery |
|---|---|---|---|---|
| 1 (primary) | Wi-Fi Aware (NAN) | Yes | Best (~30ms) | NAN subscribe/publish |
| 2 | Same-LAN UDP | Yes | Good (~40ms) | mDNS + broadcast |
| 3 | Wi-Fi Direct AP | **No** | Good (~35ms) | mDNS + broadcast |
| 4 | Wi-Fi Direct AP (manual) | **No** | Good (~35ms) | User enters SSID/pass |

---

## 7. Audio pipeline (Windows)

Identical logic to the C# sender, ported to Rust:

### Capture

```
WASAPI loopback (IAudioClient)
  └─ GetBuffer() → float32 stereo interleaved
      └─ AudioFramer
          ├─ Downmix: mono→stereo; surround→avg even/odd; >2ch→avg
          ├─ Resample: only if rate differs from 48kHz by >2%
          ├─ Clip ±1.0, truncate to int16 (no dither)
          └─ Accumulate → emit 960B chunk when 240 frames ready
              └─ Push to ConcurrentQueue<Vec<u8>> (or ringbuf)
```

Implementation via the `windows` crate's `Media::Audio` namespace:
- `IMMDeviceEnumerator` → get default render endpoint
- `IAudioClient::Initialize` in loopback mode (`AUDCLNT_STREAMFLAGS_LOOPBACK`)
- `IAudioCaptureClient::GetBuffer` / `ReleaseBuffer`

### Send (StreamEngine)

```
Dedicated thread, wall-clock anchored:
  nextSend = now + 5ms
  loop:
    wait_until(nextSend)  // high-res waitable timer or spin
    if overslept >10ms: nextSend = now  // resync, don't burst
    chunk = queue.pop_or_repeat_last(max 3 repeats, then silence)
    datagram = build_m1_or_m6(chunk, seq, session_key)
    udp_send(datagram, receiver_addr, 50000)
    seq++
    nextSend += 5ms
```

**Pacing primitive:** `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` via `windows` crate (Win10 1803+). Fallback: spin-wait with `thread::yield_now()`.

### Silence injection / PLC

If capture queue is empty:
- Miss 1–3: repeat last real chunk (PLC)
- Miss 4+: emit zero chunk
Sequence numbers always advance.

---

## 8. Crypto & protocol (Rust port)

**Byte-identical to the existing Kotlin implementation.** Every endianness choice, HKDF info string, and nonce order is preserved from Rewave.md §6.

### Rust crypto stack

| Primitive | Crate | Notes |
|---|---|---|
| SHA-256 | `sha2` | |
| HMAC-SHA256 | `hmac` + `sha2` | |
| HKDF-SHA256 | `hkdf` + `sha2` | RFC 5869 |
| ECDH P-256 | `p256` + `rand_core` | secp256r1 |

### Critical gotchas (frozen, cross-implementation)

1. **Nonce order:** response HMAC = `HMAC(SenderNonce ‖ ReceiverNonce)`; HKDF salt = `ReceiverNonce ‖ SenderNonce`
2. **M6 tag:** covers `seq_BE_4 ‖ pcm_960` (964 bytes) — NOT flags, NOT full datagram
3. **Seq is big-endian u32; PCM is little-endian int16.** Three endiannesses total.
4. **Datagram length dispatches M1 (965B) vs M6 (973B).** No in-band flag.
5. **Control header:** 6-byte `REWAVE` magic (`0x52 0x45 0x57 0x41 0x56 0x45`) + version byte + type byte + length u16 BE
6. **ECDH Confirm IKM:** `shared(32) ‖ Ns(16) ‖ Nr(16)` = 64 bytes
7. **TOFU store:** `%APPDATA%\rewave\pairings.json` — same schema

### Test strategy

The `crypto` and `protocol` modules are the highest-risk code. They MUST be tested against known-answer test vectors generated from the existing Kotlin implementation:

1. Capture real wire traffic (tcpdump/UDP dump) from a working C# → Kotlin session
2. Extract: M1 datagram, M6 datagram, HELLO, CHALLENGE, HANDSHAKE_RESPONSE, PAIR_RESUME
3. Write Rust unit tests that decode these known byte arrays and produce identical output
4. Write Rust unit tests that encode and produce byte-identical output

At minimum, the following must pass byte-identical comparison:
- HKDF derivation (pairing key, key ID, session key)
- M6 tag computation
- ECDH shared secret derivation
- Control message encode/decode for all 11 types
- M1/M6 datagram encode/decode

---

## 9. Discovery

Three discovery mechanisms, tried sequentially:

### Broadcast discovery

- Enumerate local IPv4 addresses
- Compute `/24` subnet-directed broadcast + `255.255.255.255` fallback
- Send v2 HELLO to `(bcast_addr, 50001)` every 1s for 3s
- Listen for HERE + CHALLENGE responses on sender's ephemeral port

**Known limitation (from Rewave.md §15.1):** `/24` assumption fails on `/16`/`/8` subnets. mDNS compensates. Compute broadcast from actual netmask as a follow-up.

### mDNS discovery

- Browse `_rewave._udp.` via `mdns-sd` crate (~1.5s browse)
- Announce `_rewave-sender._udp.` for receiver-side visibility
- Extract host, port, name, paired key_ids from TXT records

### Wi-Fi Aware discovery

- Subscribe to NAN service `"rewave"` via `Windows.Devices.WiFiDirect`
- On peer found → establish data path → get IP + port
- Proceed to auth

---

## 10. Pairing flow

### ECDH Confirm (primary, no PIN)

1. Sender sends `PAIR_REQUEST` to `(receiver, 50001)` with `(name, sender_pubkey, sender_nonce)`
2. Receiver renders `PairRequestCard` in WebView UI, user taps **Confirm** or **Deny**
3. On Confirm: receiver sends `PAIR_CONFIRM` with `(receiver_pubkey, receiver_nonce, key_id)`
4. Sender derives `pairing_key`, verifies `key_id`, derives `session_key`
5. Stream audio as 973B M6 datagrams → `(receiver, 50000)`
6. Persist to TOFU store on both sides

**Sender UI**: Shows "Waiting for confirmation on tablet..." with a spinner and 30s timeout.

### PIN (fallback)

1. Receiver generates 4-digit PIN, displays it in WebView
2. User types PIN into Windows UI (--pin equivalent)
3. Sender sends `HANDSHAKE_RESPONSE` to `(receiver, 50000)`
4. On success: derive synthetic pairing key, persist for future TOFU resume

### TOFU Resume (silent, preferred)

1. On every subsequent launch, sender reads `pairings.json`, finds matching `peerId` for target IP
2. Sends `PAIR_RESUME` to `(receiver, 50001)`
3. On `PAIR_RESUME_OK`: adopt session key, start streaming
4. **Zero UI.** The web dashboard transitions directly from "Discovering..." to "Streaming"

### Fix: PairRequestCard rendering

The current receiver has a known bug where `PairRequestCard` doesn't render in Compose UI (§15.1 in Rewave.md). In the new architecture, this becomes a WebView-rendered React component — sidestepping the Kotlin/Compose rendering issue entirely.

---

## 11. Wi-Fi Aware implementation

### Windows side

Use `Windows.Devices.WiFiDirect.WiFiDirectAdvertisementPublisher` via the `windows` crate:

```rust
// Conceptual API
let publisher = WiFiDirectAdvertisementPublisher::new()?;
publisher.set_listen_state_discoverability(
    WiFiDirectAdvertisementListenStateDiscoverability::Intensive
)?;
publisher.start()?;

// Subscribe to connection requests
let listener = publisher.connection_requested()?;
while let Some(request) = listener.next().await {
    let device_info = request.device_information();
    let args = WiFiDirectConnectionParameters::new()?;
    args.set_preferred_pairing_procedure(
        WiFiDirectPairingProcedure::Invite
    )?;
    let result = request.accept_async(args)?.await?;
    // result provides the socket/endpoint for audio streaming
}
```

### Android side

Use `WifiAwareManager` via a new Kotlin class `RewaveAwarePublisher`:

```kotlin
val session = wifiAwareManager.attach(
    attachCallback, identityChangedListener, handler
)
val publishConfig = PublishConfig.Builder()
    .setServiceName("rewave")
    .setServiceSpecificInfo(/* device name + key_ids */)
    .build()
session.publish(publishConfig, discoverySessionCallback, handler)
```

### Data path

Both sides negotiate a L2 data path via `WifiAwareNetworkSpecifier` / `WiFiDirectConnectionParameters`. The result is a socket — audio streams over the same M1/M6 protocol, same ports (50000/50001), no wire format changes.

---

## 12. Theming

### CSS custom properties (generated by HeroUI + Tailwind)

```css
:root {
  /* Background */
  --rewave-bg: #0b0b10;
  --rewave-bg-gradient: radial-gradient(ellipse at 50% 0%, rgba(99,102,241,0.04), transparent 50%);

  /* Glass */
  --rewave-glass-bg: rgba(255,255,255,0.03);
  --rewave-glass-border: rgba(255,255,255,0.06);
  --rewave-glass-blur: blur(24px);

  /* Text */
  --rewave-text-primary: #e8e8ed;
  --rewave-text-secondary: rgba(255,255,255,0.4);
  --rewave-text-muted: rgba(255,255,255,0.2);

  /* Accent */
  --rewave-accent: #818cf8;
  --rewave-accent-gradient: linear-gradient(135deg, #818cf8, #a78bfa);

  /* Success */
  --rewave-success: #34d399;
  --rewave-success-bg: rgba(52,211,153,0.06);
}

[data-theme="light"] {
  --rewave-bg: #f8f8fb;
  --rewave-bg-gradient: radial-gradient(ellipse at 50% 0%, rgba(99,102,241,0.03), transparent 50%);
  --rewave-glass-bg: rgba(255,255,255,0.65);
  --rewave-glass-border: rgba(0,0,0,0.06);
  --rewave-text-primary: #1a1a24;
  --rewave-text-secondary: rgba(0,0,0,0.35);
  --rewave-text-muted: rgba(0,0,0,0.15);
  --rewave-accent: #6366f1;
  --rewave-accent-gradient: linear-gradient(135deg, #6366f1, #8b5cf6);
  --rewave-success: #059669;
  --rewave-success-bg: rgba(5,150,105,0.06);
}
```

### Theme detection

- Default: system preference (`prefers-color-scheme`)
- User can override in Settings → persists to `localStorage`
- HeroUI's `NextUIProvider` handles the `dark`/`light` class toggling

---

## 13. Project structure

```
rewave/
├── rewave-ui/                    # Shared web UI (React + HeroUI)
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.ts
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx             # React entry
│   │   ├── App.tsx              # Router + layout
│   │   ├── routes/
│   │   │   ├── devices.tsx
│   │   │   ├── dashboard.tsx
│   │   │   ├── settings.tsx
│   │   │   └── pair.tsx
│   │   ├── components/
│   │   │   ├── layout/
│   │   │   │   ├── sidebar.tsx
│   │   │   │   └── topbar.tsx
│   │   │   ├── devices/
│   │   │   │   ├── device-card.tsx
│   │   │   │   └── discovery-list.tsx
│   │   │   ├── dashboard/
│   │   │   │   ├── metric-grid.tsx
│   │   │   │   ├── waveform-card.tsx
│   │   │   │   ├── spectral-display.tsx
│   │   │   │   ├── session-info.tsx
│   │   │   │   └── source-bar.tsx
│   │   │   ├── pair/
│   │   │   │   ├── pin-entry.tsx
│   │   │   │   ├── pin-display.tsx
│   │   │   │   └── confirm-card.tsx
│   │   │   └── shared/
│   │   │       ├── glass-card.tsx
│   │   │       ├── eq-animation.tsx
│   │   │       └── connection-badge.tsx
│   │   ├── stores/
│   │   │   ├── app-store.ts      # Zustand: global app state
│   │   │   ├── device-store.ts   # device discovery + connection
│   │   │   └── stats-store.ts    # real-time streaming stats
│   │   ├── bridges/
│   │   │   ├── tauri-bridge.ts   # Tauri IPC + WebSocket client
│   │   │   └── android-bridge.ts # Android JS bridge client
│   │   ├── hooks/
│   │   │   ├── use-stats.ts
│   │   │   ├── use-discovery.ts
│   │   │   └── use-pairing.ts
│   │   └── types/
│   │       └── index.ts          # Shared type definitions
│   └── public/
│
├── rewave-core/                   # Rust backend (Windows only)
│   ├── Cargo.toml
│   └── src/                       # (see §3 for module tree)
│
├── rewave-app/                    # Tauri binary + configuration
│   ├── Cargo.toml                 # depends on rewave-core
│   ├── tauri.conf.json
│   ├── icons/
│   └── src/
│       └── main.rs                # Tauri entry, window config, tray
│
├── android/                       # Existing Android receiver + WebView wrapper
│   ├── app/
│   │   ├── build.gradle.kts
│   │   └── src/main/
│   │       ├── java/.../rewave/
│   │       │   ├── MainActivity.kt         # WebView host
│   │       │   ├── AudioStreamService.kt   # (modified: +WebView bridge init)
│   │       │   ├── RewaveJsBridge.kt       # (new)
│   │       │   ├── RewaveAwarePublisher.kt # (new: NAN)
│   │       │   ├── AudioReceiver.kt        # (unchanged)
│   │       │   ├── AuthSession.kt          # (unchanged)
│   │       │   ├── RewaveDiscovery.kt      # (unchanged)
│   │       │   ├── RewaveLink.kt           # (unchanged)
│   │       │   └── PairingStore.kt         # (unchanged)
│   │       ├── cpp/                        # Native Oboe engine (unchanged)
│   │       └── assets/
│   │           └── rewave-ui/              # Built web UI (copied by Gradle)
│   └── ...
│
└── Rewave.md                      # Original engineering spec (reference)
```

---

## 14. Known risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| **Wi-Fi Aware unavailable on Windows adapter** | Primary connection path fails | Fallback chain lands on Same-LAN, which covers most home users. Wi-Fi Aware becomes an "if available" premium feature. |
| **Wi-Fi Aware unavailable on Android device** | Receiver can't publish NAN service | Same fallback chain. All devices support mDNS + broadcast. Wi-Fi Direct AP is the universal fallback. |
| **`windows` crate WASAPI loopback bugs** | Audio capture unreliable | The `cpal` crate is a backup. If both fail, consider a thin C++ DLL via FFI that wraps the WASAPI COM calls — exactly one known-good pattern. |
| **Tauri IPC latency for streaming stats** | Stale UI updates | Bypass Tauri IPC for real-time data. Use a local WebSocket server (in-process, `ws://127.0.0.1:PORT`) — sub-ms latency for stats push. |
| **Byte-level crypto divergence from Kotlin** | Auth failures, silent audio drops | Mandatory known-answer tests from real wire captures before any integration testing. The `crypto` module ships with a test vector JSON file. |
| **Android WebView performance** | UI jank during streaming | The WebView is UI-only — no audio processing. Stats push is throttled to ~10 Hz. All heavy work stays in native threads. |
| **HeroUI bundle size** | Slow first load on Android | Vite tree-shaking + code splitting. HeroUI is ~200KB gzipped for core components — acceptable for a local asset load. |
| **PairRequestCard rendering** (was broken in Compose) | ECDH Confirm broken | Moved to React — sidesteps the Kotlin/Compose rendering bug entirely. Tested via standard React testing. |

---

*End of design document*
