//! One window for everything about a device, as the Linux app's: what's known about it, its
//! name, its speed, a fixed address; and Disconnect, Block / Unblock and Forget, which happen
//! right away.

use super::editor::Editor;
use super::rows::{ago, name, span};
use super::App;
use crate::rules::{fmt_limit, fmt_speed, human_bytes, Limit, PRESETS};
use crate::store::now;
use crate::theme;
use crate::wnd::{self, choice_index, enable, is_checked, place, send, set_choice, set_visible, text, Window, FIELD, LINE};
use crate::{client, vendor};
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 600;
const EM_SETCUEBANNER: u32 = 0x1501;

pub fn open(app: &Rc<App>, mac: &str) {
    let Some(device) = app.store.borrow().devices.get(mac).cloned() else { return };
    let rules = app.rules.borrow().clone();
    let title = name(mac, &device);
    let status = app.status.borrow().clone();
    let client_now = status.as_ref().and_then(|s| s.as_ref().ok()).and_then(|s| s.clients.iter().find(|c| c.mac == mac).cloned());
    let online = client_now.is_some();
    let ip_now = client_now.as_ref().and_then(|c| c.ip.clone()).unwrap_or_default();
    let used = app.helper.borrow().as_ref().ok().and_then(|h| h.used(mac, &ip_now));
    let mut facts = if online {
        let since = app.helper.borrow().as_ref().ok().and_then(|h| h.since(mac)).or(device.since).unwrap_or(now());
        vec![format!("Connected for {}", span(now().saturating_sub(since))), ip_now,
             used.map(|u| format!("used {}", human_bytes(u))).unwrap_or_default()]
    } else {
        vec![format!("Offline, last seen {}", ago(device.last_seen)), device.ip.clone()]
    };
    facts.extend([vendor::of(mac).to_string(), mac.to_string()]);
    let facts: Vec<String> = facts.into_iter().filter(|f| !f.is_empty()).collect();

    let window = Window::dialog(&title, Some(app.window.hwnd()), (WIDTH, 420), false);
    let about = window.note(&facts.join("  ·  "));
    window.set_text_color(about, theme::DIM);
    let labels = ["Name", "Speed"].map(|l| window.name(l));
    let name_field = window.edit(&device.name, || {});
    let hint = if device.host.is_empty() { "e.g. Ahmed's phone".to_string() } else { device.host.clone() };
    send(name_field, EM_SETCUEBANNER, 1, HSTRING::from(hint).as_ptr() as isize);
    let everyone = rules.as_ref().map(|r| r.everyone).unwrap_or_default();
    let own = rules.as_ref().and_then(|r| r.devices.get(mac).copied());
    let mut items = vec![format!("Same as everyone ({})", fmt_speed(&everyone)), "No limit".to_string()];
    items.extend(PRESETS.iter().map(|k| fmt_limit(*k)));
    items.push("Custom…".into());
    let custom_at = items.len() - 1;
    let picked = match own {
        None => 0,
        Some(l) if l.is_none() => 1,
        Some(l) => PRESETS.iter().position(|k| l == Limit::both(*k)).map_or(custom_at, |i| i + 2),
    };
    let start = own.filter(|l| !l.is_none()).unwrap_or(if everyone.is_none() { Limit { down: 10000, up: 5000, ..Default::default() } } else { everyone });
    let editor = Editor::new(&window, &start, true);
    let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let layout_slot: Rc<std::cell::RefCell<Option<Box<dyn Fn()>>>> = Rc::default();
    let l = layout_slot.clone();
    let speed = window.choice(&item_refs, picked, move |_| if let Some(f) = l.borrow().as_ref() { f() });
    let fixed_ip = rules.as_ref().and_then(|r| r.network.fixed.get(mac).cloned());
    let fixed = window.checkbox("Fixed IP address: always give it", fixed_ip.is_some(), |_| {});
    let ip = window.edit(fixed_ip.as_deref().unwrap_or(client_now.as_ref().and_then(|c| c.ip.as_deref()).unwrap_or(&device.ip)), || {});
    let blocked = rules.as_ref().is_some_and(|r| r.blocked.contains(mac));

    let (a, w, m, n) = (app.clone(), window.clone(), mac.to_string(), title.clone());
    let disconnect = window.button("Disconnect", move || {
        let said = format!("Disconnected {n}: its connection stops for a minute (Windows can't push a device off its hotspot).");
        a.change_rules(said, |r| { r.cut.insert(m.clone(), now() + 60); });
        w.destroy();
    });
    let (a, w, m, n) = (app.clone(), window.clone(), mac.to_string(), title.clone());
    let block = window.button(if blocked { "Unblock" } else { "Block" }, move || {
        let said = if blocked { format!("Unblocked {n}.") } else { format!("Blocked {n}. It gets no connection until you unblock it.") };
        a.change_rules(said, |r| if blocked { r.blocked.remove(&m); } else { r.blocked.insert(m.clone()); });
        w.destroy();
    });
    if !blocked {
        wnd::look::mark(block, wnd::look::DANGER);
    }
    let (a, w, m, n) = (app.clone(), window.clone(), mac.to_string(), title.clone());
    let forget = window.button("Forget", move || {
        a.store.borrow_mut().devices.remove(&m);
        a.store.borrow().save();
        if a.rules.borrow().is_some() {
            a.change_rules(format!("Removed {n} from the list."), |r| {
                r.devices.remove(&m);
                r.blocked.remove(&m);
                r.network.fixed.remove(&m);
            });
        } else {
            a.say(format!("Removed {n} from the list."), false);
            a.update();
        }
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let (a, w, m, e) = (app.clone(), window.clone(), mac.to_string(), editor.clone());
    let save = window.default_button("Save", move || {
        let given = text(name_field).trim().to_string();
        if let Some(d) = a.store.borrow_mut().devices.get_mut(&m) {
            d.name = given.clone();
        }
        a.store.borrow().save();
        let shown = if given.is_empty() { name(&m, &a.store.borrow().devices.get(&m).cloned().unwrap_or_default()) } else { given };
        if a.rules.borrow().is_none() {
            a.say(format!("Saved {shown}."), false);
            a.update();
            return w.destroy();
        }
        let choice = choice_index(speed);
        let limit = match choice {
            0 => None,
            1 => Some(Limit::default()),
            i if i == custom_at => Some(e.get()),
            i => Some(Limit::both(PRESETS[i - 2])),
        };
        let address = is_checked(fixed).then(|| text(ip).trim().to_string());
        a.change_rules(format!("Saved {shown}."), |r| {
            match limit {
                Some(l) => r.devices.insert(m.clone(), l),
                None => r.devices.remove(&m),
            };
            match address {
                Some(ip) => r.network.fixed.insert(m.clone(), ip),
                None => r.network.fixed.remove(&m),
            };
        });
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let missing = window.note(&format!("{}: its speed, a fixed address and blocking need it (Settings → Advanced).", client::MISSING));
    window.set_text_color(missing, theme::DANGER);

    // lay out again when "Custom…" shows or hides the speed fields
    let hwnd = window.hwnd();
    let (e, has_rules) = (editor.clone(), rules.is_some());
    let layout = move || {
        let custom = choice_index(speed) == custom_at;
        let mut y = 18;
        place(about, 20, y, WIDTH - 40, 2 * LINE + 4);
        y += 2 * LINE + 16;
        for (label, field, width) in [(labels[0], name_field, 300), (labels[1], speed, 230)] {
            place(label, 20, y + (FIELD - LINE) / 2, 70, LINE);
            place(field, 100, y, width, FIELD);
            y += FIELD + 12;
        }
        e.show(custom);
        if custom {
            y = e.place(20, y) + 4;
        }
        place(fixed, 20, y + 2, 250, 26);
        place(ip, 280, y, 170, FIELD);
        y += FIELD + 18;
        let actions: Vec<_> = [(disconnect, online), (block, true), (forget, !online)].into_iter().filter(|(_, on)| *on).collect();
        let mut x = 20;
        for (button, _) in &actions {
            place(*button, x, y, 110, FIELD + 2);
            x += 118;
        }
        set_visible(disconnect, online);
        set_visible(forget, !online);
        y += FIELD + 2 + 14;
        set_visible(missing, !has_rules);
        if !has_rules {
            place(missing, 20, y, WIDTH - 40, 2 * LINE + 4);
            y += 2 * LINE + 12;
        }
        place(cancel, WIDTH - 20 - 2 * 104 - 8, y, 104, FIELD + 2);
        place(save, WIDTH - 20 - 104, y, 104, FIELD + 2);
        wnd::set_client_height(hwnd, y + FIELD + 2 + 18);
    };
    layout();
    *layout_slot.borrow_mut() = Some(Box::new(layout));
    for control in [speed, fixed, ip, disconnect, block] {
        enable(control, rules.is_some());
    }
    set_choice(speed, picked);
    wnd::center(window.hwnd());
    window.show();
}
