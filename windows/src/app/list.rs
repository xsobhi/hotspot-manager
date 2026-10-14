//! The device list, drawn as GTK draws a sortable list on Linux Mint: a row of column names (click
//! one to sort by it), then a row per device: its state, name and address, speed now, speed
//! limit and data used.

use super::rows::{Dot, Row};
use crate::theme::{self, measure, Align, Font, Painter};
use crate::wnd::px;
use windows::Win32::Foundation::RECT;

pub const HEAD: i32 = 30;
pub const ROW: i32 = 50;
pub const NAMES: [&str; 5] = ["", "Device", "Using now", "Speed limit", "Data used"];

pub struct View {
    pub rows: Vec<Row>,
    pub hover: Option<usize>,
    pub scroll: usize,
    pub sort: (usize, bool), // column, high to low
    pub empty: String,
}

impl Default for View {
    fn default() -> View {
        View { rows: vec![], hover: None, scroll: 0, sort: (1, false), empty: String::new() } // by name, as on Linux
    }
}

/// Each column's left and right (canvas pixels, `width` wide).
pub fn columns(width: i32) -> [(i32, i32); 5] {
    let right = width - px(24);
    let data = (right - px(84), right);
    let limit = (data.0 - px(214), data.0 - px(12));
    let now = (limit.0 - px(100), limit.0 - px(12));
    [(px(24), px(40)), (px(48), now.0 - px(12)), now, limit, data]
}

/// Which column's name is at `x`.
pub fn column_at(width: i32, x: i32) -> Option<usize> {
    columns(width).iter().position(|(l, r)| x >= *l && x < *r).filter(|&i| i > 0)
}

/// The row (index into the rows) at `y`.
pub fn row_at(view: &View, y: i32) -> Option<usize> {
    (y >= px(HEAD)).then(|| ((y - px(HEAD)) / px(ROW)) as usize + view.scroll).filter(|&i| i < view.rows.len())
}

pub fn sort(rows: &mut [Row], (column, high_first): (usize, bool)) {
    rows.sort_by(|a, b| {
        let order = match column {
            0 => (a.dot as u8).cmp(&(b.dot as u8)),
            2 => (a.down + a.up).total_cmp(&(b.down + b.up)),
            3 => a.limit.cmp(&b.limit),
            4 => a.used.unwrap_or(0).cmp(&b.used.unwrap_or(0)),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        if high_first { order.reverse() } else { order }
    });
}

pub fn paint(p: &Painter, view: &View) {
    p.fill(p.rect, theme::VIEW);
    let cols = columns(p.rect.right);
    let head = RECT { bottom: px(HEAD), ..p.rect };
    for (i, (left, right)) in cols.iter().enumerate() {
        let mut name = NAMES[i].to_string();
        if view.sort.0 == i && i > 0 {
            name.push_str(if view.sort.1 { "  ▾" } else { "  ▴" });
        }
        let align = if i == 4 { Align::Right } else { Align::Left };
        p.text(RECT { left: *left, right: *right, ..head }, &name, Font::Body, theme::DIM, align);
    }
    p.fill(RECT { top: head.bottom - 1, ..head }, theme::BORDER);
    if view.rows.is_empty() {
        let r = RECT { top: px(HEAD + 30), bottom: px(HEAD + 60), ..p.rect };
        p.text(r, &view.empty, Font::Body, theme::DIM, Align::Center);
        return;
    }
    for (i, row) in view.rows.iter().enumerate().skip(view.scroll) {
        let top = px(HEAD) + (i - view.scroll) as i32 * px(ROW);
        if top >= p.rect.bottom {
            break;
        }
        let r = RECT { left: 0, top, right: p.rect.right, bottom: top + px(ROW) };
        if view.hover == Some(i) {
            p.fill(r, theme::hover(theme::VIEW));
        }
        p.fill(RECT { left: px(24), top: r.bottom - 1, right: r.right - px(24), ..r }, theme::SEPARATOR);
        paint_row(p, row, r, &cols);
    }
}

fn paint_row(p: &Painter, row: &Row, r: RECT, cols: &[(i32, i32); 5]) {
    let cell = |i: usize, top: i32, bottom: i32| RECT { left: cols[i].0, right: cols[i].1, top: r.top + px(top), bottom: r.top + px(bottom) };
    // ● on, ○ off, ⛔ blocked
    let middle = (r.top + r.bottom) / 2;
    let dot = RECT { left: cols[0].0, top: middle - px(6), right: cols[0].0 + px(12), bottom: middle + px(6) };
    match row.dot {
        Dot::Online => p.round(dot, px(6), Some(theme::ACCENT), None),
        Dot::Offline => p.round(dot, px(6), None, Some(theme::DIM)),
        Dot::Blocked => {
            p.round(dot, px(6), Some(theme::DANGER), None);
            p.fill(RECT { left: dot.left + px(3), right: dot.right - px(3), top: middle - px(1), bottom: middle + px(1) }, theme::WHITE);
        }
    }
    let faded = row.dot == Dot::Offline;
    p.text(cell(1, 7, 27), &row.name, Font::Strong, if faded { theme::DIM } else { theme::TEXT }, Align::Left);
    p.text(cell(1, 27, 44), &row.sub, Font::Caption, theme::DIM, Align::Left);
    if let Some((down, up)) = row.now_text() {
        p.text(cell(2, 7, 25), &down, Font::Caption, theme::TEXT, Align::Left);
        p.text(cell(2, 25, 43), &up, Font::Caption, theme::TEXT, Align::Left);
    }
    let limit_font = if row.own_limit { Font::Strong } else { Font::Body };
    let (limit, everyone) = match row.limit.strip_suffix(" (everyone)") {
        Some(speed) => (speed, true),
        None => (row.limit.as_str(), false),
    };
    let limit_cell = cell(3, 0, 50);
    p.text(limit_cell, limit, limit_font, theme::TEXT, Align::Left);
    if everyone {
        let after = RECT { left: limit_cell.left + measure(limit, limit_font) + px(4), ..limit_cell };
        p.text(after, "(everyone)", Font::Caption, theme::DIM, Align::Left);
    }
    p.text(cell(4, 0, 50), &row.used_text(), Font::Body, theme::TEXT, Align::Right);
}
