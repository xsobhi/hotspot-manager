//! The traffic engine: the packets between the hotspot's devices and the internet come through
//! here (WinDivert, where Windows forwards them): counted per device, stopped for blocked and
//! disconnected devices (and between devices, when they're kept apart), and held to the speed
//! limits. If the helper stops, WinDivert lets everything through as before.
//!
//! Uploads, with Windows' sharing: each one passes WinDivert twice, as the device sent it and
//! as Windows' sharing translated it (from this PC's own address), and only the translated one
//! counts: Windows sends it whatever happens to the first (held back, that one only went out
//! twice). So the first is counted and noted (Translations), and the second, matched to its
//! device by that note, is the one held to the device's limit.

use super::divert::{Address, Divert, LAYER_FORWARD};
use super::shape::{self, Bucket, Lane};
use super::{log, Shared};
use crate::helper::dhcp::Ip;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Packet = (Vec<u8>, Address);

/// Ahead of anything else watching forwarded packets.
const PRIORITY: i16 = 30000; // WINDIVERT_PRIORITY_HIGHEST

/// What the engine did with the packets in one direction, for the diagnosis.
#[derive(Default, Clone, Copy, Debug)]
pub struct Counts {
    pub seen: u64,
    pub sent: u64,     // on at once
    pub queued: u64,   // waiting for the speed limit
    pub dropped: u64,  // the queue was full, or the device may not
    pub released: u64, // let out of the queue
    pub unknown: u64,  // uploads Windows translated that weren't matched to a device (let through)
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

/// Whose uploads Windows' sharing translated: noted from each upload as the device sent it, by
/// what translating keeps (where to, the IP id, the length), and by connection once it's matched
/// (where to, its port, the port Windows gave it).
#[derive(Default)]
pub struct Translations {
    packets: HashMap<(Ip, u16, u16), Ip>,
    flows: HashMap<(u8, Ip, u16, u16), Ip>,
}

impl Translations {
    /// An upload as `device` sent it.
    fn saw(&mut self, packet: &[u8], device: Ip) {
        if self.packets.len() > 8192 {
            self.packets.clear(); // (ones never seen translated)
        }
        self.packets.insert(packet_key(packet), device);
        if let Some(flow) = flow_key(packet) {
            if self.flows.len() > 65536 {
                self.flows.clear();
            }
            self.flows.insert(flow, device); // (in case Windows keeps the port)
        }
    }

