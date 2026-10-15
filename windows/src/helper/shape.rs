//! Speed limits as Linux's tc does them for the Linux app: a token bucket per device and
//! direction (HTB-like lanes, so each device gets its own speed), a second, faster bucket for
//! short bursts (TBF's peak rate), and one more for the whole hotspot. Packets over the limit
//! wait in a short queue, and are dropped when it's full or they waited too long: that's how TCP
//! learns to slow down, early and smoothly. A long queue delays everything behind it, pings too
//! (seconds of "loaded latency", "bufferbloat"), and TCP only slows when it overflows, in bursts.

use crate::rules::Limit;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MTU: f64 = 1514.0;
const QUEUE_PACKETS: usize = 300;
/// The most a lane holds: what it sends in this long at its speed (for no speed of its own, as
/// under the whole hotspot's cap only, QUEUE_BYTES).
const QUEUE_TIME: f64 = 0.15;
const QUEUE_BYTES: usize = 64 * 1024;
/// A packet that waited longer for its turn is dropped instead (as CoDel's target, roughly).
const MAX_WAIT: Duration = Duration::from_millis(100);
/// Packets up to this size go ahead of the big ones; at most this many of them wait.
const SMALL: usize = 300;
const SMALL_PACKETS: usize = 64;

/// Bytes a second from kbit/s.
fn bytes(kbit: u32) -> f64 {
    kbit as f64 * 1000.0 / 8.0
}

#[derive(Clone, Debug)]
pub struct Bucket {
    rate: f64, // bytes a second
    size: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    pub fn new(rate: f64, size: f64, now: Instant) -> Bucket {
        Bucket { rate, size, tokens: size, at: now }
    }

    fn refill(&mut self, now: Instant) {
        let seconds = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + seconds * self.rate).min(self.size);
        self.at = now;
    }

    fn has(&mut self, n: f64, now: Instant) -> bool {
        self.refill(now);
        self.tokens >= n
    }

    fn take(&mut self, n: f64) {
        self.tokens -= n;
    }
}

/// The buckets for a speed limit in one direction: (sustained, peak while bursting); none for
/// no limit.
pub fn buckets(rate_kbit: u32, burst_kbit: u32, burst_secs: u32, now: Instant) -> (Option<Bucket>, Option<Bucket>) {
    if rate_kbit == 0 {
        return (None, None);
    }
    let rate = bytes(rate_kbit);
    let smooth = (rate * 0.03).max(2.0 * MTU); // 30 ms of it: no stutter, no burst
    if burst_kbit > rate_kbit {
        let peak = bytes(burst_kbit);
        let extra = (peak - rate) * burst_secs.max(1) as f64; // what the burst lets through on top
        (Some(Bucket::new(rate, smooth + extra, now)), Some(Bucket::new(peak, (peak * 0.03).max(2.0 * MTU), now)))
    } else {
        (Some(Bucket::new(rate, smooth, now)), None)
    }
}

/// One device's traffic in one direction, under its limit. Small packets (pings, DNS, games'
/// updates, TCP's acknowledgements) wait apart from the big ones and go first, still within the
/// limit: they don't wait behind a download (as cake's and fq_codel's sparse flows don't).
pub struct Lane<P> {
    pub limit: (u32, u32, u32), // (kbit/s, burst kbit/s, burst seconds): to notice changes
    rate: Option<Bucket>, // none: only the whole hotspot's cap applies
    peak: Option<Bucket>,
    queues: [VecDeque<(P, usize, Instant)>; 2], // [small, big]: the packet, its size, since when it waits
    queued: usize, // the big ones' bytes waiting
    room: usize,   // the bytes they may hold
}

/// Which queue a packet of `n` bytes waits in: 0, the small ones'.
fn which(n: usize) -> usize {
    (n > SMALL) as usize
}

impl<P> Lane<P> {
    pub fn new(limit: (u32, u32, u32), now: Instant) -> Lane<P> {
        let (rate, peak) = buckets(limit.0, limit.1, limit.2, now);
        let room = if limit.0 == 0 { QUEUE_BYTES } else { (bytes(limit.0) * QUEUE_TIME) as usize };
        Lane { limit, rate, peak, queues: [VecDeque::new(), VecDeque::new()], queued: 0, room: room.max(4 * MTU as usize) }
    }

    fn fits(&mut self, n: usize, now: Instant) -> bool {
        let n = n as f64;
        self.rate.as_mut().is_none_or(|r| r.has(n, now)) && self.peak.as_mut().is_none_or(|p| p.has(n, now))
    }

    fn spend(&mut self, n: usize) {
        for bucket in [&mut self.rate, &mut self.peak].into_iter().flatten() {
            bucket.take(n as f64);
        }
    }

    /// A new packet: Some(it) to send now, or None (it waits, or is dropped when its queue is full).
    pub fn offer(&mut self, packet: P, n: usize, mut total: Option<&mut Bucket>, now: Instant) -> Option<P> {
        let q = which(n);
        // on at once when nothing waits ahead of it (for a small one: no other small one)
        let ahead = if q == 0 { !self.queues[0].is_empty() } else { self.waiting() };
        if !ahead && self.fits(n, now) && total.as_deref_mut().is_none_or(|t| t.has(n as f64, now)) {
            if let Some(t) = total {
                t.take(n as f64);
            }
            self.spend(n);
            return Some(packet);
        }
        let room = if q == 0 { self.queues[0].len() < SMALL_PACKETS } else {
            self.queues[1].len() < QUEUE_PACKETS && self.queued + n <= self.room
        };
        if room {
            self.queues[q].push_back((packet, n, now));
            if q == 1 {
                self.queued += n;
            }
        }
        None
    }

