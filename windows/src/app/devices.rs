//! The Devices tab, as the Linux app's: "Speed for everyone" and search at the top, the limits
//! in words under them, then the devices (sortable), and "Show offline devices". Click a device
//! for everything about it (device.rs).

use super::list::{self, View, HEAD, ROW};
use super::words::{self, Link, Words};
use super::{device, everyone, logview, rows, setup, App, HEIGHT, PAGE, WIDTH};
use crate::client;
use crate::rules::{fmt_limit, fmt_speed, Limit, PRESETS};
use crate::theme::{self, Font};
use crate::wnd::canvas::{self, Mouse};
use crate::wnd::{is_checked, place, px, send, set_choices, set_visible, text, Window, FIELD};
use std::cell::RefCell;
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, RECT};

const BAR: i32 = PAGE + 12;
const WORDS: i32 = BAR + FIELD + 8; // the limits in words
const LIST: i32 = WORDS + 26;
const FOOT: i32 = HEIGHT - 40;
const EM_SETCUEBANNER: u32 = 0x1501;

pub struct Devices {
    controls: Vec<HWND>,
    everyone: HWND,
    search: HWND,
    words: HWND,
    list: HWND,
    offline: HWND,
    view: Rc<RefCell<View>>,
    line: Rc<RefCell<(Words, Vec<(RECT, Link)>)>>,
}

/// The choices for everyone's speed, and the one that's theirs now.
fn everyone_choices(limit: &Limit) -> (Vec<String>, usize) {
    let mut items = vec!["No limit".to_string()];
    items.extend(PRESETS.iter().map(|k| fmt_limit(*k)));
    let preset = PRESETS.iter().position(|k| *limit == Limit::both(*k));
    let custom = !limit.is_none() && preset.is_none();
    if custom {
        items.push(fmt_speed(limit)); // its own row, so Custom… can always be picked to change it
    }
    items.push("Custom…".into());
    let at = if limit.is_none() { 0 } else { preset.map_or(PRESETS.len() + 1, |i| i + 1) };
    (items, at)
}

impl Devices {
    pub fn new(window: &Window) -> Devices {
        let title = window.text_label("Speed for everyone", Font::Strong, crate::wnd::labels::Kind::Line);
        let everyone = window.choice(&["No limit", "Custom…"], 0, |i| picked(&super::app(), i));
        let search = window.edit("", || super::app().update());
        send(search, EM_SETCUEBANNER, 1, HSTRING::from("Search devices").as_ptr() as isize);
        let line: Rc<RefCell<(Words, Vec<(RECT, Link)>)>> = Rc::default();
        let l = line.clone();
        let words = window.canvas(move |p| words::paint(p, &mut l.borrow_mut()));
        let l = line.clone();
        canvas::on_mouse(words, move |event| {
            let Mouse::Down(x, y) = event else { return };
            let link = l.borrow().1.iter().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, k)| *k);
            match link {
                Some(Link::ResetOwn) => {
                    let app = super::app();
                    app.change_rules("All devices now use the speed for everyone.", |r| r.devices.clear());
                }
                Some(Link::Setup) => setup::run(&super::app()),
                Some(Link::Log) => logview::open(&super::app()),
                None => {}
            }
        });
        let view: Rc<RefCell<View>> = Rc::default();
        let v = view.clone();
        let list = window.canvas(move |p| list::paint(p, &v.borrow()));
        let v = view.clone();
        canvas::on_mouse(list, move |event| mouse(list, &v, event));
        let hint = window.label("Click a device to change its speed or name, or to block it.");
        window.set_text_color(hint, theme::DIM);
        let offline = window.checkbox("Show offline devices", false, |_| super::app().update());
        place(title, 24, BAR + 6, 150, 20);
        place(everyone, 24 + 150, BAR, 190, FIELD);
        place(search, WIDTH - 24 - 220, BAR, 220, FIELD);
        place(words, 24, WORDS, WIDTH - 48, 22);
        place(list, 0, LIST, WIDTH, FOOT - LIST);
        place(hint, 24, FOOT + 10, 440, 20);
        place(offline, WIDTH - 24 - 190, FOOT + 6, 190, 28);
        Devices { controls: vec![title, everyone, search, words, list, hint, offline], everyone, search, words, list, offline,
                  view, line }
    }

    pub fn show(&self, on: bool) {
        for control in &self.controls {
            set_visible(*control, on);
        }
    }

    pub fn update(&self, app: &App) {
        let rules = app.rules.borrow().clone();
        if let Some(r) = &rules {
            let (items, at) = everyone_choices(&r.everyone);
            set_choices(self.everyone, &items, at);
        }
        *self.line.borrow_mut() = (words::words(app, rules.as_ref()), vec![]);
        let filter: Vec<String> = text(self.search).to_lowercase().split_whitespace().map(String::from).collect();
        let all = is_checked(self.offline);
        let mut found: Vec<_> = rows::rows(app).into_iter()
            .filter(|r| r.dot != rows::Dot::Offline || all)
            .filter(|r| filter.iter().all(|w| r.words.contains(w.as_str())))
            .collect();
        let mut view = self.view.borrow_mut();
        list::sort(&mut found, view.sort);
        view.rows = found;
        view.hover = view.hover.filter(|&i| i < view.rows.len());
        view.empty = match app.status.borrow().as_ref() {
            Some(Ok(s)) if s.state == crate::hotspot::State::On && filter.is_empty() =>
                "No devices are connected. Show them the QR code to join.".into(),
            Some(Ok(_)) if filter.is_empty() => "When the hotspot is on, the devices on it show here.".into(),
            _ if !filter.is_empty() => "No devices match your search.".into(),
            _ => String::new(),
        };
        drop(view);
        canvas::redraw(self.list);
        canvas::redraw(self.words);
    }
}

