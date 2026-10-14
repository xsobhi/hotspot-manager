//! The window, as the Linux app's: the hotspot's state at the top with its switch and QR code,
//! then its devices and its settings on two tabs. Windows' answers, and the helper service's,
//! come from the worker thread (worker.rs) through `post`.

mod advanced;
mod apply;
mod device;
mod devices;
mod editor;
mod everyone;
mod list;
mod logview;
mod qr;
mod radio;
mod rows;
mod settings;
mod setup;
mod start;
mod top;
mod words;

pub use start::{app, post, run};
use start::started;

use crate::hotspot::{self, State, Status};
use crate::rules::{HelperStatus, Rules};
use crate::store::Store;
use crate::wnd::{self, Window};
use crate::{client, net, worker};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The window's size below its title bar, and where its parts are (96-DPI pixels).
pub const WIDTH: i32 = 720;
pub const HEIGHT: i32 = 560;
pub const TOP: i32 = 96; // the hotspot's state
pub const PAGE: i32 = TOP + 1 + wnd::tabs::TABS; // the tabs' pages

pub struct App {
    pub window: Rc<Window>,
    pub store: RefCell<Store>,
    pub status: RefCell<Option<Result<Status, String>>>, // None: not known yet
    pub busy: Cell<Option<&'static str>>,                 // "Turning on…" while Windows does it
    note: RefCell<Option<(String, bool, Instant)>>,       // a message (true: something went wrong)
    pub speed: Cell<(f64, f64)>,                          // bytes a second to the devices, from them
    traffic: Cell<Option<(Instant, u64, u64)>>,
    pub rules: RefCell<Option<Rules>>,                    // the helper's; None: no helper (yet)
    pub helper: RefCell<Result<HelperStatus, String>>,
    pub rates: RefCell<HashMap<String, (u64, u64, Instant, f64, f64)>>, // ip → bytes then, bits/s now
    pub radio: RefCell<Option<String>>, // the adapter hosting the hotspot: "TP-Link (Wi-Fi 2)"
    asking: Cell<bool>,
    ticks: Cell<u64>,
    top: top::Top,
    devices: devices::Devices,
    settings: settings::Settings,
}

impl App {
    fn show_page(&self, page: usize) {
        wnd::batch(|| {
            self.devices.show(page == 0);
            self.settings.show(page == 1);
        });
    }

    /// Ask Windows how the hotspot is, and the helper how its devices are (once at a time).
    pub fn refresh(&self) {
        if self.asking.replace(true) {
            return;
        }
        let adapter = self.store.borrow().adapter_id();
        let need_rules = self.rules.borrow().is_none();
        worker::run(move || {
            let status = hotspot::status(adapter);
            let helper = client::status();
            let radio = net::hosting_radio().map(|a| format!("{} ({})", a.name.split_whitespace().next().unwrap_or(""), a.friendly));
            let rules = if need_rules && helper.is_ok() { client::rules().ok() } else { None };
            post(move |app| {
                app.asking.set(false);
                *app.radio.borrow_mut() = radio;
                if rules.is_some() {
                    *app.rules.borrow_mut() = rules;
                }
                if helper.is_err() {
                    app.rules.borrow_mut().take();
                }
                app.got(status, helper);
            });
        });
    }

    fn got(&self, status: Result<Status, String>, helper: Result<HelperStatus, String>) {
        if let Ok(s) = &status {
            let mut store = self.store.borrow_mut();
            let renamed = !s.local && (store.ssid != s.ssid || store.passphrase != s.passphrase);
            if renamed {
                (store.ssid, store.passphrase) = (s.ssid.clone(), s.passphrase.clone()); // for the local-only hotspot
            }
            drop(store);
            let changed = self.store.borrow_mut().seen(&s.clients) || renamed;
            if changed {
                self.store.borrow().save();
            }
            self.settings.fill(s);
        }
        if let Ok(h) = &helper {
            let now = Instant::now();
            let mut rates = self.rates.borrow_mut();
            for u in &h.usage {
                let rate = |bytes: u64, before: u64, then: Instant| {
                    bytes.saturating_sub(before) as f64 * 8.0 / now.duration_since(then).as_secs_f64().max(0.5)
                };
                let entry = rates.entry(u.ip.clone()).or_insert((u.down_bytes, u.up_bytes, now, 0.0, 0.0));
                let (down, up) = (rate(u.down_bytes, entry.0, entry.2), rate(u.up_bytes, entry.1, entry.2));
                *entry = (u.down_bytes, u.up_bytes, now, down, up);
            }
        }
        *self.status.borrow_mut() = Some(status);
        *self.helper.borrow_mut() = helper;
        self.update();
    }

    /// Show what's known now.
    pub fn update(&self) {
        self.top.update(self);
        self.devices.update(self);
        self.settings.rules(self.rules.borrow().as_ref());
    }

    pub fn state(&self) -> Option<State> {
        self.status.borrow().as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.state)
    }

    /// Change the helper's rules (`said`: the message when it's done).
    pub fn change_rules(&self, said: impl Into<String>, change: impl FnOnce(&mut Rules)) {
        let Some(mut rules) = self.rules.borrow().clone() else {
            return self.say(format!("{}. Settings → Advanced sets it up.", client::MISSING), true);
        };
        change(&mut rules);
        *self.rules.borrow_mut() = Some(rules.clone());
        self.update();
        let said = said.into();
        worker::run(move || {
            let result = client::set_rules(&rules);
            post(move |app| {
                match result {
                    Ok(()) => app.say(said, false),
                    Err(why) => {
                        app.rules.borrow_mut().take(); // ask for them again
                        app.say(why, true);
                    }
                }
                app.refresh();
            });
        });
    }

    fn tick(&self) {
        let ticks = self.ticks.get() + 1;
        self.ticks.set(ticks);
        if self.note.borrow().as_ref().is_some_and(|(_, _, at)| at.elapsed() > Duration::from_secs(10)) {
            self.note.borrow_mut().take();
        }
        self.measure();
        if ticks % 2 == 0 && self.busy.get().is_none() {
            self.refresh();
        }
        self.update(); // the speed, "Connected 12 min"
    }

    /// The hotspot's speed, from its adapter's byte counts a second apart.
    fn measure(&self) {
        let counts = (self.state() == Some(State::On)).then(net::hotspot_traffic).flatten();
        let (now, before) = (Instant::now(), self.traffic.get());
        self.traffic.set(counts.map(|(out, inn)| (now, out, inn)));
        self.speed.set(match (counts, before) {
            (Some((out, inn)), Some((then, out0, in0))) => {
                let seconds = now.duration_since(then).as_secs_f64().max(0.2);
                (out.saturating_sub(out0) as f64 / seconds, inn.saturating_sub(in0) as f64 / seconds)
            }
            _ => (0.0, 0.0),
        });
    }

    /// A message at the top for a few seconds (`error`: in red).
    pub fn say(&self, text: impl Into<String>, error: bool) {
        *self.note.borrow_mut() = Some((text.into(), error, Instant::now()));
        self.top.update(self);
    }

    pub fn note(&self) -> Option<(String, bool)> {
        self.note.borrow().as_ref().map(|(text, error, _)| (text.clone(), *error))
    }
}
