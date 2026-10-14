//! What the helper service enforces, set from the window: speed limits (for everyone, per device,
//! for the whole hotspot), blocked devices, and how devices get their addresses. The same choices
//! as the Linux app's `hotspotctl set-devices` and `set-config`. Speeds are in kbit/s, 0 = none.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Speed presets (kbit/s, the same down and up), as on Linux.
pub const PRESETS: [u32; 7] = [1000, 2000, 3000, 5000, 10000, 20000, 50000];
/// Lease times offered (hours, words).
pub const LEASES: [(u32, &str); 6] = [(1, "1 hour"), (6, "6 hours"), (12, "12 hours"), (24, "1 day"), (72, "3 days"), (168, "1 week")];

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Limit {
    pub down: u32,
    pub up: u32,
    pub burst_down: u32, // a short burst up to this speed…
    pub burst_up: u32,
    pub burst_secs: u32, // …for this long, when the device has been under its limit
}

impl Limit {
    pub fn both(kbit: u32) -> Limit {
        Limit { down: kbit, up: kbit, ..Default::default() }
    }

    pub fn is_none(&self) -> bool {
        self.down == 0 && self.up == 0
    }

    pub fn bursts(&self) -> bool {
        self.burst_down > 0 || self.burst_up > 0
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Network {
    pub gateway: String, // the hotspot's own address (Windows: ICS's, always a /24)
    pub dhcp_from: u8,   // the range devices get (last number of the address)
    pub dhcp_to: u8,
    pub lease_hours: u32,
    pub dns: Vec<String>,                 // empty: the hotspot answers (with the PC's DNS)
    pub fixed: BTreeMap<String, String>,  // MAC → address it always gets
}

impl Default for Network {
    fn default() -> Network {
        Network { gateway: "192.168.137.1".into(), dhcp_from: 10, dhcp_to: 254, lease_hours: 12, dns: vec![],
                  fixed: BTreeMap::new() }
    }
}

impl Network {
    /// Windows (Internet Connection Sharing) hands out the addresses, unless something here needs
    /// the helper's own DHCP: another range, lease time, DNS servers or fixed addresses.
    pub fn own_dhcp(&self) -> bool {
        let usual = Network { gateway: self.gateway.clone(), ..Default::default() };
        *self != usual
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Rules {
    pub everyone: Limit,                 // each device's own lane at this speed
    pub total: Limit,                    // all of them together (no bursts)
    pub devices: BTreeMap<String, Limit>, // MAC → its own speed
    pub blocked: BTreeSet<String>,
    pub cut: BTreeMap<String, u64>,      // MAC → cut off until (unix seconds): "Disconnect"
    pub isolate: bool,                   // devices can't reach each other
    pub max_devices: u32,                // 0 = no limit (Windows allows 8 at most)
    pub network: Network,
}

impl Rules {
    /// The speed a device gets: its own, or everyone's.
    pub fn limit_for(&self, mac: &str) -> Limit {
        self.devices.get(mac).copied().unwrap_or(self.everyone)
    }
}

/// What the helper reports about a device on the hotspot.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Usage {
    pub mac: String,
    pub ip: String,
    pub down_bytes: u64, // to the device, since the helper started
    pub up_bytes: u64,
    pub down_rate: f64, // bits a second, now
    pub up_rate: f64,
}

/// The helper's state, for the window.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct HelperStatus {
    pub engine: String, // "" when it's working, else why not
    pub usage: Vec<Usage>,
}

/// "no limit", "500 kbps", "2 Mbps", "2.5 Mbps".
pub fn fmt_limit(kbit: u32) -> String {
    match kbit {
        0 => "no limit".into(),
        k if k < 1000 => format!("{k} kbps"),
        k => format!("{} Mbps", trim(k as f64 / 1000.0)),
    }
}

fn trim(v: f64) -> String {
    let text = format!("{v:.1}");
    text.trim_end_matches(".0").to_string()
}

/// "No limit", "2 Mbps", "↓ 5 Mbps  ↑ 1 Mbps", with " + bursts".
pub fn fmt_speed(limit: &Limit) -> String {
    if limit.is_none() {
        return "No limit".into();
    }
    let mut text = if limit.down == limit.up {
        fmt_limit(limit.down)
    } else {
        format!("↓ {}  ↑ {}", fmt_limit(limit.down), fmt_limit(limit.up))
    };
    if limit.bursts() {
        text.push_str(" + bursts");
    }
    text
}

/// "0", "300 kbps", "1.2 Mbps": a speed now (bits a second).
pub fn fmt_rate(bits: f64) -> String {
    match bits {
        b if b < 1000.0 => "0".into(),
        b if b < 1e6 => format!("{:.0} kbps", b / 1000.0),
        b => format!("{:.1} Mbps", b / 1e6),
    }
}

/// "512 B", "1.5 MB".
pub fn human_bytes(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if v < 1024.0 {
            return if unit == "B" { format!("{v:.0} {unit}") } else { format!("{v:.1} {unit}") };
        }
        v /= 1024.0;
    }
    format!("{v:.1} TB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(fmt_limit(2500), "2.5 Mbps");
        assert_eq!(fmt_limit(10000), "10 Mbps");
        assert_eq!(fmt_speed(&Limit { down: 5000, up: 1000, ..Default::default() }), "↓ 5 Mbps  ↑ 1 Mbps");
        assert_eq!(fmt_speed(&Limit { burst_down: 4000, ..Limit::both(2000) }), "2 Mbps + bursts");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert!(!Network::default().own_dhcp());
        assert!(Network { lease_hours: 24, ..Default::default() }.own_dhcp());
    }
}
