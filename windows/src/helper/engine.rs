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

/// Ahead of Windows' sharing: it translates the devices' uploads where WinDivert sees them and
/// sends its own copy on, so the engine has to have them first (downloads come translated already).
const PRIORITY: i16 = 30000; // WINDIVERT_PRIORITY_HIGHEST

/// What the engine did with the packets in one direction, for the diagnosis.
#[derive(Default, Clone, Copy, Debug)]
pub struct Counts {
    pub seen: u64,
    pub sent: u64,     // on at once
    pub queued: u64,   // waiting for the speed limit
    pub dropped: u64,  // the queue was full, or the device may not
    pub released: u64, // let out of the queue
}

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
    uploads: Arc<Divert>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// Count and limit the devices' traffic (they're in `gateway`'s subnet).
    pub fn start(shared: Arc<Mutex<Shared>>, gateway: Ip) -> Result<Engine, String> {
        let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
        let inside = |field: &str| format!("ip.{field} >= {net}.0 and ip.{field} <= {net}.255");
        let outside = |field: &str| format!("(ip.{field} < {net}.0 or ip.{field} > {net}.255)");
        // to the devices (and between them): held for the speed limits, blocks and keeping apart
        let filter = format!("ip and {}", inside("DstAddr"));
        // from the devices out: Windows' sharing sends them on itself, whatever WinDivert does with
        // them (held back, they only went out twice), so they're counted, not held
        let uploads = format!("ip and {} and {}", inside("SrcAddr"), outside("DstAddr"));
        let divert = Arc::new(Divert::open(&filter, LAYER_FORWARD, PRIORITY)?);
        let uploads = Arc::new(Divert::sniff(&uploads, LAYER_FORWARD)?);
        let (u, s) = (uploads.clone(), shared.clone());
        let counting = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65535];
            while let Some((n, _)) = u.recv(&mut buffer) {
                if n >= 20 {
                    let device: Ip = buffer[12..16].try_into().unwrap();
                    let mut s = s.lock().unwrap();
                    s.counts[0].seen += 1;
                    s.usage.entry(device).or_default().1 += n as u64;
                }
            }
        });
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
                    let mut guard = s.lock().unwrap();
                    let shared = &mut *guard; // (its fields borrowed apart)
                    let lanes = &mut shared.lanes;
                    let now = Instant::now();
                    let count = lanes.lanes.len();
                    lanes.turn = lanes.turn.wrapping_add(1);
                    for k in 0..count {
                        let i = (k + lanes.turn) % count;
                        let ((_, down), lane) = &mut lanes.lanes[i];
                        if lane.waiting() {
                            let before = out.len();
                            lane.release(lanes.totals[*down as usize].as_mut(), now, &mut out);
                            shared.counts[*down as usize].released += (out.len() - before) as u64;
                        }
                    }
                }
                for (packet, address) in out.drain(..) {
                    d.send(&packet, &address);
                }
            }
        });
        log::line(format!("Counting and limiting the hotspot's traffic ({net}.x)"));
        Ok(Engine { divert, uploads, stop, threads: vec![capture, release, counting] })
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.divert.shutdown();
        self.uploads.shutdown();
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

/// A packet between a device and the rest: Some(it) to send on now; None: it waits or is dropped.
fn handle(shared: &Mutex<Shared>, packet: &[u8], address: Address, gateway: Ip) -> Option<Packet> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return Some((packet.to_vec(), address));
    }
    let packet = packet.to_vec();
    let ip = |at: usize| -> Ip { packet[at..at + 4].try_into().unwrap() };
    let (src, dst) = (ip(12), ip(16));
    let ours = |ip: Ip| ip[..3] == gateway[..3];
    let mut s = shared.lock().unwrap();
    if ours(src) && ours(dst) {
        // from one device to another
        let apart = s.rules.isolate && src != gateway && dst != gateway;
        return if apart { None } else { Some((packet, address)) };
    }
    let (device, down) = match (ours(dst), ours(src)) {
        (true, _) => (dst, true),
        (_, true) => (src, false),
        _ => return Some((packet, address)),
    };
    if device == gateway || device[3] == 255 {
        return Some((packet, address));
    }
    let s = &mut *s; // (its fields borrowed apart)
    let counts = &mut s.counts[down as usize];
    counts.seen += 1;
    if s.rules.local_only {
        counts.dropped += 1;
        return None; // "Share internet" is off: the devices only reach each other and the PC
    }
    let used = s.usage.entry(device).or_default();
    if down { used.0 += packet.len() as u64 } else { used.1 += packet.len() as u64 }
    let mac = s.macs.get(&device).cloned();
    if let Some(mac) = &mac
        && !s.let_through(mac) {
        s.counts[down as usize].dropped += 1;
        return None;
    }
    let counts = &mut s.counts[down as usize];
    let limit = mac.as_deref().map_or(s.rules.everyone, |m| s.rules.limit_for(m));
    let wanted = shape::direction(&limit, down);
    let total = if down { s.rules.total.down } else { s.rules.total.up };
    if wanted.0 == 0 && total == 0 {
        counts.sent += 1;
        return Some((packet, address));
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
    let (size, waiting) = (packet.len(), lane.waiting_count());
    let offered = lane.offer((packet, address), size, lanes.totals[down as usize].as_mut(), now);
    match (&offered, lane.waiting_count() > waiting) {
        (Some(_), _) => counts.sent += 1,
        (None, true) => counts.queued += 1,
        (None, false) => counts.dropped += 1,
    }
    offered
}
