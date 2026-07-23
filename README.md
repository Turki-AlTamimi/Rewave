<picture>
  <source media="(prefers-color-scheme: dark)" srcset="">
  <img alt="Rewave" src="" width="180">
</picture>

<h1 align="center">Rewave</h1>

<p align="center">
  <strong>Wireless PC audio to your tablet — with 30–80 ms latency.</strong><br/>
  Raw PCM, no codec. Per-packet crypto. One direct Wi-Fi hop.
</p>

<p align="center">
  <a href="#why-rewave"><strong>Why</strong></a> ·
  <a href="#how-it-works"><strong>How</strong></a> ·
  <a href="#roadmap"><strong>Roadmap</strong></a> ·
  <a href="#project-structure"><strong>Structure</strong></a> ·
  <a href="#quick-start"><strong>Quick Start</strong></a>
</p>

<hr>

## Why Rewave?

| | Bluetooth | Rewave |
|---|---|---|
| Latency | 150–300 ms | **30–80 ms** |
| Codec | Lossy (SBC, AAC, aptX) | **Raw PCM** (lossless) |
| Pairing | Every time | **Once** (TOFU resume forever) |
| Range | 10 m | **Full Wi-Fi range** |
| Multi-device | Headphones only | **Any Android tablet** |
| Security | Link-layer | **Per-packet HMAC-SHA256** |

Rewave streams whatever your PC is playing — games, DAWs, YouTube, video calls — to your Android tablet in near real-time. Use your tablet as a wireless speaker, monitor audio while recording, or fill a room without cables.

## How It Works

```
┌── Windows Laptop ──────────────────────┐     ┌── Android Tablet ────────────┐
│                                         │     │                              │
│  WASAPI loopback  ──►  AudioFramer      │     │   UDP :50000                 │
│  (system audio)       (downmix/48k/i16) │     │   ┌──────────────────────┐   │
│                            │            │     │   │ AuthSession (M6)     │   │
│                            ▼            │     │   └──────┬───────────────┘   │
│                       StreamEngine      │     │          │ PCM 960B          │
│                       (200 packets/sec) │     │          ▼                   │
│                            │            │     │   ┌──────────────────────┐   │
│                            ▼            │     │   │ Native AudioEngine   │   │
│  UDP ───────────────────────────────►  Wi-Fi ─► │ SPSC ring + PLC       │   │
│  (965B unauthed / 973B M6 auth)       (1 hop)   │ + drift correction    │   │
│                                         │     │   └──────┬───────────────┘   │
│  mDNS _rewave-sender._udp              │     │          │ 240-frame bursts   │
│  ECDH Confirm / PIN / TOFU Resume      │     │          ▼                   │
│  WLANProfile (Wi-Fi Direct auto-join)  │     │   ┌──────────────────────┐   │
│                                         │     │   │ Oboe (AAudio/MMAP)   │   │
│                                         │     │   └──────────────────────┘   │
└─────────────────────────────────────────┘     └──────────────────────────────┘
```

1. **Capture** — WASAPI loopback grabs system audio (48 kHz stereo float32)
2. **Frame** — Downmix/resample to 48 kHz int16, chunk into 5 ms frames (960 B PCM)
3. **Authenticate** — Per-packet HMAC-SHA256 tag (8 B) using ECDH P-256 session key
4. **Pace** — 200 packets/sec over UDP, wall-clock-anchored, high-res Windows timers
5. **Receive** — SPSC lock-free ring buffer, PLC concealment, drift EWMA correction
6. **Play** — Oboe AAudio low-latency output via Android MMAP/Exclusive mode

## Feature Highlights

