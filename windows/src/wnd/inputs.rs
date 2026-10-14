//! More inputs, as GTK's: drop-downs (whose choices may change), number fields with a decimal
//! point (speeds in Mbps), and a box of read-only text (the log).

use super::look::{self, mark};
use super::{field, set_text, text, Window};
use crate::theme;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Controls::WC_EDITW;
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    /// Drop-downs: the one picked, and the choices.
    static CHOICES: RefCell<HashMap<isize, (Rc<Cell<usize>>, Vec<String>)>> = RefCell::default();
}

impl Window {
    /// A drop-down, as GTK's on Linux: a button showing the choice, which opens the list.
    pub fn choice(&self, items: &[&str], selected: usize, on_pick: impl Fn(usize) + 'static) -> HWND {
        let items: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let (picked, button, owner) = (Rc::new(Cell::new(selected)), Rc::new(Cell::new(HWND::default())), self.hwnd());
        let (p, b) = (picked.clone(), button.clone());
        let hwnd = self.button(&items[selected.min(items.len() - 1)], move || {
            let list = CHOICES.with(|c| c.borrow().get(&(b.get().0 as isize)).map(|(_, items)| items.clone())).unwrap_or_default();
            let mut r = RECT::default();
            // SAFETY: a menu of the choices under the button; it returns the one picked (0: none)
            let chosen = unsafe {
                let menu = CreatePopupMenu().unwrap_or_default();
                for (i, item) in list.iter().enumerate() {
                    let checked = if i == p.get() { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(menu, MF_STRING | checked, i + 1, &HSTRING::from(item.as_str()));
                }
                let _ = GetWindowRect(b.get(), &mut r);
                let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN, r.left, r.bottom, None, owner, None);
                let _ = DestroyMenu(menu);
                chosen.0 as usize
            };
            if let Some((i, item)) = chosen.checked_sub(1).and_then(|i| list.get(i).map(|item| (i, item))) {
                p.set(i);
                set_text(b.get(), item);
                on_pick(i);
            }
        });
        button.set(hwnd);
        mark(hwnd, look::DROPDOWN);
        CHOICES.with(|c| c.borrow_mut().insert(hwnd.0 as isize, (picked, items)));
        hwnd
    }

    /// A number field for speeds and such (0.5, 2.5…), with − and + buttons stepping by `step`.
    pub fn decimal(&self, min: f64, max: f64, step: f64, value: f64) -> HWND {
        let edit = self.edit(&decimal_text(value), || {});
        let change = move |by: f64| {
            let now = decimal_value(edit).unwrap_or(min);
            set_text(edit, &decimal_text((((now + by) / step).round() * step).clamp(min, max)));
        };
        let buttons = [(theme::icon::REMOVE, -step), (theme::icon::ADD, step)].map(|(glyph, by)| self.icon_button(glyph, move || change(by)));
        field::make_field(edit, buttons.to_vec());
        edit
    }

    /// Text to read (and copy), several lines, scrolled to its end.
    pub fn text_area(&self, value: &str) -> HWND {
        let style = WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32);
        let (edit, _) = self.control(WC_EDITW, value, style, WINDOW_EX_STYLE(0));
        field::make_field(edit, vec![]);
        edit
    }
}

fn decimal_text(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_string()
}

/// The number in a decimal field (a comma works as the point too).
pub fn decimal_value(edit: HWND) -> Option<f64> {
    text(edit).trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// The drop-down's choice (its place in the list).
pub fn choice_index(hwnd: HWND) -> usize {
    CHOICES.with(|c| c.borrow().get(&(hwnd.0 as isize)).map_or(0, |(picked, _)| picked.get()))
}

/// Show `index` as the drop-down's choice.
pub fn set_choice(hwnd: HWND, index: usize) {
    let item = CHOICES.with(|c| c.borrow().get(&(hwnd.0 as isize)).and_then(|(picked, items)| {
        picked.set(index.min(items.len().saturating_sub(1)));
        items.get(picked.get()).cloned()
    }));
    if let Some(item) = item {
        set_text(hwnd, &item);
    }
}

/// New choices for a drop-down, `index` picked.
pub fn set_choices(hwnd: HWND, items: &[String], index: usize) {
    CHOICES.with(|c| {
        if let Some((_, list)) = c.borrow_mut().get_mut(&(hwnd.0 as isize)) {
            *list = items.to_vec();
        }
    });
    set_choice(hwnd, index);
}
