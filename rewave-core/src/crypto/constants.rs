pub const PAIRING_INFO: &[u8] = b"rewave-pairing";
pub const KEYID_INFO: &[u8] = b"rewave-keyid";
pub const SESSION_INFO: &[u8] = b"rewave-session";
pub const RESUME_SENDER_CTX: &[u8] = b"resume-sender";
pub const RESUME_RECV_CTX: &[u8] = b"resume-recv";

pub const SHA256: usize = 32;
pub const NONCE_BYTES: usize = 16;
pub const PAIRING_KEY: usize = 32;
pub const SESSION_KEY: usize = 32;
pub const KEY_ID: usize = 8;
pub const EC_PUBKEY_BYTES: usize = 65;
pub const RESUME_HMAC_BYTES: usize = 16;
pub const AUTH_TAG_BYTES: usize = 8;
pub const FINGERPRINT: usize = 8;
