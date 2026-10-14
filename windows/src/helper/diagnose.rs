//! Where the hotspot's traffic shows up, for the log when limits don't work: ten seconds of
//! watching both of WinDivert's layers, without touching anything.

use super::divert::{Divert, LAYER_FORWARD, LAYER_NETWORK};
use super::log;
use crate::helper::dhcp::Ip;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn diagnose(gateway: Ip, hotspot: u32) {
    let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
    let any = format!("ip and ((ip.SrcAddr >= {net}.0 and ip.SrcAddr <= {net}.255) or \
                       (ip.DstAddr >= {net}.0 and ip.DstAddr <= {net}.255))");
    for (name, layer, filter) in [("forwarded", LAYER_FORWARD, any.clone()),
                                  ("to/from this PC", LAYER_NETWORK, format!("ifIdx == {hotspot} or ({any})"))] {
        let found = Arc::new(Mutex::new(std::collections::BTreeMap::<String, (u64, u64)>::new()));
        let Ok(divert) = Divert::sniff(&filter, layer) else { continue };
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
}
