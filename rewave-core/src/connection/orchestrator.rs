use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Aware,
    Lan,
    Direct,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectResult {
    Connected { mode: ConnectionMode, device: String },
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendOutcome {
    Connected,
    Unsupported,
    Unavailable,
    Timeout,
}

pub trait ConnectionBackend {
    fn connect(&mut self, target: &str) -> BackendOutcome;
}

/// The three auto-connect backends, in priority order (design doc §6).
/// Manual Wi-Fi Direct (priority 4) is user-driven and not part of run_connect.
pub trait BackendSet {
    fn aware(&mut self) -> &mut dyn ConnectionBackend;
    fn lan(&mut self) -> &mut dyn ConnectionBackend;
    fn direct(&mut self) -> &mut dyn ConnectionBackend;
}

pub const BACKOFF_INITIAL_SECS: u64 = 1;
pub const BACKOFF_MAX_SECS: u64 = 30;
pub const LINK_LOST_THRESHOLD: Duration = Duration::from_secs(5);

pub struct Orchestrator<B: BackendSet> {
    backends: B,
    mode: ConnectionMode,
}

impl<B: BackendSet> Orchestrator<B> {
    pub fn new(backends: B) -> Self {
        Self { backends, mode: ConnectionMode::None }
    }

    pub fn mode(&self) -> ConnectionMode {
        self.mode
    }

    /// Priority chain Aware → Same-LAN → Direct (design doc §6).
    pub fn run_connect(&mut self, target: &str) -> ConnectResult {
        for (mode, pick) in [
            (ConnectionMode::Aware, BackendSet::aware as fn(&mut B) -> &mut dyn ConnectionBackend),
            (ConnectionMode::Lan, BackendSet::lan),
            (ConnectionMode::Direct, BackendSet::direct),
        ] {
            if pick(&mut self.backends).connect(target) == BackendOutcome::Connected {
                self.mode = mode;
                return ConnectResult::Connected { mode, device: target.to_string() };
            }
        }
        self.mode = ConnectionMode::None;
        ConnectResult::Disconnected
    }

    /// 1, 2, 4, 8, 16, then capped at 30 s (design doc §6 DISCONNECTED state).
    pub fn backoff_schedule(&self) -> impl Iterator<Item = u64> {
        let mut next = BACKOFF_INITIAL_SECS;
        std::iter::from_fn(move || {
            let cur = next;
            next = (next * 2).min(BACKOFF_MAX_SECS);
            Some(cur)
        })
    }

    /// Link-lost watchdog rule (Rewave.md §13.3): stalled strictly > 5 s.
    pub fn link_lost(stall: Duration) -> bool {
        stall > LINK_LOST_THRESHOLD
    }
}

/// Wi-Fi Aware backend — stub until Stage 8.
pub struct AwareBackend;
impl ConnectionBackend for AwareBackend {
    fn connect(&mut self, _target: &str) -> BackendOutcome {
        BackendOutcome::Unsupported
    }
}

/// Wi-Fi Direct auto-join backend — stub until Stage 9 (`wifi/direct.rs`).
pub struct DirectBackend;
impl ConnectionBackend for DirectBackend {
    fn connect(&mut self, _target: &str) -> BackendOutcome {
        BackendOutcome::Unavailable
    }
}

/// Same-LAN backend from a discovery closure (e.g. broadcast + mDNS).
pub struct LanBackend<F: FnMut(&str) -> BackendOutcome>(pub F);
impl<F: FnMut(&str) -> BackendOutcome> ConnectionBackend for LanBackend<F> {
    fn connect(&mut self, target: &str) -> BackendOutcome {
        (self.0)(target)
    }
}
