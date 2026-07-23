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
    (*shared.raw_secret_bytes()).into()
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
