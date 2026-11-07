//! Box layout primitives. `y` grows downward and the baseline of every layout is `y = 0`, so the things above the
//! baseline have negative `y`. Colours are not decided here: items carry a [`Role`] and the painter maps it to a theme.

use std::cell::Cell;

/// Width of text in the math font. The painter implements this with the real font; tests use a fixed width.
pub trait Metrics {
    fn advance(&self, text: &str, size: f32) -> f32;
}

/// What an item is, so the painter can colour it from the current theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Text,
    Variable,
    Number,
    Operator,
    Function,
    Constant,
    Delim,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Text whose baseline-left corner is at `(x, y)`.
    Text { x: f32, y: f32, text: String, size: f32, role: Role },
    /// A filled rectangle with its top-left corner at `(x, y)`.
    Rule { x: f32, y: f32, w: f32, h: f32, role: Role },
    /// An open polyline.
    Stroke { points: Vec<[f32; 2]>, width: f32, role: Role },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub width: f32,
    /// Extent above the baseline.
    pub ascent: f32,
    /// Extent below the baseline.
    pub descent: f32,
    pub items: Vec<Item>,
}

impl Layout {
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// Add `other` with its baseline origin at `(dx, dy)` and grow the bounds to hold it.
    pub fn push(&mut self, other: &Layout, dx: f32, dy: f32) {
        for item in &other.items {
            self.items.push(match item {
                Item::Text { x, y, text, size, role } => Item::Text { x: x + dx, y: y + dy, text: text.clone(), size: *size, role: *role },
                Item::Rule { x, y, w, h, role } => Item::Rule { x: x + dx, y: y + dy, w: *w, h: *h, role: *role },
                Item::Stroke { points, width, role } => {
                    Item::Stroke { points: points.iter().map(|[px, py]| [px + dx, py + dy]).collect(), width: *width, role: *role }
                }
            });
        }
        self.width = self.width.max(dx + other.width);
        self.ascent = self.ascent.max(other.ascent - dy);
        self.descent = self.descent.max(other.descent + dy);
    }
}

/// Which delimiter pair surrounds something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delim {
    Paren,
    Bracket,
    Brace,
    Bar,
}

// Proportions of the type size, tuned for STIX Two Math.
const AXIS: f32 = 0.26;
const RULE: f32 = 0.06;
const TEXT_ASCENT: f32 = 0.72;
const TEXT_DESCENT: f32 = 0.22;
const SCRIPT: f32 = 0.7;

/// Layout state: the font metrics and a budget that bounds the work done for one output.
pub(crate) struct Tx<'a> {
    pub m: &'a dyn Metrics,
    budget: Cell<i32>,
}

impl<'a> Tx<'a> {
    pub fn new(m: &'a dyn Metrics, budget: i32) -> Self {
        Tx { m, budget: Cell::new(budget) }
    }

    /// Spend one unit of the layout budget; `None` once it is used up.
    pub fn tick(&self) -> Option<()> {
        let left = self.budget.get() - 1;
        self.budget.set(left);
        (left >= 0).then_some(())
    }

    pub fn script(size: f32) -> f32 {
        (size * SCRIPT).max(6.5)
    }

    pub fn text(&self, s: &str, size: f32, role: Role, descender: bool) -> Layout {
        let width = self.m.advance(s, size);
        let item = Item::Text { x: 0.0, y: 0.0, text: s.to_string(), size, role };
        Layout { width, ascent: TEXT_ASCENT * size, descent: if descender { TEXT_DESCENT * size } else { 0.0 }, items: vec![item] }
    }

    pub fn space(w: f32) -> Layout {
        Layout { width: w, ..Layout::default() }
    }

    pub fn hcat(parts: Vec<Layout>) -> Layout {
        let mut out = Layout::default();
        let mut x = 0.0;
        for p in &parts {
            out.push(p, x, 0.0);
            x += p.width;
        }
        out.width = x;
        out
    }

    pub fn frac(&self, num: &Layout, den: &Layout, size: f32) -> Layout {
        let pad = 0.12 * size;
        let gap = 0.14 * size;
        let t = RULE * size;
        let axis = AXIS * size;
        let width = num.width.max(den.width) + 2.0 * pad;
        let mut out = Layout { width, ..Layout::default() };
        out.items.push(Item::Rule { x: 0.0, y: -axis - t / 2.0, w: width, h: t, role: Role::Operator });
        out.ascent = axis + t / 2.0;
        out.descent = (t / 2.0 - axis).max(0.0);
        out.push(num, (width - num.width) / 2.0, -axis - t / 2.0 - gap - num.descent);
        out.push(den, (width - den.width) / 2.0, -axis + t / 2.0 + gap + den.ascent);
        out
    }

    /// `base` with `exp` (already at script size) as a superscript.
    pub fn sup(&self, base: &Layout, exp: &Layout, size: f32) -> Layout {
        let shift = (0.40 * size).max(base.ascent * 0.62);
        let mut out = Layout::default();
        out.push(base, 0.0, 0.0);
        out.push(exp, base.width + 0.03 * size, -shift);
        out.width = base.width + 0.03 * size + exp.width;
        out
    }

    pub fn sub(&self, base: &Layout, idx: &Layout, size: f32) -> Layout {
        let mut out = Layout::default();
        out.push(base, 0.0, 0.0);
        out.push(idx, base.width + 0.02 * size, 0.2 * size);
        out.width = base.width + 0.02 * size + idx.width;
        out
    }

