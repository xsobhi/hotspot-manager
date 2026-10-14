//! "Speed for everyone" (Custom…): each device's own lane, with bursts if wanted, and a cap for
//! the whole hotspot, as the Linux app's dialog.

use super::editor::Editor;
use super::App;
use crate::rules::{fmt_speed, Limit};
use crate::theme::{self, Font};
use crate::wnd::{self, is_checked, place, Rows, Window};
use std::rc::Rc;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 600;

pub fn open(app: &Rc<App>) {
    let Some(rules) = app.rules.borrow().clone() else { return };
    let window = Window::dialog("Speed for everyone", Some(app.window.hwnd()), (WIDTH, 400), false);
    let heading = window.text_label("Each device", Font::Strong, wnd::labels::Kind::Line);
    let note = window.label("Every device gets its own lane at this speed.");
    window.set_text_color(note, theme::DIM);
    let each = Editor::new(&window, &rules.everyone, true);
    let capped = !rules.total.is_none();
    let total = Editor::new(&window, &rules.total, false);
    let cap_note = window.label("All devices together share at most this much.");
    window.set_text_color(cap_note, theme::DIM);
    let (t, n) = (total.clone(), cap_note);
    let cap = window.checkbox("Cap for the whole hotspot", capped, move |on| {
        t.show(on);
        wnd::set_visible(n, on);
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let (a, w, e, t) = (app.clone(), window.clone(), each.clone(), total.clone());
    let apply = window.default_button("Apply", move || {
        let (each, total) = (e.get(), if is_checked(cap) { t.get() } else { Limit::default() });
        let said = format!("Every device now gets {}.", fmt_speed(&each).to_lowercase());
        a.change_rules(said, |r| {
            r.everyone = each;
            r.total = total;
        });
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());

    place(heading, 20, 18, WIDTH - 40, 20);
    place(note, 20, 40, WIDTH - 40, 20);
    let y = each.place(20, 68);
    place(cap, 20, y + 8, 300, 26);
    place(cap_note, 44, y + 38, WIDTH - 64, 20);
    let y = total.place(44, y + 64);
    total.show(capped);
    wnd::set_visible(cap_note, capped);
    let mut rows = Rows::new(20, y + 10, 0, WIDTH - 40);
    let bottom = rows.buttons(&[cancel, apply]);
    wnd::set_client_height(window.hwnd(), bottom + 18);
    wnd::center(window.hwnd());
    window.show();
}
