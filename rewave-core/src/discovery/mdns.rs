use super::broadcast::{DiscoveredDevice, DiscoveryVia};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const RECEIVER_SERVICE_TYPE: &str = "_rewave._udp.local.";
pub const SENDER_SERVICE_TYPE: &str = "_rewave-sender._udp.local.";

/// Announces this sender as `_rewave-sender._udp.` (Rewave.md §8.2).
pub struct MdnsAnnouncer {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsAnnouncer {
    pub fn start(name: &str, port: u16, key_ids: Vec<String>) -> mdns_sd::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let mut props = HashMap::new();
        props.insert("name".to_string(), name.to_string());
        props.insert(
            "paired".to_string(),
            if key_ids.is_empty() { "none".into() } else { key_ids.join(",") },
        );
        props.insert("flags".to_string(), "0".to_string());
        let info = ServiceInfo::new(
            SENDER_SERVICE_TYPE,
            name,
            &format!("{}.local.", name.to_lowercase().replace(' ', "-")),
            (),
            port,
            props,
        )?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info)?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for MdnsAnnouncer {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Browse `_rewave._udp.` for `timeout`, parsing TXT keys name/paired/flags
/// (Rewave.md §8.2). The announced port is reported as `disc_port`;
/// `audio_port` is 0 (not carried by mDNS — it comes from HERE, §5.2).
pub fn browse_receivers(timeout: Duration) -> mdns_sd::Result<Vec<DiscoveredDevice>> {
    let daemon = ServiceDaemon::new()?;
    let rx = daemon.browse(RECEIVER_SERVICE_TYPE)?;
    let deadline = Instant::now() + timeout;
    let mut found: HashMap<String, DiscoveredDevice> = HashMap::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let name = info.get_property_val_str("name").unwrap_or("").to_string();
                let paired = info.get_property_val_str("paired").unwrap_or("none");
                let key_ids = if paired == "none" || paired.is_empty() {
                    Vec::new()
                } else {
                    paired.split(',').map(str::to_string).collect()
                };
                // IPv4-only protocol (§8.3); prefer a v4 address, tolerate v6-only.
                let host = info
                    .get_addresses_v4()
                    .into_iter()
                    .next()
                    .map(|a| std::net::IpAddr::V4(*a))
                    .or_else(|| info.get_addresses().iter().next().copied());
                let Some(host) = host else { continue };
                let dev = DiscoveredDevice {
                    name,
                    host,
                    audio_port: 0,
                    disc_port: info.get_port(),
                    key_ids,
                    via: DiscoveryVia::Mdns,
                };
                // Later resolutions of the same instance may carry more addresses;
                // replace a v6-only entry with a v4 one, otherwise keep the first.
                match found.get(info.get_fullname()) {
                    Some(existing) if existing.host.is_ipv4() || dev.host.is_ipv6() => {}
                    _ => {
                        found.insert(info.get_fullname().to_string(), dev);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(found.into_values().collect())
}
