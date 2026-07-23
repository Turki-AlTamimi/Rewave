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

fn push_name(out: &mut Vec<u8>, name: &str) {
    out.push(u8::try_from(name.len()).expect("name must fit in u8 length"));
    out.extend_from_slice(name.as_bytes());
}

pub fn encode(m: &ControlMessage) -> Vec<u8> {
    let (version, msg_type, payload): (u8, u8, Vec<u8>) = match m {
        ControlMessage::HelloV1 => (1, TYPE_HELLO, Vec::new()),
        ControlMessage::HelloV2 { name, proto_flags } => {
            let mut p = Vec::new();
            push_name(&mut p, name);
            p.push(*proto_flags);
            (2, TYPE_HELLO, p)
        }
        ControlMessage::HereV1 { port, ipv4 } => {
            let mut p = Vec::with_capacity(6);
            p.extend_from_slice(&port.to_be_bytes());
            p.extend_from_slice(&ipv4.to_be_bytes());
            (1, TYPE_HERE, p)
        }
        ControlMessage::HereV2 { port, ipv4, name, receiver_flags } => {
            let mut p = Vec::new();
            p.extend_from_slice(&port.to_be_bytes());
            p.extend_from_slice(&ipv4.to_be_bytes());
            push_name(&mut p, name);
            p.push(*receiver_flags);
            (2, TYPE_HERE, p)
        }
        ControlMessage::Challenge { receiver_nonce, salt } => {
            let mut p = Vec::with_capacity(32);
            p.extend_from_slice(receiver_nonce);
            p.extend_from_slice(salt);
            (2, TYPE_CHALLENGE, p)
        }
        ControlMessage::HandshakeResponse { sender_nonce, response } => {
            let mut p = Vec::with_capacity(48);
            p.extend_from_slice(sender_nonce);
            p.extend_from_slice(response);
            (2, TYPE_HANDSHAKE_RESPONSE, p)
        }
        ControlMessage::HandshakeOk => (2, TYPE_HANDSHAKE_OK, Vec::new()),
        ControlMessage::HandshakeFail => (2, TYPE_HANDSHAKE_FAIL, Vec::new()),
        ControlMessage::PairRequest { name, sender_pubkey, sender_nonce } => {
            let mut p = Vec::new();
            push_name(&mut p, name);
            p.extend_from_slice(sender_pubkey);
            p.extend_from_slice(sender_nonce);
            (2, TYPE_PAIR_REQUEST, p)
        }
        ControlMessage::PairConfirm { receiver_pubkey, receiver_nonce, key_id } => {
            let mut p = Vec::with_capacity(89);
            p.extend_from_slice(receiver_pubkey);
            p.extend_from_slice(receiver_nonce);
            p.extend_from_slice(key_id);
            (2, TYPE_PAIR_CONFIRM, p)
        }
        ControlMessage::PairDeny { reason } => (2, TYPE_PAIR_DENY, vec![*reason]),
        ControlMessage::PairResume { key_id, sender_nonce, hmac } => {
            let mut p = Vec::with_capacity(40);
            p.extend_from_slice(key_id);
            p.extend_from_slice(sender_nonce);
            p.extend_from_slice(hmac);
            (2, TYPE_PAIR_RESUME, p)
        }
        ControlMessage::PairResumeOk { receiver_nonce, hmac } => {
            let mut p = Vec::with_capacity(32);
            p.extend_from_slice(receiver_nonce);
            p.extend_from_slice(hmac);
            (2, TYPE_PAIR_RESUME_OK, p)
        }
    };
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&encode_header(
        version,
        msg_type,
        u16::try_from(payload.len()).expect("payload must fit in u16 length"),
    ));
    out.extend_from_slice(&payload);
    out
}

fn read_name(p: &[u8]) -> Option<(String, usize)> {
    let name_len = *p.first()? as usize;
    let bytes = p.get(1..1 + name_len)?;
    Some((String::from_utf8(bytes.to_vec()).ok()?, 1 + name_len))
}

fn arr<const N: usize>(p: &[u8]) -> Option<[u8; N]> {
    p.try_into().ok()
}