    /// The waiting packets that may go now, small ones first; how many were dropped for waiting
    /// too long.
    pub fn release(&mut self, mut total: Option<&mut Bucket>, now: Instant, out: &mut Vec<P>) -> usize {
        let mut dropped = 0;
        for q in 0..2 {
            while let Some(&(_, n, since)) = self.queues[q].front() {
                let late = now.saturating_duration_since(since) > MAX_WAIT;
                if !late && (!self.fits(n, now) || total.as_mut().is_some_and(|t| !t.has(n as f64, now))) {
                    return dropped; // (the speed limit's reached: the rest wait)
                }
                let (packet, _, _) = self.queues[q].pop_front().unwrap();
                if q == 1 {
                    self.queued -= n;
                }
                if late {
                    dropped += 1;
                    continue;
                }
                self.spend(n);
                if let Some(t) = total.as_mut() {
                    t.take(n as f64);
                }
                out.push(packet);
            }
        }
        dropped
    }

    pub fn waiting(&self) -> bool {
        self.queues.iter().any(|q| !q.is_empty())
    }

    pub fn waiting_count(&self) -> usize {
        self.queues.iter().map(VecDeque::len).sum()
    }
}

/// The (kbit/s, burst, seconds) of a limit in one direction (`down`: to the device).
pub fn direction(limit: &Limit, down: bool) -> (u32, u32, u32) {
    if down { (limit.down, limit.burst_down, limit.burst_secs) } else { (limit.up, limit.burst_up, limit.burst_secs) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// How many bytes a lane lets through in `seconds` of a sender pushing as fast as it can.
    fn throughput(limit: (u32, u32, u32), seconds: f64) -> f64 {
        let start = Instant::now();
        let mut lane: Lane<usize> = Lane::new(limit, start);
        let (mut sent, mut out) = (0usize, vec![]);
        let steps = (seconds * 1000.0) as u64;
        for ms in 0..steps {
            let now = start + Duration::from_millis(ms);
            for _ in 0..4 {
                if lane.offer(1500, 1500, None, now).is_some() {
                    sent += 1500;
                }
            }
            lane.release(None, now, &mut out);
        }
        sent += out.len() * 1500;
        sent as f64
    }

    #[test]
    fn holds_the_rate() {
        let got = throughput((2000, 0, 0), 10.0) * 8.0 / 10.0 / 1000.0; // kbit/s
        assert!((1900.0..2150.0).contains(&got), "{got}");
    }

    #[test]
    fn bursts_then_settles() {
        // 2 Mbps, bursting to 4 Mbps for 2 s: the first 2 s go about twice as fast
        let first = throughput((2000, 4000, 2), 2.0) * 8.0 / 2.0 / 1000.0;
        assert!(first > 3400.0, "{first}");
        let long = throughput((2000, 4000, 2), 20.0) * 8.0 / 20.0 / 1000.0;
        assert!(long < 2400.0, "{long}");
    }

    #[test]
    fn keeps_the_queue_short() {
        // a sender pushing far over 2 Mbps: nothing waits much longer than MAX_WAIT (as pings
        // behind it would), and the rate holds
        let start = Instant::now();
        let mut lane: Lane<Instant> = Lane::new((2000, 0, 0), start);
        let (mut out, mut longest, mut sent) = (vec![], Duration::ZERO, 0usize);
        for ms in 0..5000u64 {
            let now = start + Duration::from_millis(ms);
            for _ in 0..4 {
                if lane.offer(now, 1500, None, now).is_some() {
                    sent += 1;
                }
            }
            lane.release(None, now, &mut out);
            for since in out.drain(..) {
                longest = longest.max(now - since);
                sent += 1;
            }
        }
        assert!(longest <= MAX_WAIT, "{longest:?}");
        let kbit = (sent * 1500) as f64 * 8.0 / 5.0 / 1000.0;
        assert!((1900.0..2150.0).contains(&kbit), "{kbit}");
    }

    #[test]
    fn small_packets_go_first() {
        // a download filling 2 Mbps, and a ping every 50 ms: the pings hardly wait
        let start = Instant::now();
        let mut lane: Lane<(Instant, bool)> = Lane::new((2000, 0, 0), start);
        let (mut out, mut longest, mut pings) = (vec![], Duration::ZERO, 0);
        for ms in 0..5000u64 {
            let now = start + Duration::from_millis(ms);
            for _ in 0..4 {
                let _ = lane.offer((now, false), 1500, None, now);
            }
            if ms % 50 == 0 && lane.offer((now, true), 80, None, now).is_some() {
                pings += 1;
            }
            lane.release(None, now, &mut out);
            for (since, ping) in out.drain(..) {
                if ping {
                    longest = longest.max(now - since);
                    pings += 1;
                }
            }
        }
        assert_eq!(pings, 100); // (none dropped)
        assert!(longest <= Duration::from_millis(5), "{longest:?}");
    }

    #[test]
    fn total_cap_holds_lanes() {
        let start = Instant::now();
        let mut total = Bucket::new(bytes(1000), 2.0 * MTU, start);
        let mut lanes: Vec<Lane<usize>> = (0..3).map(|_| Lane::new((5000, 0, 0), start)).collect();
        let (mut sent, mut out) = (0usize, vec![]);
        for ms in 0..5000u64 {
            let now = start + Duration::from_millis(ms);
            for lane in &mut lanes {
                if lane.offer(1500, 1500, Some(&mut total), now).is_some() {
                    sent += 1500;
                }
                lane.release(Some(&mut total), now, &mut out);
            }
        }
        let kbit = (sent + out.len() * 1500) as f64 * 8.0 / 5.0 / 1000.0;
        assert!((900.0..1150.0).contains(&kbit), "{kbit}");
    }
}
