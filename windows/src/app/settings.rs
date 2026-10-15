//! The Settings tab: the hotspot's name, password and band, the Wi-Fi adapter it runs on (a USB
//! dongle, say), and turning it on at sign-in. Saved to Windows when you press Save; a hotspot
//! that's on restarts with them.

use super::{App, HEIGHT, PAGE, WIDTH};
use crate::hotspot::{self, Band, State, Status, BANDS};
use crate::theme;
use crate::rules::Rules;
use crate::wnd::{choice_index, enable, field, is_checked, place, send, set_checked, set_choice, set_text, set_visible,
                text, Window, FIELD, LINE};
use crate::{client, net, startup, worker};
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
    adapters: Vec<(GUID, String, String)>, // id, its name in the list, Windows' name ("Wi-Fi 2")
    five: Vec<(String, bool)>,             // which adapters' drivers can host on 5 GHz
    band_note: HWND,
    share: HWND,
    filled: Cell<bool>, // shown Windows' settings once (then they're the user's to edit)
}

impl Settings {
    pub fn new(window: &Window) -> Settings {
        let adapters: Vec<(GUID, String, String)> = net::adapters().into_iter().filter(|a| a.wifi)
            .map(|a| (a.id, format!("{} ({})", a.name, a.friendly), a.friendly)).collect();
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
        let band = window.choice(&BANDS.map(|(_, name)| name), 0, |_| super::app().settings.explain());
        let mut names = vec!["Let Windows choose".to_string()];
        names.extend(adapters.iter().map(|(_, name, _)| name.clone()));
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let adapter = window.choice(&names, 0, |_| super::app().settings.explain());
        let band_note = window.label("");
        window.set_text_color(band_note, theme::DIM);
        let share = window.checkbox("Share this PC's internet with the devices", true, |on| {
            let said = if on { "The devices get the internet again." } else { "The devices reach each other and this PC, not the internet." };
            super::app().change_rules(said, |r| r.local_only = !on);
        });
        let sign_in = window.checkbox("Turn on automatically when I sign in", startup::enabled(), |on| {
            if !startup::set(on) {
                super::app().say("Windows didn't let the app start at sign-in", true);
            }
        });
        let note = window.note("Up to 8 devices. The hotspot keeps going when you close this window. Pick your USB \
            Wi-Fi adapter to keep the PC's own Wi-Fi for its internet: the others' Wi-Fi Direct is turned off for it.");
        window.set_text_color(note, theme::DIM);
        let save = window.default_button("Save", || super::app().save_settings());
        let advanced = window.button("Advanced…", || super::advanced::open(&super::app()));
        let labels = ["Network name", "Password", "Band", "Wi-Fi adapter"].map(|l| window.name(l));
        let (x, mut y) = (24 + 150 + 12, PAGE + 22);
        for (i, (control, width)) in [(name, 300), (password, 300), (band, 300), (adapter, 410)].into_iter().enumerate() {
            place(labels[i], 24, y + (FIELD - LINE) / 2, 150, LINE);
            place(control, x, y, width, FIELD);
            y += FIELD + 14;
            if i == 2 {
                place(band_note, x, y - 10, WIDTH - x - 24, LINE);
                y += LINE;
            }
        }
        place(sign_in, x, y, 360, 26);
        place(share, x, y + 30, 400, 26);
        y += 56 + 14;
        place(generate, x + 310, PAGE + 22 + FIELD + 14, 100, FIELD);
        place(note, 24, y + 6, WIDTH - 48, 3 * LINE + 6);
        place(save, WIDTH - 24 - 110, HEIGHT - 24 - FIELD - 2, 110, FIELD + 2);
        place(advanced, 24, HEIGHT - 24 - FIELD - 2, 130, FIELD + 2);
        let mut controls = vec![name, password, generate, band, adapter, sign_in, note, save, advanced, band_note, share];
        controls.extend(labels);
        Settings { controls, name, password, band, adapter, adapters, five: net::hosts_on_5ghz(), band_note, share,
                   filled: Cell::new(false) }
    }

