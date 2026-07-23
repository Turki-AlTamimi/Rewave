mod simreceiver;

use rewave_core::pairing::flow::PairingFlow;
use rewave_core::protocol::datagram::{encode_m6, FLAG_STREAM_START};
use simreceiver::SimReceiver;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn tempfile_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rewave-test-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn new_flow() -> PairingFlow {
    PairingFlow::with_store_path("TestPC".into(), &tempfile_dir().join("pairings.json")).unwrap()
}

#[test]
fn pin_handshake_end_to_end_vs_sim() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = new_flow();
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    flow.pin_handshake("1234", ("127.0.0.1".parse().unwrap(), sim.audio_port)).unwrap();
    assert!(flow.session_key().is_some());
    assert_eq!(flow.session_key().copied(), sim.last_session_key());
    let key = *flow.session_key().unwrap();
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    for seq in 0..5u32 {
        let pcm = vec![0xAB; 960];
        sock.send_to(
            &encode_m6(seq, if seq == 0 { FLAG_STREAM_START } else { 0 }, &pcm, &key),
            format!("127.0.0.1:{}", sim.audio_port),
        )
        .unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    let st = sim.stats.lock().unwrap();
    assert_eq!(st.accepted, 5);
    assert_eq!(st.auth_failures, 0);
    assert_eq!(st.replay_drops, 0);
}

#[test]
fn wrong_pin_fails_handshake() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = new_flow();
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    assert!(flow.pin_handshake("9999", ("127.0.0.1".parse().unwrap(), sim.audio_port)).is_err());
}

#[test]
fn pin_success_persists_synthetic_pairing_for_resume() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = new_flow();
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    flow.pin_handshake("1234", ("127.0.0.1".parse().unwrap(), sim.audio_port)).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(flow.store_path()).unwrap()).unwrap();
    let row = raw.as_object().unwrap().values().next().unwrap();
    assert_eq!(row["pairing_key"].as_str().unwrap().len(), 64);
    assert_eq!(row["key_id"].as_str().unwrap().len(), 16);
    assert_eq!(row["name"], "SimTab");
}

#[test]
fn replayed_and_tampered_m6_are_dropped() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let mut flow = new_flow();
    flow.hello_and_challenge(("127.0.0.1".parse().unwrap(), sim.disc_port)).unwrap();
    flow.pin_handshake("1234", ("127.0.0.1".parse().unwrap(), sim.audio_port)).unwrap();
    let key = *flow.session_key().unwrap();
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let pcm = vec![0x11; 960];
    sock.send_to(&encode_m6(0, 0, &pcm, &key), format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    sock.send_to(&encode_m6(0, 0, &pcm, &key), format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    let mut bad = encode_m6(1, 0, &pcm, &key);
    bad[972] ^= 0xff;
    sock.send_to(&bad, format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    sock.send_to(&encode_m6(2, 0, &pcm, &key), format!("127.0.0.1:{}", sim.audio_port)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let st = sim.stats.lock().unwrap();
    assert_eq!(st.accepted, 2);
    assert_eq!(st.replay_drops, 1);
    assert_eq!(st.auth_failures, 1);
}
