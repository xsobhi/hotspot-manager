//! Hotspot Manager for Windows: Windows' own Wi-Fi hotspot, on the adapter you choose (a USB
//! dongle, say), with its devices, a QR code to join and turning on at sign-in. The window looks
//! as the Linux app does on Linux Mint.
//!
//!   hotspot-manager             open the window (or bring the open one forward)
//!   hotspot-manager --turn-on   turn the hotspot on and exit (at sign-in)
//!   hotspot-manager --service   the helper, as the HotspotManager service (Windows starts it)
//!   hotspot-manager --helper    the helper in a console, as administrator (for trying it)

#![windows_subsystem = "windows"] // no console window behind the app

mod app;
mod client;
mod helper;
mod hotspot;
mod net;
mod rules;
mod startup;
mod store;
mod vendor;
#[allow(dead_code)] // (the look is TurboDM's, shared: not all of it is used here)
mod theme;
#[allow(dead_code)]
mod wnd;
mod worker;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    std::process::exit(match mode.as_str() {
        "--turn-on" => startup::turn_on(),
        "--service" => helper::service::run(),
        "--helper" => helper::run_here(),
        _ => app::run(),
    });
}
