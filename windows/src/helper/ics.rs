//! The hotspot's own address, which Windows' Internet Connection Sharing gives it
//! (192.168.137.1 unless changed): its devices get addresses next to it. Changing it needs
//! administrator rights, and takes effect when the hotspot next starts.

use crate::helper::dhcp::Ip;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_SZ};
use windows::Win32::System::Services::*;

const KEY: PCWSTR = w!(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters");

/// Use `ip` (a private address, x.y.z.1 to .254) for the hotspot.
pub fn set_gateway(ip: Ip) -> Result<(), String> {
    let private = ip[0] == 10 || (ip[0] == 172 && (16..32).contains(&ip[1])) || (ip[0] == 192 && ip[1] == 168);
    if !private || ip[3] == 0 || ip[3] == 255 {
        return Err("The hotspot's address must be a private one (10.x, 172.16–31.x or 192.168.x), not ending in 0 or 255".into());
    }
    let text: Vec<u16> = format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]).encode_utf16().chain([0]).collect();
    // (ICS reads ScopeAddress; Windows' mobile hotspot, StandaloneDhcpAddress)
    for name in [w!("ScopeAddress"), w!("ScopeAddressBackup"), w!("StandaloneDhcpAddress")] {
        // SAFETY: writes a string value as long as we say
        let result = unsafe {
            RegSetKeyValueW(HKEY_LOCAL_MACHINE, KEY, name, REG_SZ.0, Some(text.as_ptr().cast()), (text.len() * 2) as u32)
        };
        if result != ERROR_SUCCESS {
            return Err("Windows didn't let the helper change the hotspot's address".into());
        }
    }
    restart_sharing();
    Ok(())
}

/// Windows' sharing and its mobile hotspot read the address when they start: restart them (the
/// hotspot is off meanwhile; it starts the mobile hotspot service again when it's turned on).
fn restart_sharing() {
    stop_service(w!("icssvc"), false);
    stop_service(w!("SharedAccess"), true);
}

fn stop_service(name: PCWSTR, start_again: bool) {
    // SAFETY: Windows' service manager, on one of its services; handles closed at the end
    unsafe {
        let Ok(manager) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return };
        if let Ok(service) = OpenServiceW(manager, name, SERVICE_STOP | SERVICE_START | SERVICE_QUERY_STATUS) {
            let mut status = SERVICE_STATUS::default();
            if ControlService(service, SERVICE_CONTROL_STOP, &mut status).is_ok() {
                for _ in 0..50 {
                    let _ = QueryServiceStatus(service, &mut status);
                    if status.dwCurrentState == SERVICE_STOPPED {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                if start_again {
                    let _ = StartServiceW(service, None);
                }
            }
            let _ = CloseServiceHandle(service);
        }
        let _ = CloseServiceHandle(manager);
    }
}
