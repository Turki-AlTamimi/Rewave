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
    assert!(decode_m6(&d, &key).is_none());
    assert!(decode_m6(&d[..972], &key).is_none());
}
