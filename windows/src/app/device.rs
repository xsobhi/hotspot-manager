//! A device's window: give it a name you'll recognise, see what's known about it, or forget it.

use super::App;
use crate::store::random_mac;
use crate::theme;
use crate::wnd::{self, text, Rows, Window, FIELD, LINE};
use std::rc::Rc;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 460;

pub fn open(app: &Rc<App>, mac: &str) {
    let Some(device) = app.store.borrow().devices.get(mac).cloned() else { return };
    let window = Window::dialog(&device.title(mac), Some(app.window.hwnd()), (WIDTH, 240), false);
    let name = window.edit(&device.name, || {});
    // what's known about it, a line each
    let mut facts = vec![];
    if !device.host.is_empty() {
        facts.push(format!("It calls itself “{}”", device.host));
    }
    if !device.ip.is_empty() {
        facts.push(format!("Address {}  ·  MAC {mac}", device.ip));
    } else {
        facts.push(format!("MAC {mac}"));
    }
    if random_mac(mac) {
        facts.push("It uses a private address, which it may change on other networks".into());
    }
    let about = window.note(&facts.join("\n"));
    window.set_text_color(about, theme::DIM);
    let label = window.name("Name");
    let (a, w, m) = (app.clone(), window.clone(), mac.to_string());
    let forget = window.button("Forget", move || {
        a.store.borrow_mut().devices.remove(&m);
        a.store.borrow().save();
        a.update();
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let (a, w, m) = (app.clone(), window.clone(), mac.to_string());
    let save = window.default_button("Save", move || {
        if let Some(d) = a.store.borrow_mut().devices.get_mut(&m) {
            d.name = text(name).trim().to_string();
        }
        a.store.borrow().save();
        a.update();
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());

    let mut rows = Rows::new(20, 20, 70, WIDTH - 40);
    rows.field(Some(label), name, FIELD);
    rows.gap(4);
    rows.full(about, (facts.len() as i32) * (LINE + 4));
    rows.gap(10);
    let bottom = rows.buttons(&[forget, cancel, save]);
    wnd::set_client_height(window.hwnd(), bottom + 20);
    wnd::center(window.hwnd());
    window.show();
}
