//! Windows' own hotspot (Settings → Mobile hotspot) through its WinRT API: on and off, its name,
//! password and band, the Wi-Fi adapter it runs on, and the devices connected to it. Windows
//! shares the PC's internet connection with them (Internet Connection Sharing, 192.168.137.x).
//! The calls wait for Windows: run them off the UI thread (worker.rs).

use windows::core::{Error, GUID, HSTRING};
use windows::Devices::WiFi::WiFiAdapter;
use windows::Networking::Connectivity::{NetworkAdapter, NetworkInformation};
use windows::Networking::HostNameType;
use windows::Networking::NetworkOperators::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Off,
    On,
    Changing,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Any,
    TwoPointFour,
    Five,
}

pub const BANDS: [(Band, &str); 3] = [(Band::Any, "Any available"), (Band::Five, "5 GHz (faster)"),
                                      (Band::TwoPointFour, "2.4 GHz (longer range, older devices)")];

#[derive(Clone, Debug)]
pub struct Client {
    pub mac: String, // aa:bb:cc:dd:ee:ff
    pub host: Option<String>,
    pub ip: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub state: State,
    pub ssid: String,
    pub passphrase: String,
    pub band: Band,
    pub clients: Vec<Client>,
    pub max_clients: u32,
}

fn message(e: &Error) -> String {
    let text = e.message();
    if text.trim().is_empty() { format!("Windows said {:#010x}", e.code().0) } else { text.trim().to_string() }
}

/// The hotspot sharing this PC's internet connection, on `adapter` (or where Windows puts it).
fn manager(adapter: Option<GUID>) -> Result<NetworkOperatorTetheringManager, String> {
    let profile = NetworkInformation::GetInternetConnectionProfile()
        .map_err(|_| "This PC isn't connected to the internet, so there's nothing to share".to_string())?;
    match adapter.and_then(network_adapter) {
        Some(target) => NetworkOperatorTetheringManager::CreateFromConnectionProfileWithTargetAdapter(&profile, &target),
        None => NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile),
    }.map_err(|e| message(&e))
}

/// The adapter's object for Windows' hotspot API: from the Wi-Fi adapters, or from a network it
/// has been connected to.
fn network_adapter(id: GUID) -> Option<NetworkAdapter> {
    if let Ok(all) = WiFiAdapter::FindAllAdaptersAsync().and_then(|op| op.get()) {
        for i in 0..all.Size().unwrap_or(0) {
            if let Ok(adapter) = all.GetAt(i).and_then(|a| a.NetworkAdapter())
                && adapter.NetworkAdapterId().ok() == Some(id) {
                return Some(adapter);
            }
        }
    }
    let profiles = NetworkInformation::GetConnectionProfiles().ok()?;
    (0..profiles.Size().unwrap_or(0)).filter_map(|i| profiles.GetAt(i).and_then(|p| p.NetworkAdapter()).ok())
        .find(|a| a.NetworkAdapterId().ok() == Some(id))
}

pub fn status(adapter: Option<GUID>) -> Result<Status, String> {
    let m = manager(adapter)?;
    let state = match m.TetheringOperationalState() {
        Ok(TetheringOperationalState::On) => State::On,
        Ok(TetheringOperationalState::Off) => State::Off,
        Ok(TetheringOperationalState::InTransition) => State::Changing,
        _ => State::Unknown,
    };
    let config = m.GetCurrentAccessPointConfiguration().map_err(|e| message(&e))?;
    let band = match config.Band() {
        Ok(TetheringWiFiBand::TwoPointFourGigahertz) => Band::TwoPointFour,
        Ok(TetheringWiFiBand::FiveGigahertz) => Band::Five,
        _ => Band::Any,
    };
    Ok(Status {
        state,
        ssid: config.Ssid().map(|s| s.to_string()).unwrap_or_default(),
        passphrase: config.Passphrase().map(|s| s.to_string()).unwrap_or_default(),
        band,
        clients: if state == State::On { clients(&m) } else { vec![] },
        max_clients: m.MaxClientCount().unwrap_or(8),
    })
}

fn clients(m: &NetworkOperatorTetheringManager) -> Vec<Client> {
    let Ok(list) = m.GetTetheringClients() else { return vec![] };
    (0..list.Size().unwrap_or(0)).filter_map(|i| list.GetAt(i).ok()).map(|c| {
        let mac = c.MacAddress().map(|m| m.to_string()).unwrap_or_default().to_lowercase().replace('-', ":");
        let (mut host, mut ip) = (None, None);
        if let Ok(names) = c.HostNames() {
            for name in (0..names.Size().unwrap_or(0)).filter_map(|j| names.GetAt(j).ok()) {
                let text = name.DisplayName().map(|t| t.to_string()).unwrap_or_default();
                match name.Type() {
                    Ok(HostNameType::Ipv4) => { ip.get_or_insert(text); }
                    Ok(HostNameType::DomainName) => {
                        // "Galaxy-S23.mshome.net": the name the device gave
                        host.get_or_insert(text.trim_end_matches(".mshome.net").to_string());
                    }
                    _ => {}
                }
            }
        }
        Client { mac, host, ip }
    }).collect()
}

/// Turn the hotspot on or off.
pub fn switch(adapter: Option<GUID>, on: bool) -> Result<(), String> {
    let m = manager(adapter)?;
    let operation = if on { m.StartTetheringAsync() } else { m.StopTetheringAsync() };
    let result = operation.and_then(|op| op.get()).map_err(|e| message(&e))?;
    let status = result.Status().map_err(|e| message(&e))?;
    if status == TetheringOperationStatus::Success || status == TetheringOperationStatus::AlreadyOn {
        return Ok(());
    }
    let extra = result.AdditionalErrorMessage().map(|m| m.to_string()).unwrap_or_default();
    let why = match status {
        TetheringOperationStatus::WiFiDeviceOff => "The Wi-Fi adapter is off (or airplane mode is on)".to_string(),
        TetheringOperationStatus::OperationInProgress => "Windows is already turning the hotspot on or off".to_string(),
        TetheringOperationStatus::NetworkLimitedConnectivity => "The internet connection isn't working well enough to share".to_string(),
        TetheringOperationStatus::RadioRestriction => "The Wi-Fi adapter can't host a hotspot right now".to_string(),
        TetheringOperationStatus::BandInterference => "That band can't be used while the PC's Wi-Fi is on another band".to_string(),
        other => format!("Windows couldn't do it (reason {})", other.0),
    };
    Err(if extra.trim().is_empty() { why } else { format!("{why}: {}", extra.trim()) })
}

/// Set the hotspot's name, password and band (Windows applies them when it next turns on).
pub fn configure(adapter: Option<GUID>, ssid: &str, passphrase: &str, band: Band) -> Result<(), String> {
    let m = manager(adapter)?;
    let config = NetworkOperatorTetheringAccessPointConfiguration::new().map_err(|e| message(&e))?;
    config.SetSsid(&HSTRING::from(ssid)).map_err(|e| message(&e))?;
    config.SetPassphrase(&HSTRING::from(passphrase)).map_err(|e| message(&e))?;
    let wanted = match band {
        Band::Any => TetheringWiFiBand::Auto,
        Band::TwoPointFour => TetheringWiFiBand::TwoPointFourGigahertz,
        Band::Five => TetheringWiFiBand::FiveGigahertz,
    };
    if config.SetBand(wanted).is_err() && band != Band::Any {
        return Err("This version of Windows can't choose the hotspot's band".into());
    }
    m.ConfigureAccessPointAsync(&config).and_then(|op| op.get()).map_err(|e| message(&e))
}
