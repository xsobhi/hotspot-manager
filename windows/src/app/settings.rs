//! The Settings tab: the hotspot's name, password and band, the Wi-Fi adapter it runs on (a USB
//! dongle, say), and turning it on at sign-in. Saved to Windows when you press Save; a hotspot
//! that's on restarts with them.

use super::{App, HEIGHT, PAGE, WIDTH};
use crate::hotspot::{self, Band, State, Status, BANDS};
use crate::theme;
use crate::wnd::{choice_index, field, place, send, set_choice, set_text, set_visible, text, Window, FIELD, LINE};
use crate::{net, startup, worker};
use std::cell::Cell;
use windows::core::GUID;
use windows::Win32::Foundation::HWND;
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

const EM_SETPASSWORDCHAR: u32 = 0x00CC;
const DOT: usize = 0x25CF; // ●
const EYE: char = '\u{E890}'; // Windows' icon font: "View"

pub struct Settings {
    controls: Vec<HWND>,
    name: HWND,
    password: HWND,
    band: HWND,
    adapter: HWND,
    adapters: Vec<(GUID, String)>,
    filled: Cell<bool>, // shown Windows' settings once (then they're the user's to edit)
}

impl Settings {
    pub fn new(window: &Window) -> Settings {
        let adapters: Vec<(GUID, String)> = net::adapters().into_iter().filter(|a| a.wifi)
            .map(|a| (a.id, format!("{} ({})", a.name, a.friendly))).collect();
        let name = window.edit("", || {});
        let password = window.edit("", || {});
        send(password, EM_SETPASSWORDCHAR, DOT, 0);
        let shown = Cell::new(false);
        let eye = window.icon_button(EYE, move || {
            shown.set(!shown.get());
            send(password, EM_SETPASSWORDCHAR, if shown.get() { 0 } else { DOT }, 0);
            set_text(password, &text(password)); // drawn again
        });
        field::make_field(password, vec![eye]);
        let generate = window.button("Generate", move || set_text(password, &new_password()));
        let band = window.choice(&BANDS.map(|(_, name)| name), 0, |_| {});
        let mut names = vec!["Let Windows choose".to_string()];
        names.extend(adapters.iter().map(|(_, name)| name.clone()));
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let adapter = window.choice(&names, 0, |_| {});
        let sign_in = window.checkbox("Turn on automatically when I sign in", startup::enabled(), |on| {
            if !startup::set(on) {
                super::app().say("Windows didn't let the app start at sign-in", true);
            }
        });
        let note = window.note("Windows shares this PC's internet connection with the devices on the hotspot \
            (up to 8). The hotspot keeps going when you close this window. Pick your USB Wi-Fi adapter to keep \
            the PC's own Wi-Fi for its internet.");
        window.set_text_color(note, theme::DIM);
        let save = window.default_button("Save", || super::app().save_settings());
        let advanced = window.button("Advanced…", || super::advanced::open(&super::app()));
        let labels = ["Network name", "Password", "Band", "Wi-Fi adapter"].map(|l| window.name(l));
        let (x, mut y) = (24 + 150 + 12, PAGE + 22);
        for (i, (control, width)) in [(name, 300), (password, 300), (band, 300), (adapter, 410)].into_iter().enumerate() {
            place(labels[i], 24, y + (FIELD - LINE) / 2, 150, LINE);
            place(control, x, y, width, FIELD);
            y += FIELD + 14;
        }
        place(sign_in, x, y, 360, 26);
        y += 26 + 14;
        place(generate, x + 310, PAGE + 22 + FIELD + 14, 100, FIELD);
        place(note, 24, y + 6, WIDTH - 48, 3 * LINE + 6);
        place(save, WIDTH - 24 - 110, HEIGHT - 24 - FIELD - 2, 110, FIELD + 2);
        place(advanced, 24, HEIGHT - 24 - FIELD - 2, 130, FIELD + 2);
        let mut controls = vec![name, password, generate, band, adapter, sign_in, note, save, advanced];
        controls.extend(labels);
        Settings { controls, name, password, band, adapter, adapters, filled: Cell::new(false) }
    }

    pub fn show(&self, on: bool) {
        for control in &self.controls {
            set_visible(*control, on);
        }
    }

    /// Show what's saved: the adapter now, Windows' settings once they're known.
    pub fn start(&self) {
        let chosen = super::app().store.borrow().adapter_id();
        let place = chosen.and_then(|id| self.adapters.iter().position(|(a, _)| *a == id)).map_or(0, |i| i + 1);
        set_choice(self.adapter, place);
    }

    pub fn fill(&self, s: &Status) {
        if self.filled.replace(true) {
            return;
        }
        set_text(self.name, &s.ssid);
        set_text(self.password, &s.passphrase);
        set_choice(self.band, BANDS.iter().position(|(b, _)| *b == s.band).unwrap_or(0));
    }
}

/// 12 letters and digits, without look-alikes (l, 1, O, 0), from Windows' random numbers.
fn new_password() -> String {
    const LETTERS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 12];
    // SAFETY: fills our buffer
    let _ = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    bytes.iter().map(|b| LETTERS[*b as usize % LETTERS.len()] as char).collect()
}

impl App {
    fn save_settings(&self) {
        let s = &self.settings;
        let (ssid, password) = (text(s.name).trim().to_string(), text(s.password));
        if ssid.is_empty() || ssid.len() > 32 {
            return self.say("The network name needs 1 to 32 characters", true);
        }
        if password.chars().count() < 8 || password.chars().count() > 63 || !password.is_ascii() {
            return self.say("The password needs 8 to 63 characters (English letters, digits and symbols)", true);
        }
        let band: Band = BANDS[choice_index(s.band)].0;
        let adapter = choice_index(s.adapter).checked_sub(1).and_then(|i| s.adapters.get(i)).map(|(id, _)| *id);
        let before = self.store.borrow().adapter_id();
        self.store.borrow_mut().adapter = adapter.map(|id| format!("{id:?}"));
        self.store.borrow().save();
        let was_on = self.state() == Some(State::On);
        self.busy.set(Some("Saving…"));
        self.update();
        worker::run(move || {
            // Windows uses new settings when the hotspot starts: restart it if it's on
            let mut result = if was_on { hotspot::switch(before, false) } else { Ok(()) };
            result = result.and_then(|()| hotspot::configure(adapter, &ssid, &password, band));
            if was_on {
                result = result.and(hotspot::switch(adapter, true));
            }
            super::post(move |app| {
                app.busy.set(None);
                match result {
                    Ok(()) if was_on => app.say("Saved, and the hotspot restarted with it. Devices join again with the new settings.", false),
                    Ok(()) => app.say("Saved. The hotspot uses it when it's turned on.", false),
                    Err(why) => app.say(why, true),
                }
                app.refresh();
            });
        });
    }
}
