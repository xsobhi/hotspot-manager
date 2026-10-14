//! "View log…": what the helper service has done and why something failed, as the Linux app shows
//! hostapd's log; and a check of where the hotspot's traffic shows up, for when limits don't work.

use super::App;
use crate::wnd::{self, place, set_text, Window, FIELD};
use crate::{client, worker};
use std::rc::Rc;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 760;
const HEIGHT: i32 = 460;

fn load(area: HWND) {
    let area = area.0 as isize;
    worker::run(move || {
        let text = match client::log() {
            Ok(lines) if lines.is_empty() => "Nothing yet.".to_string(),
            Ok(lines) => lines.join("\r\n"),
            Err(why) => why,
        };
        super::post(move |_| {
            let area = HWND(area as *mut _);
            set_text(area, &text);
            wnd::send(area, 0x00B1, usize::MAX, -1); // EM_SETSEL: to the end
            wnd::send(area, 0x00B7, 0, 0); // EM_SCROLLCARET
        });
    });
}

pub fn open(app: &Rc<App>) {
    let window = Window::dialog("Hotspot log", Some(app.window.hwnd()), (WIDTH, HEIGHT), false);
    let area = window.text_area("Loading…");
    let a = app.clone();
    let check = window.button("Check the traffic", move || {
        let result = client::diagnose();
        match result {
            Ok(()) => a.say("Watching the hotspot's traffic for 10 seconds: then Refresh the log.", false),
            Err(why) => a.say(why, true),
        }
    });
    let refresh = window.button("Refresh", move || load(area));
    let w = window.clone();
    let close = window.default_button("Close", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    place(area, 16, 16, WIDTH - 32, HEIGHT - 32 - FIELD - 14);
    let y = HEIGHT - 16 - FIELD - 2;
    place(check, 16, y, 160, FIELD + 2);
    place(refresh, WIDTH - 16 - 2 * 104 - 8, y, 104, FIELD + 2);
    place(close, WIDTH - 16 - 104, y, 104, FIELD + 2);
    wnd::set_client_height(window.hwnd(), HEIGHT);
    wnd::center(window.hwnd());
    window.show();
    load(area);
}
