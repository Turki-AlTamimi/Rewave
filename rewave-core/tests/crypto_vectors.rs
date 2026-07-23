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
