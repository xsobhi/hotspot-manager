//! Where the hotspot's traffic shows up, for the log when limits don't work: ten seconds of
//! watching each of WinDivert's layers (and the internet side of the forwarded traffic), without
//! touching anything, and what the engine did with the packets meanwhile.

use super::divert::{Divert, LAYER_FORWARD, LAYER_NETWORK};
use super::{log, shared};
use crate::helper::dhcp::Ip;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn diagnose(gateway: Ip, hotspot: u32) {
    let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
    let any = format!("ip and ((ip.SrcAddr >= {net}.0 and ip.SrcAddr <= {net}.255) or \
                       (ip.DstAddr >= {net}.0 and ip.DstAddr <= {net}.255))");
    let internet = crate::net::internet_index().unwrap_or(0);
    let before = shared().lock().unwrap().counts;
    for (name, layer, filter) in [("forwarded", LAYER_FORWARD, any.clone()),
                                  ("to/from this PC", LAYER_NETWORK, format!("ifIdx == {hotspot} or ({any})")),
                                  ("forwarded, the internet side", LAYER_FORWARD, format!("ip and ifIdx == {internet}")),
                                  ("sent to the internet", LAYER_NETWORK, format!("outbound and ip and ifIdx == {internet}"))] {
        let found = Arc::new(Mutex::new(std::collections::BTreeMap::<String, (u64, u64)>::new()));
        let divert = match Divert::sniff(&filter, layer) {
            Ok(divert) => divert,
            Err(why) => {
                log::line(format!("Diagnosis, {name}: can't watch ({why})"));
                continue;
            }
        };
        let divert = Arc::new(divert);
        let (d, f) = (divert.clone(), found.clone());
        let watcher = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65535];
            while let Some((n, a)) = d.recv(&mut buffer) {
                if n >= 20 {
                    let key = format!("{} {}.{}.{}.{} → {}.{}.{}.{} (if {})", if a.outbound() { "out" } else { "in" },
                                      buffer[12], buffer[13], buffer[14], buffer[15], buffer[16], buffer[17], buffer[18],
                                      buffer[19], a.if_idx);
                    let mut f = f.lock().unwrap();
                    let entry = f.entry(key).or_default();
                    (entry.0, entry.1) = (entry.0 + 1, entry.1 + n as u64);
                }
            }
        });
        std::thread::sleep(Duration::from_secs(10));
        divert.shutdown();
        let _ = watcher.join();
        let found = found.lock().unwrap();
        log::line(format!("Diagnosis, {name}: {} kinds of packets", found.len()));
        let mut busiest: Vec<_> = found.iter().collect();
        busiest.sort_by_key(|(_, (_, bytes))| std::cmp::Reverse(*bytes));
        for (key, (count, bytes)) in busiest.into_iter().take(12) {
            log::line(format!("  {key}: {count} packets, {bytes} bytes"));
        }
    }
    // which TTLs the packets from the internet come with (a network stopping sharing sends 1)
    let ttls = Arc::new(Mutex::new(std::collections::BTreeMap::<u8, u64>::new()));
    if let Ok(divert) = Divert::sniff(&format!("inbound and ip and ifIdx == {internet}"), LAYER_NETWORK) {
        let divert = Arc::new(divert);
        let (d, t) = (divert.clone(), ttls.clone());
        let watcher = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65535];
            while let Some((n, _)) = d.recv(&mut buffer) {
                if n >= 20 {
                    *t.lock().unwrap().entry(buffer[8]).or_default() += 1;
                }
            }
        });
        std::thread::sleep(Duration::from_secs(10));
        divert.shutdown();
        let _ = watcher.join();
        let ttls: Vec<String> = ttls.lock().unwrap().iter().map(|(ttl, n)| format!("{ttl}: {n}")).collect();
        log::line(format!("Diagnosis, the internet's packets by TTL: {}", ttls.join(", ")));
    }
    log::line(format!("Diagnosis, packets with TTL 1 or 2 raised so far: {}", shared().lock().unwrap().ttl_fixed));
    let after = shared().lock().unwrap().counts;
    for (i, name) in ["up", "down"].iter().enumerate() {
        let (a, b) = (after[i], before[i]);
        log::line(format!("Diagnosis, the engine's packets {name}: {} seen, {} sent at once, {} queued, {} released, \
                           {} dropped, {} not matched to a device", a.seen - b.seen, a.sent - b.sent, a.queued - b.queued,
                          a.released - b.released, a.dropped - b.dropped, a.unknown - b.unknown));
    }
}
