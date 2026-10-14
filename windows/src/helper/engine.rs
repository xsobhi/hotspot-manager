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
    /// Count and limit the devices' traffic: they're in `gateway`'s subnet; Windows' sharing is
    /// in `own`'s (the same, unless another hotspot address was asked for: then their packets
    /// pass Windows' sharing as if from its subnet, and come back translated).
    pub fn start(shared: Arc<Mutex<Shared>>, gateway: Ip, own: Ip) -> Result<Engine, String> {
        let subnet = |ip: Ip| {
            let net = format!("{}.{}.{}", ip[0], ip[1], ip[2]);
            format!("(ip.SrcAddr >= {net}.0 and ip.SrcAddr <= {net}.255) or (ip.DstAddr >= {net}.0 and ip.DstAddr <= {net}.255)")
        };
        let filter = if own[..3] == gateway[..3] { format!("ip and ({})", subnet(gateway)) }
                     else { format!("ip and ({} or {})", subnet(gateway), subnet(own)) };
        let divert = Arc::new(Divert::open(&filter, LAYER_FORWARD, 0)?);
        let stop = Arc::new(AtomicBool::new(false));
        let (d, s) = (divert.clone(), shared.clone());
        let capture = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 65535];
            while let Some((n, address)) = d.recv(&mut buffer) {
                if let Some((packet, address)) = handle(&s, &buffer[..n], address, gateway, own) {
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
        log::line(format!("Counting and limiting the hotspot's traffic ({}.{}.{}.x)", gateway[0], gateway[1], gateway[2]));
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
fn handle(shared: &Mutex<Shared>, packet: &[u8], mut address: Address, gateway: Ip, own: Ip) -> Option<Packet> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return Some((packet.to_vec(), address));
    }
    let mut packet = packet.to_vec();
    let ip = |p: &[u8], at: usize| -> Ip { p[at..at + 4].try_into().unwrap() };
    let ours = |ip: Ip| ip[..3] == gateway[..3];
    let windows = |ip: Ip| own[..3] != gateway[..3] && ip[..3] == own[..3];
    // another address than Windows' own: out as from Windows' subnet, back to the device's
    let (src, dst) = (ip(&packet, 12), ip(&packet, 16));
    let translated = if ours(src) && !ours(dst) && !windows(dst) {
        packet[12..15].copy_from_slice(&own[..3]);
        true
    } else if windows(dst) && dst != own && !ours(src) && !windows(src) {
        packet[16..19].copy_from_slice(&gateway[..3]);
        true
    } else {
        false
    };
    if translated {
        Divert::checksums(&mut packet, &mut address);
    }
    let device = |ip: Ip| if windows(ip) { [gateway[0], gateway[1], gateway[2], ip[3]] } else { ip };
    let (src, dst) = (device(ip(&packet, 12)), device(ip(&packet, 16)));
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
    if s.rules.local_only {
        return None; // "Share internet" is off: the devices only reach each other and the PC
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
    let size = packet.len();
    lane.offer((packet, address), size, lanes.totals[down as usize].as_mut(), now)
}
