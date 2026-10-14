//! The limits in words, under "Speed for everyone", as the Linux app writes them: "Each device
//! gets its own lane at this speed · whole hotspot capped at … · 2 devices have their own speed
//! (use this speed for them too)", with that last part a link; or why there are no limits.

use super::App;
use crate::rules::{fmt_speed, Rules};
use crate::theme::{self, measure, Align, Font, Painter};
use crate::wnd::px;
use windows::Win32::Foundation::{COLORREF, RECT};

#[derive(Clone, Copy)]
pub enum Link {
    ResetOwn,
    Setup,
    Log,
}

/// The line of words under the bar: (text, colour, a link?).
pub type Words = Vec<(String, COLORREF, Option<Link>)>;

/// The limits in words, as on Linux: "Each device gets its own lane at this speed · …".
pub fn words(app: &App, rules: Option<&Rules>) -> Words {
    let dim = theme::DIM;
    let Some(r) = rules else {
        return vec![("Speed limits, data used and blocking need the Hotspot Manager service.  ".into(), dim, None),
                    ("Set it up…".into(), theme::ACCENT, Some(Link::Setup))];
    };
    if let Ok(h) = app.helper.borrow().as_ref() && !h.engine.is_empty() {
        return vec![(format!("{}  ", h.engine), theme::DANGER, None), ("View log".into(), theme::ACCENT, Some(Link::Log))];
    }
    let mut out: Words = vec![];
    let each = if r.everyone.is_none() { "Devices aren't limited" } else { "Each device gets its own lane at this speed" };
    out.push((each.into(), dim, None));
    if !r.total.is_none() {
        out.push((format!(" · whole hotspot capped at {}", fmt_speed(&r.total)), dim, None));
    }
    let own = r.devices.len();
    if own > 0 {
        let (has, them) = if own == 1 { ("1 device has its own speed", "it") } else { ("devices have their own speed", "them") };
        let lead = if own == 1 { format!(" · {has} (") } else { format!(" · {own} {has} (") };
        out.push((lead, dim, None));
        out.push((format!("use this speed for {them} too"), theme::ACCENT, Some(Link::ResetOwn)));
        out.push((")".into(), dim, None));
    }
    out.push((".".into(), dim, None));
    out
}

pub fn paint(p: &Painter, line: &mut (Words, Vec<(RECT, Link)>)) {
    p.fill(p.rect, theme::VIEW);
    let mut x = 0;
    line.1.clear();
    for (part, color, link) in &line.0 {
        let width = measure(part, Font::Body);
        let r = RECT { left: x, right: (x + width + px(2)).min(p.rect.right), ..p.rect };
        p.text(r, part, Font::Body, *color, Align::Left);
        if let Some(link) = link {
            line.1.push((r, *link));
        }
        x += width;
    }
}
