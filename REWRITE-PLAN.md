# Rewave Tauri Rewrite — Staged Execution Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the Rewave Windows sender as a Tauri app (Rust core + shared React web UI) and wrap the existing Android receiver in a WebView, per `docs/superpowers/specs/2026-07-23-rewave-tauri-design.md`, without breaking any frozen wire contract in `Rewave.md`.

**Architecture:** One monorepo: `rewave-ui/` (React 18 + HeroUI + Vite), `rewave-core/` (Rust lib: audio/stream/crypto/protocol/discovery/connection/wifi/pairing/server), `rewave-app/` (Tauri 2 binary), `android/` (existing Kotlin receiver + WebView wrapper). Windows talks to the UI via Tauri IPC + a localhost WebSocket for stats; Android via `@JavascriptInterface`.

**Tech Stack:** Rust 1.79+ (tokio, windows 0.58, hmac/sha2/hkdf/p256, mdns-sd, tokio-tungstenite, rubato), Node 20+, React 18, HeroUI v2, Tailwind 4, Zustand, Vitest, Tauri 2, Android SDK 34, Kotlin.

---

## How the stage gates work

- Each stage ends with a **GATE — STOP HERE** block. The implementation agent runs every **Agent check**, pastes results, and **stops**.
- The user then runs the **User checks** (manual, hardware where noted).
- Proceed only when all checks pass. On failure: fix within the stage, re-run the gate.
- Commit at every task boundary. Tag each gate: `git tag stage-N-done`.

## Environment requirements

| Stages | Where | Hardware |
|---|---|---|
| 0–3 | Any OS with Rust + Node (WSL/Linux fine) | none |
| 4, 6 | Windows 10 1803+ dev machine, MSVC toolchain | — |
| 4 (user gate), 6 (user gate) | Windows laptop + Android tablet on same Wi-Fi | receiver app installed |
| 7 | Android SDK 34, device or emulator (UI only) + real device for audio | Android tablet |
| 8 | Wi-Fi Aware-capable Windows adapter + Android device | both devices |
| 9 | Windows dev machine | — |

## Preflight (do before Stage 0)

- [ ] **P-1** Import the existing Android receiver source into `android/` in the monorepo (it is NOT in this workspace; get it from the existing repo). Verify it builds: `cd android && ./gradlew assembleDebug` → BUILD SUCCESSFUL.
- [ ] **P-2** `git init` the monorepo, commit the imported receiver + `Rewave.md` as the baseline.
- [ ] **P-3** Install toolchain: `rustup target add x86_64-pc-windows-msvc`, Node 20, `npm i -g @tauri-apps/cli@2`.

## Stage overview

| Stage | Delivers | Gate proof |
|---|---|---|
| 0 | Monorepo skeleton | everything builds/CI green |
| 1 | `crypto` module | byte-exact known-answer tests |
| 2 | `protocol` module | byte-exact codec tests |
| 3 | discovery + pairing + **simulated receiver** | integration tests vs sim (no hardware) |
| 4 | audio pipeline (WASAPI, pacing) | cadence soak test; **user hears audio on tablet** |
| 5 | `rewave-ui` complete, mock-backed | vitest green; user click-through |
| 6 | Tauri shell wired to core | **user: full discover→pair→stream on hardware** |
| 7 | Android WebView wrapper + ConfirmCard fix | **user: ECDH Confirm pair on device** |
| 8 | Wi-Fi Aware both sides | **user: NAN connect on capable hardware** |
| 9 | hardening + packaging | netmask fix test, MSIX, power modes |

---

## Stage 0 — Monorepo scaffolding

**Files:**
- Create: `Cargo.toml` (workspace), `rewave-core/Cargo.toml`, `rewave-core/src/lib.rs`, `rewave-app/{Cargo.toml,tauri.conf.json,src/main.rs}`, `rewave-ui/package.json`, `check.ps1`, `check.sh`

- [ ] **Step 1: Root layout**

```bash
mkdir -p rewave-core/src rewave-app/src rewave-ui
cat > Cargo.toml << 'EOF'
[workspace]
members = ["rewave-core", "rewave-app"]
resolver = "2"
EOF
```

- [ ] **Step 2: `rewave-core/Cargo.toml`**

```toml
[package]
name = "rewave-core"
version = "0.1.0"
edition = "2021"

[dependencies]
windows = { version = "0.58", features = [
    "Media_Audio", "Media_Devices", "Devices_WiFiDirect",
    "Networking_Sockets", "System_Threading",
    "Win32_Media_Audio", "Win32_System_Com", "Win32_System_Threading",
    "Win32_System_Power", "Win32_NetworkManagement_WiFi",
] }
hmac = "0.12"
sha2 = "0.10"
hkdf = "0.12"
p256 = { version = "0.13", features = ["ecdh"] }
rand = "0.8"
tokio = { version = "1", features = ["full"] }
socket2 = "0.5"
mdns-sd = "0.10"
tokio-tungstenite = "0.24"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rubato = "0.15"
log = "0.4"
env_logger = "0.11"
thiserror = "2"
parking_lot = "0.12"
ringbuf = "0.4"

[dev-dependencies]
hex = "0.4"
```

> Deviation from design doc: adds `rubato` (sinc resampler) — linear interpolation is not acceptable for 44.1→48 kHz audio. Drops `bincode` (unused).

- [ ] **Step 3: `rewave-core/src/lib.rs`**

```rust
pub mod audio;
pub mod connection;
pub mod crypto;
pub mod discovery;
pub mod pairing;
pub mod protocol;
pub mod server;
pub mod stats;
pub mod stream;
pub mod wifi;
```

Create each `mod.rs` as an empty file with `// Stage N` marker.

- [ ] **Step 4: Tauri shell** — `cd rewave-app && cargo tauri init --app-name rewave --window-title Rewave --dist-dir ../rewave-ui/dist --dev-url http://localhost:5173 --before-dev-command "npm run dev --prefix ../rewave-ui" --before-build-command "npm run build --prefix ../rewave-ui"`. Add `rewave-core = { path = "../rewave-core" }` to its `Cargo.toml`.

- [ ] **Step 5: UI scaffold**

```bash
cd rewave-ui
npm create vite@latest . -- --template react-ts
npm i react-router-dom zustand @heroui/react framer-motion
npm i -D tailwindcss @tailwindcss/vite vitest @testing-library/react @testing-library/user-event jsdom
```

- [ ] **Step 6: Verify**

Run: `cargo build --workspace` → compiles. `cd rewave-ui && npm run build` → emits `dist/`.

- [ ] **Step 7: Commit + tag** `git add -A && git commit -m "chore: monorepo scaffold" && git tag stage-0-done`

### GATE 0 — STOP HERE
- **Agent checks:** both build commands above pass on a clean clone (`cargo clean && cargo build --workspace`).
- **User checks:** none (scaffold only).
- **Go:** builds green.

---

## Stage 1 — Crypto module (pure Rust, TDD, no hardware)

Frozen contract: `Rewave.md` §6. All test vectors below were generated with an independent Python (`hashlib`/`cryptography`) implementation of the spec — they are the known answers.

**Files:**
- Create: `rewave-core/src/crypto/{mod.rs,constants.rs,hkdf.rs,session.rs,ecdh.rs}`
- Test: `rewave-core/tests/crypto_vectors.rs`

### Task 1.1: Constants

- [ ] **Step 1: `crypto/constants.rs`**

```rust
pub const PAIRING_INFO: &[u8] = b"rewave-pairing";
pub const KEYID_INFO: &[u8] = b"rewave-keyid";
pub const SESSION_INFO: &[u8] = b"rewave-session";
pub const RESUME_SENDER_CTX: &[u8] = b"resume-sender";
pub const RESUME_RECV_CTX: &[u8] = b"resume-recv";

pub const SHA256: usize = 32;
pub const NONCE_BYTES: usize = 16;
pub const PAIRING_KEY: usize = 32;
pub const SESSION_KEY: usize = 32;
pub const KEY_ID: usize = 8;
pub const EC_PUBKEY_BYTES: usize = 65;
pub const RESUME_HMAC_BYTES: usize = 16;
pub const AUTH_TAG_BYTES: usize = 8;
pub const FINGERPRINT: usize = 8;
```

`crypto/mod.rs`: `pub mod constants; pub mod ecdh; pub mod hkdf; pub mod session;`

- [ ] **Step 2: Commit** `git commit -m "feat(crypto): constants"`

### Task 1.2: HKDF-SHA256 (RFC 5869)

- [ ] **Step 1: Write the failing test** — `rewave-core/tests/crypto_vectors.rs`

```rust
use rewave_core::crypto::hkdf::hkdf_sha256;

#[test]
fn rfc5869_test_case_1() {
    let ikm = [0x0b_u8; 22];
    let salt: Vec<u8> = (0x00..=0x0c).collect();
    let info: Vec<u8> = (0xf0..=0xf9).collect();
    let okm = hkdf_sha256(&ikm, &salt, &info, 42);
    assert_eq!(
        hex::encode(okm),
        "3cb25f25faacd57a90434f64d0362f2a\
         2d2d0a90cf1a5a4c5db02d56ecc4c5bf\
         34007208d5b887185865"
    );
}

#[test]
fn empty_salt_becomes_32_zero_bytes() {
    // RFC 5869 Test Case 2 uses empty salt/info; here we just pin the empty-salt rule.
    let a = hkdf_sha256(b"ikm", &[], b"info", 32);
    let b = hkdf_sha256(b"ikm", &[0u8; 32], b"info", 32);
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run, expect FAIL** — `cargo test -p rewave-core --test crypto_vectors` → compile error (`hkdf` module missing).

- [ ] **Step 3: `crypto/hkdf.rs`**

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// RFC 5869 HKDF-SHA256. Empty salt becomes 32 zero bytes (per Rewave.md §6.1).
pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let zero_salt = [0u8; 32];
    let salt = if salt.is_empty() { &zero_salt[..] } else { salt };
    let prk = <Hmac<Sha256> as Mac>::new_from_slice(salt)
        .expect("HMAC accepts any key length")
        .chain_update(ikm)
        .finalize()
        .into_bytes();

    let mut okm = Vec::with_capacity(length);
    let mut t: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while okm.len() < length {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&prk).expect("prk is 32 bytes");
        mac.update(&t);
        mac.update(info);
        mac.update(&[counter]);
        t = mac.finalize().into_bytes().to_vec();
        okm.extend_from_slice(&t);
        counter = counter.wrapping_add(1);
    }
    okm.truncate(length);
    okm
}
```