    pub fn sqrt(&self, c: &Layout, size: f32) -> Layout {
        let t = RULE * size;
        let top = -(c.ascent + 0.14 * size);
        let bottom = c.descent + 0.04 * size;
        let h = bottom - top;
        let lead = 0.62 * size;
        let tail = 0.08 * size;
        let mut out = Layout::default();
        out.items.push(Item::Stroke {
            points: vec![[0.02 * size, top + 0.60 * h], [0.11 * size, top + 0.54 * h], [0.30 * size, bottom], [0.54 * size, top], [lead + c.width + tail, top]],
            width: t * 1.1,
            role: Role::Operator,
        });
        out.width = lead + c.width + tail;
        out.ascent = -top + t;
        out.descent = bottom;
        out.push(c, lead, 0.0);
        out.width = lead + c.width + tail;
        out
    }

    pub fn delimited(&self, d: Delim, c: &Layout, size: f32) -> Layout {
        let (open, close) = match d {
            Delim::Paren => ("(", ")"),
            Delim::Bracket => ("[", "]"),
            Delim::Brace => ("{", "}"),
            Delim::Bar => ("|", "|"),
        };
        let small = c.ascent <= TEXT_ASCENT * size * 1.05 && c.descent <= TEXT_DESCENT * size * 1.05;
        let (l, r, ascent, descent) = if small {
            (self.text(open, size, Role::Delim, true), self.text(close, size, Role::Delim, true), TEXT_ASCENT * size, TEXT_DESCENT * size)
        } else {
            let top = -(c.ascent + 0.08 * size);
            let bot = c.descent + 0.08 * size;
            let w = match d {
                Delim::Bar => 0.14 * size,
                Delim::Brace => 0.34 * size,
                _ => 0.28 * size,
            };
            let mk = |left: bool| {
                let pts = delimiter_points(d, left, w, top, bot);
                Layout { width: w, ascent: -top, descent: bot, items: vec![Item::Stroke { points: pts, width: 0.065 * size, role: Role::Delim }] }
            };
            (mk(true), mk(false), -top, bot)
        };
        let gap = 0.04 * size;
        let mut out = Layout::default();
        out.push(&l, 0.0, 0.0);
        out.push(c, l.width + gap, 0.0);
        out.push(&r, l.width + gap + c.width + gap, 0.0);
        out.width = l.width + c.width + r.width + 2.0 * gap;
        out.ascent = out.ascent.max(ascent);
        out.descent = out.descent.max(descent);
        out
    }

    /// A grid, columns centred, drawn between tall brackets.
    pub fn matrix(&self, rows: &[Vec<Layout>], size: f32) -> Layout {
        let gap_x = 0.9 * size;
        let gap_y = 0.45 * size;
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut col_w = vec![0.0f32; cols];
        for row in rows {
            for (w, cell) in col_w.iter_mut().zip(row) {
                *w = w.max(cell.width);
            }
        }
        let mut body = Layout::default();
        let mut y = 0.0;
        for (i, row) in rows.iter().enumerate() {
            let asc = row.iter().map(|c| c.ascent).fold(0.0, f32::max);
            let desc = row.iter().map(|c| c.descent).fold(0.0, f32::max);
            let mut x = 0.0;
            for (cell, w) in row.iter().zip(&col_w) {
                body.push(cell, x + (w - cell.width) / 2.0, y + asc);
                x += w + gap_x;
            }
            y += asc + desc + if i + 1 < rows.len() { gap_y } else { 0.0 };
        }
        let total = y;
        let width = col_w.iter().sum::<f32>() + gap_x * cols.saturating_sub(1) as f32;
        let axis = AXIS * size;
        // Move so the vertical middle of the grid sits on the math axis.
        let dy = -(total / 2.0 + axis);
        let mut centred = Layout { width, ascent: total / 2.0 + axis, descent: (total / 2.0 - axis).max(0.0), items: Vec::new() };
        centred.push(&body, 0.0, dy);
        centred.width = width;
        self.delimited(Delim::Bracket, &centred, size)
    }
}

/// The outline of one side of a delimiter, `w` wide, from `top` to `bot` (baseline at 0).
fn delimiter_points(d: Delim, left: bool, w: f32, top: f32, bot: f32) -> Vec<[f32; 2]> {
    let mid = (top + bot) / 2.0;
    let flip = |x: f32| if left { x } else { w - x };
    match d {
        Delim::Bar => vec![[w / 2.0, top], [w / 2.0, bot]],
        Delim::Bracket => vec![[flip(w * 0.9), top], [flip(w * 0.1), top], [flip(w * 0.1), bot], [flip(w * 0.9), bot]],
        Delim::Brace => {
            let q = (bot - top) / 4.0;
            vec![
                [flip(w * 0.9), top],
                [flip(w * 0.55), top + 0.1 * q],
                [flip(w * 0.5), top + q],
                [flip(w * 0.45), mid - 0.5 * q],
                [flip(w * 0.1), mid],
                [flip(w * 0.45), mid + 0.5 * q],
                [flip(w * 0.5), bot - q],
                [flip(w * 0.55), bot - 0.1 * q],
                [flip(w * 0.9), bot],
            ]
        }
        Delim::Paren => {
            // Quadratic curve from the top, bulging outward through the middle, to the bottom.
            let (p0x, p1x) = (w * 0.85, -w * 0.65);
            (0..=10)
                .map(|i| {
                    let t = i as f32 / 10.0;
                    let u = 1.0 - t;
                    let x = u * u * p0x + 2.0 * u * t * p1x + t * t * p0x;
                    let y = u * u * top + 2.0 * u * t * mid + t * t * bot;
                    [flip(x), y]
                })
                .collect()
        }
    }
}
