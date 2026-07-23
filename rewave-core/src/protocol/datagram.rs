use crate::crypto::session::m6_tag;

pub const PCM_BYTES: usize = 960;
pub const M1_DATAGRAM: usize = 965;
pub const M6_DATAGRAM: usize = 973;
pub const FLAG_STREAM_START: u8 = 0x01;
pub const FLAG_STREAM_END: u8 = 0x02;

pub fn encode_m1(seq: u32, flags: u8, pcm: &[u8]) -> Vec<u8> {
    debug_assert_eq!(pcm.len(), PCM_BYTES);
    let mut out = Vec::with_capacity(M1_DATAGRAM);
    out.extend_from_slice(&seq.to_be_bytes());
    out.push(flags);
    out.extend_from_slice(pcm);
    out
}

pub fn encode_m6(seq: u32, flags: u8, pcm: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let mut out = encode_m1(seq, flags, pcm);
    out.extend_from_slice(&m6_tag(key, seq, pcm));
    out
}

pub fn decode_m1(d: &[u8]) -> Option<(u32, u8, Vec<u8>)> {
    if d.len() != M1_DATAGRAM { return None; }
    Some((u32::from_be_bytes(d[0..4].try_into().ok()?), d[4], d[5..].to_vec()))
}

/// HMAC-verified decode. Returns None on wrong length or tag mismatch.
pub fn decode_m6(d: &[u8], key: &[u8; 32]) -> Option<(u32, u8, Vec<u8>)> {
    if d.len() != M6_DATAGRAM { return None; }
    let seq = u32::from_be_bytes(d[0..4].try_into().ok()?);
    let expected = m6_tag(key, seq, &d[5..965]);
    if d[965..973] != expected { return None; }
    Some((seq, d[4], d[5..965].to_vec()))
}