- [ ] **Step 4: Run, expect PASS** — both tests green.
- [ ] **Step 5: Commit** `git commit -m "feat(crypto): HKDF-SHA256 with RFC 5869 vectors"`

### Task 1.3: PIN session (M6 v1 path) — the nonce-order trap

Fixed test nonces (used across all Stage 1 vectors):

```
Ns (sender nonce)   = 0011223344556677889900aabbccddee
Nr (receiver nonce) = ffeeddccbbaa00998877665544332211
PIN                 = "1234"
```

- [ ] **Step 1: Append failing tests**

```rust
use rewave_core::crypto::session::{pin_handshake_response, pin_session_key, m6_tag};

const NS: [u8; 16] = hex_literal("0011223344556677889900aabbccddee");
const NR: [u8; 16] = hex_literal("ffeeddccbbaa00998877665544332211");

#[test]
fn pin_response_uses_sender_nonce_first() {
    let r = pin_handshake_response(b"1234", &NS, &NR);
    assert_eq!(
        hex::encode(r),
        "6f2f652fe6a0cdc9f8a462e51ee2887674d41e36c545abd25e12bc9c767f20ba"
    );
}

#[test]
fn pin_session_key_uses_receiver_nonce_first_in_salt() {
    let k = pin_session_key(b"1234", &NS, &NR);
    assert_eq!(
        hex::encode(k),
        "6f623754cb2b8282da799817ae81796b9551242c2d058c54ec2846f47288cac0"
    );
}

#[test]
fn m6_tag_covers_seq_be_and_pcm_only() {
    let key = pin_session_key(b"1234", &NS, &NR);
    let pcm: Vec<u8> = (0..960).map(|i| (i % 256) as u8).collect();
    let tag = m6_tag(&key, 1, &pcm);
    assert_eq!(hex::encode(tag), "593a4d9af4447956");
}
```

Add to the test file top:

```rust
const fn hex_literal(s: &str) -> [u8; 16] {
    // tiny const hex decoder for readability
    let b = s.as_bytes();
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = (hex_val(b[i * 2]) << 4) | hex_val(b[i * 2 + 1]);
        i += 1;
    }
    out
}
const fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("bad hex"),
    }
}
```

- [ ] **Step 2: Run, expect FAIL** (functions missing).
- [ ] **Step 3: `crypto/session.rs`**

```rust
use super::constants::{AUTH_TAG_BYTES, SESSION_INFO, SESSION_KEY};
use super::hkdf::hkdf_sha256;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// HMAC-SHA256(pin, Ns ‖ Nr) — sender nonce FIRST (Rewave.md §6.4).
pub fn pin_handshake_response(pin_ascii: &[u8], ns: &[u8; 16], nr: &[u8; 16]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pin_ascii).expect("any len");
    mac.update(ns);
    mac.update(nr);
    mac.finalize().into_bytes().into()
}

/// HKDF(ikm=pin, salt=Nr‖Ns, info="rewave-session", 32) — receiver nonce FIRST in salt.
pub fn pin_session_key(pin_ascii: &[u8], ns: &[u8; 16], nr: &[u8; 16]) -> [u8; SESSION_KEY] {
    let mut salt = Vec::with_capacity(32);
    salt.extend_from_slice(nr);
    salt.extend_from_slice(ns);
    hkdf_sha256(pin_ascii, &salt, SESSION_INFO, SESSION_KEY)
        .try_into()
        .expect("length pinned")
}

/// tag = HMAC-SHA256(session_key, seq_BE_4 ‖ pcm_960)[:8]. NOT flags, NOT full datagram.
pub fn m6_tag(session_key: &[u8; 32], seq: u32, pcm_960: &[u8]) -> [u8; AUTH_TAG_BYTES] {
    debug_assert_eq!(pcm_960.len(), 960);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(session_key).expect("32 bytes");
    mac.update(&seq.to_be_bytes());
    mac.update(pcm_960);
    let digest = mac.finalize().into_bytes();
    digest[..AUTH_TAG_BYTES].try_into().expect("slice len")
}
```

- [ ] **Step 4: Run, expect PASS** (all 3).
- [ ] **Step 5: Commit** `git commit -m "feat(crypto): PIN session derivation, M6 tag"`

### Task 1.4: ECDH P-256 + pairing key + key id + ECDH session

Fixed test keys (deterministic):

```
sender priv scalar  = 0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20
receiver priv scalar= 0x2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40
```

Expected:

```
sender pubkey   = 04515c3d6eb9e396b904d3feca7f54fdcd0cc1e997bf375dca515ad0a6c3b4035f4536be3a50f318fbf9a5475902a221502bef0d57e08c53b2cc0a56f17d9f9354
receiver pubkey = 041f140146bfb1b251f84f4ddbe0d4cdcfd77afd984a9520e35794021f8312bb9eec995a08b1fa7704df3dcc0b50a9665263fb7711f95f9f8a449c5096e47c892b
shared secret   = 4fe243908f378aa1c2a69538822e6ed908c3225d8692575507c649901245150a
pairing_key     = df11da0a69555c8462e9b53020fb3b1307635a207fa141ddc629901dba796ceb
key_id          = c134ef58cb87e775
ECDH session    = 8d53f3e63960fe48eec89405147d4dd338ff11dcf9b84276667c7bd718c4ab38
```

- [ ] **Step 1: Append failing tests**

```rust
use rewave_core::crypto::ecdh::*;

fn sender_priv() -> p256::SecretKey {
    p256::SecretKey::from_slice(&(1u8..=32).collect::<Vec<_>>()).unwrap()
}
fn receiver_priv() -> p256::SecretKey {
    p256::SecretKey::from_slice(&(33u8..=64).collect::<Vec<_>>()).unwrap()
}

#[test]
fn pubkey_encoding_is_65_byte_uncompressed() {
    let (priv_out, pub_out) = generate_keypair_from(&sender_priv());
    let _ = priv_out;
    assert_eq!(hex::encode(pub_out), "04515c3d6eb9e396b904d3feca7f54fdcd0cc1e997bf375dca515ad0a6c3b4035f4536be3a50f318fbf9a5475902a221502bef0d57e08c53b2cc0a56f17d9f9354");
}

#[test]
fn ecdh_shared_secret_matches_vector() {
    let rpub = decode_pubkey(&hex::decode("041f140146bfb1b251f84f4ddbe0d4cdcfd77afd984a9520e35794021f8312bb9eec995a08b1fa7704df3dcc0b50a9665263fb7711f95f9f8a449c5096e47c892b").unwrap()).unwrap();
    let shared = ecdh_shared(&sender_priv(), &rpub);
    assert_eq!(hex::encode(shared), "4fe243908f378aa1c2a69538822e6ed908c3225d8692575507c649901245150a");
}

#[test]
fn pairing_key_keyid_session_chain() {
    let shared = hex::decode("4fe243908f378aa1c2a69538822e6ed908c3225d8692575507c649901245150a").unwrap();
    let pk = derive_pairing_key(&shared, &NS, &NR);
    assert_eq!(hex::encode(pk), "df11da0a69555c8462e9b53020fb3b1307635a207fa141ddc629901dba796ceb");
    assert_eq!(hex::encode(derive_key_id(&pk)), "c134ef58cb87e775");
    assert_eq!(hex::encode(derive_session_key(&pk, &NS, &NR)), "8d53f3e63960fe48eec89405147d4dd338ff11dcf9b84276667c7bd718c4ab38");
}

#[test]
fn decode_pubkey_rejects_non_uncompressed_prefix() {
    let mut bad = vec![0x02u8]; // compressed prefix
    bad.extend_from_slice(&[0u8; 32]);
    assert!(decode_pubkey(&bad).is_err());
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: `crypto/ecdh.rs`**

```rust
use super::constants::{KEYID_INFO, KEY_ID, PAIRING_INFO, PAIRING_KEY, SESSION_INFO, SESSION_KEY};
use super::hkdf::hkdf_sha256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};

pub fn generate_keypair() -> (SecretKey, [u8; 65]) {
    let sk = SecretKey::random(&mut rand::rngs::OsRng);
    let pk = encode_pubkey(&sk.public_key());
    (sk, pk)
}

pub fn generate_keypair_from(sk: &SecretKey) -> (SecretKey, [u8; 65]) {
    (sk.clone(), encode_pubkey(&sk.public_key()))
}

/// 65-byte uncompressed point: 0x04 ‖ X[32] ‖ Y[32] (Rewave.md §6.7).
pub fn encode_pubkey(pk: &PublicKey) -> [u8; 65] {
    pk.to_encoded_point(false).as_bytes().try_into().expect("65 bytes")
}

/// Rejects anything that is not a 65-byte 0x04-prefixed point.
pub fn decode_pubkey(bytes: &[u8]) -> Result<PublicKey, CryptoError> {
    if bytes.len() != 65 || bytes[0] != 0x04 {
        return Err(CryptoError::BadPubkeyEncoding);
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| CryptoError::BadPubkeyEncoding)
}

/// Shared secret = 32-byte X coordinate of ECDH (Rewave.md §6.7).
pub fn ecdh_shared(our_priv: &SecretKey, peer_pub: &PublicKey) -> [u8; 32] {
    let shared = p256::ecdh::diffie_hellman(our_priv.to_nonzero_scalar(), peer_pub.as_affine());
    shared.raw_secret_bytes().into()
}

/// IKM = shared(32) ‖ Ns(16) ‖ Nr(16); salt = Nr‖Ns; info = "rewave-pairing".
pub fn derive_pairing_key(shared: &[u8; 32], ns: &[u8; 16], nr: &[u8; 16]) -> [u8; PAIRING_KEY] {
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(shared);
    ikm.extend_from_slice(ns);
    ikm.extend_from_slice(nr);
    let mut salt = Vec::with_capacity(32);
    salt.extend_from_slice(nr);
    salt.extend_from_slice(ns);
    hkdf_sha256(&ikm, &salt, PAIRING_INFO, PAIRING_KEY).try_into().expect("len")
}

/// key_id = HKDF(pairing_key, salt="", info="rewave-keyid", 8).
pub fn derive_key_id(pairing_key: &[u8; 32]) -> [u8; KEY_ID] {
    hkdf_sha256(pairing_key, &[], KEYID_INFO, KEY_ID).try_into().expect("len")
}

