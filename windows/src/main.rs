//! Hotspot Manager for Windows: Windows' own Wi-Fi hotspot, on the adapter you choose (a USB
//! dongle, say), with its devices, a QR code to join and turning on at sign-in. The window looks
//! as the Linux app does on Linux Mint.
//!
//!   hotspot-manager             open the window (or bring the open one forward)
//!   hotspot-manager --turn-on   turn the hotspot on and exit (at sign-in)

#![windows_subsystem = "windows"] // no console window behind the app

mod app;
mod hotspot;
mod net;
mod startup;
mod store;
#[allow(dead_code)] // (the look is TurboDM's, shared: not all of it is used here)
mod theme;
#[allow(dead_code)]
mod wnd;
mod worker;

fn main() {
    let turn_on = std::env::args().any(|a| a == "--turn-on");
    std::process::exit(if turn_on { startup::turn_on() } else { app::run() });
}
