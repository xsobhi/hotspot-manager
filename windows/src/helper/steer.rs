//! Which Wi-Fi adapter hosts Windows' hotspot: Windows takes one that isn't busy, and apps can't
//! tell it which. So the helper turns Wi-Fi Direct off on the other adapters (each has its
//! "Microsoft Wi-Fi Direct Virtual Adapter"s, with its MAC address or the twin of it), and Windows
//! hosts on the one chosen. "Let Windows choose" turns them all on again.

use super::log;
use crate::driver::{powershell, quoted};
use crate::net;
use windows::core::GUID;

pub fn steer(chosen: Option<GUID>) -> Result<(), String> {
    let chosen = match chosen {
        Some(id) => Some(net::adapters().into_iter().find(|a| a.wifi && a.id == id).ok_or("That Wi-Fi adapter isn't here")?),
        None => None,
    };
    let mut found = wifi_direct()?;
    // Windows gives Wi-Fi Direct to one Wi-Fi card at a time: one plugged in later (a USB
    // adapter) has none while another has it, and only WLAN AutoConfig starting again moves it
    // (the PC's own Wi-Fi drops for a few seconds meanwhile)
    if let Some(c) = &chosen
        && !found.iter().any(|(_, radio, _)| radio.id == c.id) {
        log::line(format!("{} has no Wi-Fi Direct adapter: moving Wi-Fi Direct to it (Wi-Fi restarts)", c.friendly));
        for (name, _, _) in found.iter().filter(|(_, _, off)| !off) {
            let _ = powershell(&format!("Get-NetAdapter -IncludeHidden -Name {} | Disable-NetAdapter -Confirm:$false", quoted(name)));
        }
        let _ = powershell("Restart-Service wlansvc -Force; Start-Sleep -Seconds 8");
        found = wifi_direct()?;
    }
    // still none: the others stay on, so something can host
    let chosen = chosen.filter(|c| {
        let can = found.iter().any(|(_, radio, _)| radio.id == c.id);
        if !can {
            log::line(format!("{} has no Wi-Fi Direct adapter (plug it in again?): Windows hosts on another", c.friendly));
        }
        can
    });
    for (name, radio, off) in found {
        let wanted = chosen.as_ref().is_none_or(|c| c.id == radio.id);
        let command = match (wanted, off) {
            (true, true) => "Enable-NetAdapter",
            (false, false) => "Disable-NetAdapter",
            _ => continue,
        };
        powershell(&format!("Get-NetAdapter -IncludeHidden -Name {} | {command} -Confirm:$false", quoted(&name)))?;
        let what = if wanted { "on" } else { "off" };
        log::line(format!("Wi-Fi Direct {what} for {} ({name})", radio.friendly));
    }
    Ok(())
}

/// The Wi-Fi Direct adapters there are: (name, the Wi-Fi card it's on, turned off).
fn wifi_direct() -> Result<Vec<(String, net::Adapter, bool)>, String> {
    let list = powershell("Get-NetAdapter -IncludeHidden | Where-Object { $_.InterfaceDescription -like 'Microsoft Wi-Fi Direct Virtual Adapter*' } | \
                           ForEach-Object { \"$($_.Name)`t$($_.MacAddress)`t$($_.Status)\" }")?;
    Ok(list.lines().filter_map(|line| {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        let [name, mac, status] = parts[..] else { return None };
        let bytes: Vec<u8> = mac.split('-').filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        let radio = net::radio_of(<[u8; 6]>::try_from(bytes).ok()?)?;
        Some((name.to_string(), radio, status == "Disabled"))
    }).collect())
}
