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
