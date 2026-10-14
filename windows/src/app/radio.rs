//! The radio part of Settings → Advanced: the hotspot's channel, channel width and Wi-Fi mode.
//! Windows' hotspot has no say in them; an adapter's driver may (Device Manager → Advanced), so
//! they're offered as the driver names them, when the hotspot has an adapter of its own (a USB
//! one, say). Greyed, with why, when the driver doesn't have them, or when the hotspot shares its
//! radio with the PC's own Wi-Fi connection (then it's on that network's channel).

use crate::driver::{self, Kind, Property};
use crate::theme;
use crate::wnd::{choice_index, enable, place, Window, FIELD, LINE};
use crate::{hotspot, net};
use windows::core::GUID;
use windows::Win32::Foundation::HWND;

/// What the hotspot's adapter and its driver allow (found off the UI thread: it asks PowerShell).
pub struct Found {
    adapter: Option<(String, String)>, // (description, name: "Wi-Fi 2")
    shared: bool,                      // it's the internet connection's adapter too (or Windows picks)
    settings: Vec<(Kind, Property)>,
}

pub fn find(chosen: Option<GUID>) -> Found {
    let adapter = chosen.and_then(|id| net::adapters().into_iter().find(|a| a.id == id));
    let shared = chosen.is_none() || chosen == hotspot::internet_adapter();
    let settings = adapter.as_ref().filter(|_| !shared).map(|a| driver::hotspot_settings(&a.name)).unwrap_or_default();
    Found { adapter: adapter.map(|a| (a.name, a.friendly)), shared, settings }
}

pub struct Radio {
    rows: Vec<(HWND, HWND, Option<Property>)>, // name, choice, the driver's setting
    note: HWND,
    adapter: Option<String>,
}

impl Radio {
    pub fn new(window: &Window, found: &Found) -> Radio {
        let usable = found.adapter.is_some() && !found.shared;
        let mut rows = vec![];
        for (kind, name) in [(Kind::Channel, "Channel"), (Kind::Width, "Channel width"), (Kind::Mode, "Wi-Fi mode")] {
            let offered: Vec<&Property> = found.settings.iter().filter(|(k, _)| *k == kind).map(|(_, p)| p).collect();
            if offered.is_empty() && kind != Kind::Mode {
                let choice = window.choice(&["Automatic"], 0, |_| {});
                enable(choice, false);
                rows.push((window.name(name), choice, None));
            }
            for p in offered {
                let choices = p.choices();
                let names: Vec<&str> = choices.iter().map(String::as_str).collect();
                let at = choices.iter().position(|c| *c == p.value).unwrap_or(0);
                let choice = window.choice(&names, at, |_| {});
                enable(choice, usable);
                rows.push((window.name(&p.name), choice, Some(p.clone())));
            }
        }
        let has = |kind: Kind| found.settings.iter().any(|(k, _)| *k == kind);
        let why = match (&found.adapter, found.shared) {
            (None, _) => "Windows picks the hotspot's adapter, so its channel and width too. Pick the adapter that hosts it \
                          (Settings) to see what its driver lets you set.".to_string(),
            (Some((_, name)), true) => format!("{name} is also the PC's internet connection: the hotspot shares its radio, so \
                                               it's on that network's channel and width."),
            (Some((_, name)), false) if !has(Kind::Channel) && !has(Kind::Width) => format!(
                "{name}'s driver doesn't let programs choose the channel or its width: Windows picks them. Changing the Wi-Fi \
                 mode restarts the adapter, and the hotspot with it."),
            (Some((_, name)), false) if !has(Kind::Channel) => format!(
                "{name}'s driver doesn't offer a channel: Windows picks it. Changing these restarts the adapter, and the \
                 hotspot with it."),
            _ => "Changing these restarts the adapter, and the hotspot with it.".to_string(),
        };
        let note = window.note(&why);
        window.set_text_color(note, theme::DIM);
        Radio { rows, note, adapter: found.adapter.as_ref().map(|(d, _)| d.clone()) }
    }

    /// Lay it out from `y` (names from `left`, choices at `x`); returns the y under it.
    pub fn place(&self, left: i32, names: i32, x: i32, width: i32, mut y: i32) -> i32 {
        for (name, choice, _) in &self.rows {
            place(*name, left, y + (FIELD - LINE) / 2, names, LINE);
            place(*choice, x, y, 300, FIELD);
            y += FIELD + 10;
        }
        place(self.note, x, y, width - x - left, 3 * LINE + 6);
        y + 3 * LINE + 14
    }

    /// The driver settings changed: (adapter, keyword, value).
    pub fn changes(&self) -> Vec<(String, String, String)> {
        let Some(adapter) = &self.adapter else { return vec![] };
        self.rows.iter().filter_map(|(_, choice, p)| {
            let p = p.as_ref()?;
            let picked = choice_index(*choice);
            let value = p.values().get(picked)?.clone();
            (p.choices().get(picked) != Some(&p.value)).then(|| (adapter.clone(), p.keyword.clone(), value))
        }).collect()
    }
}
