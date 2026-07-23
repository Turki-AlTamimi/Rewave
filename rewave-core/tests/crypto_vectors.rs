use rewave_core::crypto::hkdf::hkdf_sha256;

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

use rewave_core::crypto::session::{m6_tag, pin_handshake_response, pin_session_key};

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