    /// Say what decides the band on the chosen adapter (its driver's Wireless Mode, apply.rs).
    fn explain(&self) {
        let band = BANDS[choice_index(self.band)].0;
        let chosen = choice_index(self.adapter).checked_sub(1).and_then(|i| self.adapters.get(i));
        let text = match chosen {
            None => String::new(),
            Some((id, _, name)) if Some(*id) == hotspot::internet_adapter() => match band {
                Band::Any => String::new(),
                _ => format!("{name} is this PC's internet connection too: Windows picks the band."),
            },
            Some((_, _, name)) => match band {
                Band::Five => format!("{name} is switched to 5 GHz only for it (its driver's Wireless Mode)."),
                Band::TwoPointFour => format!("{name} is switched to 2.4 GHz only for it (its driver's Wireless Mode)."),
                Band::Any if self.five.iter().any(|(n, can)| n == name && !can) => {
                    format!("{name}'s driver hosts on 2.4 GHz unless the band is set to 5 GHz.")
                }
                Band::Any => String::new(),
            },
        };
        set_text(self.band_note, &text);
    }

    /// Show the helper's rules ("Share internet").
    pub fn rules(&self, rules: Option<&Rules>) {
        enable(self.share, rules.is_some());
        let sharing = rules.is_none_or(|r| !r.local_only);
        if is_checked(self.share) != sharing {
            set_checked(self.share, sharing);
        }
    }

    pub fn show(&self, on: bool) {
        for control in &self.controls {
            set_visible(*control, on);
        }
    }

    /// Show what's saved: the adapter now, Windows' settings once they're known.
    pub fn start(&self) {
        let chosen = super::app().store.borrow().adapter_id();
        let place = chosen.and_then(|id| self.adapters.iter().position(|(a, _, _)| *a == id)).map_or(0, |i| i + 1);
        set_choice(self.adapter, place);
        self.explain();
    }

    pub fn fill(&self, s: &Status) {
        if self.filled.replace(true) {
            return;
        }
        set_text(self.name, &s.ssid);
        set_text(self.password, &s.passphrase);
        set_choice(self.band, BANDS.iter().position(|(b, _)| *b == s.band).unwrap_or(0));
        self.explain();
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
        let adapter = choice_index(s.adapter).checked_sub(1).and_then(|i| s.adapters.get(i)).map(|(id, _, _)| *id);
        let before = self.store.borrow().adapter_id();
        {
            let mut store = self.store.borrow_mut();
            store.adapter = adapter.map(|id| format!("{id:?}"));
            (store.ssid, store.passphrase) = (ssid.clone(), password.clone()); // (the local-only hotspot's too)
            store.band = band.key().to_string();
            store.save();
        }
        let was_on = self.state() == Some(State::On);
        let local = self.status.borrow().as_ref().and_then(|s| s.as_ref().ok()).is_some_and(|s| s.local);
        self.busy.set(Some("Saving…"));
        self.update();
        worker::run(move || {
            // Windows uses new settings when the hotspot starts: restart it if it's on
            let mut result = if was_on { hotspot::switch(before, false) } else { Ok(()) };
            let _ = client::steer(adapter); // (on the adapter chosen; without the helper, where Windows likes)
            // the band, through the adapter's driver (5 GHz: Windows' Wi-Fi Direct needs it kept there)
            if let Some(id) = adapter {
                match super::apply::keep_to_band(id, band) {
                    Ok(true) => super::apply::settled(id),
                    Ok(false) => {}
                    Err(why) => result = result.and(Err(why)),
                }
            }
            // (Windows' own hotspot's settings: not for the app's own, which keeps them itself)
            if !local && !hotspot::own_address() {
                result = result.and_then(|()| hotspot::configure(adapter, &ssid, &password, band));
            }
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
