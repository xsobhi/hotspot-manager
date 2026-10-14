//! What the window may ask of the helper (one JSON line each), checked before anything changes:
//! the helper has administrator rights, the window doesn't.

use super::dhcp::parse_ip;
use super::{diagnose, ics, log, save, shared};
use crate::net;
use crate::rules::{HelperStatus, Rules, Usage};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Request {
    Status,
    Rules,
    SetRules { rules: Rules },
    Log,
    Gateway { gateway: String },
    SetDriver { adapter: String, keyword: String, value: String },
    Steer { adapter: Option<String> },
    Diagnose,
    Hosting { on: bool }, // the app's own hotspot is on (said every few seconds) or off
}

pub fn answer(line: &str) -> String {
    let reply = match serde_json::from_str::<Request>(line) {
        Err(e) => Err(format!("The helper didn't understand: {e}")),
        Ok(request) => handle(request),
    };
    match reply {
        Ok(value) => json!({ "ok": value }),
        Err(why) => json!({ "error": why }),
    }.to_string()
}

fn handle(request: Request) -> Result<Value, String> {
    let shared = shared();
    match request {
        Request::Hosting { on } => {
            shared.lock().unwrap().hosting = on.then(std::time::Instant::now);
            Ok(json!(true))
        }
        Request::Status => {
            let s = shared.lock().unwrap();
            let usage = s.usage.iter().map(|(ip, (down, up))| Usage {
                mac: s.macs.get(ip).cloned().unwrap_or_default(),
                ip: format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
                host: s.macs.get(ip).and_then(|m| s.hosts.get(m)).cloned().unwrap_or_default(),
                down_bytes: *down,
                up_bytes: *up,
                ..Default::default()
            }).collect();
            Ok(json!(HelperStatus { engine: s.problem.clone(), usage }))
        }
        Request::Rules => Ok(json!(shared.lock().unwrap().rules)),
        Request::SetRules { rules } => {
            check(&rules)?;
            let mut s = shared.lock().unwrap();
            s.lanes.reset(rules.total.up, rules.total.down);
            s.rules = rules;
            save("rules.json", &s.rules);
            Ok(json!(true))
        }
        Request::Log => Ok(json!(log::lines())),
        Request::Gateway { gateway } => {
            let ip = parse_ip(&gateway).ok_or("That isn't an IP address")?;
            ics::set_gateway(ip)?;
            log::line(format!("The hotspot's address is now {gateway} (from when it next starts)"));
            Ok(json!(true))
        }
        Request::SetDriver { adapter, keyword, value } => {
            // only a Wi-Fi adapter's, and only a choice its driver offers (driver.rs checks)
            if !net::adapters().iter().any(|a| a.wifi && a.name == adapter) {
                return Err("That isn't one of this PC's Wi-Fi adapters".into());
            }
            crate::driver::set(&adapter, &keyword, &value)?;
            log::line(format!("{adapter}: {keyword} is now {value}"));
            Ok(json!(true))
        }
        Request::Steer { adapter } => {
            let id = adapter.map(|a| windows::core::GUID::try_from(a.as_str()).map_err(|_| "That isn't an adapter".to_string())).transpose()?;
            super::steer::steer(id)?;
            Ok(json!(true))
        }
        Request::Diagnose => {
            let hotspot = net::hotspot_adapter().ok_or("The hotspot isn't on")?;
            std::thread::spawn(move || diagnose::diagnose(net::gateway(), hotspot.index));
            Ok(json!(true))
        }
    }
}

fn mac(text: &str) -> bool {
    text.len() == 17 && text.split(':').all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Everything in the rules makes sense (as the Linux helper checks its input).
fn check(rules: &Rules) -> Result<(), String> {
    let net = &rules.network;
    let gateway = parse_ip(&net.gateway).ok_or("The hotspot's address isn't an IP address")?;
    let macs = rules.devices.keys().chain(&rules.blocked).chain(rules.cut.keys()).chain(net.fixed.keys());
    if let Some(bad) = macs.into_iter().find(|m| !mac(m)) {
        return Err(format!("{bad} isn't a MAC address"));
    }
    if net.dhcp_from == 0 || net.dhcp_from > net.dhcp_to || net.dhcp_to == 255 {
        return Err("The DHCP range must run from .1 to .254, low to high".into());
    }
    if !(1..=24 * 7 * 4).contains(&net.lease_hours) {
        return Err("The lease time must be between an hour and four weeks".into());
    }
    if net.dns.len() > 3 || net.dns.iter().any(|d| parse_ip(d).is_none()) {
        return Err("Up to 3 DNS servers, as IP addresses".into());
    }
    for ip in net.fixed.values() {
        let fixed = parse_ip(ip).ok_or(format!("{ip} isn't an IP address"))?;
        if fixed[..3] != gateway[..3] || fixed == gateway || fixed[3] == 0 || fixed[3] == 255 {
            return Err(format!("{ip} isn't one of the hotspot's addresses"));
        }
    }
    let speeds = rules.devices.values().chain([&rules.everyone, &rules.total]);
    if speeds.into_iter().any(|l| [l.down, l.up, l.burst_down, l.burst_up].iter().any(|&k| k > 10_000_000) || l.burst_secs > 600) {
        return Err("That speed is too high".into());
    }
    if rules.max_devices > 64 {
        return Err("Max devices: 64 at most".into());
    }
    Ok(())
}
