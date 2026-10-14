//! Settings → Advanced, as the Linux app's: Wi-Fi (channel, width and mode from the adapter's
//! driver (radio.rs), country, max devices, hidden, devices kept apart), Network (the hotspot's
//! address, the DHCP range, lease time, DNS servers) and Troubleshooting. What Windows decides
//! itself is shown, greyed, with why.

use super::radio::{self, Found, Radio};
use super::apply::{move_hotspot, set_driver};
use super::{logview, setup, App};
use crate::helper::dhcp::parse_ip;
use crate::rules::LEASES;
use crate::theme::{self, Font};
use crate::wnd::{self, choice_index, enable, is_checked, place, set_text, text, Window, FIELD, LINE};
use crate::{client, net, worker};
use std::cell::Cell;
use std::rc::Rc;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

const WIDTH: i32 = 640;
const NAMES: i32 = 190;
const X: i32 = 20 + NAMES + 12;

pub fn ip_text(ip: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

/// Open it once the adapter's driver settings are known (PowerShell takes a moment).
pub fn open(app: &Rc<App>) {
    let chosen = app.store.borrow().adapter_id();
    app.say("Reading the Wi-Fi adapter's settings…", false);
    worker::run(move || {
        let found = radio::find(chosen);
        super::post(move |app| show(app, found));
    });
}

fn show(app: &Rc<App>, found: Found) {
    let rules = app.rules.borrow().clone();
    let net_rules = rules.as_ref().map(|r| r.network.clone()).unwrap_or_default();
    let gateway = net::gateway();
    let window = Window::dialog("Advanced settings", Some(app.window.hwnd()), (WIDTH, 640), false);
    let heading = |t: &str| window.text_label(t, Font::Strong, wnd::labels::Kind::Line);
    let dim = |t: &str| {
        let note = window.note(t);
        window.set_text_color(note, theme::DIM);
        note
    };
    let wifi = heading("Wi-Fi");
    let radio = Rc::new(Radio::new(&window, &found));
    let country = window.edit("From Windows' region settings", || {});
    let max = window.spin(0, 8, rules.as_ref().map_or(0, |r| r.max_devices as i32), |_| {});
    let hidden = window.checkbox("Hide network name", false, |_| {});
    let isolate = window.checkbox("Stop connected devices from seeing each other", rules.as_ref().is_some_and(|r| r.isolate), |_| {});
    let windows_note = dim("Windows' hotspot takes the country from Windows' region, can't hide its name, and takes up to 8 devices.");
    let network = heading("Network");
    let network_note = dim("Devices get an address from the DHCP range, with the hotspot IP as their gateway. Windows' \
        own hotspot is always at 192.168.137.1: at another address, this app hosts it itself.");
    let range = |n: u8| ip_text([gateway[0], gateway[1], gateway[2], n]);
    let (from, to) = (window.edit(&range(net_rules.dhcp_from), || {}), window.edit(&range(net_rules.dhcp_to), || {}));
    let me = Rc::new(Cell::new(HWND::default()));
    let m = me.clone();
    let gw = window.edit(&ip_text(gateway), move || {
        // the range moves along with the hotspot's address, as on Linux
        let Some(g) = parse_ip(&text(m.get())) else { return };
        let inside = |edit: HWND| parse_ip(&text(edit)).is_some_and(|ip| ip[..3] == g[..3]);
        if !(inside(from) && inside(to)) {
            let (first, last) = if g[3] < 254 { ((g[3] as u16 + 9).min(254) as u8, 254) } else { (1, 253) };
            set_text(from, &ip_text([g[0], g[1], g[2], first]));
            set_text(to, &ip_text([g[0], g[1], g[2], last]));
        }
    });
    me.set(gw);
    // (Windows' own hotspot, turned on elsewhere, stays at 192.168.137.1)
    let now_at = net::hotspot_adapter().and_then(|a| a.ipv4.iter().copied().find(|ip| ip[..2] != [169, 254]))
        .filter(|ip| *ip != gateway);
    let kept = dim(&now_at.map(|ip| format!("The hotspot is at {} now: it moves to {} when it's turned off and on here.",
                                            ip_text(ip), ip_text(gateway))).unwrap_or_default());
    window.set_text_color(kept, theme::DANGER);
    let to_word = window.label("to");
    let mask = window.choice(&["255.255.255.0   (/24)"], 0, |_| {});
    let lease_names: Vec<&str> = LEASES.iter().map(|(_, n)| *n).collect();
    let lease = window.choice(&lease_names, LEASES.iter().position(|(h, _)| *h == net_rules.lease_hours).unwrap_or(2), |_| {});
    let dns = window.edit(&net_rules.dns.join(", "), || {});
    wnd::send(dns, 0x1501, 1, windows::core::HSTRING::from("Automatic").as_ptr() as isize);
    let trouble = heading("Troubleshooting");
    let a = app.clone();
    let log = window.button("View log…", move || logview::open(&a));
    let a = app.clone();
    let service = window.button(if rules.is_some() { "Reinstall the service…" } else { "Set up the service…" }, move || setup::run(&a));
    let missing = dim(&format!("{}: max devices, keeping devices apart and the network settings need it.", client::MISSING));
    window.set_text_color(missing, theme::DANGER);

    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let (a, w, r) = (app.clone(), window.clone(), radio.clone());
    let save = window.default_button("Save", move || {
        let parsed = (parse_ip(&text(gw)), parse_ip(&text(from)), parse_ip(&text(to)));
        let (Some(g), Some(f), Some(t)) = parsed else {
            return a.say("The hotspot IP and the DHCP range need IP addresses like 192.168.137.1", true);
        };
        if f[..3] != g[..3] || t[..3] != g[..3] || f[3] > t[3] {
            return a.say("The DHCP range must be in the hotspot's subnet (the same first three numbers), low to high", true);
        }
        let servers: Vec<String> = text(dns).replace([',', ';'], " ").split_whitespace().map(String::from).collect();
        let lease_hours = LEASES[choice_index(lease)].0;
        let (max_devices, apart) = (text(max).parse().unwrap_or(0), is_checked(isolate));
        a.change_rules("Saved.", |r| {
            (r.max_devices, r.isolate) = (max_devices, apart);
            (r.network.gateway, r.network.dhcp_from, r.network.dhcp_to) = (ip_text(g), f[3], t[3]);
            (r.network.lease_hours, r.network.dns) = (lease_hours, servers);
        });
        if g != net::gateway() {
            move_hotspot(&a, g);
        }
        let driver = r.changes();
        if !driver.is_empty() {
            set_driver(&a, driver);
        }
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());

    // the form: headings, then names on the left and their settings on the right
    let mut y = 18;
    let name = |label: &str, y: i32| place(window.name(label), 20, y + (FIELD - LINE) / 2, NAMES, LINE);
    let row = |label: &str, control, width, y: &mut i32| {
        name(label, *y);
        place(control, X, *y, width, FIELD);
        *y += FIELD + 10;
    };
    place(wifi, 20, y, 200, 22);
    y += 30;
    y = radio.place(20, NAMES, X, WIDTH, y);
    row("Country", country, 300, &mut y);
    row("Max devices (0 = no limit)", max, 140, &mut y);
    place(hidden, X, y, 380, 26);
    place(isolate, X, y + 28, 400, 26);
    y += 62;
    place(windows_note, X, y, WIDTH - X - 20, 2 * LINE + 4);
    y += 2 * LINE + 18;
    place(network, 20, y, 200, 22);
    place(network_note, 20, y + 24, WIDTH - 40, 2 * LINE + 4);
    y += 52 + LINE;
    row("Hotspot IP (gateway)", gw, 180, &mut y);
    if now_at.is_some() {
        place(kept, X, y - 6, WIDTH - X - 20, 2 * LINE + 4);
        y += 2 * LINE;
    } else {
        wnd::set_visible(kept, false);
    }
    row("Subnet mask", mask, 220, &mut y);
    name("DHCP range", y);
    place(from, X, y, 160, FIELD);
    place(to_word, X + 168, y + (FIELD - LINE) / 2, 24, LINE);
    place(to, X + 196, y, 160, FIELD);
    y += FIELD + 10;
    row("Lease time", lease, 180, &mut y);
    row("DNS servers", dns, 300, &mut y);
    place(trouble, 20, y + 6, 200, 22);
    y += 36;
    place(log, X, y, 130, FIELD + 2);
    place(service, X + 140, y, 200, FIELD + 2);
    y += FIELD + 16;
    if rules.is_none() {
        place(missing, 20, y, WIDTH - 40, 2 * LINE + 4);
        y += 2 * LINE + 12;
    } else {
        wnd::set_visible(missing, false);
    }
    place(cancel, WIDTH - 20 - 2 * 104 - 8, y, 104, FIELD + 2);
    place(save, WIDTH - 20 - 104, y, 104, FIELD + 2);
    wnd::set_client_height(window.hwnd(), y + FIELD + 2 + 18);
    for fixed in [country, hidden, mask] {
        enable(fixed, false);
    }
    for needs_helper in [max, isolate, gw, from, to, lease, dns, save] {
        enable(needs_helper, rules.is_some());
    }
    wnd::center(window.hwnd());
    window.show();
}
