mod simreceiver;

use rewave_core::discovery::broadcast::discover_broadcast;
use rewave_core::discovery::mdns::{browse_receivers, MdnsAnnouncer, SENDER_SERVICE_TYPE};
use rewave_core::protocol::control::{decode, encode, ControlMessage};
use simreceiver::SimReceiver;
use std::collections::HashMap;
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

#[test]
fn broadcast_discovery_finds_sim_on_loopback() {
    let sim = SimReceiver::start("SimTab".into(), "1234".into());
    let found =
        discover_broadcast(Duration::from_secs(3), vec!["127.0.0.1".parse().unwrap()], sim.disc_port)
            .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "SimTab");
    assert_eq!(found[0].audio_port, sim.audio_port);
    assert_eq!(found[0].via, rewave_core::discovery::broadcast::DiscoveryVia::Broadcast);
}

#[test]
fn mdns_browse_finds_announced_receiver_and_parses_txt() {
    let daemon = mdns_sd::ServiceDaemon::new().unwrap();
    let mut props = HashMap::new();
    props.insert("name".to_string(), "FakeTab".to_string());
    props.insert("paired".to_string(), "c134ef58cb87e775,deadbeefdeadbeef".to_string());
    props.insert("flags".to_string(), "0".to_string());
    let info = mdns_sd::ServiceInfo::new(
        "_rewave._udp.local.",
        "FakeTab",
        "faketab.local.",
        (),
        50001,
        props,
    )
    .unwrap()
    .enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info).unwrap();

    let found = browse_receivers(Duration::from_secs(3)).unwrap();
    daemon.unregister(&fullname).unwrap();
    daemon.shutdown().unwrap();

    let dev = found.iter().find(|d| d.name == "FakeTab").expect("FakeTab not found");
    assert_eq!(dev.key_ids, vec!["c134ef58cb87e775", "deadbeefdeadbeef"]);
    assert_eq!(dev.disc_port, 50001);
    assert_eq!(dev.via, rewave_core::discovery::broadcast::DiscoveryVia::Mdns);
}

#[test]
fn mdns_sender_announcer_is_browsable() {
    let ann = MdnsAnnouncer::start("TestPC", 54321, vec!["c134ef58cb87e775".into()]).unwrap();
    let daemon = mdns_sd::ServiceDaemon::new().unwrap();
    let rx = daemon.browse(SENDER_SERVICE_TYPE).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut resolved = None;
    while let Ok(ev) = rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = ev {
            if info.get_fullname().starts_with("TestPC.") {
                resolved = Some(info);
                break;
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
    }
    let info = resolved.expect("sender announcement not browsed");
    assert_eq!(info.get_property_val_str("name"), Some("TestPC"));
    assert_eq!(info.get_property_val_str("paired"), Some("c134ef58cb87e775"));
    assert_eq!(info.get_port(), 54321);
    drop(ann);
    daemon.shutdown().unwrap();
}