    /// Whose upload this is, as Windows translated it.
    fn whose(&mut self, packet: &[u8]) -> Option<Ip> {
        let flow = flow_key(packet);
        if let Some(device) = self.packets.remove(&packet_key(packet)) {
            if let Some(flow) = flow {
                self.flows.insert(flow, device);
            }
            return Some(device);
        }
        flow.and_then(|f| self.flows.get(&f).copied())
    }
}

fn packet_key(p: &[u8]) -> (Ip, u16, u16) {
    (p[16..20].try_into().unwrap(), u16::from_be_bytes([p[4], p[5]]), u16::from_be_bytes([p[2], p[3]]))
}

/// (protocol, where to, its port, the sender's port) of a TCP or UDP packet.
fn flow_key(p: &[u8]) -> Option<(u8, Ip, u16, u16)> {
    let header = ((p[0] & 0xf) as usize) * 4;
    let protocol = p[9];
    if (protocol != 6 && protocol != 17) || p.len() < header + 4 {
        return None;
    }
    let port = |at: usize| u16::from_be_bytes([p[header + at], p[header + at + 1]]);
    Some((protocol, p[16..20].try_into().unwrap(), port(2), port(0)))
}

pub struct Engine {
    divert: Arc<Divert>,
    uploads: Arc<Divert>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// Count and limit the devices' traffic (they're in `gateway`'s subnet; `internet`: the
    /// adapter the internet goes out of, where their uploads leave translated).
    pub fn start(shared: Arc<Mutex<Shared>>, gateway: Ip, internet: Option<u32>) -> Result<Engine, String> {
        let net = format!("{}.{}.{}", gateway[0], gateway[1], gateway[2]);
        let inside = |field: &str| format!("ip.{field} >= {net}.0 and ip.{field} <= {net}.255");
        let outside = |field: &str| format!("(ip.{field} < {net}.0 or ip.{field} > {net}.255)");
        // held: to the devices (and between them), and their uploads as Windows translated them
        let translated = internet.map(|i| format!(" or (ifIdx == {i} and {})", outside("SrcAddr"))).unwrap_or_default();
        let filter = format!("ip and ({}{translated})", inside("DstAddr"));
        // only seen: their uploads as they sent them
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
                    s.usage.entry(device).or_default().1 += n as u64;
                    s.translations.saw(&buffer[..n], device);
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

/// A packet to a device, between devices, or a device's upload as Windows translated it: Some(it)
/// to send on now; None: it waits or is dropped.
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
    if ours(dst) {
        if dst == gateway || dst[3] == 255 {
            return Some((packet, address));
        }
        s.usage.entry(dst).or_default().0 += packet.len() as u64;
        return limit(&mut s, dst, true, (packet, address));
    }
    // an upload, translated (counted as the device sent it)
    match s.translations.whose(&packet) {
        Some(device) => limit(&mut s, device, false, (packet, address)),
        None => {
            s.counts[0].unknown += 1;
            Some((packet, address))
        }
    }
}

/// One device's packet, one way (`down`: to it): stopped when it may not, else held to its limit.
fn limit(s: &mut Shared, device: Ip, down: bool, packet: Packet) -> Option<Packet> {
    let counts = &mut s.counts[down as usize];
    counts.seen += 1;
    if s.rules.local_only {
        counts.dropped += 1;
        return None; // "Share internet" is off: the devices only reach each other and the PC
    }
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
        return Some(packet);
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
    let (size, waiting) = (packet.0.len(), lane.waiting_count());
    let offered = lane.offer(packet, size, lanes.totals[down as usize].as_mut(), now);
    match (&offered, lane.waiting_count() > waiting) {
        (Some(_), _) => counts.sent += 1,
        (None, true) => counts.queued += 1,
        (None, false) => counts.dropped += 1,
    }
    offered
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An IPv4 TCP packet: from, to, IP id, ports.
    fn tcp(from: Ip, to: Ip, id: u16, ports: (u16, u16), length: usize) -> Vec<u8> {
        let mut p = vec![0u8; length];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&(length as u16).to_be_bytes());
        p[4..6].copy_from_slice(&id.to_be_bytes());
        p[9] = 6;
        p[12..16].copy_from_slice(&from);
        p[16..20].copy_from_slice(&to);
        p[20..22].copy_from_slice(&ports.0.to_be_bytes());
        p[22..24].copy_from_slice(&ports.1.to_be_bytes());
        p
    }

    #[test]
    fn translated_uploads_are_matched() {
        let (phone, pc, server) = ([10, 10, 10, 11], [192, 0, 80, 6], [142, 250, 75, 170]);
        let mut t = Translations::default();
        // Windows gives the connection another port: matched by the packet, then by connection
        t.saw(&tcp(phone, server, 7, (40000, 443), 1500), phone);
        assert_eq!(t.whose(&tcp(pc, server, 7, (61000, 443), 1500)), Some(phone));
        assert_eq!(t.whose(&tcp(pc, server, 8, (61000, 443), 1500)), Some(phone)); // (its note came late)
        // Windows keeps the port: matched by connection even before
        t.saw(&tcp(phone, server, 20, (40001, 443), 60), phone);
        assert_eq!(t.whose(&tcp(pc, server, 99, (40001, 443), 1500)), Some(phone));
        assert_eq!(t.whose(&tcp(pc, [1, 1, 1, 1], 5, (5000, 53), 80)), None);
    }
}
