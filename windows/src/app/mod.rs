//! The window, as the Linux app's: the hotspot's state at the top with its switch and QR code,
//! then its devices and its settings on two tabs. Windows' answers come from the worker thread
//! (worker.rs) through `post`.

mod device;
mod devices;
mod qr;
mod settings;
mod start;
mod top;

pub use start::{app, post, run};
use start::started;

use crate::hotspot::{self, State, Status};
use crate::store::Store;
use crate::wnd::{self, Window};
use crate::{net, worker};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The window's size below its title bar, and where its parts are (96-DPI pixels).
pub const WIDTH: i32 = 640;
pub const HEIGHT: i32 = 540;
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

    /// Ask Windows how the hotspot is (once at a time).
    pub fn refresh(&self) {
        if self.asking.replace(true) {
            return;
        }
        let adapter = self.store.borrow().adapter_id();
        worker::run(move || {
            let status = hotspot::status(adapter);
            post(move |app| {
                app.asking.set(false);
                app.got(status);
            });
        });
    }

    fn got(&self, status: Result<Status, String>) {
        if let Ok(s) = &status {
            let changed = self.store.borrow_mut().seen(&s.clients);
            if changed {
                self.store.borrow().save();
            }
            self.settings.fill(s);
        }
        *self.status.borrow_mut() = Some(status);
        self.update();
    }

    /// Show what's known now.
    pub fn update(&self) {
        self.top.update(self);
        self.devices.update(self);
    }

    pub fn state(&self) -> Option<State> {
        self.status.borrow().as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.state)
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

    /// The switch: turn the hotspot on or off.
    pub fn turn(&self, on: bool) {
        self.busy.set(Some(if on { "Turning on…" } else { "Turning off…" }));
        self.update();
        let adapter = self.store.borrow().adapter_id();
        worker::run(move || {
            let result = hotspot::switch(adapter, on);
            post(move |app| {
                app.busy.set(None);
                match result {
                    Ok(()) if on => app.say("The hotspot is on. It stays on when you close this window.", false),
                    Ok(()) => app.say("The hotspot is off.", false),
                    Err(why) => app.say(why, true),
                }
                app.refresh();
            });
        });
    }
}
