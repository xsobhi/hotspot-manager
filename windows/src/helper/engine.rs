//! The traffic engine: the packets between the hotspot's devices and the internet come through
//! here (WinDivert, where Windows forwards them): counted per device, stopped for blocked and
//! disconnected devices (and between devices, when they're kept apart), and held to the speed
//! limits. If the helper stops, WinDivert lets everything through as before.

use super::divert::{Address, Divert, LAYER_FORWARD};
use super::shape::{self, Bucket, Lane};
use super::{log, Shared};
use crate::helper::dhcp::Ip;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Packet = (Vec<u8>, Address);

/// The speed-limit queues: one per device and direction, and the whole hotspot's caps.
#[derive(Default)]
pub struct Lanes {
    lanes: Vec<((Ip, bool), Lane<Packet>)>,
    totals: [Option<Bucket>; 2], // [up, down]
    turn: usize,                 // whose queue goes first (fair when the total cap is reached)
}

impl Lanes {
    /// Start again with the whole hotspot's caps (kbit/s), e.g. when the rules change.
    pub fn reset(&mut self, up: u32, down: u32) {
        let now = Instant::now();
        self.totals = [shape::buckets(up, 0, 0, now).0, shape::buckets(down, 0, 0, now).0];
        self.lanes.retain(|(_, lane)| lane.waiting());
    }
}

pub struct Engine {
    divert: Arc<Divert>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    pub fn start(shared: Arc<Mutex<Shared>>, gateway: Ip) -> Result<Engine, String> {
        let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
        let filter = format!("ip and ((ip.SrcAddr >= {net}.0 and ip.SrcAddr <= {net}.255) or \
                              (ip.DstAddr >= {net}.0 and ip.DstAddr <= {net}.255))");
        let divert = Arc::new(Divert::open(&filter, LAYER_FORWARD, 0)?);
        let stop = Arc::new(AtomicBool::new(false));
        let (d, s) = (divert.clone(), shared.clone());
        let capture = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65535];
            while let Some((n, address)) = d.recv(&mut buffer) {
                if let Some((packet, address)) = handle(&s, &buffer[..n], address, gateway) {
                    d.send(&packet, &address);
                }
            }
        });
        let (d, s, st) = (divert.clone(), shared, stop.clone());
        let release = std::thread::spawn(move || {
            let mut out = vec![];
            while !st.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(2));
                {
                    let mut shared = s.lock().unwrap();
                    let lanes = &mut shared.lanes;
                    let now = Instant::now();
                    let count = lanes.lanes.len();
                    lanes.turn = lanes.turn.wrapping_add(1);
                    for k in 0..count {
                        let i = (k + lanes.turn) % count;
                        let ((_, down), lane) = &mut lanes.lanes[i];
                        if lane.waiting() {
                            lane.release(lanes.totals[*down as usize].as_mut(), now, &mut out);
                        }
                    }
                }
                for (packet, address) in out.drain(..) {
                    d.send(&packet, &address);
                }
            }
        });
        log::line(format!("Counting and limiting the hotspot's traffic ({net}.x)"));
        Ok(Engine { divert, stop, threads: vec![capture, release] })
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.divert.shutdown();
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

/// A packet between a device and the rest: Some(it) to send on now; None: it waits or is dropped.
fn handle(shared: &Mutex<Shared>, packet: &[u8], address: Address, gateway: Ip) -> Option<Packet> {
    let pass = || Some((packet.to_vec(), address));
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return pass();
    }
    let (src, dst): (Ip, Ip) = (packet[12..16].try_into().unwrap(), packet[16..20].try_into().unwrap());
    let ours = |ip: Ip| ip[..3] == gateway[..3];
    let mut s = shared.lock().unwrap();
    if ours(src) && ours(dst) {
        // from one device to another
        let apart = s.rules.isolate && src != gateway && dst != gateway;
        return if apart { None } else { pass() };
    }
    let (device, down) = match (ours(dst), ours(src)) {
        (true, _) => (dst, true),
        (_, true) => (src, false),
        _ => return pass(),
    };
    if device == gateway || device[3] == 255 {
        return pass();
    }
    let used = s.usage.entry(device).or_default();
    if down { used.0 += packet.len() as u64 } else { used.1 += packet.len() as u64 }
    let mac = s.macs.get(&device).cloned();
    if let Some(mac) = &mac
        && !s.let_through(mac) {
        return None;
    }
    let limit = mac.as_deref().map_or(s.rules.everyone, |m| s.rules.limit_for(m));
    let wanted = shape::direction(&limit, down);
    let total = if down { s.rules.total.down } else { s.rules.total.up };
    if wanted.0 == 0 && total == 0 {
        return pass();
    }
    let now = Instant::now();
    let lanes = &mut s.lanes;
    let at = match lanes.lanes.iter().position(|(key, _)| *key == (device, down)) {
        Some(i) => i,
        None => {
            lanes.lanes.push(((device, down), Lane::new(wanted, now)));
            lanes.lanes.len() - 1
        }
    };
    let lane = &mut lanes.lanes[at].1;
    if lane.limit != wanted {
        *lane = Lane::new(wanted, now);
    }
    lane.offer((packet.to_vec(), address), packet.len(), lanes.totals[down as usize].as_mut(), now)
}

/// Where the hotspot's traffic shows up (for the log, when something doesn't work): ten seconds
/// of watching both layers without touching anything.
pub fn diagnose(gateway: Ip, hotspot: u32) {
    let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
    let any = format!("ip and ((ip.SrcAddr >= {net}.0 and ip.SrcAddr <= {net}.255) or \
                       (ip.DstAddr >= {net}.0 and ip.DstAddr <= {net}.255))");
    for (name, layer, filter) in [("forwarded", LAYER_FORWARD, any.clone()),
                                  ("to/from this PC", super::divert::LAYER_NETWORK, format!("ifIdx == {hotspot} or ({any})"))] {
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
