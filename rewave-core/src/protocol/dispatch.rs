#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class { LegacyAccept, LegacyRejected, AuthCandidate, Control, Other }

pub struct Dispatcher {
    session_key: Option<[u8; 32]>,
    last_authenticated_seq: i64,
}

impl Default for Dispatcher {
    fn default() -> Self { Self::new() }
}

impl Dispatcher {
    pub fn new() -> Self { Self { session_key: None, last_authenticated_seq: -1 } }
    pub fn adopt_session(&mut self, key: [u8; 32]) { self.session_key = Some(key); self.last_authenticated_seq = -1; }
    pub fn reset_session(&mut self) { self.session_key = None; self.last_authenticated_seq = -1; }

    pub fn classify(&self, d: &[u8]) -> Class {
        match (d.len(), self.session_key.is_some()) {
            (965, false) => Class::LegacyAccept,
            (965, true) => Class::LegacyRejected,
            (973, true) => Class::AuthCandidate,
            (973, false) => Class::Other,               // dropped as bytesDropped
            (n, _) if n >= 10 && &d[0..6] == b"REWAVE" => Class::Control,
            _ => Class::Other,
        }
    }

    /// Strict seq > last (Rewave.md §6.6). u32 wrap intentionally unhandled.
    pub fn check_and_advance_seq(&mut self, seq: u32) -> bool {
        if (seq as i64) > self.last_authenticated_seq {
            self.last_authenticated_seq = seq as i64;
            true
        } else { false }
    }
}