| Feature | Detail |
|---|---|
| **Latency** | 30–80 ms end-to-end (30 ms prebuffer + ~5 ms Wi-Fi + 20–60 ms Oboe) |
| **Audio** | 48 kHz, 16-bit, stereo PCM — zero lossy compression |
| **Cryptography** | ECDH P-256 key exchange, HKDF-SHA256 key derivation, HMAC-SHA256 per-packet auth |
| **Pairing** | PIN (4-digit) or ECDH Confirm (tap-to-pair) — TOFU silent resume on every subsequent launch |
| **Discovery** | Zero-config: subnet broadcast HELLO + mDNS (_rewave._udp.) |
| **Connectivity** | Same-LAN / Wi-Fi Direct persistent group / Wi-Fi Aware (planned) |
| **Cadence** | 200 pps ± 0.1% — high-res waitable timer, wall-clock anchor, no burst after stalls |
| **Recovery** | Packet-loss concealment (crossfaded repeats), drift correction (±20 ppm threshold), auto-reconnect |
| **UI** | Shared React web UI (HeroUI v2 + Tailwind 4) — same UI on Windows (Tauri) and Android (WebView) |

## Roadmap

| Stage | Status | What |
|:---:|:---:|---|
| **0** | ✅ | Monorepo skeleton — workspace builds green |
| **1** | ✅ | Crypto module — HKDF, HMAC, ECDH P-256 (15 byte-exact test vectors) |
| **2** | ✅ | Protocol codec — M1/M6 datagrams, 11 control messages, dispatch, replay protection |
| **3** | ✅ | Discovery + pairing + simulated receiver — 16 e2e integration tests |
| **4** | 🚧 | Audio pipeline — WASAPI capture, framer, pacing engine |
| **5** | 📋 | Shared web UI — Devices, Dashboard, Settings, Pairing flow |
| **6** | 📋 | Tauri IPC + WebSocket stats — wired to core, full hardware test |
| **7** | 📋 | Android WebView wrapper + ConfirmCard fix |
| **8** | 📋 | Wi-Fi Aware (NAN) — zero-touch proximity connect |
| **9** | 📋 | Hardening — netmask fix, MSIX packaging, power modes, DPAPI key store |

## Project Structure

```
rewave/
├── rewave-core/        Rust library — all logic
│   └── src/
│       ├── crypto/     HKDF, HMAC, ECDH P-256, pairing keys, session keys
│       ├── protocol/   M1/M6 audio datagrams, 11 control message types
│       ├── discovery/  Subnet broadcast + mDNS browsing/announcing
│       ├── pairing/    PIN handshake, ECDH Confirm, TOFU Resume flows
│       ├── connection/ Orchestrator state machine (Aware → LAN → Direct)
│       ├── audio/      Stage 4 — WASAPI capture, downmix, resample
│       ├── stream/     Stage 4 — pacing engine, high-res timer
│       ├── server/     Stage 6 — Tauri IPC, WebSocket stats
│       └── wifi/       Stage 8 — Wi-Fi Aware, Wi-Fi Direct auto-join
├── rewave-app/         Tauri 2 desktop binary
├── rewave-ui/          Shared React web UI (HeroUI + Tailwind 4 + Zustand)
├── Rewave.md           Engineering reference — every byte-level contract
├── REWRITE-PLAN.md     Staged execution plan with gate checkpoints
└── docs/
    └── superpowers/
        └── specs/      Architecture design documents
```

## Quick Start

### Prerequisites

- **Rust** 1.79+ with `x86_64-pc-windows-msvc` target (for Stage 4+)
- **Node** 20+
- **Windows 10 1803+** (for audio pipeline stages)

### Build (Stages 0–3 — no hardware required)

```bash
# Build the entire workspace
cargo build --workspace

# Run all tests (crypto, protocol, discovery, pairing, sim receiver)
cargo test --workspace

# Build the web UI
cd rewave-ui && npm install && npm run build
```

### Gate Check

```bash
# Windows
.\check.ps1

# Linux / WSL
./check.sh
```

## License

[MIT](LICENSE) © 2026 Turki Al-Tamimi

---

<p align="center">
  <sub>Built with Rust, Tauri, React, HeroUI, and Oboe.</sub>
</p>
