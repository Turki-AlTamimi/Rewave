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
    let pk = derive_pairing_key(&shared.try_into().unwrap(), &NS, &NR);
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

use rewave_core::crypto::session::{resume_receiver_hmac, resume_sender_hmac, synthetic_fingerprint_v1};

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
