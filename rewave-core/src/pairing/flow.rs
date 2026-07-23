use super::store::{Pairing, PairingStore, StoreError};
use crate::crypto::ecdh;
use crate::crypto::ecdh::CryptoError;
use crate::crypto::session as csession;
use crate::protocol::control::{
    decode, encode, ControlMessage, PROTO_FLAG_PAIRED, PROTO_FLAG_SUPPORTS_CONFIRM,
};
use std::io::{self, ErrorKind};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("timed out waiting for receiver reply")]
    Timeout,
    #[error("no CHALLENGE held; call hello_and_challenge first")]
    NoChallenge,
    #[error("receiver rejected the PIN (HANDSHAKE_FAIL)")]
    HandshakeFailed,
    #[error("pairing denied by receiver, reason {0}")]
    PairDenied(u8),
    #[error("key_id from PAIR_CONFIRM does not match the derived pairing key")]
    KeyIdMismatch,
    #[error("PAIR_RESUME_OK hmac mismatch")]
    BadResumeHmac,
    #[error("no stored pairing to resume")]
    NoPairing,
    #[error("corrupt hex field in pairing store")]
    BadStoreHex,
}

#[derive(Debug, Clone)]
pub struct HelloOutcome {
    pub name: Option<String>,
    pub audio_port: u16,
    pub version: u8,
}

#[derive(Debug, Clone)]
pub struct ConfirmResult {
    pub pairing_key: [u8; 32],
    pub key_id: [u8; 8],
    pub session_key: [u8; 32],
}

pub struct PairingFlow {
    name: String,
    sock: UdpSocket,
    store: PairingStore,
    challenge: Option<([u8; 16], [u8; 16])>,
    peer_name: Option<String>,
    session_key: Option<[u8; 32]>,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex<const N: usize>(s: &str) -> Result<[u8; N], FlowError> {
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<_, _>>()
        .map_err(|_| FlowError::BadStoreHex)?;
    bytes.try_into().map_err(|_| FlowError::BadStoreHex)
}

pub fn default_store_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("rewave").join("pairings.json");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("rewave").join("pairings.json");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config").join("rewave").join("pairings.json");
        }
    }
    std::env::temp_dir().join("rewave").join("pairings.json")
}

impl PairingFlow {
    pub fn new(name: String) -> Self {
        Self::with_store_path(name, &default_store_path()).expect("open default pairing store")
    }

    pub fn with_store_path(name: String, path: &Path) -> Result<Self, FlowError> {
        let sock = UdpSocket::bind((IpAddr::from([0, 0, 0, 0]), 0))?;
        Ok(Self {
            name,
            sock,
            store: PairingStore::open(path)?,
            challenge: None,
            peer_name: None,
            session_key: None,
        })
    }

    pub fn load_store(&mut self, path: &Path) -> Result<(), FlowError> {
        self.store = PairingStore::open(path)?;
        Ok(())
    }

    pub fn store_path(&self) -> &Path {
        self.store.path()
    }

    pub fn session_key(&self) -> Option<&[u8; 32]> {
        self.session_key.as_ref()
    }