/// session = HKDF(pairing_key, salt=Nr‖Ns, info="rewave-session", 32).
pub fn derive_session_key(pairing_key: &[u8; 32], ns: &[u8; 16], nr: &[u8; 16]) -> [u8; SESSION_KEY] {
    let mut salt = Vec::with_capacity(32);
    salt.extend_from_slice(nr);
    salt.extend_from_slice(ns);
    hkdf_sha256(pairing_key, &salt, SESSION_INFO, SESSION_KEY).try_into().expect("len")
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("pubkey must be 65-byte uncompressed (0x04 prefix)")]
    BadPubkeyEncoding,
}
```

- [ ] **Step 4: Run, expect PASS.** Note `p256::SecretKey::random(&mut rand::rngs::OsRng)` — if the rand-core versions conflict, use `p256::SecretKey::random(&mut p256::elliptic_curve::rand_core::OsRng)` instead. Pick whichever compiles; do not change test vectors.
- [ ] **Step 5: Commit** `git commit -m "feat(crypto): ECDH P-256, pairing chain"`

### Task 1.5: Resume HMACs + synthetic fingerprint

Expected vectors (same keys/nonces as 1.4):

```
resume sender-leg hmac = c109abb6870e36fa114f417bf314840b
resume receiver-leg    = 267579c8567441087b61a8495fea5614
synthetic fingerprint (Ns ‖ "192.168.1.50" ‖ "192.168.1.50") = b66d0e4c94f4d364
```

- [ ] **Step 1: Append failing tests**

```rust
use rewave_core::crypto::session::{resume_sender_hmac, resume_receiver_hmac, synthetic_fingerprint_v1};

#[test]
fn resume_hmac_labels_and_order() {
    let pk = hex::decode("df11da0a69555c8462e9b53020fb3b1307635a207fa141ddc629901dba796ceb").unwrap();
    let kid: [u8; 8] = hex::decode("c134ef58cb87e775").unwrap().try_into().unwrap();
    let pk: &[u8; 32] = pk.as_slice().try_into().unwrap();
    assert_eq!(hex::encode(resume_sender_hmac(pk, &kid, &NS)), "c109abb6870e36fa114f417bf314840b");
    assert_eq!(hex::encode(resume_receiver_hmac(pk, &kid, &NS, &NR)), "267579c8567441087b61a8495fea5614");
}

#[test]
fn v1_synthetic_fingerprint() {
    assert_eq!(hex::encode(synthetic_fingerprint_v1(&NS, "192.168.1.50")), "b66d0e4c94f4d364");
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Append to `crypto/session.rs`**

```rust
use super::constants::{FINGERPRINT, RESUME_HMAC_BYTES, RESUME_RECV_CTX, RESUME_SENDER_CTX};

/// HMAC(pairing_key, key_id ‖ Ns ‖ "resume-sender")[:16] — sender has not seen Nr yet.
pub fn resume_sender_hmac(pk: &[u8; 32], key_id: &[u8; 8], ns: &[u8; 16]) -> [u8; RESUME_HMAC_BYTES] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pk).expect("32");
    mac.update(key_id);
    mac.update(ns);
    mac.update(RESUME_SENDER_CTX);
    mac.finalize().into_bytes()[..RESUME_HMAC_BYTES].try_into().expect("len")
}

/// HMAC(pairing_key, key_id ‖ Nr ‖ Ns ‖ "resume-recv")[:16].
pub fn resume_receiver_hmac(pk: &[u8; 32], key_id: &[u8; 8], ns: &[u8; 16], nr: &[u8; 16]) -> [u8; RESUME_HMAC_BYTES] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pk).expect("32");
    mac.update(key_id);
    mac.update(nr);
    mac.update(ns);
    mac.update(RESUME_RECV_CTX);
    mac.finalize().into_bytes()[..RESUME_HMAC_BYTES].try_into().expect("len")
}

/// SHA-256(senderNonce ‖ host ‖ host)[:8] — v1 PIN-path synthetic fingerprint (§7.4).
pub fn synthetic_fingerprint_v1(ns: &[u8; 16], host_addr: &str) -> [u8; FINGERPRINT] {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(ns);
    h.update(host_addr.as_bytes());
    h.update(host_addr.as_bytes());
    h.finalize()[..FINGERPRINT].try_into().expect("len")
}
```

- [ ] **Step 4: Run, expect PASS** — full suite: `cargo test -p rewave-core` → 11 tests green.
- [ ] **Step 5: Commit + tag** `git commit -m "feat(crypto): resume HMACs, v1 fingerprint" && git tag stage-1-done`

### GATE 1 — STOP HERE
- **Agent checks:** `cargo test -p rewave-core` → 11/11 pass. `cargo clippy -p rewave-core -- -D warnings` clean.
- **User checks:** read `rewave-core/tests/crypto_vectors.rs`; spot-check one vector against `Rewave.md` §6 formulas (any Python REPL).
- **Go:** all vectors byte-exact. Any divergence = STOP, find endianness/nonce-order bug before continuing.

---

## Stage 2 — Wire protocol codec (TDD)

Frozen contract: `Rewave.md` §4, §5. Three endiannesses: seq BE u32, PCM LE int16, control scalars BE.

**Files:**
- Create: `rewave-core/src/protocol/{mod.rs,datagram.rs,control.rs,dispatch.rs}`
- Test: `rewave-core/tests/protocol_vectors.rs`

### Task 2.1: M1/M6 audio datagrams

- [ ] **Step 1: Write the failing tests** — `rewave-core/tests/protocol_vectors.rs`

```rust
use rewave_core::protocol::datagram::*;

fn pcm_pattern() -> Vec<u8> { (0..960).map(|i| (i % 256) as u8).collect() }

#[test]
fn m1_encode_layout() {
    let d = encode_m1(0x01020304, FLAG_STREAM_START | FLAG_STREAM_END, &pcm_pattern());
    assert_eq!(d.len(), 965);
    assert_eq!(&d[0..4], &[0x01, 0x02, 0x03, 0x04]); // seq big-endian
    assert_eq!(d[4], 0x03);
    assert_eq!(&d[5..965], &pcm_pattern()[..]);
}

#[test]
fn m6_encode_appends_tag_over_seq_and_pcm_only() {
    // session key from Stage 1 (PIN "1234" vectors)
    let key: [u8; 32] = hex::decode("6f623754cb2b8282da799817ae81796b9551242c2d058c54ec2846f47288cac0")
        .unwrap().try_into().unwrap();
    let d = encode_m6(1, 0, &pcm_pattern(), &key);
    assert_eq!(d.len(), 973);
    assert_eq!(&d[965..973], &hex::decode("593a4d9af4447956").unwrap()[..]);
    // flags byte (d[4]=0) must NOT affect the tag:
    let d2 = encode_m6(1, FLAG_STREAM_START, &pcm_pattern(), &key);
    assert_eq!(&d2[965..973], &d[965..973]);
}

#[test]
fn decode_roundtrip() {
    let key = [7u8; 32];
    let d = encode_m6(42, FLAG_STREAM_START, &pcm_pattern(), &key);
    let (seq, flags, pcm) = decode_m6(&d, &key).unwrap();
    assert_eq!((seq, flags), (42, FLAG_STREAM_START));
    assert_eq!(pcm, pcm_pattern());
}

#[test]
fn decode_m6_rejects_bad_tag_and_bad_len() {
    let key = [7u8; 32];
    let mut d = encode_m6(1, 0, &pcm_pattern(), &key);
    d[972] ^= 0xff;
    assert!(decode_m6(&d, &key).is_err());
    assert!(decode_m6(&d[..972], &key).is_err());
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: `protocol/datagram.rs`**

```rust
use crate::crypto::session::m6_tag;

pub const PCM_BYTES: usize = 960;
pub const M1_DATAGRAM: usize = 965;
pub const M6_DATAGRAM: usize = 973;
pub const FLAG_STREAM_START: u8 = 0x01;
pub const FLAG_STREAM_END: u8 = 0x02;

pub fn encode_m1(seq: u32, flags: u8, pcm: &[u8]) -> Vec<u8> {
    debug_assert_eq!(pcm.len(), PCM_BYTES);
    let mut out = Vec::with_capacity(M1_DATAGRAM);
    out.extend_from_slice(&seq.to_be_bytes());
    out.push(flags);
    out.extend_from_slice(pcm);
    out
}

pub fn encode_m6(seq: u32, flags: u8, pcm: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let mut out = encode_m1(seq, flags, pcm);
    out.extend_from_slice(&m6_tag(key, seq, pcm));
    out
}

pub fn decode_m1(d: &[u8]) -> Option<(u32, u8, Vec<u8>)> {
    if d.len() != M1_DATAGRAM { return None; }
    Some((u32::from_be_bytes(d[0..4].try_into().ok()?), d[4], d[5..].to_vec()))
}

/// HMAC-verified decode. Returns None on wrong length or tag mismatch.
pub fn decode_m6(d: &[u8], key: &[u8; 32]) -> Option<(u32, u8, Vec<u8>)> {
    if d.len() != M6_DATAGRAM { return None; }
    let seq = u32::from_be_bytes(d[0..4].try_into().ok()?);
    let expected = m6_tag(key, seq, &d[5..965]);
    if d[965..973] != expected { return None; }
    Some((seq, d[4], d[5..965].to_vec()))
}
```

`protocol/mod.rs`: `pub mod control; pub mod datagram; pub mod dispatch;`

- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(protocol): M1/M6 datagram codec"`

### Task 2.2: Control messages (10-byte header, 11 types)

- [ ] **Step 1: Write the failing tests** (key byte-level pins)

```rust
use rewave_core::protocol::control::*;

#[test]
fn header_is_10_bytes_with_rewave_magic() {
    let h = encode_header(2, TYPE_HELLO, 3);
    assert_eq!(&h[0..6], b"REWAVE");
    assert_eq!(h[6], 2);      // version
    assert_eq!(h[7], 1);      // type
    assert_eq!(&h[8..10], &[0, 3]); // length BE
    assert_eq!(h.len(), 10);
}

#[test]
fn hello_v2_payload_layout() {
    let m = ControlMessage::HelloV2 { name: "Tab".into(), proto_flags: 0b11 };
    let bytes = encode(&m);
    assert_eq!(&bytes[0..10], &encode_header(2, TYPE_HELLO, 1 + 3 + 1)[..]);
    assert_eq!(bytes[10], 3);          // name_len
    assert_eq!(&bytes[11..14], b"Tab");
    assert_eq!(bytes[14], 0b11);       // proto_flags
}

#[test]
fn challenge_roundtrip() {
    let m = ControlMessage::Challenge { receiver_nonce: [1u8; 16], salt: [2u8; 16] };
    assert_eq!(decode(&encode(&m)).unwrap(), m);
}

#[test]
fn pair_request_roundtrip() {
    let m = ControlMessage::PairRequest {
        name: "PC".into(), sender_pubkey: [4u8; 65], sender_nonce: [5u8; 16],
    };
    assert_eq!(decode(&encode(&m)).unwrap(), m);
}

#[test]
fn pair_resume_wire_layout() {
    let m = ControlMessage::PairResume { key_id: [9u8; 8], sender_nonce: [1u8; 16], hmac: [2u8; 16] };
    let b = encode(&m);
    assert_eq!(b.len(), 10 + 8 + 16 + 16);
    assert_eq!(&b[10..18], &[9u8; 8]);
    assert_eq!(&b[18..34], &[1u8; 16]);
    assert_eq!(&b[34..50], &[2u8; 16]);
}

#[test]
fn all_11_types_roundtrip() {
    let msgs = vec![
        ControlMessage::HelloV1,
        ControlMessage::HereV1 { port: 50000, ipv4: 0xC0A80132 },
        ControlMessage::HereV2 { port: 50000, ipv4: 0xC0A80132, name: "T".into(), receiver_flags: 0 },
        ControlMessage::Challenge { receiver_nonce: [1; 16], salt: [2; 16] },
        ControlMessage::HandshakeResponse { sender_nonce: [3; 16], response: [4; 32] },
        ControlMessage::HandshakeOk,
        ControlMessage::HandshakeFail,
        ControlMessage::PairRequest { name: "n".into(), sender_pubkey: [5; 65], sender_nonce: [6; 16] },
        ControlMessage::PairConfirm { receiver_pubkey: [7; 65], receiver_nonce: [8; 16], key_id: [9; 8] },
        ControlMessage::PairDeny { reason: 1 },
        ControlMessage::PairResume { key_id: [9; 8], sender_nonce: [1; 16], hmac: [2; 16] },
        ControlMessage::PairResumeOk { receiver_nonce: [3; 16], hmac: [4; 16] },
    ];
    for m in msgs { assert_eq!(decode(&encode(&m)).unwrap(), m); }
}

#[test]
fn decode_rejects_bad_magic_and_truncation() {
    let mut b = encode(&ControlMessage::HandshakeOk);
    b[0] = b'X';
    assert!(decode(&b).is_none());
    assert!(decode(&encode(&ControlMessage::HandshakeOk)[..9]).is_none());
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: `protocol/control.rs`** — implement:

```rust
pub const MAGIC: &[u8; 6] = b"REWAVE";
pub const HEADER_LEN: usize = 10;

pub const TYPE_HELLO: u8 = 1;
pub const TYPE_HERE: u8 = 2;
pub const TYPE_CHALLENGE: u8 = 3;
pub const TYPE_HANDSHAKE_RESPONSE: u8 = 4;
pub const TYPE_HANDSHAKE_OK: u8 = 5;
pub const TYPE_HANDSHAKE_FAIL: u8 = 6;
pub const TYPE_PAIR_REQUEST: u8 = 7;
pub const TYPE_PAIR_CONFIRM: u8 = 8;
pub const TYPE_PAIR_DENY: u8 = 9;
pub const TYPE_PAIR_RESUME: u8 = 10;
pub const TYPE_PAIR_RESUME_OK: u8 = 11;

pub const PROTO_FLAG_PAIRED: u8 = 0x01;
pub const PROTO_FLAG_SUPPORTS_CONFIRM: u8 = 0x02;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    HelloV1,
    HelloV2 { name: String, proto_flags: u8 },
    HereV1 { port: u16, ipv4: u32 },
    HereV2 { port: u16, ipv4: u32, name: String, receiver_flags: u8 },
    Challenge { receiver_nonce: [u8; 16], salt: [u8; 16] },
    HandshakeResponse { sender_nonce: [u8; 16], response: [u8; 32] },
    HandshakeOk,
    HandshakeFail,
    PairRequest { name: String, sender_pubkey: [u8; 65], sender_nonce: [u8; 16] },
    PairConfirm { receiver_pubkey: [u8; 65], receiver_nonce: [u8; 16], key_id: [u8; 8] },
    PairDeny { reason: u8 },
    PairResume { key_id: [u8; 8], sender_nonce: [u8; 16], hmac: [u8; 16] },
    PairResumeOk { receiver_nonce: [u8; 16], hmac: [u8; 16] },
}

