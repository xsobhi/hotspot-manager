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
    for line in list.lines() {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        let [name, mac, status] = parts[..] else { continue };
        let bytes: Vec<u8> = mac.split('-').filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        let Ok(mac) = <[u8; 6]>::try_from(bytes) else { continue };
        let Some(radio) = net::radio_of(mac) else { continue }; // (only Wi-Fi adapters' own)
        let wanted = chosen.as_ref().is_none_or(|c| c.id == radio.id);
        let off = status == "Disabled";
        let command = match (wanted, off) {
            (true, true) => "Enable-NetAdapter",
            (false, false) => "Disable-NetAdapter",
            _ => continue,
        };
        powershell(&format!("Get-NetAdapter -IncludeHidden -Name {} | {command} -Confirm:$false", quoted(name)))?;
        let what = if wanted { "on" } else { "off" };
        log::line(format!("Wi-Fi Direct {what} for {} ({name})", radio.friendly));
    }
    Ok(())
}
