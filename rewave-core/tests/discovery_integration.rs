mod simreceiver;

use rewave_core::protocol::control::{decode, encode, ControlMessage};
use simreceiver::SimReceiver;
use std::net::UdpSocket;
use std::time::Duration;

#[test]
fn sim_receiver_answers_hello() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let hello = encode(&ControlMessage::HelloV2 { name: "PC".into(), proto_flags: 3 });
    sock.send_to(&hello, format!("127.0.0.1:{}", sim.disc_port)).unwrap();
    let mut buf = [0u8; 1500];
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::HereV2 { .. })));
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::Challenge { .. })));
}

#[test]
fn sim_receiver_answers_hello_v1_with_v1() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    sock.send_to(&encode(&ControlMessage::HelloV1), format!("127.0.0.1:{}", sim.disc_port))
        .unwrap();
    let mut buf = [0u8; 1500];
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(buf[6], 1);
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::HereV1 { .. })));
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(buf[6], 1);
    assert!(matches!(decode(&buf[..n]), Some(ControlMessage::Challenge { .. })));
}
