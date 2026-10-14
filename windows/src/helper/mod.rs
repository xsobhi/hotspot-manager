//! The helper service ("hotspot-manager --service"), Windows' counterpart of the Linux app's
//! hotspotctl: it runs with administrator rights (LocalSystem), takes the window's requests on
//! a named pipe (pipe.rs), and does what needs them: counting, limiting and blocking the
//! devices' traffic (engine.rs), giving out their addresses (dhcpd.rs) and setting the
//! hotspot's own (ics.rs). Its rules stay in force without the window.

pub mod dhcp;
mod dhcpd;
mod diagnose;
mod divert;
mod engine;
mod ics;
pub mod log;
mod pipe;
mod requests;
pub mod service;
pub mod shape;
mod steer;

use crate::net;
use crate::rules::Rules;
use dhcp::{Ip, Leases};
use engine::{Engine, Lanes};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// What the helper's threads share.
#[derive(Default)]
pub struct Shared {
    pub rules: Rules,
    pub macs: HashMap<Ip, String>,       // address → device (neighbour table, our leases)
    pub usage: HashMap<Ip, (u64, u64)>,  // bytes (to it, from it) since the helper started
    pub lanes: Lanes,
    pub leases: Leases,
    pub online: Vec<String>,             // devices on it now, in the order they came
    pub hosts: HashMap<String, String>,  // MAC → the name it gave DHCP
    pub problem: String,                 // why the traffic engine isn't working, if it isn't
}

impl Shared {
    /// May the device's traffic go through? (Not when blocked, disconnected, or one too many.)
    pub fn let_through(&self, mac: &str) -> bool {
        if self.rules.blocked.contains(mac) || self.rules.cut.get(mac).is_some_and(|until| *until > now()) {
            return false;
        }
        let most = self.rules.max_devices as usize;
        most == 0 || self.online.iter().position(|m| m == mac).is_none_or(|i| i < most)
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn save(name: &str, value: &impl Serialize) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let path = log::folder().join(name);
        let temp = path.with_extension("new");
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    std::fs::read_to_string(log::folder().join(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn shared() -> &'static Arc<Mutex<Shared>> {
    static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let rules: Rules = load("rules.json");
        let mut lanes = Lanes::default();
        lanes.reset(rules.total.up, rules.total.down);
        Arc::new(Mutex::new(Shared { rules, lanes, leases: load("leases.json"), ..Default::default() }))
    })
}

/// What's running for the hotspot now.
struct Running {
    gateway: Ip,
    hotspot: u32,
    engine: Option<Engine>,
    dhcp: Option<dhcpd::DhcpServer>,
}

impl Running {
    fn stop(self) {
        if let Some(e) = self.engine {
            e.stop();
        }
        if let Some(d) = self.dhcp {
            d.stop();
        }
    }
}

/// The helper's life: until `stop`.
pub fn run(stop: &AtomicBool) {
    // SAFETY: finer timer ticks for this process (the speed limits' queues are let out every 2 ms)
    unsafe { windows::Win32::Media::timeBeginPeriod(1) };
    log::line(format!("Helper started (version {})", env!("CARGO_PKG_VERSION")));
    let shared = shared().clone();
    std::thread::spawn(pipe::serve);
    let mut running: Option<Running> = None;
    while !stop.load(Ordering::Relaxed) {
        upkeep(&shared, &mut running);
        for _ in 0..20 {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if let Some(r) = running.take() {
        r.stop();
    }
    log::line("Helper stopped");
}

/// Every two seconds: follow the hotspot (on, off, its adapter), who's on it, and the rules.
fn upkeep(shared: &Arc<Mutex<Shared>>, running: &mut Option<Running>) {
    let adapter = net::hotspot_adapter();
    if let Some(a) = &adapter {
        tidy(a);
    }
    // the hotspot's address is the one Windows gave it (one asked for applies once Windows takes it)
    let gateway = adapter.as_ref().and_then(|a| a.ipv4.iter().copied().find(|ip| ip[..2] != [169, 254]))
        .unwrap_or_else(net::gateway);
    let same = |r: &Running| adapter.as_ref().is_some_and(|a| a.index == r.hotspot) && r.gateway == gateway;
    if running.as_ref().is_some_and(|r| !same(r)) {
        running.take().unwrap().stop();
        log::line("The hotspot is off");
    }
    let Some(adapter) = adapter else { return };
    let own_dhcp = shared.lock().unwrap().rules.network.own_dhcp();
    let r = running.get_or_insert_with(|| {
        log::line(format!("The hotspot is on: {} ({})", adapter.friendly, adapter.name));
        let engine = Engine::start(shared.clone(), gateway);
        let mut s = shared.lock().unwrap();
        s.problem = engine.as_ref().err().cloned().unwrap_or_default();
        if !s.problem.is_empty() {
            log::line(format!("Can't count or limit traffic: {}", s.problem));
        }
        Running { gateway, hotspot: adapter.index, engine: engine.ok(), dhcp: None }
    });
    if own_dhcp != r.dhcp.is_some() {
        if let Some(d) = r.dhcp.take() {
            d.stop();
        } else {
            match dhcpd::DhcpServer::start(shared.clone(), gateway, gateway, adapter.index) {
                Ok(d) => r.dhcp = Some(d),
                Err(e) => log::line(format!("Can't give out addresses: {e}")),
            }
        }
    }
    let seen = net::neighbors(adapter.index);
    let mut s = shared.lock().unwrap();
    for (ip, mac) in &seen {
        s.macs.insert(*ip, mac.clone());
    }
    let here: Vec<&String> = seen.iter().map(|(_, mac)| mac).collect();
    s.online.retain(|mac| here.contains(&mac));
    for mac in here {
        if !s.online.contains(mac) {
            s.online.push(mac.clone());
        }
    }
    let time = now();
    s.rules.cut.retain(|_, until| *until > time);
}

/// An address an earlier version added to the hotspot's adapter, beside Windows' own
/// 192.168.137.1 (Windows' sharing only passes its own subnet): taken off again.
fn tidy(adapter: &net::Adapter) {
    use std::os::windows::process::CommandExt;
    let asked = net::gateway();
    if asked == [192, 168, 137, 1] || !adapter.ipv4.contains(&asked) || !adapter.ipv4.contains(&[192, 168, 137, 1]) {
        return;
    }
    let address = format!("address={}.{}.{}.{}", asked[0], asked[1], asked[2], asked[3]);
    let _ = std::process::Command::new("netsh")
        .args(["interface", "ipv4", "delete", "address", &format!("name={}", adapter.friendly), &address])
        .creation_flags(0x0800_0000) // no console window
        .status();
}

/// The helper in a console window (as administrator), for trying it out: `--helper`.
pub fn run_here() -> i32 {
    static STOP: AtomicBool = AtomicBool::new(false);
    run(&STOP);
    0
}
