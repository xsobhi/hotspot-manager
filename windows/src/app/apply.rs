//! Applying Settings → Advanced's changes that take the hotspot down for a moment: the
//! adapter's driver settings (the adapter restarts) and the hotspot's address.

use super::advanced::ip_text;
use super::App;
use crate::driver::{self, Kind};
use crate::hotspot::{self, Band, State};
use crate::{client, net, worker};
use std::rc::Rc;
use windows::core::GUID;

/// Change the adapter's driver settings (the helper does: they need administrator rights); the
/// adapter restarts, so the hotspot is turned on again after.
pub fn set_driver(app: &Rc<App>, changes: Vec<(String, String, String)>) {
    let was_on = app.state() == Some(State::On);
    let adapter = app.store.borrow().adapter_id();
    app.busy.set(Some("Changing the adapter's settings…"));
    app.update();
    worker::run(move || {
        // off first: the adapter restarts under it
        let mut result = if was_on { hotspot::switch(adapter, false) } else { Ok(()) };
        for (name, keyword, value) in &changes {
            result = result.and_then(|()| client::set_driver(name, keyword, value));
        }
        if was_on {
            let changed = changes.first().and_then(|(name, _, _)| net::adapters().into_iter().find(|a| a.name == *name));
            match changed {
                Some(a) => settled(a.id),
                None => std::thread::sleep(std::time::Duration::from_secs(8)),
            }
            result = result.and(hotspot::switch(adapter, true));
        }
        super::post(move |app| {
            app.busy.set(None);
            match result {
                Ok(()) => app.say("The adapter's settings are changed.", false),
                Err(why) => app.say(why, true),
            }
            app.refresh();
        });
    });
}

/// Keep the hotspot's adapter to its band through the driver's Wireless Mode: Windows' Wi-Fi Direct
/// hosts on 2.4 GHz unless the adapter may only use 5 GHz (as MyPublicWiFi's users do by hand).
/// Not when it's the PC's internet connection too; for any band, only a single band is undone (a
/// mode chosen in Advanced stays). Whether it changed (the adapter restarts then).
pub fn keep_to_band(adapter: GUID, band: Band) -> Result<bool, String> {
    if hotspot::internet_adapter() == Some(adapter) {
        return Ok(false);
    }
    let Some(a) = net::adapters().into_iter().find(|a| a.id == adapter) else { return Ok(false) };
    let Some(mode) = driver::properties(&a.name).into_iter().find(|p| driver::kind(p) == Some(Kind::Mode)) else {
        return Ok(false);
    };
    // (any band: only a single band is undone; a band: one kept to it already stays, at its width)
    if band == Band::Any && !driver::single_band(&mode.value) || driver::band_of(&mode.value) == Some(band) {
        return Ok(false);
    }
    let Some((value, shown)) = driver::mode_for(&mode, band) else { return Ok(false) };
    if shown == mode.value {
        return Ok(false);
    }
    client::set_driver(&a.name, &mode.keyword, &value)?;
    Ok(true)
}

/// Wait for an adapter to be back after its driver settings changed, and a little more: Wi-Fi
/// Direct started while it's still restarting can hang Windows' Wi-Fi service until it's replugged.
pub fn settled(adapter: GUID) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    std::thread::sleep(std::time::Duration::from_secs(2));
    while std::time::Instant::now() < until && !net::adapters().iter().any(|a| a.id == adapter && a.wifi) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    std::thread::sleep(std::time::Duration::from_secs(6));
}

/// The hotspot's new address: the helper sets it, then the hotspot restarts with it (if it's on).
pub fn move_hotspot(app: &Rc<App>, gateway: [u8; 4]) {
    let was_on = app.state() == Some(State::On);
    let adapter = app.store.borrow().adapter_id();
    app.busy.set(Some("Moving the hotspot…"));
    app.update();
    worker::run(move || {
        // off, the new address (Windows' sharing restarts with it), on again
        let mut result = if was_on { hotspot::switch(adapter, false) } else { Ok(()) };
        result = result.and_then(|()| client::set_gateway(&ip_text(gateway)));
        if was_on && result.is_ok() {
            result = hotspot::switch(adapter, true);
        }
        // the helper gives the hotspot its address once it's up (a few seconds)
        let mut kept = None;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(if was_on && result.is_ok() { 15 } else { 0 });
        while std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let at = crate::net::hotspot_adapter().and_then(|a| a.ipv4.iter().copied().find(|ip| ip[..2] != [169, 254]));
            kept = at.filter(|ip| *ip != gateway);
            if at == Some(gateway) {
                break;
            }
        }
        super::post(move |app| {
            app.busy.set(None);
            match (result, kept) {
                (Ok(()), Some(ip)) => app.say(format!("The hotspot is still at {}: is the Hotspot Manager service running?",
                                                      ip_text(ip)), true),
                (Ok(()), None) if was_on => app.say(format!("The hotspot restarted at {}.", ip_text(gateway)), false),
                (Ok(()), None) => app.say(format!("The hotspot will be at {} when it's turned on.", ip_text(gateway)), false),
                (Err(why), _) => app.say(why, true),
            }
            app.refresh();
        });
    });
}
