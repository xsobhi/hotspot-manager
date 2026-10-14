//! The device list's rows, as the Linux app builds them: who's on (or was), its name and address,
//! its speed now, its speed limit and the data it has used, and the words search looks in.

use super::App;
use crate::rules::{fmt_rate, fmt_speed, human_bytes};
use crate::store::{now, Device};
use crate::vendor;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Online,
    Offline,
    Blocked,
}

pub struct Row {
    pub mac: String,
    pub dot: Dot,
    pub name: String,
    pub sub: String,       // its address, or when it was last seen
    pub down: f64,         // bits a second now
    pub up: f64,
    pub limit: String,
    pub own_limit: bool,   // its own speed, not everyone's
    pub used: Option<u64>, // bytes, while it's on
    pub words: String,     // for search
}

/// "2 h ago", "just now".
pub fn ago(when: u64) -> String {
    let s = now().saturating_sub(when);
    for (unit, n) in [("day", 86400), ("h", 3600), ("min", 60)] {
        if s >= n {
            let v = s / n;
            return format!("{v} {unit}{} ago", if unit == "day" && v > 1 { "s" } else { "" });
        }
    }
    "just now".into()
}

/// "1h 5m", "4m 10s".
pub fn span(seconds: u64) -> String {
    let (h, m) = (seconds / 3600, seconds / 60 % 60);
    if h > 0 { format!("{h}h {m}m") } else { format!("{m}m {}s", seconds % 60) }
}

/// What to call a device: the name you gave it, the name it gave, its maker, or "Unknown device".
pub fn name(mac: &str, d: &Device) -> String {
    [d.name.trim(), d.host.trim(), vendor::of(mac)].into_iter().find(|n| !n.is_empty())
        .unwrap_or("Unknown device").to_string()
}

pub fn rows(app: &App) -> Vec<Row> {
    let store = app.store.borrow();
    let status = app.status.borrow();
    let online: Vec<(&str, &str)> = match status.as_ref() {
        Some(Ok(s)) => s.clients.iter().map(|c| (c.mac.as_str(), c.ip.as_deref().unwrap_or(""))).collect(),
        _ => vec![],
    };
    let rules = app.rules.borrow();
    let helper = app.helper.borrow();
    let rates = app.rates.borrow();
    store.devices.iter().map(|(mac, d)| {
        let ip = online.iter().find(|(m, _)| *m == mac).map(|(_, ip)| *ip);
        let blocked = rules.as_ref().is_some_and(|r| r.blocked.contains(mac));
        let dot = if blocked { Dot::Blocked } else if ip.is_some() { Dot::Online } else { Dot::Offline };
        let ip = ip.unwrap_or(d.ip.as_str());
        let usage = helper.as_ref().ok().and_then(|h| h.usage.iter().find(|u| u.mac == *mac || (!ip.is_empty() && u.ip == ip)));
        let (down, up) = rates.get(ip).filter(|_| dot == Dot::Online).map_or((0.0, 0.0), |r| (r.3, r.4));
        let (limit, own_limit) = match rules.as_ref() {
            None => (String::new(), false),
            Some(r) => match r.devices.get(mac) {
                Some(own) => (fmt_speed(own), true),
                None if r.everyone.is_none() => ("No limit".into(), false),
                None => (format!("{} (everyone)", fmt_speed(&r.everyone)), false),
            },
        };
        let name = name(mac, d);
        let here = online.iter().any(|(m, _)| *m == mac);
        let sub = if here { ip.to_string() } else { format!("offline · {}", ago(d.last_seen)) };
        let words = [name.as_str(), &d.host, ip, mac, vendor::of(mac)].join(" ").to_lowercase();
        let used = usage.filter(|_| here).map(|u| u.down_bytes + u.up_bytes);
        Row { mac: mac.clone(), dot, name, sub, down, up, limit, own_limit, used, words }
    }).collect()
}

impl Row {
    pub fn now_text(&self) -> Option<(String, String)> {
        (self.dot == Dot::Online && self.used.is_some()).then(|| (format!("↓ {}", fmt_rate(self.down)), format!("↑ {}", fmt_rate(self.up))))
    }

    pub fn used_text(&self) -> String {
        self.used.map(human_bytes).unwrap_or_default()
    }
}