pub fn encode_header(version: u8, msg_type: u8, payload_len: u16) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..6].copy_from_slice(MAGIC);
    h[6] = version;
    h[7] = msg_type;
    h[8..10].copy_from_slice(&payload_len.to_be_bytes());
    h
}
```

Then `encode(&ControlMessage) -> Vec<u8>` and `decode(&[u8]) -> Option<ControlMessage>` with one match arm per variant, following the exact payload orders in `Rewave.md` §5.2 (name fields: `name_len u8 ‖ utf8`; multi-byte scalars BE). `HelloV1`/`HelloV2` share TYPE 1 — encode picks version by variant; decode of TYPE 1 picks by payload length (0 → V1, else V2). Same for HERE (TYPE 2): 6-byte payload → V1, longer → V2. Full match implementation is mechanical; the tests above pin every offset.

- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(protocol): control message codec (11 types)"`

### Task 2.3: Length-based dispatch + replay window (receiver-side rules, §4.3/§6.6)

- [ ] **Step 1: Write the failing test**

```rust
use rewave_core::protocol::dispatch::*;

#[test]
fn dispatch_table_matches_spec() {
    let mut d = Dispatcher::new();
    let m1 = vec![0u8; 965];
    let m6 = vec![0u8; 973];
    assert_eq!(d.classify(&m1), Class::LegacyAccept);       // 965, no session
    d.adopt_session([1u8; 32]);                              // session established
    assert_eq!(d.classify(&m1), Class::LegacyRejected);      // 965 with session
    assert_eq!(d.classify(&m6), Class::AuthCandidate);       // 973 with session → verify HMAC next
    assert_eq!(d.classify(&[0u8; 100]), Class::Other);       // unknown length
}

#[test]
fn replay_protection_strictly_increasing() {
    let mut d = Dispatcher::new();
    d.adopt_session([1u8; 32]);
    assert!(d.check_and_advance_seq(0));   // first seq 0 accepted (init -1)
    assert!(d.check_and_advance_seq(1));
    assert!(!d.check_and_advance_seq(1));  // replay
    assert!(!d.check_and_advance_seq(0));  // behind
    d.reset_session();
    assert!(d.check_and_advance_seq(0));   // reset re-accepts seq 0
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: `protocol/dispatch.rs`**

```rust
pub enum Class { LegacyAccept, LegacyRejected, AuthCandidate, Control, Other }

pub struct Dispatcher {
    session_key: Option<[u8; 32]>,
    last_authenticated_seq: i64,
}

impl Dispatcher {
    pub fn new() -> Self { Self { session_key: None, last_authenticated_seq: -1 } }
    pub fn adopt_session(&mut self, key: [u8; 32]) { self.session_key = Some(key); self.last_authenticated_seq = -1; }
    pub fn reset_session(&mut self) { self.session_key = None; self.last_authenticated_seq = -1; }

    pub fn classify(&self, d: &[u8]) -> Class {
        match (d.len(), self.session_key.is_some()) {
            (965, false) => Class::LegacyAccept,
            (965, true) => Class::LegacyRejected,
            (973, true) => Class::AuthCandidate,
            (973, false) => Class::Other,               // dropped as bytesDropped
            (n, _) if n >= 10 && &d[0..6] == b"REWAVE" => Class::Control,
            _ => Class::Other,
        }
    }

    /// Strict seq > last (Rewave.md §6.6). u32 wrap intentionally unhandled.
    pub fn check_and_advance_seq(&mut self, seq: u32) -> bool {
        if (seq as i64) > self.last_authenticated_seq {
            self.last_authenticated_seq = seq as i64;
            true
        } else { false }
    }
}
```

- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit + tag** `git commit -m "feat(protocol): dispatch + replay guard" && git tag stage-2-done`

### GATE 2 — STOP HERE
- **Agent checks:** `cargo test -p rewave-core` → all green (Stage 1 + Stage 2 suites). Confirm the `m6_encode_appends_tag_over_seq_and_pcm_only` test proves flags byte is excluded from the tag.
- **User checks:** none (pure codec).
- **Go:** green suite.

---

## Stage 3 — Discovery, pairing & the simulated receiver (no hardware)

The **simulated receiver** (`rewave-core/tests/simreceiver/mod.rs`) is a Rust implementation of the receiver side of the spec. It doubles as an executable spec and lets the agent test discovery + pairing end-to-end without Android hardware.

**Files:**
- Create: `rewave-core/src/discovery/{mod.rs,broadcast.rs,mdns.rs}`, `rewave-core/src/pairing/{mod.rs,store.rs,flow.rs}`, `rewave-core/src/connection/{mod.rs,orchestrator.rs}`
- Test: `rewave-core/tests/simreceiver/mod.rs`, `rewave-core/tests/pairing_integration.rs`, `rewave-core/tests/discovery_integration.rs`

### Task 3.1: TOFU store (`%APPDATA%\rewave\pairings.json` schema)

- [ ] **Step 1: Failing test** — `rewave-core/tests/tofu_store.rs` (uses a temp dir, NOT real %APPDATA%)

```rust
use rewave_core::pairing::store::*;

#[test]
fn schema_matches_frozen_json() {
    let dir = tempfile_dir();
    let mut s = PairingStore::open(&dir.join("pairings.json")).unwrap();
    s.upsert(Pairing {
        peer_id: "b66d0e4c94f4d364".into(),
        fingerprint: "b66d0e4c94f4d364".into(),
        pairing_key: hex::encode([7u8; 32]),
        name: "Tab".into(),
        key_id: "c134ef58cb87e775".into(),
    }).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pairings.json")).unwrap()).unwrap();
    let row = &raw["b66d0e4c94f4d364"];
    assert_eq!(row["fingerprint"], "b66d0e4c94f4d364");
    assert_eq!(row["pairing_key"], hex::encode([7u8; 32]));
    assert_eq!(row["name"], "Tab");
    assert_eq!(row["key_id"], "c134ef58cb87e775");
    // reload + lookup by key_id (used by PAIR_RESUME path)
    let s2 = PairingStore::open(&dir.join("pairings.json")).unwrap();
    assert!(s2.find_by_key_id("c134ef58cb87e775").is_some());
}
```

(`tempfile_dir()` = create a unique dir under `std::env::temp_dir()`; no external crate.)

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `pairing/store.rs`: `PairingStore { path, map: HashMap<String, Pairing> }` with `open` (create-if-missing), `upsert`, `find_by_key_id`, `find_by_peer_id`, `remove`, atomic write (write tmp + rename). Serde derives on `Pairing`.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(pairing): TOFU store"`

