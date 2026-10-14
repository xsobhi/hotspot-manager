//! A speed limit to edit, as the Linux app's LimitEditor: download and upload in Mbps (0 = no
//! limit), and, if wanted, short bursts above them ("pages load fast, then back to the limit").

use crate::rules::Limit;
use crate::wnd::{decimal_value, enable, is_checked, place, set_visible, text, Window, FIELD, LINE};
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

pub struct Editor {
    down: HWND,
    up: HWND,
    burst: Option<HWND>, // its check box
    burst_down: HWND,
    burst_up: HWND,
    secs: HWND,
    words: Vec<HWND>,
    burst_row: Vec<HWND>,
}

impl Editor {
    pub fn new(window: &Window, limit: &Limit, bursts: bool) -> Rc<Editor> {
        let mbps = |kbit: u32| kbit as f64 / 1000.0;
        let down = window.decimal(0.0, 1000.0, 0.5, mbps(limit.down));
        let up = window.decimal(0.0, 1000.0, 0.5, mbps(limit.up));
        let words: Vec<HWND> = ["Download", "Upload", "Mbps  (0 = no limit)"].iter().map(|w| window.label(w)).collect();
        let on = limit.bursts();
        let none = HWND::default();
        let (burst_down, burst_up, secs, burst_row) = if bursts {
            let burst_down = window.decimal(0.0, 1000.0, 0.5, mbps(if on { limit.burst_down } else { limit.down * 2 }));
            let burst_up = window.decimal(0.0, 1000.0, 0.5, mbps(if on { limit.burst_up } else { limit.up * 2 }));
            let secs = window.spin(1, 60, if on { limit.burst_secs.max(1) as i32 } else { 2 }, |_| {});
            let mut row: Vec<HWND> = ["Up to ↓", "↑", "Mbps for", "seconds"].iter().map(|w| window.label(w)).collect();
            row.extend([burst_down, burst_up, secs]);
            (burst_down, burst_up, secs, row)
        } else {
            (none, none, none, vec![])
        };
        let editor = Rc::new_cyclic(|me: &std::rc::Weak<Editor>| {
            let me = me.clone();
            let burst = bursts.then(|| window.checkbox("Allow short bursts (pages load fast, then back to the limit)", on,
                                                       move |_| if let Some(e) = me.upgrade() { e.sync() }));
            Editor { down, up, burst, burst_down, burst_up, secs, words, burst_row }
        });
        editor.sync();
        editor
    }

    /// Lay it out from (x, y); returns the y under it.
    pub fn place(&self, x: i32, y: i32) -> i32 {
        let label = |i: usize, at: i32, width: i32| place(self.words[i], at, y + (FIELD - LINE) / 2, width, LINE);
        label(0, x, 74);
        place(self.down, x + 76, y, 104, FIELD);
        label(1, x + 194, 56);
        place(self.up, x + 252, y, 104, FIELD);
        label(2, x + 368, 160);
        let Some(burst) = self.burst else { return y + FIELD + 8 };
        place(burst, x, y + FIELD + 8, 480, 26);
        let row = y + 2 * (FIELD + 8);
        let (x, words) = (x + 24, &self.burst_row);
        place(words[0], x, row + (FIELD - LINE) / 2, 58, LINE);
        place(self.burst_down, x + 60, row, 94, FIELD);
        place(words[1], x + 162, row + (FIELD - LINE) / 2, 14, LINE);
        place(self.burst_up, x + 178, row, 94, FIELD);
        place(words[2], x + 280, row + (FIELD - LINE) / 2, 64, LINE);
        place(self.secs, x + 346, row, 80, FIELD);
        place(words[3], x + 434, row + (FIELD - LINE) / 2, 64, LINE);
        row + FIELD + 8
    }

    pub fn show(&self, on: bool) {
        for hwnd in [self.down, self.up].iter().chain(&self.words).chain(&self.burst).chain(&self.burst_row) {
            set_visible(*hwnd, on);
        }
    }

    /// The burst fields only while bursts are allowed.
    fn sync(&self) {
        let on = self.burst.is_some_and(is_checked);
        for hwnd in &self.burst_row {
            enable(*hwnd, on);
        }
    }

    pub fn get(&self) -> Limit {
        let kbit = |edit: HWND| (decimal_value(edit).unwrap_or(0.0) * 1000.0).round() as u32;
        let (down, up) = (kbit(self.down), kbit(self.up));
        let mut limit = Limit { down, up, ..Default::default() };
        if self.burst.is_some_and(is_checked) {
            limit.burst_down = if down > 0 { kbit(self.burst_down).max(down) } else { 0 };
            limit.burst_up = if up > 0 { kbit(self.burst_up).max(up) } else { 0 };
            limit.burst_secs = text(self.secs).parse().unwrap_or(2);
        }
        limit
    }
}
