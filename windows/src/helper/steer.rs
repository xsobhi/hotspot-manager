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
    let list = powershell("Get-NetAdapter -IncludeHidden | Where-Object { $_.InterfaceDescription -like 'Microsoft Wi-Fi Direct Virtual Adapter*' } | \
                           ForEach-Object { \"$($_.Name)`t$($_.MacAddress)`t$($_.Status)\" }")?;
    // (name, its radio, off): only Wi-Fi adapters' own
    let found: Vec<(String, net::Adapter, bool)> = list.lines().filter_map(|line| {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        let [name, mac, status] = parts[..] else { return None };
        let bytes: Vec<u8> = mac.split('-').filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        let radio = net::radio_of(<[u8; 6]>::try_from(bytes).ok()?)?;
        Some((name.to_string(), radio, status == "Disabled"))
    }).collect();
    // a USB adapter plugged in again may come without its Wi-Fi Direct adapter (Windows makes it
    // later, or once it's plugged in again): then the others stay on, so something can host
    let chosen = chosen.filter(|c| {
        let can = found.iter().any(|(_, radio, _)| radio.id == c.id);
        if !can {
            log::line(format!("{} has no Wi-Fi Direct adapter now (plug it in again?): Windows hosts on another", c.friendly));
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