### Task 3.2: Simulated receiver harness

- [ ] **Step 1: Implement `tests/simreceiver/mod.rs`**

```rust
//! Simulated Rewave receiver — executable spec for sender-side integration tests.
//! Binds UDP 127.0.0.1:{audio_port, disc_port}; speaks the full §5/§6/§7 protocol.

pub struct SimReceiver {
    pub audio_port: u16,
    pub disc_port: u16,
    pub name: String,
    pub pin: String,
    pub pairing_key: Option<[u8; 32]>,  // set after Confirm
    pub key_id: Option<[u8; 8]>,
    pub last_session_key: Option<[u8; 32]>,
    pub pcm_sink: Arc<Mutex<Vec<(u32, Vec<u8>)>>>, // verified (seq, pcm) pairs
    pub stats: Arc<Mutex<SimStats>>,
}

#[derive(Default)]
pub struct SimStats { pub auth_failures: u64, pub replay_drops: u64, pub accepted: u64, pub pair_requests: u64 }
```

Behavior (one thread per socket):
- **disc socket** on datagram:
  - `HELLO` (v2) → reply `HERE` v2 (port=audio_port, ipv4=0x7F000001, name, flags=0) **then** `CHALLENGE` (random 16B nonce + salt; store nonce).
  - `PAIR_REQUEST` → auto-confirm: generate receiver keypair, compute pairing_key via §7.1 formulas, store key_id, reply `PAIR_CONFIRM`.
  - `PAIR_RESUME` → look up stored pairing_key by key_id, verify sender HMAC (§7.2), generate Nr, adopt session key **before** replying, reply `PAIR_RESUME_OK`.
- **audio socket** on datagram:
  - `HANDSHAKE_RESPONSE` → verify against stored CHALLENGE nonce + PIN (§6.4); on match derive session key, adopt, reply `HANDSHAKE_OK`; else `HANDSHAKE_FAIL`.
  - 973 B → verify M6 tag with adopted session key + strict seq; count stats; push good PCM to `pcm_sink`.
  - 965 B while session adopted → drop (legacy rejected).

Implement with `std::net::UdpSocket` + `read_timeout`, reusing `rewave_core::crypto` and `rewave_core::protocol` (the same code under test — divergence shows up as integration failures in Stage 3.3/3.4, and against the real receiver in Stage 4's user gate).

- [ ] **Step 2: Smoke test the harness itself** — `tests/discovery_integration.rs`:

```rust
#[test]
fn sim_receiver_answers_hello() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let hello = encode(&ControlMessage::HelloV2 { name: "PC".into(), proto_flags: 3 });
    sock.send_to(&hello, format!("127.0.0.1:{}", sim.disc_port)).unwrap();
    let mut buf = [0u8; 1500];
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::HereV2 { .. })));
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::Challenge { .. })));
}
```

- [ ] **Step 3: Run, expect PASS.**
- [ ] **Step 4: Commit** `git commit -m "test: simulated receiver harness"`

### Task 3.3: PIN pairing flow (sender side) vs sim

- [ ] **Step 1: Failing integration test** — `tests/pairing_integration.rs`

```rust
#[test]
fn pin_handshake_end_to_end_vs_sim() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = PairingFlow::new("TestPC".into());
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    flow.pin_handshake("1234", ("127.0.0.1".parse().unwrap(), sim.audio_port)).unwrap();
    assert!(flow.session_key().is_some());
    // streaming 5 M6 packets must all be accepted by the sim
    let key = *flow.session_key().unwrap();
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    for seq in 0..5u32 {
        let pcm = vec![0xAB; 960];
        sock.send_to(&encode_m6(seq, if seq == 0 { FLAG_STREAM_START } else { 0 }, &pcm, &key),
                     format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    let st = sim.stats.lock().unwrap();
    assert_eq!(st.accepted, 5);
    assert_eq!(st.auth_failures, 0);
    assert_eq!(st.replay_drops, 0);
}

#[test]
fn wrong_pin_fails_handshake() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = PairingFlow::new("TestPC".into());
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    assert!(flow.pin_handshake("9999", ("127.0.0.1".parse().unwrap(), sim.audio_port)).is_err());
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement `pairing/flow.rs`** — `PairingFlow` owns one ephemeral UDP socket for control (§2 "single ephemeral source socket"), `hello_and_challenge` (send v2 HELLO to disc port, await HERE+CHALLENGE with 3 s timeout, version-aware per §5.3), `pin_handshake` (send HANDSHAKE_RESPONSE to **audio port**, await OK/FAIL, derive session key via `crypto::session::pin_session_key`, persist synthetic pairing via `PairingStore`, §7.5).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(pairing): PIN flow vs sim"`

### Task 3.4: ECDH Confirm + TOFU Resume vs sim

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn confirm_pairing_end_to_end() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = PairingFlow::new("TestPC".into());
    let result = flow.confirm_pair(("127.0.0.1".parse().unwrap(), sim.disc_port), Duration::from_secs(5)).unwrap();
    assert_eq!(result.key_id, sim.key_id.unwrap());      // key_id verified against derivation
    assert!(flow.session_key().is_some());
    assert!(sim.pairing_key.is_some());
    assert_eq!(sim.pairing_key.unwrap(), result.pairing_key); // both sides derived identical key
}

#[test]
fn tofu_resume_silent_reconnect() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = PairingFlow::new("TestPC".into());
    flow.confirm_pair(("127.0.0.1".parse().unwrap(), sim.disc_port), Duration::from_secs(5)).unwrap();
    // new "launch": fresh flow with persisted store, resume without UI
    let mut flow2 = PairingFlow::new("TestPC".into());
    flow2.load_store(flow.store_path());
    flow2.resume(("127.0.0.1".parse().unwrap(), sim.disc_port), Duration::from_secs(3)).unwrap();
    let key = *flow2.session_key().unwrap();
    // stream 3 packets; sim must accept (it adopted the same session key before replying)
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    for seq in 0..3u32 {
        sock.send_to(&encode_m6(seq, 0, &[0xCD; 960], &key), format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sim.stats.lock().unwrap().accepted, 3);
}
```

> Note: sim must persist its pairing across the two flows (in-memory is fine — same sim instance).

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `confirm_pair` (send PAIR_REQUEST to disc port, await PAIR_CONFIRM/PAIR_DENY with caller-supplied timeout — UI will use 30 s; derive pairing key via §7.1 IKM order, verify `derive_key_id(pairing_key) == key_id` from CONFIRM, derive session key, persist) and `resume` (PAIR_RESUME with `resume_sender_hmac`, await PAIR_RESUME_OK 3 s timeout, verify `resume_receiver_hmac`, derive session key via `derive_session_key`).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(pairing): ECDH Confirm + TOFU Resume"`

### Task 3.5: Broadcast discovery + mDNS vs sim

- [ ] **Step 1: Failing test** — extend sim to answer subnet broadcasts is impractical in CI; instead inject the local broadcast address:

```rust
#[test]
fn broadcast_discovery_finds_sim_on_loopback() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    // override: pretend 127.0.0.1 is our subnet broadcast (unit seam)
    let found = discover_broadcast(Duration::from_secs(3), vec!["127.0.0.1".parse().unwrap()], sim.disc_port).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "SimTab");
    assert_eq!(found[0].audio_port, sim.audio_port);
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement `discovery/broadcast.rs`** — `discover_broadcast(deadline, bcast_addrs, disc_port)`: send v2 HELLO every 1 s until deadline, collect HERE replies into `DiscoveredDevice { name, host, audio_port, disc_port, key_ids: Vec<String>, via: DiscoveryVia::Broadcast }`. Production caller computes `/24` broadcast per interface + `255.255.255.255` fallback (netmask-correct version is Stage 9).
- [ ] **Step 4: mDNS (`discovery/mdns.rs`)** — `MdnsAnnouncer::start(name, port, key_ids)` announces `_rewave-sender._udp.`; `browse_receivers` browses `_rewave._udp.` for 1.5 s and parses TXT (`name`, `paired`, `flags`) into `DiscoveredDevice`. Test: announce a fake `_rewave._udp.` service with `mdns-sd` in the test, assert browse finds it with correct TXT parsing. (mDNS works on loopback on Linux/macOS; on Windows CI mark test `#[ignore]` and verify at Stage 6 gate.)
- [ ] **Step 5: Run both, expect PASS.**
- [ ] **Step 6: Commit** `git commit -m "feat(discovery): broadcast + mDNS"`

### Task 3.6: Connection orchestrator state machine (aware/lan/direct skeleton)

- [ ] **Step 1: Failing test** (pure state machine, injected discovery results)

```rust
#[test]
fn falls_through_aware_to_lan() {
    let mut o = Orchestrator::new(FakeBackends { aware: AwareOutcome::Unsupported, lan: lan_succeeds(), direct: direct_fails() });
    let s = o.run_connect("SimTab");
    assert_eq!(s, ConnectResult::Connected { mode: ConnectionMode::Lan, .. });
}

#[test]
fn all_fail_gives_disconnected_with_backoff_schedule() {
    let mut o = Orchestrator::new(FakeBackends::all_fail());
    assert_eq!(o.run_connect("SimTab"), ConnectResult::Disconnected);
    assert_eq!(o.backoff_schedule().take(5).collect::<Vec<_>>(), vec![1, 2, 4, 8, 16]);
}
```

- [ ] **Step 2: Implement `connection/orchestrator.rs`** — priority chain Aware → Same-LAN → Direct → manual (design §6), `ConnectionMode::{Aware,Lan,Direct,None}`, link-lost at >5 s stall, exponential backoff capped at 30 s. Wi-Fi Aware backend is a stub returning `Unsupported` until Stage 8; Direct backend shells to Stage 9's `wifi/direct.rs` (stub returns `Unavailable` until then).
- [ ] **Step 3: Run, expect PASS.**
- [ ] **Step 4: Commit + tag** `git commit -m "feat(connection): orchestrator state machine" && git tag stage-3-done`

### GATE 3 — STOP HERE
- **Agent checks:** full `cargo test -p rewave-core` green including integration suites; `cargo test -- --ignored` only skipped for platform mDNS if on Windows CI.
- **User checks:** optional — read `tests/pairing_integration.rs` and confirm the flows match `Rewave.md` §7 diagrams.
- **Go:** all pairing flows pass against the sim.