/// A pick from "Speed for everyone".
fn picked(app: &Rc<App>, i: usize) {
    let Some(rules) = app.rules.borrow().clone() else {
        app.say(format!("{}. Settings → Advanced sets it up.", client::MISSING), true);
        return app.update();
    };
    let (items, _) = everyone_choices(&rules.everyone);
    if i + 1 == items.len() {
        return everyone::open(app);
    }
    if i > PRESETS.len() {
        return; // the custom speed it has already
    }
    let kbit = if i == 0 { 0 } else { PRESETS[i - 1] };
    let said = if kbit == 0 { "Devices are no longer limited.".to_string() } else { format!("Every device now gets {}.", fmt_limit(kbit)) };
    app.change_rules(said, |r| r.everyone = Limit::both(kbit));
}

fn mouse(list: HWND, view: &RefCell<View>, event: Mouse) {
    let mut v = view.borrow_mut();
    let width = px(WIDTH);
    let before = (v.hover, v.scroll, v.sort);
    let mut open = None;
    match event {
        Mouse::Move(_, y) => v.hover = list::row_at(&v, y),
        Mouse::Leave => v.hover = None,
        Mouse::Down(x, y) if y < px(HEAD) => {
            if let Some(column) = list::column_at(width, x) {
                v.sort = if v.sort.0 == column { (column, !v.sort.1) } else { (column, column >= 2) };
            }
        }
        Mouse::Down(_, y) => open = list::row_at(&v, y).map(|i| v.rows[i].mac.clone()),
        Mouse::Wheel(delta) => {
            let fits = ((FOOT - LIST - HEAD) / ROW) as usize;
            let most = v.rows.len().saturating_sub(fits);
            v.scroll = if delta > 0 { v.scroll.saturating_sub(1) } else { (v.scroll + 1).min(most) };
        }
        _ => {}
    }
    let changed = (v.hover, v.scroll, v.sort) != before;
    let resort = v.sort != before.2;
    drop(v);
    if resort {
        super::app().update();
    } else if changed {
        canvas::redraw(list);
    }
    if let Some(mac) = open {
        device::open(&super::app(), &mac);
    }
}