    fn recv_datagram(&self, deadline: Instant) -> Result<ControlMessage, FlowError> {
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(FlowError::Timeout);
            }
            self.sock.set_read_timeout(Some(deadline - now))?;
            let mut buf = [0u8; 2048];
            match self.sock.recv_from(&mut buf) {
                Ok((n, _)) => {
                    if let Some(m) = decode(&buf[..n]) {
                        return Ok(m);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    return Err(FlowError::Timeout);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Send v2 HELLO to the discovery port; await HERE + CHALLENGE (3 s). Rewave.md §5.3.
    pub fn hello_and_challenge(
        &mut self,
        target: (IpAddr, u16),
    ) -> Result<HelloOutcome, FlowError> {
        let dest = SocketAddr::new(target.0, target.1);
        let mut proto_flags = PROTO_FLAG_SUPPORTS_CONFIRM;
        if !self.store.is_empty() {
            proto_flags |= PROTO_FLAG_PAIRED;
        }
        self.sock.send_to(
            &encode(&ControlMessage::HelloV2 { name: self.name.clone(), proto_flags }),
            dest,
        )?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut here: Option<HelloOutcome> = None;
        let mut challenge: Option<([u8; 16], [u8; 16])> = None;
        while here.is_none() || challenge.is_none() {
            match self.recv_datagram(deadline)? {
                ControlMessage::HereV1 { port, .. } => {
                    here = Some(HelloOutcome { name: None, audio_port: port, version: 1 });
                }
                ControlMessage::HereV2 { port, name, .. } => {
                    here = Some(HelloOutcome { name: Some(name), audio_port: port, version: 2 });
                }
                ControlMessage::Challenge { receiver_nonce, salt } => {
                    challenge = Some((receiver_nonce, salt));
                }
                _ => {}
            }
        }
        let here = here.expect("loop guarantees both");
        self.challenge = challenge;
        if let Some(n) = &here.name {
            self.peer_name = Some(n.clone());
        }
        Ok(here)
    }

    /// PIN handshake (§6.4): HANDSHAKE_RESPONSE goes to the AUDIO port (§5.3).
    /// On success derives the session key and persists a synthetic pairing (§7.5).
    pub fn pin_handshake(
        &mut self,
        pin: &str,
        audio_target: (IpAddr, u16),
    ) -> Result<(), FlowError> {
        let (nr, _salt) = self.challenge.ok_or(FlowError::NoChallenge)?;
        let ns: [u8; 16] = rand::random();
        let response = csession::pin_handshake_response(pin.as_bytes(), &ns, &nr);
        self.sock.send_to(
            &encode(&ControlMessage::HandshakeResponse { sender_nonce: ns, response }),
            SocketAddr::new(audio_target.0, audio_target.1),
        )?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.recv_datagram(deadline)? {
                ControlMessage::HandshakeOk => break,
                ControlMessage::HandshakeFail => return Err(FlowError::HandshakeFailed),
                _ => {}
            }
        }
        let session_key = csession::pin_session_key(pin.as_bytes(), &ns, &nr);
        self.session_key = Some(session_key);
        let fp = csession::synthetic_fingerprint_v1(&ns, &audio_target.0.to_string());
        let key_id = ecdh::derive_key_id(&session_key);
        self.store.upsert(Pairing {
            peer_id: to_hex(&fp),
            fingerprint: to_hex(&fp),
            pairing_key: to_hex(&session_key),
            name: self
                .peer_name
                .clone()
                .unwrap_or_else(|| audio_target.0.to_string()),
            key_id: to_hex(&key_id),
        })?;
        Ok(())
    }

    /// ECDH Confirm flow (§7.1): PAIR_REQUEST to the discovery port, await
    /// PAIR_CONFIRM/PAIR_DENY with the caller-supplied timeout.
    pub fn confirm_pair(
        &mut self,
        target: (IpAddr, u16),
        timeout: Duration,
    ) -> Result<ConfirmResult, FlowError> {
        let (sk, pub65) = ecdh::generate_keypair();
        let ns: [u8; 16] = rand::random();
        self.sock.send_to(
            &encode(&ControlMessage::PairRequest {
                name: self.name.clone(),
                sender_pubkey: pub65,
                sender_nonce: ns,
            }),
            SocketAddr::new(target.0, target.1),
        )?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.recv_datagram(deadline)? {
                ControlMessage::PairConfirm { receiver_pubkey, receiver_nonce: nr, key_id } => {
                    let rpub = ecdh::decode_pubkey(&receiver_pubkey)?;
                    let shared = ecdh::ecdh_shared(&sk, &rpub);
                    let pairing_key = ecdh::derive_pairing_key(&shared, &ns, &nr);
                    if ecdh::derive_key_id(&pairing_key) != key_id {
                        return Err(FlowError::KeyIdMismatch);
                    }
                    let session_key = ecdh::derive_session_key(&pairing_key, &ns, &nr);
                    self.session_key = Some(session_key);
                    let fp = fingerprint_v2(
                        &receiver_pubkey,
                        self.peer_name.as_deref().unwrap_or(""),
                        &target.0.to_string(),
                    );
                    self.store.upsert(Pairing {
                        peer_id: to_hex(&fp),
                        fingerprint: to_hex(&fp),
                        pairing_key: to_hex(&pairing_key),
                        name: self
                            .peer_name
                            .clone()
                            .unwrap_or_else(|| target.0.to_string()),
                        key_id: to_hex(&key_id),
                    })?;
                    return Ok(ConfirmResult { pairing_key, key_id, session_key });
                }
                ControlMessage::PairDeny { reason } => {
                    return Err(FlowError::PairDenied(reason));
                }
                _ => {}
            }
        }
    }

    /// TOFU Resume (§7.2): PAIR_RESUME to the discovery port, await PAIR_RESUME_OK.
    pub fn resume(&mut self, target: (IpAddr, u16), timeout: Duration) -> Result<(), FlowError> {
        let pairing = self.store.first().cloned().ok_or(FlowError::NoPairing)?;
        let pk: [u8; 32] = from_hex(&pairing.pairing_key)?;
        let kid: [u8; 8] = from_hex(&pairing.key_id)?;
        let ns: [u8; 16] = rand::random();
        let hmac = csession::resume_sender_hmac(&pk, &kid, &ns);
        self.sock.send_to(
            &encode(&ControlMessage::PairResume { key_id: kid, sender_nonce: ns, hmac }),
            SocketAddr::new(target.0, target.1),
        )?;
        let deadline = Instant::now() + timeout;
        loop {
            if let ControlMessage::PairResumeOk { receiver_nonce: nr, hmac: rh } =
                self.recv_datagram(deadline)?
            {
                if csession::resume_receiver_hmac(&pk, &kid, &ns, &nr) != rh {
                    return Err(FlowError::BadResumeHmac);
                }
                self.session_key = Some(ecdh::derive_session_key(&pk, &ns, &nr));
                return Ok(());
            }
        }
    }
}

/// SHA-256(peer_pubkey[65] ‖ peer_name_utf8 ‖ peer_ip_ascii)[:8] — §7.4.
fn fingerprint_v2(peer_pubkey: &[u8; 65], peer_name: &str, peer_ip: &str) -> [u8; 8] {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(peer_pubkey);
    h.update(peer_name.as_bytes());
    h.update(peer_ip.as_bytes());
    h.finalize()[..8].try_into().expect("len")
}