---

## Stage 4 — Audio pipeline (Windows build required)

Frozen contract: `Rewave.md` §3, §10. 48 kHz / int16 / stereo / 5 ms frames / 960 B chunks / 200 pps.

**Files:**
- Create: `rewave-core/src/audio/{mod.rs,framer.rs,capture.rs}`, `rewave-core/src/stream/{mod.rs,engine.rs,pacing.rs}`, `rewave-core/src/bin/rewave-loopback-test.rs`, `rewave-core/src/bin/udp-sink-validator.rs`
- Test: `rewave-core/tests/framer.rs`, `rewave-core/tests/pacing.rs`

### Task 4.1: AudioFramer (pure, TDD — runs on any OS)

- [ ] **Step 1: Failing tests** — `tests/framer.rs`

```rust
use rewave_core::audio::framer::AudioFramer;

#[test]
fn mono_duplicates_to_stereo() {
    let mut f = AudioFramer::new(1, 48_000);
    f.push_f32(&[0.5, -0.5]); // 2 mono frames
    // not enough for a 240-frame chunk; check internal stereo conversion via a full push
    let mut mono = vec![0.0f32; 240];
    mono[0] = 1.0;
    let mut f = AudioFramer::new(1, 48_000);
    let chunks = f.push_f32(&mono);
    assert_eq!(chunks.len(), 1);
    let pcm = &chunks[0];
    // frame 0: L=32767? no — clip/truncate of 1.0 → 32767; L==R
    let l = i16::from_le_bytes([pcm[0], pcm[1]]);
    let r = i16::from_le_bytes([pcm[2], pcm[3]]);
    assert_eq!(l, r);
    assert_eq!(l, 32767);
}

#[test]
fn clips_and_truncates_no_dither() {
    let mut f = AudioFramer::new(2, 48_000);
    let mut stereo = vec![0.0f32; 480];
    stereo[0] = 1.5;   // L0 over-range
    stereo[1] = -1.5;  // R0 under-range
    let chunks = f.push_f32(&stereo);
    let pcm = &chunks[0];
    assert_eq!(i16::from_le_bytes([pcm[0], pcm[1]]), 32767);
    assert_eq!(i16::from_le_bytes([pcm[2], pcm[3]]), -32768);
}

#[test]
fn emits_exactly_one_960b_chunk_per_240_frames() {
    let mut f = AudioFramer::new(2, 48_000);
    assert!(f.push_f32(&vec![0.0; 2 * 239]).is_empty()); // 239 frames
    let out = f.push_f32(&vec![0.0; 2 * 241]);           // +241 = 480 total
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].len(), 960);
}

#[test]
fn surround_downmix_averages_even_odd_channels() {
    let mut f = AudioFramer::new(4, 48_000); // 4ch: L,R,SL,SR
    let mut quad = vec![0.0f32; 4 * 240];
    quad[0] = 1.0; quad[2] = 0.5; // even channels of frame 0 → L = 0.75
    quad[1] = -1.0; quad[3] = 1.0; // odd → R = 0.0
    let pcm = &f.push_f32(&quad)[0];
    assert_eq!(i16::from_le_bytes([pcm[0], pcm[1]]), (0.75f32 * 32767.0) as i16);
    assert_eq!(i16::from_le_bytes([pcm[2], pcm[3]]), 0);
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement `audio/framer.rs`**

```rust
pub struct AudioFramer {
    channels: u16,
    sample_rate: u32,
    resampler: Option<rubato::FastFixedIn<f32>>, // only when |rate-48000|/48000 > 2%
    pending_stereo: Vec<f32>,                    // interleaved L,R accumulator
}

impl AudioFramer {
    pub fn new(channels: u16, sample_rate: u32) -> Self { /* resampler iff off by >2% */ }

    /// Accept interleaved f32 in the source channel layout; return completed 960-byte chunks.
    pub fn push_f32(&mut self, interleaved: &[f32]) -> Vec<Vec<u8>> {
        // 1. resample if needed (rubato FastFixedIn, stereo after downmix)
        // 2. downmix to stereo: 1ch → dup; >2ch → L=avg(even ch), R=avg(odd ch) per §3.3
        // 3. clip ±1.0, scale by 32767.0, truncate to i16 (no dither)
        // 4. LE-encode; emit 960-byte chunk per 240 frames
    }
}
```

Implement exactly in that order (downmix **before** resample so the resampler always sees stereo). Match the test expectations bit-exactly: `1.0 → 32767`, `-1.5 → -32768`, scaling `(sample.clamp(-1.0, 1.0) * 32767.0) as i16` except `-1.0` must map to `-32767`? No — spec says clip ±1.0 then truncate; test pins `-1.5 → -32768`. Resolve: use `(s.clamp(-1.0, 1.0) * 32768.0).clamp(-32768.0, 32767.0) as i16`, which gives `1.0 → 32767`, `-1.0 → -32768`. Update the first test comment accordingly; keep assertions as written.

- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(audio): framer downmix/resample/int16"`

### Task 4.2: Pacing engine (injectable clock, TDD)

- [ ] **Step 1: Failing test** — `tests/pacing.rs`

```rust
use rewave_core::stream::pacing::*;

#[test]
fn wall_clock_anchor_no_drift() {
    let mut clock = FakeClock::new();
    let mut p = Pacer::new_at(clock.now(), Duration::from_millis(5));
    // 200 ticks with 1ms of "work" each: anchor must NOT accumulate the 1ms
    let mut sends = vec![];
    for _ in 0..200 {
        let t = p.next_deadline();
        sends.push(t);
        clock.advance(Duration::from_millis(6)); // 5ms interval + 1ms overshoot (under 10ms threshold)
        p.ack_sent_at(clock.now());
    }
    let drift = (sends[199] - sends[0]).as_millis() as i64 - 199 * 5;
    assert!(drift.abs() <= 1, "drift {}ms", drift); // `nextSend += interval`, not `now += interval`
}

#[test]
fn resync_threshold_drops_backlog_instead_of_bursting() {
    let mut clock = FakeClock::new();
    let mut p = Pacer::new_at(clock.now(), Duration::from_millis(5));
    clock.advance(Duration::from_millis(37)); // simulate stall > 10ms (2 packets)
    let resynced = p.ack_sent_at(clock.now());
    assert!(resynced); // snapped forward
    assert_eq!(p.next_deadline() - clock.now(), Duration::from_millis(5));
}

#[test]
fn plc_repeat_then_silence() {
    let mut src = PlcSource::new();
    src.on_real_chunk(vec![1; 960]);
    assert_eq!(src.next_chunk(), Chunk::Real(vec![1; 960]));
    assert_eq!(src.next_chunk(), Chunk::RepeatLast);  // miss 1
    assert_eq!(src.next_chunk(), Chunk::RepeatLast);  // miss 2
    assert_eq!(src.next_chunk(), Chunk::RepeatLast);  // miss 3
    assert_eq!(src.next_chunk(), Chunk::Silence);     // miss 4
    src.on_real_chunk(vec![2; 960]);
    assert_eq!(src.next_chunk(), Chunk::Real(vec![2; 960])); // recovers
}
```

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement `stream/pacing.rs`** — `Pacer { next_send, interval }` with `RESYNC_THRESHOLD = 10ms`; `ack_sent_at(now) -> bool /*resynced*/` implementing the §10.2 loop; `PlcSource` implementing §10.5 (`MISS_REPEAT_THRESHOLD = 3`, then zero chunk; `DrainToNewest` semantics via `on_real_chunk` replacing backlog).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "feat(stream): pacer + PLC source"`

### Task 4.3: High-res timer + StreamEngine (Windows)

- [ ] **Step 1: `stream/engine.rs`** — dedicated thread: pops `ringbuf` consumer, builds M1/M6 datagram, UDP send to `(receiver, 50000)`, driven by `Pacer`. Sleep primitive on Windows: `CreateWaitableTimerExW` with `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION (0x2)` via the `windows` crate (`Win32_System_Threading`), falling back to `spin_loop` + `yield_now` when unavailable (detect once at engine start):

```rust
#[cfg(windows)]
pub enum Sleeper { HighResTimer(windows::Win32::System::Threading::HANDLE), Spin }

