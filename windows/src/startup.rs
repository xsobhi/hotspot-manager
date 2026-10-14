//! "Turn on when I sign in": Windows starts `hotspot-manager --turn-on` at sign-in (the Run key
//! of the current user), which turns the hotspot on, without a window, and exits.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const NAME: PCWSTR = w!("Hotspot Manager");

pub fn enabled() -> bool {
    let mut size = 0u32;
    // SAFETY: asks only whether the value is there (and its size)
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, NAME, RRF_RT_REG_SZ, None, None, Some(&mut size)) == ERROR_SUCCESS }
}

pub fn set(on: bool) -> bool {
    if !on {
        // SAFETY: removes our own value
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME) };
        return result == ERROR_SUCCESS || !enabled();
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let command: Vec<u16> = format!("\"{}\" --turn-on", exe.display()).encode_utf16().chain([0]).collect();
    // SAFETY: writes our own value, a string as long as we say
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, RUN, NAME, REG_SZ.0, Some(command.as_ptr().cast()),
                        (command.len() * 2) as u32) == ERROR_SUCCESS
    }
}

/// At sign-in: turn the hotspot on, trying for a while (the PC may still be connecting to the
/// internet it shares).
pub fn turn_on() -> i32 {
    // SAFETY: once, at the start of this thread
    unsafe { let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED); }
    let adapter = crate::store::Store::load().adapter_id();
    for _ in 0..36 {
        if crate::hotspot::switch(adapter, true).is_ok() {
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    1
}
