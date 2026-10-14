//! The top of the window: the hotspot's state in words, how many devices are on it and how
//! fast it's going, its switch, and the QR code to join it.

use super::{post, qr, started, App, TOP, WIDTH};
use crate::{hotspot, worker};
use crate::hotspot::State;
use crate::theme::{self, Align, Font, Painter};
use crate::wnd::{canvas, enable, is_checked, place, px, set_checked, Window};
use windows::Win32::Foundation::{HWND, RECT};

const WIFI: char = '\u{E701}'; // Windows' icon font

pub struct Top {
    canvas: HWND,
    switch: HWND,
    qr: HWND,
}

impl Top {
    pub fn new(window: &Window) -> Top {
        let canvas = window.canvas(|p| if let Some(app) = started() { paint(p, &app) });
        let switch = window.switch(false, |on| super::app().turn(on));
        let qr = window.button("QR code", || qr::open(&super::app()));
        place(canvas, 0, 0, WIDTH - 210, TOP);
        place(qr, WIDTH - 24 - 44 - 18 - 104, (TOP - 32) / 2, 104, 32);
        place(switch, WIDTH - 24 - 44, (TOP - 30) / 2, 44, 30);
        Top { canvas, switch, qr }
    }

    pub fn update(&self, app: &App) {
        let state = app.state();
        let on = match app.busy.get() {
            Some(doing) if doing.starts_with("Turning") => doing.starts_with("Turning on"),
            _ => state == Some(State::On),
        };
        if is_checked(self.switch) != on {
            set_checked(self.switch, on);
        }
        enable(self.switch, app.busy.get().is_none() && matches!(state, Some(State::On | State::Off)));
        enable(self.qr, state.is_some());
        canvas::redraw(self.canvas);
    }
}

/// "1.2 Mbps": bytes a second as network speeds are said.
pub fn rate(bytes_per_second: f64) -> String {
    let bits = bytes_per_second * 8.0;
    match bits {
        b if b >= 1e6 => format!("{:.1} Mbps", b / 1e6),
        b if b >= 1e3 => format!("{:.0} kbps", b / 1e3),
        _ => "0 kbps".into(),
    }
}

fn paint(p: &Painter, app: &App) {
    p.fill(p.rect, theme::VIEW);
    let status = app.status.borrow();
    let (state, ok) = match status.as_ref() {
        Some(Ok(s)) => (Some(s.state), Some(s)),
        Some(Err(_)) => (None, None),
        None => (Some(State::Unknown), None),
    };
    let (title, color) = match (app.busy.get(), state) {
        (Some(doing), _) => (doing.to_string(), theme::mix(theme::ACCENT, theme::VIEW, 0.45)),
        (None, Some(State::On)) => ("Hotspot is on".to_string(), theme::ACCENT),
        (None, Some(State::Off)) => ("Hotspot is off".to_string(), theme::DISABLED),
        (None, Some(State::Changing)) => ("Windows is turning the hotspot on or off…".to_string(), theme::DISABLED),
        (None, Some(State::Unknown)) => ("Checking the hotspot…".to_string(), theme::DISABLED),
        (None, None) => ("The hotspot can't start".to_string(), theme::DANGER),
    };
    // a round badge with Wi-Fi waves, in green while it's on
    let badge = RECT { left: px(24), top: px((TOP - 52) / 2), right: px(24 + 52), bottom: px((TOP + 52) / 2) };
    p.round(badge, px(26), Some(color), None);
    p.icon(badge, WIFI, 24, theme::WHITE);
    let text = |top: i32, bottom: i32| RECT { left: px(92), top: px(top), right: p.rect.right - px(8), bottom: px(bottom) };
    p.text(text(TOP / 2 - 26, TOP / 2 + 2), &title, Font::Title, theme::TEXT, Align::Left);
    let (line, line_color) = match (app.note(), &*status, ok) {
        (Some((note, error)), ..) => (note, if error { theme::DANGER } else { theme::ACCENT }),
        (None, Some(Err(why)), _) => (why.clone(), theme::DANGER),
        (None, _, Some(s)) if s.state == State::On => {
            let (down, up) = app.speed.get();
            let devices = match s.clients.len() {
                0 => "no devices yet".to_string(),
                n => format!("{n} of {} devices", s.max_clients),
            };
            (format!("“{}”  ·  {devices}  ·  ↓ {}  ↑ {}", s.ssid, rate(down), rate(up)), theme::DIM)
        }
        (None, _, Some(s)) => (format!("“{}”  ·  turn it on to share this PC's internet", s.ssid), theme::DIM),
        _ => (String::new(), theme::DIM),
    };
    p.text(text(TOP / 2 + 2, TOP / 2 + 26), &line, Font::Body, line_color, Align::Left);
}

impl App {
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
