//! A hotspot without internet ("local only"), for when the PC has no connection to share, as
//! MyPublicWiFi does: Windows' Wi-Fi Direct as a plain access point (its "legacy" mode: a Wi-Fi
//! name and password that phones join as usual). Devices reach each other and this PC (games on
//! the local network, sharing files), not the internet. Windows' own mobile hotspot won't start
//! without a connection to share.
//!
//! It's also the hotspot at an address of the user's (Windows' own stays at 192.168.137.1): it
//! tells the helper it's on, every few seconds, and the helper gives it the address, the devices'
//! addresses and the internet (helper/own.rs).
//!
//! It runs in a background process (`hotspot-manager --local-hotspot`) so it stays on when the
//! window closes; the window starts it and tells it to stop.

use crate::client;
use crate::store::Store;
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Devices::WiFiDirect::*;
use windows::Security::Credentials::PasswordCredential;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::*;

const RUNNING: PCWSTR = w!(r"Local\HotspotManagerLocal"); // held while it's on
const STOP: PCWSTR = w!(r"Local\HotspotManagerLocalStop"); // set to turn it off

/// Is the local-only hotspot on?
pub fn running() -> bool {
    // SAFETY: opens (and closes) a named mutex, if it's there
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, RUNNING) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

/// Turn it on (in its own process); waits until it's up, or says why not.
pub fn start() -> Result<(), String> {
    if running() {
        return Ok(());
    }
    let error = Store::folder().join("local-hotspot.error");
    let _ = std::fs::remove_file(&error);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new(exe).arg("--local-hotspot").creation_flags(DETACHED_PROCESS).spawn()
        .map_err(|e| format!("Couldn't start the local hotspot: {e}"))?;
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if running() {
            return Ok(());
        }
        if let Ok(why) = std::fs::read_to_string(&error) {
            return Err(why);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("Windows didn't start the local hotspot".into())
}

pub fn stop() {
    // SAFETY: opens the stop event, if it's there, and sets it
    unsafe {
        if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, STOP) {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
    let until = Instant::now() + Duration::from_secs(5);
    while running() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The background process: hold the access point until told to stop.
pub fn serve() -> i32 {
    let fail = |why: String| {
        let _ = std::fs::write(Store::folder().join("local-hotspot.error"), why);
        1
    };
    // SAFETY: once, at the start; named objects for the window to find
    let (running, stop) = unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        let running = CreateMutexW(None, true, RUNNING);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return 0; // it's on already
        }
        (running, CreateEventW(None, true, false, STOP))
    };
    let (Ok(_running), Ok(stop)) = (running, stop) else { return fail("Couldn't start the local hotspot".into()) };
    let store = Store::load();
    if store.ssid.is_empty() || store.passphrase.chars().count() < 8 {
        return fail("Set the hotspot's name and password (Settings) first".into());
    }
    let publisher = match open(&store.ssid, &store.passphrase) {
        Ok(p) => p,
        Err(e) => return fail(format!("Windows couldn't start the local hotspot: {}", e.message())),
    };
    // wait until Windows has it up, then until told to stop (or Windows stops it); meanwhile the
    // helper hears it's on (without the helper: no address of the user's, no internet)
    for second in 0u64.. {
        if second % 3 == 0 {
            let _ = client::hosting(true);
        }
        // SAFETY: our event
        let told = unsafe { WaitForSingleObject(stop, 1000) } == windows::Win32::Foundation::WAIT_OBJECT_0;
        let status = publisher.Status().unwrap_or(WiFiDirectAdvertisementPublisherStatus::Aborted);
        if told || status == WiFiDirectAdvertisementPublisherStatus::Aborted || status == WiFiDirectAdvertisementPublisherStatus::Stopped {
            let _ = publisher.Stop();
            let _ = client::hosting(false);
            return if told { 0 } else { fail("Windows stopped the hotspot (is Wi-Fi on?)".into()) };
        }
    }
    0
}

/// Windows' Wi-Fi Direct, as an access point with this name and password.
fn open(ssid: &str, passphrase: &str) -> windows::core::Result<WiFiDirectAdvertisementPublisher> {
    let publisher = WiFiDirectAdvertisementPublisher::new()?;
    let advertisement = publisher.Advertisement()?;
    advertisement.SetIsAutonomousGroupOwnerEnabled(true)?;
    let legacy = advertisement.LegacySettings()?;
    legacy.SetIsEnabled(true)?;
    legacy.SetSsid(&HSTRING::from(ssid))?;
    let credential = PasswordCredential::new()?;
    credential.SetPassword(&HSTRING::from(passphrase))?;
    legacy.SetPassphrase(&credential)?;
    publisher.Start()?;
    Ok(publisher)
}
