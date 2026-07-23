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
