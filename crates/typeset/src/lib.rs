//! MathCraft typesetting (layer L1): turns expressions and lists of them into positioned glyphs, rules and strokes.
//!
//! No UI toolkit and no I/O: the caller supplies font [`Metrics`] and paints the resulting [`Layout`]. Layout work is
//! bounded; huge or very deep input returns `None` and the caller shows plain text instead.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod expr;
mod layout;
mod names;

pub use layout::{Delim, Item, Layout, Metrics, Role};

use expr::St;
use layout::Tx;
use mathcraft_core::Expr;

/// Something to typeset.
#[derive(Debug, Clone)]
pub enum Doc {
    Expr(Expr),
    /// Plain text: `True`, `None`, a string inside a list.
    Text(String),
    /// A bracketed, comma-separated list of documents.
    List(Vec<Doc>),
    /// A tuple, in parentheses.
    Tuple(Vec<Doc>),
    /// A grid of documents between tall brackets.
    Matrix(Vec<Vec<Doc>>),
}

/// Work allowed for one output before we give up and let the caller show plain text.
const BUDGET: i32 = 60_000;

/// Lay out `doc` at `size` (in the painter's units). `None` when the output is too large or too deep.
pub fn layout(doc: &Doc, m: &dyn Metrics, size: f32) -> Option<Layout> {
    let tx = Tx::new(m, BUDGET);
    let out = doc_layout(&tx, doc, St { size, frac_depth: 0, script: false })?;
    (out.width.is_finite() && out.ascent.is_finite() && out.descent.is_finite()).then_some(out)
}

/// Like [`layout`], but a long sum is broken into lines no wider than `max_width` (each continuation line is indented
/// and starts with its sign). Anything else, and sums that fit, are laid out as by [`layout`].
pub fn layout_wrapped(doc: &Doc, m: &dyn Metrics, size: f32, max_width: f32) -> Option<Layout> {
    let whole = layout(doc, m, size)?;
    let Doc::Expr(e) = doc else { return Some(whole) };
    if whole.width <= max_width || !matches!(e.kind(), mathcraft_core::Kind::Add(_)) {
        return Some(whole);
    }
    let tx = Tx::new(m, BUDGET);
    let pieces = expr::sum_pieces(&tx, e.children().as_slice(), St { size, frac_depth: 0, script: false })?;
    let indent = 1.5 * size;
    let (mut rows, mut cur, mut width): (Vec<Layout>, Vec<Layout>, f32) = (Vec::new(), Vec::new(), 0.0);
    for p in pieces {
        if !cur.is_empty() && width + p.width > max_width {
            rows.push(Tx::hcat(std::mem::take(&mut cur)));
            cur.push(Tx::space(indent));
            width = indent;
        }
        width += p.width;
        cur.push(p);
    }
    rows.push(Tx::hcat(cur));
    let out = Tx::lines(rows, 0.3 * size);
    (out.width.is_finite() && out.ascent.is_finite() && out.descent.is_finite()).then_some(out)
}

fn doc_layout(tx: &Tx, doc: &Doc, st: St) -> Option<Layout> {
    tx.tick()?;
    match doc {
        Doc::Expr(e) => expr::expr(tx, e, st),
        Doc::Text(s) => Some(expr::plain(tx, s, st)),
        Doc::List(items) | Doc::Tuple(items) => {
            let mut parts = Vec::new();
            for (i, d) in items.iter().enumerate() {
                if i > 0 {
                    parts.push(expr::plain(tx, ",", st));
                    parts.push(Tx::space(0.3 * st.size));
                }
                parts.push(doc_layout(tx, d, st)?);
            }
            let delim = if matches!(doc, Doc::List(_)) { Delim::Bracket } else { Delim::Paren };
            Some(tx.delimited(delim, &Tx::hcat(parts), st.size))
        }
        Doc::Matrix(rows) => {
            let mut grid = Vec::new();
            for r in rows {
                let mut row = Vec::new();
                for d in r {
                    row.push(doc_layout(tx, d, st)?);
                }
                grid.push(row);
            }
            Some(tx.matrix(&grid, st.size))
        }
    }
}

#[cfg(test)]
mod tests;
