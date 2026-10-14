//! The hotspot's own address, which Windows' Internet Connection Sharing gives it
//! (192.168.137.1 unless changed): its devices get addresses next to it. Changing it needs
//! administrator rights, and takes effect when the hotspot next starts.

use crate::helper::dhcp::Ip;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_SZ};

const KEY: PCWSTR = w!(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters");

/// Use `ip` (a private address, x.y.z.1 to .254) for the hotspot.
pub fn set_gateway(ip: Ip) -> Result<(), String> {
    let private = ip[0] == 10 || (ip[0] == 172 && (16..32).contains(&ip[1])) || (ip[0] == 192 && ip[1] == 168);
    if !private || ip[3] == 0 || ip[3] == 255 {
        return Err("The hotspot's address must be a private one (10.x, 172.16–31.x or 192.168.x), not ending in 0 or 255".into());
    }
    let text: Vec<u16> = format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]).encode_utf16().chain([0]).collect();
    for name in [w!("ScopeAddress"), w!("ScopeAddressBackup")] {
        // SAFETY: writes a string value as long as we say
        let result = unsafe {
            RegSetKeyValueW(HKEY_LOCAL_MACHINE, KEY, name, REG_SZ.0, Some(text.as_ptr().cast()), (text.len() * 2) as u32)
        };
        if result != ERROR_SUCCESS {
            return Err("Windows didn't let the helper change the hotspot's address".into());
        }
    }
    Ok(())
}
