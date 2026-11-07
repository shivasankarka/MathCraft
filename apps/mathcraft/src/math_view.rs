//! Typeset mathematics in egui: measuring text with the math font, painting a layout, and turning values into documents.

use crate::fonts::math_font;
use crate::theme::Theme;
use egui::{Align2, Context, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};
use mathcraft_core::Kind;
use mathcraft_eval::Value;
use mathcraft_notebook::CellId;
use mathcraft_typeset::{Doc, Item, Layout, Metrics, Role};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// Output size of the typeset result, in points.
pub const SIZE: f32 = 20.0;

struct Measurer<'a, 'b>(RefCell<&'a mut egui::epaint::text::FontsView<'b>>);

impl Metrics for Measurer<'_, '_> {
    fn advance(&self, text: &str, size: f32) -> f32 {
        let mut fonts = self.0.borrow_mut();
        let font = math_font(size);
        text.chars().map(|c| fonts.glyph_width(&font, c)).sum()
    }
}

/// Lay out `doc` with the real math font.
pub fn layout(ctx: &Context, doc: &Doc, size: f32) -> Option<Layout> {
    ctx.fonts_mut(|f| mathcraft_typeset::layout(doc, &Measurer(RefCell::new(f)), size))
}

fn color(theme: &Theme, role: Role) -> egui::Color32 {
    let m = &theme.math;
    match role {
        Role::Text => m.text,
        Role::Variable => m.variable,
        Role::Number => m.number,
        Role::Operator => m.operator,
        Role::Function => m.function,
        Role::Constant => m.constant,
        Role::Delim => m.delim,
    }
}

/// Paint a layout in a freshly allocated rectangle.
pub fn show(ui: &mut Ui, layout: &Layout, theme: &Theme, size: f32) -> Response {
    let pad = vec2(2.0, 3.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(layout.width.ceil(), layout.height().ceil()) + pad * 2.0, Sense::click());
    if ui.is_rect_visible(rect) {
        paint(ui, layout, theme, size, rect.min + pad + vec2(0.0, layout.ascent));
    }
    resp
}

/// Distance from the top of a line of math-font text to its baseline, per unit of font size.
fn baseline_ratio(ctx: &Context) -> f32 {
    const PROBE: f32 = 100.0;
    ctx.fonts_mut(|f| {
        let galley = f.layout_no_wrap("x".to_string(), math_font(PROBE), egui::Color32::WHITE);
        galley.rows.first().map_or(0.8, |r| r.pos.y + r.row.glyphs.first().map_or(0.8 * PROBE, |g| g.pos.y)) / PROBE
    })
}

fn paint(ui: &Ui, layout: &Layout, theme: &Theme, size: f32, origin: egui::Pos2) {
    let painter = ui.painter();
    let baseline = baseline_ratio(ui.ctx());
    let clip = painter.clip_rect().expand(40.0);
    for item in &layout.items {
        match item {
            Item::Text { x, y, text, size: s, role } => {
                let at = origin + vec2(*x, *y);
                if clip.contains(at) {
                    painter.text(at - vec2(0.0, baseline * s), Align2::LEFT_TOP, text, math_font(*s), color(theme, *role));
                }
            }
            Item::Rule { x, y, w, h, role } => {
                let r = Rect::from_min_size(origin + vec2(*x, *y), vec2(*w, h.max(1.0)));
                painter.rect_filled(r, 0.0, color(theme, *role));
            }
            Item::Stroke { points, width, role } => {
                let pts: Vec<_> = points.iter().map(|[px, py]| pos2(origin.x + px, origin.y + py)).collect();
                painter.add(Shape::line(pts, Stroke::new(width.max(1.0).min(size * 0.12), color(theme, *role))));
            }
        }
    }
}

/// Values we can typeset: expressions, lists and matrices of them, booleans. Everything else shows as plain text.
pub fn to_doc(v: &Value) -> Option<Doc> {
    match v {
        Value::Expr(e) => Some(Doc::Expr(e.clone())),
        Value::List(items) | Value::Tuple(items) => {
            if items.len() > 400 {
                return None;
            }
            let tuple = matches!(v, Value::Tuple(_));
            if !tuple && let Some(m) = matrix(items) {
                return Some(m);
            }
            let docs = items.iter().map(|i| to_doc(i).unwrap_or_else(|| Doc::Text(i.to_string()))).collect();
            Some(if tuple { Doc::Tuple(docs) } else { Doc::List(docs) })
        }
        Value::Array(a) if !a.is_empty() && a.len() <= 1600 => {
            Some(Doc::Matrix(a.rows_of_exprs().into_iter().map(|r| r.into_iter().map(Doc::Expr).collect()).collect()))
        }
        Value::Bool(_) | Value::None => Some(Doc::Text(v.to_string())),
        _ => None,
    }
}

/// A list of equal-length lists of expressions is a matrix.
fn matrix(items: &[Value]) -> Option<Doc> {
    let mut rows = Vec::new();
    let mut width = None;
    for item in items {
        let Value::List(cells) = item else { return None };
        if cells.is_empty() || *width.get_or_insert(cells.len()) != cells.len() {
            return None;
        }
        let row: Option<Vec<Doc>> =
            cells.iter().map(|c| matches!(c, Value::Expr(e) if !matches!(e.kind(), Kind::List(_))).then(|| to_doc(c)).flatten()).collect();
        rows.push(row?);
    }
    (!rows.is_empty()).then_some(Doc::Matrix(rows))
}

/// Typeset results cached per cell: laying out is cheap but not free, and the output only changes when the cell runs.
#[derive(Default)]
pub struct MathCache {
    entries: HashMap<CellId, (u64, u32, Option<Arc<Layout>>)>,
}

impl MathCache {
    /// The layout of a cell's value, recomputed when `generation` (the cell's run count) or the size changed.
    pub fn get(&mut self, ctx: &Context, id: CellId, generation: u64, value: &Value) -> Option<Arc<Layout>> {
        let size_key = SIZE.to_bits();
        if let Some((g, s, l)) = self.entries.get(&id)
            && *g == generation
            && *s == size_key
        {
            return l.clone();
        }
        let l = to_doc(value).and_then(|d| layout(ctx, &d, SIZE)).map(Arc::new);
        self.entries.insert(id, (generation, size_key, l.clone()));
        l
    }
}
