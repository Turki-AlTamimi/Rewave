use crate::protocol::control::{decode, encode, ControlMessage, PROTO_FLAG_SUPPORTS_CONFIRM};
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub name: String,
    pub host: IpAddr,
    pub audio_port: u16,
    pub disc_port: u16,
    pub key_ids: Vec<String>,
    pub via: DiscoveryVia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryVia {
    Broadcast,
    Mdns,
    Manual,
}

pub const DEFAULT_SENDER_NAME: &str = "rewave-sender";

/// v2 HELLO once per second to each broadcast address until `deadline`,
/// collecting HERE replies (Rewave.md §8.1). `bcast_addrs` is injectable so
/// tests can pass 127.0.0.1; the production caller computes per-interface
/// /24 broadcasts plus the 255.255.255.255 fallback.
pub fn discover_broadcast(
    deadline: Duration,
    bcast_addrs: Vec<IpAddr>,
    disc_port: u16,
) -> io::Result<Vec<DiscoveredDevice>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    sock.set_broadcast(true)?;
    let hello = encode(&ControlMessage::HelloV2 {
        name: DEFAULT_SENDER_NAME.into(),
        proto_flags: PROTO_FLAG_SUPPORTS_CONFIRM,
    });
    let end = Instant::now() + deadline;
    let mut next_send = Instant::now() - Duration::from_secs(1);
    let mut found: Vec<DiscoveredDevice> = Vec::new();
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        if now >= next_send {
            for addr in &bcast_addrs {
                let _ = sock.send_to(&hello, SocketAddr::new(*addr, disc_port));
            }
            next_send = now + Duration::from_secs(1);
        }
        sock.set_read_timeout(Some(Duration::from_millis(50).min(end - now)))?;
        let mut buf = [0u8; 2048];
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => {
                let (name, port) = match decode(&buf[..n]) {
                    Some(ControlMessage::HereV2 { port, name, .. }) => (name, port),
                    Some(ControlMessage::HereV1 { port, .. }) => (String::new(), port),
                    _ => continue,
                };
                if !found.iter().any(|d| d.host == src.ip() && d.audio_port == port) {
                    found.push(DiscoveredDevice {
                        name,
                        host: src.ip(),
                        audio_port: port,
                        disc_port,
                        key_ids: Vec::new(),
                        via: DiscoveryVia::Broadcast,
                    });
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
    }
    Ok(found)
}
