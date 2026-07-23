use super::constants::{
    AUTH_TAG_BYTES, FINGERPRINT, RESUME_HMAC_BYTES, RESUME_RECV_CTX, RESUME_SENDER_CTX,
    SESSION_INFO, SESSION_KEY,
};
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
