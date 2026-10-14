//! The Devices tab: who's on the hotspot (and, if you like, who has been), each with its name,
//! address and how long it's been connected. Click one to name it or forget it.

use super::{device, App, HEIGHT, PAGE, WIDTH};
use crate::store::{now, Device};
use crate::theme::pictures::{self, Picture};
use crate::theme::{self, Align, Font, Painter};
use crate::wnd::canvas::{self, Mouse};
use crate::wnd::{is_checked, place, px, set_visible, Window};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, RECT};

const ROW: i32 = 60;
const BAR: i32 = 48; // the row under the list

pub struct Row {
    pub mac: String,
    title: String,
    line: String,
    online: bool,
    picture: Picture,
}

#[derive(Default)]
struct View {
    rows: Vec<Row>,
    hover: Option<usize>,
    scroll: i32, // rows scrolled past
    empty: String,
}

pub struct Devices {
    list: HWND,
    offline: HWND,
    view: Rc<RefCell<View>>,
}

impl Devices {
    pub fn new(window: &Window) -> Devices {
        let view: Rc<RefCell<View>> = Rc::default();
        let v = view.clone();
        let list = window.canvas(move |p| paint(p, &v.borrow()));
        let v = view.clone();
        canvas::on_mouse(list, move |event| {
            let mut view = v.borrow_mut();
            let at = |y: i32| {
                let i = (y / px(ROW)) as usize + view.scroll as usize;
                (i < view.rows.len()).then_some(i)
            };
            let before = (view.hover, view.scroll);
            let mut open = None;
            match event {
                Mouse::Move(_, y) => view.hover = at(y),
                Mouse::Leave => view.hover = None,
                Mouse::Down(_, y) => open = at(y).map(|i| view.rows[i].mac.clone()),
                Mouse::Wheel(delta) => {
                    let fits = (HEIGHT - PAGE - BAR) / ROW;
                    let most = (view.rows.len() as i32 - fits).max(0);
                    view.scroll = (view.scroll - delta.signum()).clamp(0, most);
                }
                _ => {}
            }
            if (view.hover, view.scroll) != before {
                canvas::redraw(list);
            }
            drop(view);
            if let Some(mac) = open {
                device::open(&super::app(), &mac);
            }
        });
        let offline = window.checkbox("Also show devices that aren't connected now", false, |_| super::app().update());
        place(list, 0, PAGE, WIDTH, HEIGHT - PAGE - BAR);
        place(offline, 24, HEIGHT - BAR + 10, 400, 28);
        Devices { list, offline, view }
    }

    pub fn show(&self, on: bool) {
        set_visible(self.list, on);
        set_visible(self.offline, on);
    }

    pub fn update(&self, app: &App) {
        let store = app.store.borrow();
        let status = app.status.borrow();
        let online: Vec<&str> = match status.as_ref() {
            Some(Ok(s)) => s.clients.iter().map(|c| c.mac.as_str()).collect(),
            _ => vec![],
        };
        let all = is_checked(self.offline);
        let mut found: Vec<(&String, &Device, bool)> = store.devices.iter()
            .map(|(mac, d)| (mac, d, online.contains(&mac.as_str()))).filter(|(_, _, on)| *on || all).collect();
        found.sort_by_key(|(_, d, on)| (!on, u64::MAX - d.last_seen));
        let rows: Vec<Row> = found.into_iter().map(|(mac, d, on)| Row {
            mac: mac.clone(),
            title: d.title(mac),
            line: [d.ip.as_str(), mac.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  "),
            online: on,
            picture: picture(d),
        }).collect();
        let mut view = self.view.borrow_mut();
        view.empty = match status.as_ref() {
            Some(Ok(s)) if s.state == crate::hotspot::State::On => "No devices are connected. Show them the QR code to join.".into(),
            _ => "When the hotspot is on, the devices on it show here.".into(),
        };
        view.rows = rows;
        view.hover = view.hover.filter(|&i| i < view.rows.len());
        drop(view);
        canvas::redraw(self.list); // (and each second: "Connected 12 min")
    }
}

/// A phone, a laptop, or some device, from the name it gave.
fn picture(d: &Device) -> Picture {
    let name = format!("{} {}", d.name, d.host).to_lowercase();
    let phone = ["iphone", "android", "galaxy", "redmi", "xiaomi", "pixel", "huawei", "honor", "oppo", "vivo",
                 "realme", "oneplus", "phone", "ipad", "tab"];
    let computer = ["desktop", "laptop", "macbook", "-pc", "pc-", "imac", "thinkpad", "surface"];
    if phone.iter().any(|w| name.contains(w)) {
        pictures::PHONE
    } else if computer.iter().any(|w| name.contains(w)) {
        pictures::LAPTOP
    } else {
        pictures::DEVICE
    }
}

/// "12 min", "3 h", "2 days".
fn span(seconds: u64) -> String {
    match seconds {
        s if s < 60 => "a moment".into(),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86400 => format!("{} h {} min", s / 3600, s % 3600 / 60),
        s => format!("{} days", s / 86400),
    }
}

fn paint(p: &Painter, view: &View) {
    p.fill(p.rect, theme::VIEW);
    if view.rows.is_empty() {
        p.text(RECT { top: px(40), bottom: px(70), ..p.rect }, &view.empty, Font::Body, theme::DIM, Align::Center);
        return;
    }
    let app = super::started();
    let store = app.as_ref().map(|a| a.store.borrow());
    for (i, row) in view.rows.iter().enumerate().skip(view.scroll as usize) {
        let top = px((i as i32 - view.scroll) * ROW);
        let r = RECT { left: 0, top, right: p.rect.right, bottom: top + px(ROW) };
        if r.top >= p.rect.bottom {
            break;
        }
        if view.hover == Some(i) {
            p.fill(r, theme::hover(theme::VIEW));
        }
        p.fill(RECT { left: px(24), top: r.bottom - 1, right: r.right - px(24), ..r }, theme::SEPARATOR);
        p.picture(RECT { left: px(24), right: px(24 + 38), ..r }, row.picture, 38, !row.online);
        let text = |a: i32, b: i32| RECT { left: px(76), top: top + px(a), right: r.right - px(190), bottom: top + px(b) };
        p.text(text(10, 31), &row.title, Font::Strong, if row.online { theme::TEXT } else { theme::DIM }, Align::Left);
        p.text(text(31, 50), &row.line, Font::Caption, theme::DIM, Align::Left);
        let device = store.as_ref().and_then(|s| s.devices.get(&row.mac));
        let (when, color) = match (row.online, device) {
            (true, Some(d)) => (format!("Connected {}", span(now().saturating_sub(d.since.unwrap_or(now())))), theme::ACCENT),
            (false, Some(d)) => (format!("Seen {} ago", span(now().saturating_sub(d.last_seen))), theme::DIM),
            _ => (String::new(), theme::DIM),
        };
        let right = RECT { left: r.right - px(190), right: r.right - px(24), ..r };
        p.text(right, &when, Font::Caption, color, Align::Right);
    }
}
