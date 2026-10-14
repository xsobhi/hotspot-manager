//! Applying Settings → Advanced's changes that take the hotspot down for a moment: the
//! adapter's driver settings (the adapter restarts) and the hotspot's address.

use super::advanced::ip_text;
use super::App;
use crate::hotspot::{self, State};
use crate::{client, worker};
use std::rc::Rc;

/// Change the adapter's driver settings (the helper does: they need administrator rights); the
/// adapter restarts, so the hotspot is turned on again after.
pub fn set_driver(app: &Rc<App>, changes: Vec<(String, String, String)>) {
    let was_on = app.state() == Some(State::On);
    let adapter = app.store.borrow().adapter_id();
    app.busy.set(Some("Changing the adapter's settings…"));
    app.update();
    worker::run(move || {
        let mut result = Ok(());
        for (name, keyword, value) in &changes {
            result = result.and_then(|()| client::set_driver(name, keyword, value));
        }
        if was_on {
            std::thread::sleep(std::time::Duration::from_secs(4)); // (the adapter coming back)
            result = result.and_then(|()| hotspot::switch(adapter, true));
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
        // Windows may keep its own address until the PC restarts
        std::thread::sleep(std::time::Duration::from_secs(if was_on { 4 } else { 0 }));
        let kept = crate::net::hotspot_adapter().and_then(|a| a.ipv4.first().copied()).filter(|ip| *ip != gateway);
        super::post(move |app| {
            app.busy.set(None);
            match (result, kept) {
                (Ok(()), Some(ip)) => app.say(format!("Windows kept the hotspot at {}: the new address may need the PC to restart.",
                                                      ip_text(ip)), true),
                (Ok(()), None) if was_on => app.say(format!("The hotspot restarted at {}.", ip_text(gateway)), false),
                (Ok(()), None) => app.say(format!("The hotspot will be at {} when it's turned on.", ip_text(gateway)), false),
                (Err(why), _) => app.say(why, true),
            }
            app.refresh();
        });
    });
}
