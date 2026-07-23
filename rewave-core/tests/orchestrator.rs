use rewave_core::connection::orchestrator::*;
use std::time::Duration;

struct FakeBackend(BackendOutcome);
impl ConnectionBackend for FakeBackend {
    fn connect(&mut self, _target: &str) -> BackendOutcome {
        self.0
    }
}

struct FakeBackends {
    aware: FakeBackend,
    lan: FakeBackend,
    direct: FakeBackend,
}

impl BackendSet for FakeBackends {
    fn aware(&mut self) -> &mut dyn ConnectionBackend {
        &mut self.aware
    }
    fn lan(&mut self) -> &mut dyn ConnectionBackend {
        &mut self.lan
    }
    fn direct(&mut self) -> &mut dyn ConnectionBackend {
        &mut self.direct
    }
}

impl FakeBackends {
    fn all_fail() -> Self {
        Self {
            aware: FakeBackend(BackendOutcome::Unsupported),
            lan: FakeBackend(BackendOutcome::Timeout),
            direct: FakeBackend(BackendOutcome::Unavailable),
        }
    }
}

#[test]
fn falls_through_aware_to_lan() {
    let mut o = Orchestrator::new(FakeBackends {
        aware: FakeBackend(BackendOutcome::Unsupported),
        lan: FakeBackend(BackendOutcome::Connected),
        direct: FakeBackend(BackendOutcome::Unavailable),
    });
    let s = o.run_connect("SimTab");
    assert!(matches!(s, ConnectResult::Connected { mode: ConnectionMode::Lan, .. }));
    assert_eq!(o.mode(), ConnectionMode::Lan);
}

#[test]
fn aware_wins_when_available() {
    let mut o = Orchestrator::new(FakeBackends {
        aware: FakeBackend(BackendOutcome::Connected),
        lan: FakeBackend(BackendOutcome::Connected),
        direct: FakeBackend(BackendOutcome::Connected),
    });
    let s = o.run_connect("SimTab");
    assert!(matches!(s, ConnectResult::Connected { mode: ConnectionMode::Aware, .. }));
}

#[test]
fn falls_through_to_direct_when_aware_and_lan_fail() {
    let mut o = Orchestrator::new(FakeBackends {
        aware: FakeBackend(BackendOutcome::Unsupported),
        lan: FakeBackend(BackendOutcome::Timeout),
        direct: FakeBackend(BackendOutcome::Connected),
    });
    let s = o.run_connect("SimTab");
    assert!(matches!(s, ConnectResult::Connected { mode: ConnectionMode::Direct, .. }));
}

#[test]
fn all_fail_gives_disconnected_with_backoff_schedule() {
    let mut o = Orchestrator::new(FakeBackends::all_fail());
    assert_eq!(o.run_connect("SimTab"), ConnectResult::Disconnected);
    assert_eq!(o.mode(), ConnectionMode::None);
    assert_eq!(o.backoff_schedule().take(5).collect::<Vec<_>>(), vec![1, 2, 4, 8, 16]);
}

#[test]
fn backoff_caps_at_30_seconds() {
    let o = Orchestrator::new(FakeBackends::all_fail());
    assert_eq!(
        o.backoff_schedule().take(8).collect::<Vec<_>>(),
        vec![1, 2, 4, 8, 16, 30, 30, 30]
    );
}

#[test]
fn link_lost_only_after_five_second_stall() {
    assert!(!Orchestrator::<FakeBackends>::link_lost(Duration::from_secs(5)));
    assert!(Orchestrator::<FakeBackends>::link_lost(Duration::from_millis(5001)));
}

#[test]
fn stub_backends_are_inert_until_stage_8_and_9() {
    assert_eq!(AwareBackend.connect("x"), BackendOutcome::Unsupported);
    assert_eq!(DirectBackend.connect("x"), BackendOutcome::Unavailable);
}