pub fn decode(d: &[u8]) -> Option<ControlMessage> {
    if d.len() < HEADER_LEN || &d[0..6] != MAGIC {
        return None;
    }
    let payload_len = u16::from_be_bytes(d[8..10].try_into().ok()?) as usize;
    let payload = d.get(HEADER_LEN..HEADER_LEN + payload_len)?;
    if payload.len() != payload_len || d.len() != HEADER_LEN + payload_len {
        return None;
    }
    match d[7] {
        TYPE_HELLO => {
            if payload.is_empty() {
                Some(ControlMessage::HelloV1)
            } else {
                let (name, used) = read_name(payload)?;
                let proto_flags = *payload.get(used)?;
                if payload.len() != used + 1 {
                    return None;
                }
                Some(ControlMessage::HelloV2 { name, proto_flags })
            }
        }
        TYPE_HERE => {
            if payload.len() == 6 {
                Some(ControlMessage::HereV1 {
                    port: u16::from_be_bytes(arr(payload.get(0..2)?)?),
                    ipv4: u32::from_be_bytes(arr(payload.get(2..6)?)?),
                })
            } else {
                let port = u16::from_be_bytes(arr(payload.get(0..2)?)?);
                let ipv4 = u32::from_be_bytes(arr(payload.get(2..6)?)?);
                let (name, used) = read_name(payload.get(6..)?)?;
                let receiver_flags = *payload.get(6 + used)?;
                if payload.len() != 6 + used + 1 {
                    return None;
                }
                Some(ControlMessage::HereV2 { port, ipv4, name, receiver_flags })
            }
        }
        TYPE_CHALLENGE => {
            if payload.len() != 32 {
                return None;
            }
            Some(ControlMessage::Challenge {
                receiver_nonce: arr(payload.get(0..16)?)?,
                salt: arr(payload.get(16..32)?)?,
            })
        }
        TYPE_HANDSHAKE_RESPONSE => {
            if payload.len() != 48 {
                return None;
            }
            Some(ControlMessage::HandshakeResponse {
                sender_nonce: arr(payload.get(0..16)?)?,
                response: arr(payload.get(16..48)?)?,
            })
        }
        TYPE_HANDSHAKE_OK => {
            if !payload.is_empty() {
                return None;
            }
            Some(ControlMessage::HandshakeOk)
        }
        TYPE_HANDSHAKE_FAIL => {
            if !payload.is_empty() {
                return None;
            }
            Some(ControlMessage::HandshakeFail)
        }
        TYPE_PAIR_REQUEST => {
            let (name, used) = read_name(payload)?;
            if payload.len() != used + 65 + 16 {
                return None;
            }
            Some(ControlMessage::PairRequest {
                name,
                sender_pubkey: arr(payload.get(used..used + 65)?)?,
                sender_nonce: arr(payload.get(used + 65..used + 81)?)?,
            })
        }
        TYPE_PAIR_CONFIRM => {
            if payload.len() != 89 {
                return None;
            }
            Some(ControlMessage::PairConfirm {
                receiver_pubkey: arr(payload.get(0..65)?)?,
                receiver_nonce: arr(payload.get(65..81)?)?,
                key_id: arr(payload.get(81..89)?)?,
            })
        }
        TYPE_PAIR_DENY => {
            if payload.len() != 1 {
                return None;
            }
            Some(ControlMessage::PairDeny { reason: payload[0] })
        }
        TYPE_PAIR_RESUME => {
            if payload.len() != 40 {
                return None;
            }
            Some(ControlMessage::PairResume {
                key_id: arr(payload.get(0..8)?)?,
                sender_nonce: arr(payload.get(8..24)?)?,
                hmac: arr(payload.get(24..40)?)?,
            })
        }
        TYPE_PAIR_RESUME_OK => {
            if payload.len() != 32 {
                return None;
            }
            Some(ControlMessage::PairResumeOk {
                receiver_nonce: arr(payload.get(0..16)?)?,
                hmac: arr(payload.get(16..32)?)?,
            })
        }
        _ => None,
    }
}
