# Rewave — Engineering Documentation
**Date:** 2026-07-23

This document describes how Rewave works end-to-end at the level of detail needed to port, reproduce, or extend it. Every byte-level contract is pinned; every threading decision is explained; every "gotcha" a new implementer would hit is called out.

---

## Table of contents

1. [What Rewave is](#1-what-rewave-is)
2. [System architecture](#2-system-architecture)
3. [The audio path (end-to-end)](#3-the-audio-path-end-to-end)
4. [Wire format — audio datagrams](#4-wire-format--audio-datagrams)
5. [Wire format — control messages](#5-wire-format--control-messages)
6. [Crypto & auth contracts (FROZEN)](#6-crypto--auth-contracts-frozen)
7. [Pairing & TOFU (Phase C)](#7-pairing--tofu-phase-c)
8. [Discovery (Phase B)](#8-discovery-phase-b)
9. [Wi-Fi Direct link (Phase D)](#9-wi-fi-direct-link-phase-d)
10. [Timing & latency machinery](#10-timing--latency-machinery)
11. [Receiver internals — the native Oboe engine](#11-receiver-internals--the-native-oboe-engine)
12. [Receiver internals — PLC & drift correction](#12-receiver-internals--plc--drift-correction)
13. [Receiver internals — service & UI](#13-receiver-internals--service--ui)
14. [Threading model (both sides)](#14-threading-model-both-sides)
15. [Known gaps, gotchas, future work](#15-known-gaps-gotchas-future-work)
16. [Glossary](#16-glossary)

---

## 1. What Rewave is

Rewave streams audio from a Windows laptop to an Android tablet over a direct Wi-Fi link with end-to-end latency in the **30–80 ms** range (vs 150–300 ms typical of Bluetooth). The audio is **raw PCM** — no codec. Authentication is **per-packet HMAC-SHA256** (M6). Pairing is **ECDH P-256** with TOFU resume (Phase C) so you authenticate once and reconnect silently forever after.

The two sides:
- **Sender** uses WASAPI loopback capture, paced UDP send at 200 packets/sec, mDNS announcement, ECDH pairing, optional Wi-Fi Direct auto-join.
- **Receiver** a Kotlin / Compose Android app. NDK Oboe (AAudio) output, SPSC packet ring with prebuffering, packet-loss concealment, drift correction, foreground service, Wi-Fi Direct persistent-group hosting.

The link between them is one Wi-Fi hop either the laptop hosts a Mobile Hotspot and the Tab joins, or (recommended) the Tab hosts a Wi-Fi Direct group and the laptop joins.
> **Note:** the connection-setup UX is being redesigned in the Tauri rewrite — see `docs/superpowers/specs/2026-07-23-rewave-tauri-design.md` (Wi-Fi Aware primary, Same-LAN / Wi-Fi Direct fallback).

---

## 2. System architecture

```
┌──────────────────────────────────────┐         ┌──────────────────────────────────────┐
│  Windows laptop                      │         │  Android tablet                 │
│                                      │         │                                      │
│  ┌────────────────┐   5ms chunks     │         │                                      │
│  │ WASAPI loopback│──────┐           │         │  UDP :50000  ┌──────────────────┐    │
│  │ (NAudio)       │      │           │         │  ─────────►  │ DatagramSocket   │    │
│  └────────────────┘      ▼           │         │              │ recv thread      │    │
│                  ┌──────────────┐    │         │              └────┬─────────────┘    │
│                 │  AudioFramer  │   │         │                   │ 965/973 dispatch  │
│                 │ (downmix/     │   │         │                   ▼                   │
│                 │  resample/    │   │         │              ┌─────────────┐           │
│                 │  int16)       │   │         │              │ AuthSession │ (M6)      │
│                 └──────┬───────┘   │         │              └────┬────────┘           │
│                        │ 960 B     │         │                   │ PCM 960B            │
│                        ▼           │         │                   ▼ (JNI)              │
│                 ┌──────────────┐   │         │              ┌─────────────────┐      │
│                 │ StreamEngine │   │         │              │ native AudioEng │      │
│                 │ (200 pps     │   │         │              │ SPSC ring + PLC │      │
│                 │  pacer)      │   │         │              │ + drift EWMA    │      │
│                 └──────┬───────┘   │         │              └────┬────────────┘      │
│                        │ 965/973 B │         │                   │ 240-frame bursts   │
│                        ▼           │         │                   ▼                   │
│  UDP :50000  ┌────────────────┐    │         │              ┌─────────────────┐      │
│  ─────────►  │     UDP        │ ═══════════════════════════► │      Oboe       │      │
│  UDP :50001  │ (two sockets)  │    │ Wi-Fi   │              │  (AAudio/MMAP)  │      │
│  ─────────►  └────────────────┘    │ (1 hop) │              └────────┬────────┘      │
│                                      │         │                       │               │
│  mDNS _rewave-sender._udp           │         │  mDNS _rewave._udp                   │
│  broadcast HELLO → :50001           │ ───────► │  broadcast responder on :50001       │
│  ECDH Confirm / TOFU Resume         │         │  PairingSession (Confirm / Resume)   │
│  WLANProfile install (Wi-Fi Direct) │         │  RewaveLink (Wi-Fi Direct host)      │
└──────────────────────────────────────┘         └──────────────────────────────────────┘
```

### Two UDP ports — this is load-bearing

| Port      | Owner                     | Traffic                                                                                                                                                   |
| --------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **50000** | Receiver audio socket     | Audio datagrams (965 or 973 B) inbound; `HANDSHAKE_RESPONSE`/`OK`/`FAIL` control messages both ways.                                                      |
| **50001** | Receiver discovery socket | `HELLO` (v1+v2) inbound; `HERE` / `CHALLENGE` outbound; `PAIR_REQUEST` / `PAIR_RESUME` inbound; `PAIR_CONFIRM` / `PAIR_DENY` / `PAIR_RESUME_OK` outbound. |

The sender uses a **single ephemeral source socket** for all its control traffic (HELLO, HANDSHAKE_RESPONSE, PAIR_REQUEST, PAIR_RESUME). The receiver always replies to the source port a datagram came from, so the sender's one socket receives every reply. 

The audio stream itself goes out from the sender on a **separate** UDP socket (also ephemeral source) to `(receiver_host, 50000)`.

---

## 3. The audio path (end-to-end)

### 3.1 Format

- **48 kHz, 16-bit signed integer, stereo, interleaved L,R,L,R…**
- **5 ms frames** → 240 frames per packet → 480 int16 samples → **960-byte PCM payload**.
- 200 packets/sec (PPS).
- Datagrams: **965 B** (M1, unauthenticated) or **973 B** (M6, with 8-byte HMAC tag).
- Well under the 1472 B Ethernet path MTU; no fragmentation.

### 3.2 Latency budget (design estimate)

| Stage | Cost | Where |
|---|---|---|
| WASAPI loopback capture | ~5–15 ms | PC `WasapiLoopbackCapture` |
| PCM framing + UDP send | <1 ms | PC `StreamEngine` |
| Wi-Fi link (1 hop, no retransmit) | ~1–5 ms | network |
| **Receiver prebuffer (target fill)** | **30 ms** (the dial) | Android `AudioEngine` |
| Oboe output latency | 20–60 ms (Shared) / ~5 ms (Exclusive/MMAP) | Android Oboe |

> The **30–80 ms** headline assumes Exclusive/MMAP output on the Oboe stage. With a Shared-mode fallback at the top of its range, the worst case is ~110 ms.

The **30 ms prebuffer is the dominant term and it's intentional** — it's the latency/jitter tradeoff dial (`nativeSetTargetFillPackets`). Everything downstream is built to keep that buffer centered: drift correction trims it on long runs, PLC conceals gaps without bursting, jitter from the link is absorbed.

### 3.3 Frame lifecycle (one packet's journey)

1. **Capture thread** (PC, NAudio's WASAPI callback) delivers a block of float32 samples to `AudioFramer`.
2. `AudioFramer` **downmixes to stereo** (mono→duplicate, surround→avg even/odd channels), **resamples** defensively if rate differs from 48 kHz by >2%, **clips ±1.0 and truncates to int16** (no dither), accumulates in a running buffer, and emits one **960-byte chunk** whenever 240 frames have accumulated.
3. The chunk lands on a `ConcurrentQueue<byte[]>`.
4. **Send thread** (PC, `StreamEngine.Run`) pops the chunk on a 5 ms wall-clock anchor, builds a 965 or 973 B datagram, sends it via UDP. On miss (queue empty for >5 ms), it injects the last real chunk (PLC, up to 3 misses) or silence.
5. **Receiver recv thread** (`AudioReceiver.recvLoop`) reads the datagram, dispatches by length: 965 → legacy path (only if no session), 973 → M6 path (HMAC-verify + replay-check), `REWAVE` magic → control path.
6. The 960-byte PCM slice goes to **native** via `nativePushPacket(seq, pcm)`.
7. Native `pushPacket` does **gap detection + PLC synthesis** before enqueuing the real frame into the SPSC ring.
8. **Oboe audio thread** runs `onAudioReady` every burst: pops packets from the ring, fills the HAL buffer, runs drift correction once per second if needed, transitions PREBUFFERING→PLAYING once the target fill is reached.

---

## 4. Wire format — audio datagrams

### 4.1 M1 datagram (965 B, unauthenticated)

```
Offset  Bytes  Field   Endianness   Notes
[0:4]   4      seq     u32 BE       wraps at 0xFFFFFFFF
[4]     1      flags   u8           bit0=start=0x01, bit1=end=0x02
[5:965] 960    pcm     int16 LE     240 frames × 2 ch, interleaved L,R,L,R…
```

> **Endianness asymmetry:** seq is big-endian; PCM samples are little-endian. This is deliberate and frozen.

Flags:
- `FLAG_STREAM_START = 0x01` — set on the very first packet of a stream (seq 0).
- `FLAG_STREAM_END   = 0x02` — set on the final graceful-shutdown packet.

### 4.2 M6 datagram (973 B, authenticated)

Same as M1 plus an 8-byte truncated HMAC at offset 965:

```
[0:4]     4    seq    u32 BE       (same)
[4]       1    flags  u8           (same)
[5:965]   960  pcm    int16 LE     (same)
[965:973] 8    tag    = HMAC-SHA256(session_key, seq_BE_4 ‖ pcm_960)[:8]
```

> **Critical:** the tag covers **`seq_BE_4 ‖ pcm_960`** — 964 bytes — **NOT the flags byte, NOT the full datagram.** This is the most common cross-implementation bug.


### 4.3 Length-based dispatch (receiver side)

The receiver distinguishes M1 vs M6 **by total datagram length**, not by any in-band flag:

| Datagram length | Session state | Action |
|---|---|---|
| 965 B | no session established | ACCEPT as legacy |
| 965 B | session established | DROP (`legacyRejected`) — injection defense |
| 973 B | no session | DROP (`bytesDropped`) — can't MAC-verify |
| 973 B | session + HMAC match + `seq > last` | ACCEPT |
| 973 B | HMAC mismatch | DROP (`authFailures`) |
| 973 B | HMAC ok + `seq ≤ last` | DROP (`replayDrops`) |

**Pick one mode per session and stick with it.** A sender that pairs via PIN and then drops back to 965 will have all its audio rejected as injection.

---

## 5. Wire format — control messages

### 5.1 The 10-byte control header

```
[0:6]   MAGIC    b"REWAVE" (0x52 0x45 0x57 0x41 0x56 0x45)
[6]     VERSION  u8       (1 = v1, 2 = v2)
[7]     TYPE     u8       (1..11)
[8:10]  LENGTH   u16 BE   (payload byte count AFTER the header)
```

All multi-byte scalars in payloads are **big-endian**. Name fields are length-prefixed `u8`.

### 5.2 The 11 message types

| TYPE | Name | Direction | Payload |
|---|---|---|---|
| 1 | `HELLO` | sender → receiver | v1: empty. v2: `name_len u8 ‖ name_utf8 ‖ proto_flags u8` |
| 2 | `HERE` | receiver → sender | v1: `port u16 BE ‖ IPv4 u32 BE`. v2: + `name_len u8 ‖ name_utf8 ‖ receiver_flags u8` |
| 3 | `CHALLENGE` | receiver → sender | `receiverNonce[16] ‖ salt[16]` |
| 4 | `HANDSHAKE_RESPONSE` | sender → receiver | `senderNonce[16] ‖ response[32]` |
| 5 | `HANDSHAKE_OK` | receiver → sender | empty |
| 6 | `HANDSHAKE_FAIL` | receiver → sender | empty |
| 7 | `PAIR_REQUEST` | sender → receiver (v2) | `name_len u8 ‖ name ‖ sender_pubkey[65] ‖ sender_nonce[16]` |
| 8 | `PAIR_CONFIRM` | receiver → sender (v2) | `receiver_pubkey[65] ‖ receiver_nonce[16] ‖ key_id[8]` |
| 9 | `PAIR_DENY` | receiver → sender (v2) | `reason u8` (0=unknown, 1=user_denied, 2=unsupported) |
| 10 | `PAIR_RESUME` | sender → receiver (v2) | `key_id[8] ‖ sender_nonce[16] ‖ hmac[16]` |
| 11 | `PAIR_RESUME_OK` | receiver → sender (v2) | `receiver_nonce[16] ‖ hmac[16]` |

Flag bits in HELLO/HERE: bit0 = `PROTO_FLAG_PAIRED` (sender already holds a pairing key); bit1 = `PROTO_FLAG_SUPPORTS_CONFIRM` (ECDH Confirm flow supported).

### 5.3 Version compatibility

The receiver replies in the **same version as the inbound HELLO**: v1 HELLO → v1 HERE + v1 CHALLENGE; v2 HELLO → v2 HERE + v2 CHALLENGE. CHALLENGE and HANDSHAKE_RESPONSE payloads are byte-identical across v1/v2 — only the version byte differs.

The receiver's audio socket (`:50000`) only decodes types 1 (HELLO) and 4 (HANDSHAKE_RESPONSE); pairing messages (7–11) decode to `null` there. The discovery socket (`:50001`) decodes everything via `decodeV2Aware`. So **send pairing messages to :50001** and **send HANDSHAKE_RESPONSE to :50000** — a sender that gets this wrong will see silent drops.

---

## 6. Crypto & auth contracts (FROZEN)

Everything below is byte-identical across the sender, and the Kotlin receiver.

### 6.1 HKDF-SHA256 (RFC 5869)

```
Extract:  PRK = HMAC-SHA256(salt, IKM)          — empty salt becomes 32 zero bytes
Expand:   T(0) = empty
          T(i) = HMAC-SHA256(PRK, T(i-1) ‖ info ‖ byte(i))    for i = 1..n
          OKM = T(1) ‖ T(2) ‖ … truncated to `length`
```

### 6.2 Info strings (lowercase ASCII, exact bytes)

| Constant | Value | Used for |
|---|---|---|
| `PAIRING_INFO` | `"rewave-pairing"` | Derive the long-term pairing key |
| `KEYID_INFO` | `"rewave-keyid"` | Derive the 8-byte public key id |
| `SESSION_INFO` | `"rewave-session"` | Derive the per-session M6 key |
| `RESUME_SENDER_CTX` | `"resume-sender"` | PAIR_RESUME HMAC label (unchanged) |
| `RESUME_RECV_CTX` | `"resume-recv"` | PAIR_RESUME_OK HMAC label (unchanged) |

### 6.3 Size constants

```
Sha256         = 32      NonceBytes         = 16
PairingKey     = 32      EcPubkeyBytes      = 65   (0x04 ‖ X[32] ‖ Y[32], uncompressed P-256)
SessionKey     = 32      ResumeHmacBytes    = 16
KeyId          = 8       AuthTagBytes (M6)  = 8
Fingerprint    = 8
```

### 6.4 M6 PIN handshake (the v1 flow)

The 4-digit PIN shown on the receiver UI. ASCII bytes of the digits are the IKM.

```
response      = HMAC-SHA256(pin_ascii, senderNonce ‖ receiverNonce)        — full 32 B
                                                            (NOTE: sender FIRST)
session_key   = HKDF-SHA256(ikm=pin_ascii, salt=receiverNonce ‖ senderNonce,
                           info="rewave-session", len=32)
                                                            (NOTE: receiver FIRST in salt)
tag           = HMAC-SHA256(session_key, seq_BE_4 ‖ pcm_960)[:8]
```

> ⚠️ **The intentional nonce-order asymmetry is the most common port bug.** The response HMAC uses `Ns ‖ Nr`; the HKDF salt uses `Nr ‖ Ns`. Both orders are frozen.

The receiver accepts either the full 32-byte response or the truncated 8-byte prefix. Senders should prefer the full 32.

### 6.5 Per-packet tag (M6)

For both the PIN path and the ECDH path — the tag formula never changes, only the derivation of `session_key` differs:

```
tag = HMAC-SHA256(session_key, seq_BE_4 ‖ pcm_960)[:8]
```

- `seq_BE_4`: the 4-byte big-endian u32 sequence number.
- `pcm_960`: the 960-byte PCM payload.
- **Not the flags byte, not the full datagram.**
- Truncated to the first 8 bytes of the 32-byte digest.

### 6.6 Replay protection

The receiver's `AuthSession.checkAndAdvanceSeq` enforces **strict** `seq > lastAuthenticatedSeq` (`AuthSession.kt:367-371`). `lastAuthenticatedSeq` initializes to -1L so first seq 0 is accepted; it resets to -1L on every successful handshake / `adoptSessionKey` / `reset`. u32 wrap is intentionally NOT handled (200 pps × 24 h ≈ 1.7 × 10⁸ — well under 2³²).

> **Edge case:** a sender must not restart `seq` at 0 mid-session without a new handshake / `adoptSessionKey`. If it does, every packet is replay-dropped until `seq` climbs past the old `lastAuthenticatedSeq` — at 200 pps that can take hours after a long prior stream. Process restarts are covered because every launch goes through Resume / PIN / Confirm (§7.5).

### 6.7 ECDH P-256

Curve: **secp256r1** (P-256, prime256v1).
Public-key wire encoding: **65 bytes uncompressed, `0x04 ‖ X[32] ‖ Y[32]`**. The `0x04` prefix is mandatory — decoders reject anything else.
Shared secret: **the 32-byte X coordinate** of `ECDH(our_priv, peer_pub)`.

C# implementation wraps the 65-byte point in a hardcoded 26-byte P-256 SPKI prefix and feeds it to `ImportSubjectPublicKeyInfo`. Kotlin rebuilds the X.509 SPKI from a fixed ASN.1 prefix.

### 6.8 Security limitations (documented, no spec change)

- **Integrity only, no confidentiality.** M6 authenticates each datagram, but the PCM rides in cleartext; confidentiality rests entirely on WPA2 at L2. The Wi-Fi Direct group passphrase (8 chars, `[A-Za-z0-9]`, §9.1) is offline-brute-forceable from a captured WPA2 handshake.
- **Confirm pairing is unauthenticated ECDH.** §7.1 has no fingerprint/SAS comparison step — an active MITM at first pair substitutes both public keys and permanently owns the pairing key; silent Resume (§7.2) means no later detection. The fingerprint (§7.4) is currently used only as a TOFU-store key and is never shown to users.
- **Sender TOFU store is plaintext.** `pairings.json` (§7.3) holds `pairing_key` unprotected at rest, and a stolen key is sufficient to pass PAIR_RESUME verification and impersonate the sender. DPAPI (`ProtectedData`) is the obvious hardening; the receiver already uses `EncryptedSharedPreferences`.
- **No PIN rate-limit.** §6.4 defines no lockout/backoff on `HANDSHAKE_FAIL`, and the 4-digit PIN is static per service run — a clean online-brute-force oracle over a 10⁴ space.

---

## 7. Pairing & TOFU (Phase C)

Two pairing flows exist. Either is sufficient; after the first successful pair, the connection **resumes silently on every subsequent launch** (TOFU — Trust On First Use).

### 7.1 The ECDH Confirm flow (no PIN needed)

```
Sender                                  Receiver
  │                                       │
  │  PAIR_REQUEST                         │
  │  (name, sender_pubkey, sender_nonce)  │
  │ ────────────────────────────────────► │ :50001
  │                                       │ user taps Confirm on the receiver UI
  │                                       │
  │                PAIR_CONFIRM           │
  │ ◄──────────────────────────────────── │
  │  (receiver_pubkey, receiver_nonce,    │
  │   key_id)                             │
  │                                       │
  │  shared = ECDH(our_priv, receiver_pub)│
  │  IKM  = shared ‖ Ns ‖ Nr    (64 B)    │
  │  pairing_key = HKDF(IKM, Nr‖Ns,       │
  │                     "rewave-pairing", │
  │                     32)               │
  │  verify DeriveKeyId(pairing_key)      │
  │         == key_id from CONFIRM        │
  │  session_key = HKDF(pairing_key,      │
  │                    Nr‖Ns,             │
  │                    "rewave-session",  │
  │                    32)                │
  │  persist pairing to TOFU store        │
  │                                       │
  │  stream audio as 973-B M6 datagrams   │
  │ ═══════════════════════════════════► │ :50000
```

> **Confirm IKM order:** `shared(32) ‖ Ns(16) ‖ Nr(16)` = 64 bytes. The HKDF salt is still `Nr ‖ Ns`.
> **Receiver requirement:** the receiver must render the `PairRequestCard` so the user can tap **Confirm** or **Deny** within 30 s. **Known receiver bug:** the redesigned Compose UI doesn't currently render this card (see §15.1 #1). PIN pairing and silent Resume both work.

### 7.2 The PAIR_RESUME flow (silent reconnect)

After the first pair, the sender persists `(peerId, fingerprint, pairing_key, key_id, name)` to its TOFU store; the receiver persists the same to `EncryptedSharedPreferences`. On every subsequent launch the sender silently resumes:

```
Sender                                  Receiver
  │                                       │
  │  PAIR_RESUME                          │
  │  (key_id, fresh_sender_nonce,         │
  │   hmac = HMAC(pairing_key,            │
  │         key_id ‖ sender_nonce         │
  │         ‖ "resume-sender")[:16])      │
  │ ────────────────────────────────────► │ :50001
  │                                       │ findByKeyId(key_id)
  │                                       │ verify hmac
  │                                       │ fresh_receiver_nonce
  │                                       │ derive fresh session_key
  │                                       │ adoptSessionKey BEFORE replying
  │                                       │
  │           PAIR_RESUME_OK              │
  │ ◄──────────────────────────────────── │
  │  (receiver_nonce,                     │
  │   hmac = HMAC(pairing_key,            │
  │         key_id ‖ receiver_nonce       │
  │         ‖ sender_nonce                │
  │         ‖ "resume-recv")[:16])        │
  │                                       │
  │  verify hmac                          │
  │  session_key = HKDF(pairing_key,      │
  │                    Nr‖Ns,             │
  │                    "rewave-session",  │
  │                    32)                │
  │                                       │
  │  stream audio as 973-B M6 datagrams   │
  │ ═══════════════════════════════════► │ :50000
```

**Resume HMAC order matters:**
- Sender-leg covers `key_id ‖ sender_nonce ‖ "resume-sender"` (sender hasn't seen Nr yet).
- Receiver-leg covers `key_id ‖ receiver_nonce ‖ sender_nonce ‖ "resume-recv"`.

Timeout: 3 s on the sender (raised from 1 s to accommodate slow receiver keystore decrypts).

### 7.3 TOFU store schema

Sender (Windows): `%APPDATA%\rewave\pairings.json`:
```json
{
  "<peer_id_hex>": {
    "fingerprint": "<16 hex chars>",
    "pairing_key": "<64 hex chars>",
    "name": "<str>",
    "key_id":    "<16 hex chars>"
  }
}
```

Receiver (Android): `EncryptedSharedPreferences` file `rewave_pairings`, master key alias `rewave_master_key` (AES256_GCM in Android Keystore). Per-row field keys: `<peerId>.fp`, `<peerId>.pk`, `<peerId>.name`, `<peerId>.kid` (Base64 NO_WRAP).

> **Config directory rename:** the per-user config directory changes from `%APPDATA%\sawt\` to `%APPDATA%\rewave\`. Existing `pairings.json` / `wifi.json` / `last_receiver.json` / `settings.json` files in the old directory are NOT migrated automatically — users will need to re-pair and re-enter Wi-Fi credentials once. Implementers may add a one-time migration step if desired.

### 7.4 Fingerprint

```
fingerprint = SHA-256(peer_pubkey[65] ‖ peer_name_utf8 ‖ peer_ip_ascii)[:8]
peerId      = lowercase hex of fingerprint (16 chars)
```

For the **v1 PIN path** (no sender pubkey), the receiver uses a synthetic fingerprint `SHA-256(senderNonce ‖ hostAddr ‖ hostAddr)[:8]` PIN-paired and Confirm-paired entries do **not** share TOFU rows.

> **Fragility:** because the fingerprint mixes in `peer_ip_ascii`, an IP change breaks the TOFU lookup and silently disables Resume (§7.5) — the sender falls through to Confirm (currently broken, §15.1 #1) and the user is forced back to PIN. The IP adds no security (the pubkey already binds identity); dropping `peer_ip_ascii` from the fingerprint is candidate hardening (§15.3).

### 7.5 Auth-method preference (sender side)

The sender's `EstablishAuthOrchestrator` tries in order:

1. **Resume** (silent, zero UI) — if `last_receiver.json` matches the target IP AND we have a stored pairing for that fingerprint.
2. **PIN** — if `--pin <code>` was provided. On success, also derives + persists a synthetic pairing so the next launch can resume.
3. **Confirm** — default-on when no PIN and no resumable pairing.

---

## 8. Discovery (Phase B)

Zero-config discovery. The chain is **broadcast → mDNS → manual IP fallback**.

### 8.1 Broadcast leg

The sender enumerates local IPv4 unicast addresses, computes the `/24` subnet-directed broadcast for each (with `255.255.255.255` as final fallback), and sends a v2 HELLO once per second to `(each_bcast, 50001)` until deadline (~3 s). The receiver's discovery socket accepts the HELLO and replies with HERE + CHALLENGE on the same socket.

> ⚠️ **The `/24` assumption is a simplification.** It works on home / hotspot networks but miscomputes the broadcast address on `/16` or `/8` subnets. Net effect: broadcast leg silently fails, mDNS leg picks up the slack. A real product should compute broadcast from the actual netmask.

### 8.2 mDNS leg

If broadcast finds nothing, the sender browses `_rewave._udp.` via Zeroconf (~1.5 s browse). The receiver announces via `NsdManager.registerService` with TXT record:

| Key | Value |
|---|---|
| `name` | receiver display name (default `Build.MODEL` truncated to 24 chars) |
| `paired` | comma-joined key_id hex list, or `"none"` |
| `flags` | receiver flag byte as decimal (currently always 0) |

The sender simultaneously announces itself as `_rewave-sender._udp.` so receivers can display the sender in their UI.

> **Service-type rename:** `_sawt._udp.` → `_rewave._udp.` and `_sawt-sender._udp.` → `_rewave-sender._udp.` (note the trailing dot in both). A Rewave sender will not discover a Sawt receiver and vice versa.

### 8.3 Limitations

- **Broadcast and multicast do NOT cross the Android emulator's host NAT.** Test discovery only on real hardware. Emulator dev uses `--host 127.0.0.1` + `adb emu redir add udp:50000:50000`.
- **Same-host mDNS loopback is off by default on Windows.** A sender announcing on a host won't see its own announcement. Cross-host discovery works normally. The dev box can validate by running a sibling-process observer (Python `zeroconf` sees the C# announcement fine).
- **IPv4-only.** The HERE payload hard-codes a 4-byte address (§5.2) and the broadcast math assumes IPv4; a v6 story would require a wire change. Fine for the one-hop direct-link use case.

---

## 9. Wi-Fi Direct link (Phase D)

The recommended topology: **the Tab hosts a Wi-Fi Direct persistent group as a WPA2 AP**, the laptop joins as an ordinary client. This is a reversal of the original Sawt-era anti-Wi-Fi-Direct decision; see Phase D for the rationale (chiefly: Tab battery, no Samsung pairing dialogs, stable credentials).

### 9.1 Receiver-side hosting (`RewaveLink`)

- Uses `WifiP2pManager` with `enablePersistentMode(true)` so the SSID/passphrase survive reboot.
- **On-air network name:** `DIRECT-Re-XXXX` (the `DIRECT-xy` prefix is mandatory per Wi-Fi Alliance P2P spec; `Re` chosen so the network is recognizable in Windows Wi-Fi manager as Rewave). The user-facing label `Rewave-XXXX` is for display only.
- **Passphrase:** 8-character `[A-Za-z0-9]` (WPA2 minimum).
- State machine: `Idle → Initializing → CreatingGroup → Up(ssid, networkName, passphrase, isGroupOwner=true)`. Failure states are terminal for the state machine: `Failed("permission" | "unsupported" | "create_failed" | "timeout" | …)`. `Failed` does NOT stop audio — other topologies keep working — but no UI retry path is currently defined; the service must be restarted to re-initialize the link.
- Persistent fallback: if the requested custom name fails (some Samsung One UI builds ignore it), `createDefaultPersistentGroup` lets the framework pick and persists whatever it actually assigned. Cited at `receiver/.../RewaveLink.kt:313-428`.
- The receiver UI shows the on-air SSID and passphrase with a Show/Copy affordance.

> ⚠️ **Emulator:** no Wi-Fi Direct radio. `RewaveLink` degrades to `Failed("unsupported")` and the audio path keeps running. Test Phase D only on real hardware.

> **P2P name prefix rename:** `DIRECT-Sa-XXXX` → `DIRECT-Re-XXXX`. Both the prefix `Re` and the friendly label `Rewave-XXXX` are derived from `RewaveLinkConfig.networkNameFromSsid`. Existing persistent groups created under `DIRECT-Sa-XXXX` are not auto-migrated — the receiver creates a fresh group on first launch after the rename.

### 9.2 Sender-side auto-join (`WifiBootstrap`)

When the user passes `--wifi-ssid`, `--wifi-pass`, and `--wifi-connect`, the sender:

1. **Warm path:** if `netsh wlan show interfaces` already shows association with the SSID → skip install/connect.
2. Build a **WLANProfile XML** (WPA2-PSK/AES) and install it via `netsh wlan add profile filename=-` (XML on stdin, idempotent overwrite).
3. `netsh wlan connect name=<profile>`.
4. Poll `IsConnectedTo(ssid)` every 0.5 s for up to 10 s (Wi-Fi Direct hands out a link-local IPv4 within ~1–2 s).

> **Firewall:** a fresh Wi-Fi Direct network defaults to the Public profile on Windows. Every documented audio/pairing flow is sender-initiated, and the receiver's replies to the sender's ephemeral source ports are solicited traffic, so no firewall rule is needed. The only unsolicited-inbound sender component is the `SenderAnnouncer` mDNS responder — if it is blocked, receivers simply don't display the sender in their UI (cosmetic).

### 9.3 The WLANProfile XML (byte-exact)

```xml
<?xml version="1.0" encoding="utf-8"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{profileName escaped}</name>
  <SSIDConfig>
    <SSID>
      <hex>{utf-8 hex of ssid, lowercase}</hex>
      <name>{ssid escaped}</name>
    </SSID>
    <connectionType>ESS</connectionType>
    <connectionMode>auto</connectionMode>
    <autoSwitch>true</autoSwitch>
  </SSIDConfig>
  <MSM>
    <security>
      <authEncryption>
        <authentication>WPA2PSK</authentication>
        <encryption>AES</encryption>
        <useOneX>false</useOneX>
      </authEncryption>
      <sharedKey>
        <keyType>passPhrase</keyType>
        <protected>false</protected>
        <keyMaterial>{passphrase escaped}</keyMaterial>
      </sharedKey>
      <enableAutoConnect>true</enableAutoConnect>
    </security>
  </MSM>
</WLANProfile>
```

**Critical byte-exact details:**
- **Line endings: `\r\n`** (Windows-style, explicit).
- **Indentation: 2 spaces per level.**
- **Manual XML escaping** of `& < > "` (NOT `'` — apostrophe stays bare). Substitution order matters: `&` first to avoid double-escape.
- `<hex>` is **lowercase UTF-8 hex** of the SSID. `"Rewave-AB12"` → `5265776176652d41423132`.

C# output is MD5-identical to the original Python output for any given input (`tests/RewaveSender.Wifi.Tests/WlanProfileBuilderTests.cs` hard-codes the comparison). The receiver requires the on-air name `DIRECT-Re-XXXX` — pass that as `--wifi-ssid`, not the friendly `Rewave-XXXX`.

---

## 10. Timing & latency machinery

This is where most of the engineering hours went. The cadence target is **200 pps ± 0.1 %, no bursts after stalls, low CPU**.

### 10.1 The Windows tick problem

On Windows, `Thread.Sleep(N)`, `Task.Delay(N)`, and `WaitHandle.WaitOne(N)` are all quantized to the **system tick**, default ~15.6 ms. Asking for a 4 ms sleep returns in ~15 ms — which would burst-catch-up and wreck the 200 pps cadence.

Rewave has three primitives to deal with this:

| Primitive | Granularity | CPU cost | When used |
|---|---|---|---|
| `Thread.Sleep` / `Task.Delay` | ~15.6 ms | ~0% | Never for pacing; OK for non-critical waits |
| `Thread.SpinWait` + `Thread.Sleep(0)` | ~sub-ms | ~96% of a core | Pacing fallback when high-res timer unavailable |
| `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` (Win10 1803+) | ~0.5 ms | ~14.5% of a core | **Default pacing strategy on supported Windows** |
| `timeBeginPeriod(1)` (winmm) | ~1 ms (process-global) | ~0% | Opt-in via `--mmcss` / `PowerMode.LowLatency` |
| MMCSS "Pro Audio" task | n/a (priority) | n/a | Opt-in; boosts send thread to multimedia scheduler |

The high-res waitable timer is the headline: a `Wait(5ms)` lands at **5.1–5.3 ms** vs `Thread.Sleep(5)` = 14.8–15.6 ms. CPU drops from 96% to 14.5% of a core.

### 10.2 Wall-clock-anchored pacing

The send loop is anchored on an absolute target send time:

```
nextSend = startTime + interval
loop:
    sleep_or_spin(nextSend - now)
    if (now - nextSend) > RESYNC_THRESHOLD:    # 10 ms = 2 packets
        nextSend = now                          # drop backlog, don't burst
    send(packet)
    nextSend += interval                        # NOT now += interval
```

The naive `now += interval` accumulates drift; `nextSend += interval` does not.

### 10.3 The 10 ms resync threshold

If the loop oversleeps by more than 10 ms (2 packets), it **snaps forward** instead of bursting. "Late audio is loss; bursty audio is loss + receiver buffer stress." Below the threshold, one back-to-back send catches up. Cited at `StreamEngineOptions.cs:77`; constant `RESYNC_THRESHOLD_S = 0.010` mirrors the deleted Python sender's A6 fix.

### 10.4 Capture-side resync: `DrainToNewest`

When the send loop resyncs after a stall, it also calls `capture.DrainToNewest()` — atomically drains the real-time capture queue keeping only the newest chunk. Without this, the unbounded capture queue would impose a permanent latency penalty equal to the stall duration.

### 10.5 Capture-side silence injection / PLC

If the WASAPI loopback produces nothing for >5 ms (true silence, or an exclusive-mode app bypassing the mixer), the send loop:

1. Misses 1–3: **repeat the last real 960-byte chunk** (PLC — far less audible than a zero-splice mid-audio).
2. Miss 4+: **emit a true zero chunk**.

Sequence numbers always advance, so the receiver sees a continuous stream and never underruns. Constant `MISS_REPEAT_THRESHOLD = 3`.

---

## 11. Receiver internals — the native Oboe engine

The native engine is a single class `rewave::AudioEngine` that extends both `oboe::AudioStreamDataCallback` and `oboe::AudioStreamErrorCallback`.

### 11.1 Format & ring

- Sample rate 48000, 2 ch, 240 frames/packet (5 ms).
- **SPSC ring**: capacity 20 packets (effective 19, ~95 ms, since headtail is the empty sentinel). One producer (recv thread via `pushPacket`), one consumer (audio thread via `onAudioReady`). Acquire/release fences on `head_` / `tail_` make the PCM bytes happen-before.
- **Target fill: 6 packets (~30 ms)** — the latency dial. Set via `nativeSetTargetFillPackets`.
- **Resume fill: 3 packets (~15 ms)** — used after underruns / stalls once `ever_played_=true`. Initial prebuffer must still reach the full target.

### 11.2 State machine

```
enum State { PREBUFFERING = 0, PLAYING = 1 }     # stored as std::atomic (relaxed)
```

- **PREBUFFERING**: writes silence, does NOT pop. Transitions to PLAYING once `depth_packets >= resume_fill`.
- **PLAYING**: pops packets and writes them to the HAL output buffer.

State is written by both the audio thread (`onAudioReady`) and the recv thread (`setStallState` atomic store — no mutex, because `pushPacket` already holds the non-recursive `stream_mutex_` for the whole call).

### 11.3 Stream lifecycle

- **`start()`**: clears `stop_requested_`, zeroes per-session counters (NOT `stream_reopens_`), resets engine state, opens Exclusive→Shared fallback via `openStream`, `requestStart`.
- **`stop()`**: latches `stop_requested_=true` FIRST (under mutex), then `started_=false`, stops + closes the stream, drains head/tail, sets state PREBUFFERING.
- **`onErrorAfterClose` (A5)**: runs on Oboe internal thread. Checks `stop_requested_` — if true, leaves the stream dead (no zombie restart). Otherwise reopens Exclusive→Shared.
- **`reopenForDevice` (M7)**: called from `AudioManager` device-callback thread. Bails if `!started_` (zombie guard). Stops+closes the stream, opens via `openStreamForDevice(Exclusive, deviceId, type)`, falls back to Shared.

Stream open builder: PerformanceMode::LowLatency, I16, 48k, stereo, FramesPerCallback=240 (request only), BufferCapacityInFrames=960, Usage::Media, ContentType::Music. Exclusive first, Shared fallback.

### 11.4 JNI stats array (21 longs, fixed order — never reorder)

`nativeGetStats()` returns 21 longs read by index from Kotlin. Indices 0–10 are M1/M2, 11–13 are M4, 14–16 are M5, 17–20 are M7. New stats are **appended**, never reordered.

| idx | name |
|---|---|
| 0–7 | framesPushed, framesConsumed, underruns, overflows, depthFrames, oboeStarted, sharingMode, measuredOutputLatencyMs |
| 8–10 | state, targetFillFrames, capacityFrames |
| 11–13 | plcFrames, stalls, lateDrops |
| 14–16 | driftPpmX1000, driftCorrections, driftDrops |
| 17–20 | framesPerBurst, framesPerCallbackActual, currentDeviceId, currentDeviceType |

---

## 12. Receiver internals — PLC & drift correction

### 12.1 Packet-loss concealment (PLC, M4)

`pushPacket` runs **before** the real frame enters the ring. It computes `gap = seq - expected_seq_` (uint32 forward distance) and synthesizes intermediate frames:

| Gap | Action |
|---|---|
| 0 (in-order) | push REAL, update `last_good_pcm_` |
| `> 0x80000000` (behind) | `late_drops_++`, silent drop. **No reorder window.** |
| 1 or 2 | push `PLC_REPEAT` frames (copy of `last_good_pcm_`) with raised-cosine crossfade (48-frame = 1 ms) on the head of the first repeat and tail of the last; push the real frame after. |
| 3..5 | push `PLC_SILENCE` (zero) frames; push the real frame unmodified. |
| > 5 | **Soft stall**: set state PREBUFFERING (without flushing the ring), `stalls_++`, reseed `expected_seq_ = seq`, push the real frame normally. |

Crossfade tables are precomputed (`w_in[i] = 0.5*(1 - cos(π i / N))`, `w_out[i] = w_in[N-1-i]`).

### 12.2 Drift correction (M5, with the A1 fix)

The receiver's audio thread estimates long-term arrival-vs-consumption drift and trims the buffer:

- **Window**: 10 s (`kDriftWindowSeconds`).
- **EWMA**: α = 0.3 (`kDriftEwmaAlpha`); first window seeds with instantaneous value.
- **Threshold**: ±20 ppm (`kDriftThresholdPpm`) — corrections only fire outside this band.
- **Rate cap**: max 1 correction per second (`kDriftMinCorrectionIntervalSec`).
- **Decision** (`applyDriftCorrection`, A1 fix):
  - **DROP** (sender faster, buffer filling) only when `depth_packets > target`.
  - **DUPLICATE** (sender slower, buffer draining) only when `depth_packets < target`.
- **DROP** = pop+play one packet, then discard one extra (`popAndDiscardOnePacket`). Net depth −1.
- **DUPLICATE** = replay the last-played packet with a raised-cosine head fade-in. Net depth +1.
- Gate: only correct when `read_offset_frames_ == kFramesPerPacket` (the in-flight packet is fully consumed — don't skip a partial burst).

> The A1 fix is the answer to the "periodic 5 ms skip" bug. The pre-fix DROP branch wrote silence WITHOUT popping → net depth +1 instead of −1 → ring walked 6→19 in ~13 s → overflowed without concealment → periodic artifact. Fix: DROP = pop+play then discard; verify Test 4 (+1000 ppm) → 27 drops / 0 dups.

### 12.3 AudioDeviceCallback (M7) — Bluetooth route handling

The receiver registers an `AudioManager.AudioDeviceCallback` on a dedicated `HandlerThread "rewave-audiodev"` (so callbacks never block audio or main). On any device add/remove, it enumerates `getDevices(GET_DEVICES_OUTPUTS)`, picks the active route by priority (BT_A2DP > wired > USB > builtin speaker), and calls `nativeReopenForDevice(active.id, typeHint)` when the route changes. The engine reopens the Oboe stream against the new device, re-enters PREBUFFERING, and resumes.

The sender does not need to do anything special for BT route switches — the stream just gaps briefly while the receiver re-opens.

---

## 13. Receiver internals — service & UI

### 13.1 `AudioStreamService` (the foreground service)

- `Service` subclass, started (not bound). `startForeground` with `FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK` on API 34+.
- Owns the `AudioReceiver`, `AuthSession`, `RewavePairingSession`, `PairingStore`, `RewaveDiscovery`, `RewaveLink`.
- Returns `START_STICKY` so Android restarts it if killed.
- Generates a fresh 4-digit PIN and 16-byte receiver nonce at first init.

### 13.2 WifiLock

Acquired with `WIFI_MODE_FULL_LOW_LATENCY` (default, low-latency mode); falls back to deprecated `WIFI_MODE_FULL_HIGH_PERF` on `RuntimeException`; gives up silently if both fail. M8 battery-saver mode releases the lock. Same-mode no-op fix prevents a leak on toggles that don't change the mode.

### 13.3 Link-lost watchdog (M8)

`updateLinkLostWatchdog(currentReceived)` is called by the UI poller (~2 Hz). If `received` advances: reset timer, clear `linkLost`. If stalled > 5 s AND we've ever received: set `linkLost`, refresh notification. Does NOT tear anything down — the sender has warm-start reconnect.

### 13.4 Pairing-request UI (have some issues)

Pending `PAIR_REQUEST`s live in `pendingPairRequests: ConcurrentHashMap<String, PairRequestInfo>`, expiring after 30 s (`PAIR_REQUEST_TIMEOUT_MS`). The MainActivity is supposed to render a `PairRequestCard` for each pending request with Accept / Deny buttons.

> ⚠️ **Known receiver bug:** the redesigned Compose UI doesn't render the `PairRequestCard` even though the entry IS in the map and the service polls every 500 ms. The PAIR_REQUEST is accepted at the service layer but the UI never shows it, so the 30 s timeout fires and prunes. **PIN pairing and silent Resume both work fine** — only ECDH Confirm (first-pair via tap) is broken on the current build.
### 13.5 Power-mode toggle (M8)

Two modes, persisted in `rewave_prefs/power_mode`:
- **Low latency (default)**: target fill 6 packets (~30 ms), `WIFI_MODE_FULL_LOW_LATENCY`, all bells.
- **Battery saver**: target fill 12 packets (~60 ms), releases WifiLock. More crackle-tolerant of jitter; lower Tab battery drain.

---

## 14. Threading model (both sides)

### 14.1 PC sender

| Thread                                 | Role                                                          |
| -------------------------------------- | ------------------------------------------------------------- |
| **Main / UI**                          | WPF dispatcher, ViewModel, tray menu                          |
| **Engine send** (background Task)      | `StreamEngine.Run` — the 200 pps paced loop                   |
| **WASAPI capture** (NAudio-internal)   | `WasapiLoopbackCapture.DataAvailable` → `AudioFramer` → queue |
| **mDNS announcer** (background)        | `SenderAnnouncer` responder loop                              |
| **Stats collector** (background Task)  | 2 Hz poll, raises `Updated` event                             |
| **Jitter worker** (only if `--jitter`) | Sleeps to release times, sends                                |

The engine's `_lock` protects `_sent`, `_seq`, `_dropped`, `_startUtc` (written in `Run`, read by `Snapshot()`). The capture queue is a `ConcurrentQueue<byte[]>` with `Monitor.Wait/Pulse` for blocking up to 5 ms.

### 14.2 Android receiver

| Thread | Role | Key rule |
|---|---|---|
| **Main (UI)** | Compose, ViewModel | Never blocks; reads via `instance?.statsSnapshot()` |
| **rewave-recv** (daemon, URGENT_AUDIO prio) | `AudioReceiver.recvLoop` | Takes `stream_mutex_` inside `pushPacket` |
| **Oboe audio** (HAL-internal) | `onAudioReady`, drift correction, PLC pop | **NEVER takes `stream_mutex_`** |
| **rewave-disc** (daemon) | `RewaveDiscovery.responderLoop` on :50001 | Decodes v2-aware; dispatches PAIR_* to handler |
| **rewave-audiodev** (HandlerThread) | `AudioDeviceCallback` | Calls `nativeReopenForDevice` under `stream_mutex_` |
| **Oboe error** (internal) | `onErrorAfterClose` | Checks `stop_requested_` latch; reopens if false |
| **Kotlin stats poller** (UI-driven, ~2 Hz) | reads stats via `nativeGetStats` | Lock-free atomic reads; `measureLatencyNow` under mutex |

> **The non-recursive `stream_mutex_` discipline is the most fragile part of the native code.** `pushPacket` holds it for the whole call, so `setStallState()` (called from within `pushPacket`'s stall path) must NOT take it — uses an atomic store instead. A future implementer who adds a new recv-thread code path that takes `stream_mutex_` and then calls something that also takes it will deadlock.

---

## 15. Known gaps, gotchas, future work

### 15.1 Known bugs / limitations

1. **`PairRequestCard` not rendering** (receiver UI bug, MainActivity.kt). ECDH Confirm pairing can't complete end-to-end on the current build. PIN pairing and silent Resume both work. **Fix this before shipping.**
2. **`/24` broadcast assumption** in `BroadcastDiscovery` — fails silently on `/16`/`/8` subnets. mDNS leg compensates. Compute broadcast from the actual netmask.
3. **u32 seq wraparound NOT handled** in receiver replay protection. Sessions >24 h at 200 pps would wrap. Fine for v1; revisit for always-on use cases.
4. **Same-host mDNS loopback off on Windows** — a sender doesn't see its own announcement. Cross-validation needs a sibling-process observer. Not a product bug; dev-tooling note.
5. **`exclusive-mode hint` may not fire on the C# capture path** during total silence: NAudio delivers prompt zero buffers, so the gap-based detector doesn't accumulate. The silence-injection path still works. Cosmetic only.
6. **No installer / MSIX packaging** — `RewaveSender.exe` ships as a raw exe. End users need either the .NET 8 runtime (framework-dependent) or accept the ~70 MB self-contained build. Add WiX / MSIX before consumer distribution.
7. **Rename is breaking.** A Rewave endpoint cannot interop with a legacy Sawt endpoint (magic bytes, control header length, mDNS service types, HKDF info strings all differ). Users moving from Sawt to Rewave must re-pair all devices. See the migration notes in §7.3, §8.2, and §9.1.
8. **No audio confidentiality** — M6 is integrity-only; PCM is cleartext, protected only by WPA2 at L2 (see §6.8).
9. **Confirm pairing is MITM-able at first pair** — unauthenticated ECDH with no fingerprint/SAS comparison; silent Resume hides it forever (see §6.8).
10. **Sender TOFU store is plaintext** — a stolen `pairing_key` enables silent Resume impersonation (see §6.8).
11. **No PIN attempt rate-limit / lockout** — the static 4-digit PIN is online-brute-forceable (see §6.8).
12. **Multi-sender arbitration is unspecified** — the receiver runs `adoptSessionKey` *before* replying to PAIR_RESUME (§7.2), so a second sender merely resuming silently kills the first sender's stream (`authFailures`). Last successful handshake wins, with no rejection and no UI signal.
13. **M1 downgrade window** — while no session is established, unauthenticated 965 B datagrams are accepted (§4.3); an attacker racing the real sender at startup can inject audio until the handshake lands.
14. **Control-plane retransmission is unspecified** — only HELLO (1 s interval, ~3 s deadline) and the 3 s PAIR_RESUME timeout are defined; retry/backoff for CHALLENGE, HANDSHAKE_RESPONSE, and PAIR_REQUEST/CONFIRM is implementation-defined. A lost PAIR_RESUME_OK after the receiver already adopted the key is the desync case in #12.

### 15.2 Cross-implementation gotchas (read before porting)

1. **Nonce order is asymmetric and frozen**: response HMAC uses `Ns ‖ Nr`; HKDF salts use `Nr ‖ Ns`; ECDH IKM is `shared ‖ Ns ‖ Nr` (sender nonce at offset 32).
2. **M6 tag covers `seq_BE_4 ‖ pcm_960`** — NOT the flags byte, NOT the full datagram.
3. **Datagram length dispatches M1 vs M6** — no in-band flag. Pick one mode per session.
4. **Two UDP ports**: pairing to :50001, HANDSHAKE_RESPONSE to :50000, audio to :50000. Get this wrong → silent drops.
5. **PCM is little-endian int16; seq is big-endian u32; control scalars are big-endian.** Three endiannesses in one protocol.
6. **Wi-Fi Direct network name on air is `DIRECT-Re-XXXX`**, not the friendly `Rewave-XXXX`. Join by the on-air name.
7. **ECDH IKM differs by path**: Confirm = 64-byte `shared‖Ns‖Nr`; PIN = 32-byte `session_key` directly.
8. **WLANProfile XML must be byte-identical** to the spec: `\r\n` line endings, 2-space indent, manual escape of `& < > "` (not `'`), lowercase UTF-8 hex SSID.
9. **The non-recursive `stream_mutex_`** on the receiver: don't add new recv-thread code paths that take it twice.
10. **`nativeGetStats` returns 21 longs by index — never reorder.** Append new stats at the end.
11. **Control header is 10 bytes in Rewave (6-byte REWAVE magic + 4 bytes), not 8 bytes.** Anyone porting from Sawt must update every byte-offset calc in the control codec.

### 15.3 Future work (not in scope for v1)

- **MSIX / Store packaging + auto-update** for the sender.
- **Receiver `PairRequestCard` rendering fix** (the §15.1 #1 bug) — **pre-ship blocker**, listed here only for tracking.
- **SAS / fingerprint comparison UI** for Confirm pairing (closes the §6.8 MITM gap).
- **PIN attempt rate-limit / lockout** on the receiver.
- **DPAPI protection** for the sender TOFU store.
- **Decouple the TOFU fingerprint from peer IP** (§7.4) so IP changes don't break Resume.
- **Netmask-correct broadcast** computation (replace the `/24` assumption).
- **u32 wrap handling** for >24 h sessions.
- **A real latency-calibration tool** (Test 8 automation) — click generator + Tab-mic round-trip measurement.
- **iOS receiver port** — the wire contracts are platform-agnostic; AVAudioEngine + Network.framework could host the receiver side. The native Oboe PLC/drift logic would need porting to CoreAudio.
- **Linux/macOS sender port** — the RewaveSender.Cli targets plain `net8.0` for most of its projects; WASAPI capture is the only Windows-specific piece. A CoreAudio/PipeWire capture implementation would unlock macOS/Linux.

---

## 16. Glossary

| Term | Meaning |
|---|---|
| **M1** | The 965-byte unauthenticated audio datagram (Milestones 1–5). |
| **M6** | The 973-byte authenticated audio datagram with the 8-byte HMAC tag (Milestone 6). |
| **PLC** | Packet-Loss Concealment — synthesizing inaudible frames to cover gaps. Receiver-side. |
| **TOFU** | Trust On First Use — pair once, cache the key, reconnect silently forever. |
| **Confirm** | The ECDH P-256 pairing flow (Phase C). Requires a UI tap on the receiver. |
| **Resume** | The silent reconnect flow using a cached pairing key + fresh nonces + HMAC proof. |
| **Prebuffer** | The 30 ms (configurable) target ring fill — the latency dial. |
| **EWMA** | Exponentially Weighted Moving Average — used for drift estimation (α=0.3, 10 s window). |
| **Drift correction** | Receiver-side frame drop/duplicate to keep the buffer centered on the target fill. |
| **MMCSS** | Multimedia Class Scheduler Service — Windows multimedia thread priority bump ("Pro Audio"). |
| **High-res waitable timer** | `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` (Win10 1803+) — sub-ms sleep granularity. |
| **SPSC** | Single-Producer Single-Consumer — the lock-free ring buffer discipline used for the receiver packet queue. |
| **REWAVE magic** | The 6-byte `0x52 0x45 0x57 0x41 0x56 0x45` literal at the head of every control datagram. |
