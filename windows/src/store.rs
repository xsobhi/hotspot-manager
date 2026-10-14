//! What the app remembers (%APPDATA%\Hotspot Manager\settings.json): the Wi-Fi adapter to host
//! on, and the devices that have connected, by MAC address, with the names you gave them.
//! `HOTSPOT_HOME` keeps it somewhere else (for trying a build).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Store {
    pub adapter: Option<String>, // network adapter id of the Wi-Fi adapter to host on
    pub ssid: String,            // the hotspot's name and password, for the local-only hotspot
    pub passphrase: String,      // (Windows keeps them for its own, but asks for an internet connection)
    pub band: String,            // the band chosen ("5", "2.4", "any"; "" before): the app's own hotspot's
    pub devices: BTreeMap<String, Device>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Device {
    pub name: String, // given in the app ("" = none)
    pub host: String, // the name the device gave
    pub ip: String,
    pub first_seen: u64,
    pub last_seen: u64,
    #[serde(skip)]
    pub since: Option<u64>, // connected since (this run of the app)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Where the app keeps its things.
pub fn folder_path() -> PathBuf {
    match std::env::var_os("HOTSPOT_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("Hotspot Manager"),
    }
}

impl Store {
    pub fn folder() -> PathBuf {
        folder_path()
    }

    pub fn load() -> Store {
        std::fs::read_to_string(folder_path().join("settings.json")).ok()
            .and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = folder_path();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let temp = dir.join("settings.json.new"); // written whole, then swapped in
            if std::fs::write(&temp, text).is_ok() {
                let _ = std::fs::rename(&temp, dir.join("settings.json"));
            }
        }
    }

    pub fn adapter_id(&self) -> Option<windows::core::GUID> {
        self.adapter.as_deref().and_then(|id| windows::core::GUID::try_from(id).ok())
    }

    /// Note who's connected now; true when that changed what's saved.
    pub fn seen(&mut self, clients: &[crate::hotspot::Client]) -> bool {
        let time = now();
        let mut changed = false;
        for client in clients {
            let device = self.devices.entry(client.mac.clone()).or_insert_with(|| {
                changed = true;
                Device { first_seen: time, ..Default::default() }
            });
            device.since.get_or_insert(time);
            device.last_seen = time;
            for (field, value) in [(&mut device.host, &client.host), (&mut device.ip, &client.ip)] {
                if let Some(value) = value.as_ref().filter(|v| v.as_str() != field.as_str()) {
                    *field = value.clone();
                    changed = true;
                }
            }
        }
        for (mac, device) in &mut self.devices {
            if !clients.iter().any(|c| &c.mac == mac) {
                device.since = None;
            }
        }
        changed
    }
}

/// Phones use a made-up ("private") address per network: its second digit is 2, 6, A or E.
pub fn random_mac(mac: &str) -> bool {
    mac.chars().nth(1).is_some_and(|c| matches!(c.to_ascii_lowercase(), '2' | '6' | 'a' | 'e'))
}
