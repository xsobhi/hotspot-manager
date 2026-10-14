//! "Join the hotspot": a QR code a phone's camera reads to join it, with its name and password.

use super::App;
use crate::hotspot::Status;
use crate::theme::{self, Align, Font};
use crate::wnd::{self, place, px, Window};
use qrcodegen::{QrCode, QrCodeEcc};
use std::rc::Rc;
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 380;
const CODE: i32 = 260;

/// The text phones read from Wi-Fi QR codes ("WIFI:T:WPA;S:name;P:password;;").
fn wifi_text(s: &Status) -> String {
    let escape = |t: &str| t.chars().flat_map(|c| {
        let special = matches!(c, '\\' | ';' | ',' | ':' | '"');
        special.then_some('\\').into_iter().chain([c])
    }).collect::<String>();
    format!("WIFI:T:WPA;S:{};P:{};;", escape(&s.ssid), escape(&s.passphrase))
}

pub fn open(app: &Rc<App>) {
    let Some(Ok(status)) = app.status.borrow().clone() else { return };
    let Ok(code) = QrCode::encode_text(&wifi_text(&status), QrCodeEcc::Medium) else { return };
    let window = Window::dialog("Join the hotspot", Some(app.window.hwnd()), (WIDTH, 420), false);
    let picture = window.canvas(move |p| {
        p.fill(p.rect, theme::WHITE);
        let size = code.size();
        let quiet = 2; // white around it, so cameras find its edges
        let cell = (p.rect.right.min(p.rect.bottom)) / (size + 2 * quiet);
        let offset = (p.rect.right - cell * size) / 2;
        for y in 0..size {
            for x in 0..size {
                if code.get_module(x, y) {
                    let (left, top) = (offset + x * cell, offset + y * cell);
                    p.fill(RECT { left, top, right: left + cell, bottom: top + cell }, theme::TEXT);
                }
            }
        }
    });
    let (ssid, password) = (status.ssid.clone(), status.passphrase.clone());
    let words = window.canvas(move |p| {
        p.fill(p.rect, theme::VIEW);
        let line = |i: i32| RECT { top: px(i * 24), bottom: px(i * 24 + 24), ..p.rect };
        p.text(line(0), "Scan it with a phone's camera to join", Font::Body, theme::DIM, Align::Center);
        p.text(line(1), &ssid, Font::Strong, theme::TEXT, Align::Center);
        p.text(line(2), &format!("Password: {password}"), Font::Body, theme::TEXT, Align::Center);
    });
    let w = window.clone();
    let close = window.default_button("Close", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    place(picture, (WIDTH - CODE) / 2, 20, CODE, CODE);
    place(words, 20, 20 + CODE + 12, WIDTH - 40, 72);
    place(close, WIDTH - 20 - 96, 20 + CODE + 12 + 72 + 12, 96, 32);
    window.set_surfaces(vec![(wnd::rect(0, 0, WIDTH, 420), theme::VIEW)]);
    wnd::set_client_height(window.hwnd(), 20 + CODE + 12 + 72 + 12 + 32 + 20);
    wnd::center(window.hwnd());
    window.show();
}