impl Sleeper {
    pub fn new() -> Self {
        // try CreateWaitableTimerExW(.., CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS)
        // on Err or unsupported OS → Spin
    }
    pub fn wait_until(&mut self, deadline: Instant) { /* SetWaitableTimer(negative 100ns rel) / spin */ }
}
```

- [ ] **Step 2: WASAPI capture (`audio/capture.rs`)** — `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)` → `IAudioClient::Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, ...)` → `IAudioCaptureClient` polling loop on its own thread, pushing `Vec<f32>` blocks into the framer; framer chunks into a `ringbuf::HeapRb<u8>` (capacity 100 chunks) shared with the engine. If the `windows`-crate loopback proves unreliable, fall back to `cpal` (design §14 risk table).
- [ ] **Step 3: Manual-run binary** — `src/bin/rewave-loopback-test.rs`: `rewave-loopback-test <receiver_ip> [--pin 1234 | --no-auth]`: with `--pin`, pairs (Stage 3 flows) and streams M6; with `--no-auth`, streams M1 (965 B) — used for the local soak below. Starts capture + engine, prints 1-line stats every second (pps, drops, ring depth).
- [ ] **Step 4: Cadence soak validator** — `src/bin/udp-sink-validator.rs`: binds `:50000`, runs N seconds, reports packet count, mean/p50/p99 inter-arrival, out-of-order count, datagram size histogram.
- [ ] **Step 5: Agent soak test**

```bash
# terminal A (Windows):  cargo run -p rewave-core --bin udp-sink-validator -- 10
# terminal B (Windows):  cargo run -p rewave-core --bin rewave-loopback-test -- 127.0.0.1 --no-auth
```
Expected: 2000 ± 2 packets in 10 s (±0.1%), p99 inter-arrival < 8 ms, all datagrams 965 or 973 B.

- [ ] **Step 6: Commit** `git commit -m "feat(stream): engine + WASAPI capture"`

### GATE 4 — STOP HERE
- **Agent checks:** soak test numbers as above; `cargo test` still green (framer/pacing cross-platform).
- **User checks (HARDWARE):** install/open the existing Android receiver on the tablet, note its IP + PIN, run `rewave-loopback-test <tablet_ip> --pin <pin>` on the Windows laptop while playing audio on the laptop. **Audio must be audible on the tablet**, roughly in sync (<100 ms vs laptop speakers). Confirm no crackling for 5 minutes (PLC/drift on the receiver handles the rest — receiver is unchanged).
- **Go:** user confirms audible, stable audio. If cadence passes but tablet is silent → wire-format divergence; do NOT proceed.

---

## Stage 5 — Shared web UI (`rewave-ui`, mock-backed)

**Files:**
- Create: `rewave-ui/src/{App.tsx,main.tsx,index.css}`, `routes/{devices,dashboard,settings,pair}.tsx`, `bridges/{types.ts,tauri-bridge.ts,android-bridge.ts,mock-bridge.ts}`, `stores/{app-store.ts,device-store.ts,stats-store.ts}`, `components/**` per design §13
- Test: `rewave-ui/src/**/*.test.tsx` (Vitest + Testing Library)

### Task 5.1: Bridge contract + mock

- [ ] **Step 1: `bridges/types.ts`** (single source of truth; mirrors design §5)

```typescript
export type ConnectionMode = 'aware' | 'lan' | 'direct' | 'none';
export interface DiscoveredDevice { id: string; name: string; host: string; audioPort: number; discPort: number; paired: boolean; via: 'broadcast' | 'mdns' | 'aware'; }
export interface StatsEvent { type: 'stats'; latency: number; bitrate: number; packets_sent: number; uptime_seconds: number; buffer_depth: number; buffer_target: number; waveform: number[]; connection_mode: ConnectionMode; link_lost: boolean; }
export type PairState = 'idle' | 'requested' | 'confirmed' | 'denied' | 'completed' | 'failed';
export interface RewaveBridge {
  startStream(host: string, port: number): Promise<void>;
  stopStream(): Promise<void>;
  discover(): Promise<DiscoveredDevice[]>;
  pairPin(pin: string, host: string, port: number): Promise<'completed' | 'failed'>;
  pairConfirm(host: string, port: number): Promise<'completed' | 'denied' | 'failed'>;
  unpair(peerId: string): Promise<void>;
  setPowerMode(mode: 'low_latency' | 'battery_saver'): Promise<void>;
  getConnectionMode(): Promise<ConnectionMode>;
  onStats(cb: (s: StatsEvent) => void): () => void;
  onDiscovery(cb: (d: DiscoveredDevice[]) => void): () => void;
  onPairing(cb: (e: { state: PairState; method: 'pin' | 'confirm'; pin?: string; peerName?: string }) => void): () => void;
}
```

- [ ] **Step 2: `bridges/mock-bridge.ts`** — full in-memory implementation: fake device list, 10 Hz `setInterval` stats generator (sine-wave waveform array of 28 values, latency jittering 28–45 ms, connection_mode 'lan'), scripted pairing (PIN '1234' succeeds, others fail; Confirm resolves 'completed' after 2 s). Bridge selected at startup: `window.Android ? androidBridge : (window.__TAURI__ ? tauriBridge : mockBridge)` in `bridges/index.ts`.

- [ ] **Step 3: Commit** `git commit -m "feat(ui): bridge contract + mock"`

### Task 5.2: Theme + layout + routing

- [ ] **Step 1: `index.css`** — Tailwind 4 (`@import "tailwindcss";`) + the exact CSS custom properties from design §12 (dark defaults, `[data-theme="light"]` overrides) + font imports (Instrument Serif italic for display, DM Sans for body) via `@fontsource-variable/dm-sans` and `@fontsource/instrument-serif` npm packages.
- [ ] **Step 2: `App.tsx`** — React Router v7 routes `/devices`, `/devices/:id`, `/settings`, `/pair` inside `Layout` (Sidebar with Logo/EQ animation, NavItems, StreamBadge; Content = `<Outlet/>`). HeroUI `HeroUIProvider` wrapper; theme toggle writes `data-theme` on `<html>` and persists to `localStorage` key `rewave-theme` ('dark' | 'light' | 'system').
- [ ] **Step 3: Zustand stores** — `app-store` (theme, powerMode), `device-store` (devices[], selectedId, connection state machine per design §2 UI state machine), `stats-store` (latest StatsEvent + 60-sample history ring for sparklines).
- [ ] **Step 4: Commit** `git commit -m "feat(ui): theme, layout, routing, stores"`

### Task 5.3: Pages (Devices, Dashboard, Settings)

- [ ] **Step 1: DevicesPage** — `DiscoveryList` of `DeviceCard`s (name, via-badge, paired badge, Connect button), `EmptyState` with "Scan Again" wired to `bridge.discover()`; auto-scan on mount (design §2: 3 s broadcast + 1.5 s mDNS).
- [ ] **Step 2: DashboardPage** — `StatusBadge`, `MetricGrid` (latency ms, bitrate kbps, packets, uptime), `WaveformCard` (28 bars from `stats.waveform`, animated via framer-motion, accent gradient), `SpectralDisplay` (CSS bar spectrum driven by waveform FFT-lite — bucketed amplitudes are fine), `SessionInfo` (link type, format "48 kHz / 16-bit / stereo", encryption "HMAC-SHA256 (M6)", buffer `depth/target`), `SourceBar` (WASAPI → Oboe, uptime, Stop button).
- [ ] **Step 3: SettingsPage** — ThemeToggle (3-way), PowerModeToggle wired to `bridge.setPowerMode`, AudioSettings (read-only format display), ConnectionSettings (Wi-Fi Direct SSID/pass display placeholder → real values arrive Stage 9).
- [ ] **Step 4: Commit** `git commit -m "feat(ui): devices, dashboard, settings pages"`

### Task 5.4: PairPage + ConfirmCard (the PairRequestCard bug fix — test first)

- [ ] **Step 1: Failing test** — `src/routes/pair.test.tsx`

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PairPage } from './pair';

test('ConfirmCard renders pending pair request with Accept and Deny', async () => {
  render(<PairPage />, { wrapper: TestProviders });
  // mock bridge emits a pairing 'requested' event with peerName 'SimTab'
  mockBridge.emitPairing({ state: 'requested', method: 'confirm', peerName: 'SimTab' });
  expect(await screen.findByText(/SimTab wants to pair/i)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: /accept/i })).toBeEnabled();
  expect(screen.getByRole('button', { name: /deny/i })).toBeEnabled();
});

test('Accept within 30s calls pairConfirm; countdown shows remaining time', async () => {
  render(<PairPage />, { wrapper: TestProviders });
  mockBridge.emitPairing({ state: 'requested', method: 'confirm', peerName: 'SimTab' });
  await screen.findByText(/SimTab wants to pair/i);
  expect(screen.getByText(/30s/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /accept/i }));
  expect(mockBridge.pairConfirm).toHaveBeenCalled();
});

test('Deny emits denied and dismisses card', async () => { /* …calls deny path, card leaves DOM… */ });
```

- [ ] **Step 2: Run `npx vitest`, expect FAIL.**
- [ ] **Step 3: Implement `routes/pair.tsx`** — `PairMethodSelector` (Confirm default / PIN fallback), `PinEntry` (4-box OTP input, submits `pairPin`), `PinDisplay` (receiver mode: large mono PIN + copy button — rendered when bridge reports method 'pin' with `pin`), `ConfirmCard` (peer name, 30 s countdown ring, Accept/Deny; subscribes `bridge.onPairing`). This component is the direct fix for `Rewave.md` §15.1 #1 — it must render purely from bridge events (no platform rendering dependencies).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit + tag** `git commit -m "feat(ui): pair page with ConfirmCard" && git tag stage-5-done`

### GATE 5 — STOP HERE
- **Agent checks:** `npx vitest run` all green; `npm run build` emits `dist/` < 1 MB gzip total.
- **User checks:** `npm run dev`, open `http://localhost:5173` (mock bridge auto-active): devices list populates, dashboard animates, `/pair` shows the Confirm card when you trigger it from the mock's debug panel (`window.__mockBridge.emitPairing(...)` in devtools), theme toggle persists across reload, light mode looks correct.
- **Go:** user approves UI.

---

## Stage 6 — Tauri shell wired to real backend (Windows)

**Files:**
- Create: `rewave-core/src/server/{mod.rs,ipc.rs,ws.rs}`, `rewave-app/src/main.rs`
- Modify: `rewave-ui/src/bridges/tauri-bridge.ts`

### Task 6.1: Tauri IPC commands

- [ ] **Step 1: `server/ipc.rs`** — implement the exact command set from design §5: `start_stream`, `stop_stream`, `discover`, `pair_pin`, `pair_confirm`, `unpair`, `set_power_mode`, `connect_wifi`, `disconnect_wifi`, `get_connection_mode`. Each is a thin async wrapper over an `AppCore` singleton (parking_lot `RwLock<Option<RunningEngine>>`) that composes Stage 3 + Stage 4 modules. Serde-serializable DTOs matching `bridges/types.ts` field-for-field (snake_case on the wire).
- [ ] **Step 2: `rewave-app/src/main.rs`** — register commands, single-instance window (1280×800, min 960×640), system tray (Show Window / Quit), `rewave-core` logger init.
- [ ] **Step 3: Commit** `git commit -m "feat(app): Tauri IPC commands"`

### Task 6.2: WebSocket stats server + UI wiring

- [ ] **Step 1: `server/ws.rs`** — `tokio-tungstenite` server on `127.0.0.1:8765`, broadcasting `StatsEvent` JSON at 10 Hz while streaming (fields exactly as `bridges/types.ts`, snake_case). Waveform: 28 RMS buckets computed from the last 140 ms of PCM in the send path.
- [ ] **Step 2: `tauri-bridge.ts`** — implement `RewaveBridge` over `@tauri-apps/api/core.invoke` + `new WebSocket('ws://127.0.0.1:8765')` with auto-reconnect (1 s) and event demux (`stats`/`discovery`/`pairing`).
- [ ] **Step 3: Commit** `git commit -m "feat(app): WS stats server + tauri bridge"`

### Task 6.3: End-to-end smoke on Windows

- [ ] **Step 1: `cargo tauri dev`** — app launches, discovers the tablet (Same-LAN), pairs (PIN and Confirm), streams, dashboard shows live stats.
- [ ] **Step 2: Commit + tag** `git commit -m "feat(app): end-to-end wired" && git tag stage-6-done`

### GATE 6 — STOP HERE (major user gate)
- **Agent checks:** `cargo tauri build` produces an installer/exe without errors; app cold-starts < 3 s; no IPC errors in console.
- **User checks (HARDWARE, full pass):**
  1. Launch app on laptop + existing receiver on tablet (same Wi-Fi).
  2. Devices page finds the tablet ≤ 5 s.
  3. First-ever pair: Confirm flow — tap Confirm on the tablet's **current (old Compose) UI if it renders, else use PIN**. Note: the receiver-side Confirm UI is fixed only in Stage 7.
  4. Stream 5 min: dashboard latency reads 30–80 ms, no dropouts, link-lost badge never appears.
  5. Quit both apps, relaunch: **silent TOFU resume** — streaming starts with zero pairing UI.
  6. `unpair` from Settings → next connect requires pairing again.
- **Go:** all six pass.

---

## Stage 7 — Android WebView wrapper (+ ConfirmCard fix on device)

**Files:**
- Create: `android/app/src/main/java/.../rewave/RewaveJsBridge.kt`, `android/app/src/main/java/.../rewave/RewaveAwarePublisher.kt` (stub until Stage 8)
- Modify: `android/app/src/main/java/.../rewave/MainActivity.kt`, `android/app/src/main/java/.../rewave/AudioStreamService.kt`, `android/app/build.gradle.kts`
- Build dep: `rewave-ui/dist` → `android/app/src/main/assets/rewave-ui/`

### Task 7.1: Gradle asset pipeline

- [ ] **Step 1: `app/build.gradle.kts` addition**

```kotlin
tasks.register<Exec>("buildRewaveUi") {
    workingDir = rootProject.file("../rewave-ui")
    commandLine = listOf("cmd", "/c", "npm", "run", "build")  // 'npm run build' on macOS/Linux
}
tasks.register<Copy>("copyRewaveUi") {
    dependsOn("buildRewaveUi")
    from(rootProject.file("../rewave-ui/dist"))
    into("src/main/assets/rewave-ui")
}
preBuild.dependsOn("copyRewaveUi")
```

- [ ] **Step 2: Commit** `git commit -m "build(android): UI asset pipeline"`

### Task 7.2: RewaveJsBridge

- [ ] **Step 1: `RewaveJsBridge.kt`** — implements every method/callback in design §4/§5 exactly:

```kotlin
class RewaveJsBridge(
    private val service: AudioStreamService,
    private val webView: WebView,
    private val scope: CoroutineScope,
) {
    @JavascriptInterface fun getDevices(): String        // JSON array of DiscoveredDevice
    @JavascriptInterface fun getThisDevice(): String     // { name, ip, keyIds }
    @JavascriptInterface fun getPairingCode(): String    // 4-digit PIN
    @JavascriptInterface fun getPairingState(): String
    @JavascriptInterface fun confirmPair(peerId: String)
    @JavascriptInterface fun denyPair(peerId: String)
    @JavascriptInterface fun getStats(): String          // JSON StatsSnapshot (21-long array mapped to named fields)
    @JavascriptInterface fun getConnectionMode(): String
    @JavascriptInterface fun startWifiAware(): Boolean   // Stage 8; return false for now
    @JavascriptInterface fun stopWifiAware()
    @JavascriptInterface fun disconnect()
    @JavascriptInterface fun setPowerMode(mode: String)  // "low_latency" | "battery_saver" → existing M8 toggle

    private fun emit(jsFn: String, payload: String) =
        webView.post { webView.evaluateJavascript("window.__rewave_$jsFn($payload)", null) }
    fun onPairRequest(info: PairRequestInfo) = emit("onPairRequest", info.toJson())
    fun onStatsUpdate(s: StatsSnapshot) = emit("onStatsUpdate", s.toJson())
    /* …onDeviceDiscovered, onPairComplete, onConnectionModeChanged, onLinkLost… */
}
```

- [ ] **Step 2: Wire into service** — `AudioStreamService` constructs the bridge; its existing pending-pair poller (500 ms) calls `bridge.onPairRequest(...)` for each entry in `pendingPairRequests`; confirm/deny route to the existing `RewavePairingSession` handlers. **No changes to AudioReceiver/AuthSession/native engine.**
- [ ] **Step 3: Commit** `git commit -m "feat(android): JS bridge"`

### Task 7.3: MainActivity WebView host

- [ ] **Step 1: Replace Compose content with WebView**

```kotlin
@SuppressLint("SetJavaScriptEnabled")
class MainActivity : ComponentActivity() {
    override fun onCreate(b: Bundle?) {
        super.onCreate(b)
        val webView = WebView(this).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            addJavascriptInterface(bridge, "Android")
            webViewClient = object : WebViewClient() {
                override fun onPageFinished(v: WebView?, url: String?) { bridge.onUiReady() }
            }
            loadUrl("file:///android_asset/rewave-ui/index.html")
        }
        setContentView(webView)
    }
}
```

- [ ] **Step 2: `bridges/android-bridge.ts`** — implement `RewaveBridge` over `window.Android.*`; register `window.__rewave_*` callbacks that demux into the bridge event subscriptions; poll `getStats()` at 10 Hz (WebView has no push channel for stats; `evaluateJavascript` stats push at 10 Hz is acceptable too — pick poll, simpler).
- [ ] **Step 3: Commit** `git commit -m "feat(android): WebView host"`

### Task 7.4: Device verification

- [ ] **Step 1:** `./gradlew installDebug` on the tablet; open app → new UI loads from assets.
- [ ] **Step 2: Commit + tag** `git commit -m "feat(android): WebView wrapper complete" && git tag stage-7-done`

### GATE 7 — STOP HERE (major user gate)
- **Agent checks:** gradle build green; `RewaveJsBridge` methods all present (lint); app launches on emulator without crashing (audio stack will fail on emulator — expected, `Rewave.md` §8.3/§9.1).
- **User checks (HARDWARE):**
  1. New receiver UI on tablet; Windows app from Stage 6.
  2. **ECDH Confirm first-pair**: connect from Windows → tablet shows ConfirmCard → tap Accept → streaming starts. **This is the §15.1 #1 bug fix — it MUST work.**
  3. Deny path: re-attempt, tap Deny → Windows shows denied.
  4. PIN path still works; silent Resume after kill/relaunch works.
  5. Bluetooth headphones connect to tablet mid-stream → audio routes (M7 unchanged).
- **Go:** Confirm pairing works on device.

---

## Stage 8 — Wi-Fi Aware (hardware-dependent; skip if adapters unsupported)

**Files:**
- Create: `rewave-core/src/wifi/aware.rs`, `rewave-core/src/discovery/wifi_aware.rs`
- Modify: `android/.../RewaveAwarePublisher.kt`, orchestrator backend wiring

- [ ] **Step 1: Preflight capability check** — Windows: `netsh wlan show wirelesscapabilities` (Wi-Fi Direct supported ≠ NAN supported; check adapter/driver docs). Android: `WifiAwareManager.isAvailable`. If either is unsupported, mark stage SKIPPED, tag `stage-8-skipped`, proceed to Stage 9 — the fallback chain already covers connectivity.
- [ ] **Step 2: Android publisher** — implement `RewaveAwarePublisher` per design §11 (service name `"rewave"`, `WifiAwareNetworkSpecifier` data path), wire `startWifiAware`/`stopWifiAware` in the JS bridge.
- [ ] **Step 3: Windows subscriber** — `wifi/aware.rs` via `Windows.Devices.WiFiDirect` per design §11; surface as a `DiscoveryBackend` producing `DiscoveredDevice { via: 'aware' }`.
- [ ] **Step 4: Orchestrator wiring** — replace the Aware stub; aware success → `ConnectionMode::Aware`.
- [ ] **Step 5: Tests** — unit: orchestrator prefers aware when available (fake backend). Hardware: manual only.
- [ ] **Step 6: Commit + tag** `git commit -m "feat: Wi-Fi Aware data path" && git tag stage-8-done`

### GATE 8 — STOP HERE
- **Agent checks:** unit tests green.
- **User checks (HARDWARE):** disable same-LAN (separate networks), both devices Wi-Fi on: connection establishes with mode badge showing "aware"; stream 5 min stable.
- **Go:** NAN path works, or stage cleanly skipped.

---

## Stage 9 — Hardening, Wi-Fi Direct auto-join, packaging

- [ ] **Task 9.1: Netmask-correct broadcast** — replace `/24` assumption: enumerate interfaces with their real netmasks (Windows: `GetAdaptersAddresses` via `windows` crate; compute `ip | ~mask`). Regression test: pure function `broadcast_of(ip, prefix_len)` table-driven (`192.168.1.10/24 → 192.168.1.255`, `10.0.4.7/16 → 10.0.255.255`, `172.16.5.5/12 → 172.31.255.255`).
- [ ] **Task 9.2: `wifi/direct.rs`** — port WLANProfile XML builder **byte-exact** per `Rewave.md` §9.3 (`\r\n`, 2-space indent, escape `& < > "` not `'`, lowercase UTF-8 hex SSID). Test: pin MD5/golden string for SSID `DIRECT-Re-AB12` / pass `aB3xY9kQ`. Then `netsh wlan add profile filename=-` (stdin) + `netsh wlan connect` + `IsConnectedTo` poll (0.5 s × 20).
- [ ] **Task 9.3: Power modes** — `set_power_mode` end-to-end: low_latency → target fill 6 / battery_saver → 12 on the receiver via existing M8 path; persist per side.
- [ ] **Task 9.4: ConnectionSettings real values** — show actual `RewaveLink` SSID/pass on Android; show `--wifi-ssid` equivalent helper text on Windows.
- [ ] **Task 9.5: Packaging** — `cargo tauri build` MSIX/NSIS installer for Windows; Android release APK signing config. Smoke-test install on a clean profile.
- [ ] **Step final: Commit + tag** `git tag stage-9-done && git tag v1.0.0-rewrite`

### GATE 9 — FINAL
- **Agent checks:** full test suite green on Windows + Linux; installer builds.
- **User checks:** fresh install on laptop; Wi-Fi Direct path: tablet shows `DIRECT-Re-XXXX` credentials, laptop auto-joins via Settings → Connect; stream stable; battery-saver mode measurably raises buffer target on dashboard.
- **Go:** ship.

---

## Definition of done (any stage)

- All tests in the repo pass (`cargo test --workspace`, `cd rewave-ui && npx vitest run`).
- `cargo clippy --workspace -- -D warnings` clean.
- No wire-format change vs `Rewave.md` §4–§7 without a doc update in the same commit.
- Gate tag pushed.

## Explicit non-goals (v1 rewrite)

- DPAPI protection of sender TOFU store, SAS/fingerprint UI, PIN rate-limiting, IPv6, u32 seq wrap, iOS/Linux/macOS ports (all tracked in `Rewave.md` §15.3 — do not implement here).
- Changing any constant in `Rewave.md` §6.
