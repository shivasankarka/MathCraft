//! Expression to layout. Sum and product ordering come from `mathcraft_core` so the text and typeset forms agree.

use crate::layout::{Delim, Layout, Role, Tx};
use crate::names::{function_name, is_trig, symbol};
use mathcraft_core::expr::SYM_E;
use mathcraft_core::{Expr, Kind, Num, Product, product_of, split_product, sum_terms};

/// Deepest expression we lay out; deeper ones are shown as plain text by the caller.
pub(crate) const MAX_DEPTH: u32 = 120;

const MINUS: &str = "\u{2212}";

fn is_half(x: &Expr) -> bool {
    Expr::half() == *x
}

/// Does this expression print as one unit, so it needs no parentheses as a base or factor?
fn is_atomic(e: &Expr) -> bool {
    match e.kind() {
        Kind::Num(n) => !n.is_negative() && (n.is_integer() || !matches!(n, Num::Rat(_))),
        Kind::Sym(_) | Kind::Call(..) | Kind::List(_) => true,
        Kind::Pow(b, x) => is_half(x) && b.as_sym() != Some(SYM_E),
        _ => false,
    }
}

/// Does the first thing printed for this factor look like a number (so juxtaposition would read as one number)?
fn starts_with_number(e: &Expr) -> bool {
    match e.kind() {
        Kind::Num(_) => true,
        Kind::Pow(b, _) => matches!(b.kind(), Kind::Num(_)),
        _ => false,
    }
}

/// Style for a nested position: the type size and how deeply fractions are stacked.
#[derive(Clone, Copy)]
pub(crate) struct St {
    pub size: f32,
    pub frac_depth: u8,
    /// In an exponent or subscript, where fractions are written inline (`x/5`) to stay legible.
    pub script: bool,
}

impl St {
    fn script(self) -> St {
        St { size: Tx::script(self.size), script: true, ..self }
    }
    fn in_frac(self) -> St {
        St { size: if self.frac_depth == 0 { self.size } else { (self.size * 0.88).max(6.5) }, frac_depth: self.frac_depth.saturating_add(1), ..self }
    }
}

pub(crate) fn expr(tx: &Tx, e: &Expr, st: St) -> Option<Layout> {
    tx.tick()?;
    if e.depth() > MAX_DEPTH {
        return None;
    }
    match e.kind() {
        Kind::Num(n) => Some(number(tx, n, st)),
        Kind::Sym(s) => Some(sym(tx, s, st)),
        Kind::Add(terms) => sum(tx, terms, st),
        Kind::Mul(_) => {
            let p = product_of(e);
            let body = product(tx, &p, st)?;
            Some(with_sign(tx, p.negative, body, st))
        }
        Kind::Pow(b, x) => pow(tx, e, b, x, st),
        Kind::Call(name, args) => call(tx, name, args, st),
        Kind::List(items) => list(tx, items, st),
        Kind::Eq(a, b) => {
            let l = expr(tx, a, st)?;
            let r = expr(tx, b, st)?;
            Some(Tx::hcat(vec![l, operator(tx, "=", st), r]))
        }
    }
}

fn operator(tx: &Tx, op: &str, st: St) -> Layout {
    let gap = Tx::space(0.24 * st.size);
    Tx::hcat(vec![gap.clone(), tx.text(op, st.size, Role::Operator, false), gap])
}

fn with_sign(tx: &Tx, negative: bool, body: Layout, st: St) -> Layout {
    if negative { Tx::hcat(vec![tx.text(MINUS, st.size, Role::Operator, false), Tx::space(0.04 * st.size), body]) } else { body }
}

fn number(tx: &Tx, n: &Num, st: St) -> Layout {
    if matches!(n, Num::Rat(_)) {
        let p = split_product(n, &[]);
        if let Some(l) = product(tx, &p, st) {
            return with_sign(tx, p.negative, l, st);
        }
    }
    let s = match n {
        Num::Float(f) if f.is_finite() && *f != 0.0 && (f.abs() < 1e-4 || f.abs() >= 1e12) => format!("{f:e}"),
        _ => n.to_string(),
    };
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, s),
    };
    let body = match digits.split_once('e') {
        // 1.5e-10 -> 1.5 x 10^-10
        Some((mant, exp)) if mant.parse::<f64>().is_ok() => {
            let exp = exp.replace('-', MINUS).replace('+', "");
            let ms = st.script();
            let base = Tx::hcat(vec![tx.text(mant, st.size, Role::Number, false), tx.text(" \u{00D7} 10", st.size, Role::Operator, false)]);
            tx.sup(&base, &tx.text(&exp, ms.size, Role::Number, false), st.size)
        }
        _ => tx.text(&digits, st.size, Role::Number, false),
    };
    with_sign(tx, neg, body, st)
}

fn sym(tx: &Tx, name: &str, st: St) -> Layout {
    let parts = symbol(name);
    let base = tx.text(&parts.base, st.size, parts.role, parts.descender && parts.sub.is_none());
    match parts.sub {
        Some(sub) => {
            let idx = tx.text(&sub, st.script().size, Role::Variable, false);
            tx.sub(&base, &idx, st.size)
        }
        None => base,
    }
}

fn sum(tx: &Tx, terms: &[Expr], st: St) -> Option<Layout> {
    let mut parts = Vec::new();
    for (i, t) in sum_terms(terms).iter().enumerate() {
        tx.tick()?;
        let p: Product = match t.rest.kind() {
            Kind::Mul(fs) => split_product(&t.coeff, fs),
            _ if t.rest.is_one() => split_product(&t.coeff, &[]),
            _ => split_product(&t.coeff, std::slice::from_ref(&t.rest)),
        };
        let body = product(tx, &p, st)?;
        if i == 0 {
            if t.negative {
                parts.push(tx.text(MINUS, st.size, Role::Operator, false));
                parts.push(Tx::space(0.04 * st.size));
            }
        } else {
            parts.push(operator(tx, if t.negative { MINUS } else { "+" }, st));
        }
        parts.push(body);
    }
    Some(Tx::hcat(parts))
}

fn factor(tx: &Tx, f: &Expr, st: St) -> Option<Layout> {
    let l = expr(tx, f, st)?;
    Some(if matches!(f.kind(), Kind::Add(_) | Kind::Eq(..)) { tx.delimited(Delim::Paren, &l, st.size) } else { l })
}

fn factors(tx: &Tx, num: Option<&str>, fs: &[Expr], st: St) -> Option<Layout> {
    let mut parts: Vec<Layout> = Vec::new();
    if let Some(n) = num {
        parts.push(tx.text(n, st.size, Role::Number, false));
    }
    for f in fs {
        if !parts.is_empty() {
            if starts_with_number(f) {
                parts.push(Tx::space(0.12 * st.size));
                parts.push(tx.text("\u{00B7}", st.size, Role::Operator, false));
                parts.push(Tx::space(0.12 * st.size));
            } else {
                parts.push(Tx::space(0.1 * st.size));
            }
        }
        parts.push(factor(tx, f, st)?);
    }
    if parts.is_empty() {
        parts.push(tx.text("1", st.size, Role::Number, false));
    }
    Some(Tx::hcat(parts))
}

/// The product without its sign: a fraction when anything sits below the bar.
fn product(tx: &Tx, p: &Product, st: St) -> Option<Layout> {
    if p.denom_num.is_none() && p.denom.is_empty() {
        return factors(tx, p.numer_num.as_deref(), &p.numer, st);
    }
    if st.script && p.denom.len() + usize::from(p.denom_num.is_some()) == 1 && p.numer.len() + usize::from(p.numer_num.is_some()) <= 1 {
        let top = factors(tx, p.numer_num.as_deref(), &p.numer, st)?;
        let bottom = factors(tx, p.denom_num.as_deref(), &p.denom, st)?;
        let slash = tx.text("/", st.size, Role::Operator, false);
        return Some(Tx::hcat(vec![top, slash, bottom]));
    }
    let inner = st.in_frac();
    let top = factors(tx, p.numer_num.as_deref(), &p.numer, inner)?;
    let bottom = factors(tx, p.denom_num.as_deref(), &p.denom, inner)?;
    Some(tx.frac(&top, &bottom, st.size))
}

fn pow(tx: &Tx, whole: &Expr, b: &Expr, x: &Expr, st: St) -> Option<Layout> {
    if x.as_num().is_some_and(Num::is_negative) {
        let p = product_of(whole);
        let body = product(tx, &p, st)?;
        return Some(with_sign(tx, p.negative, body, st));
    }
    if b.as_sym() == Some(SYM_E) {
        let base = sym(tx, SYM_E, st);
        let e = expr(tx, x, st.script())?;
        return Some(tx.sup(&base, &e, st.size));
    }
    if is_half(x) {
        let inner = expr(tx, b, st)?;
        return Some(tx.sqrt(&inner, st.size));
    }
    // sin(x)^2 is written sin^2(x)
    if let (Kind::Call(name, args), Some(k)) = (b.kind(), x.as_i64())
        && is_trig(name)
        && k > 0
    {
        let (shown, role) = function_name(name);
        let f = tx.text(&shown, st.size, role, false);
        let sup = tx.sup(&f, &expr(tx, x, st.script())?, st.size);
        let arg = arguments(tx, args, st)?;
        return Some(Tx::hcat(vec![sup, arg]));
    }
    let base = expr(tx, b, st)?;
    let base = if is_atomic(b) { base } else { tx.delimited(Delim::Paren, &base, st.size) };
    Some(tx.sup(&base, &expr(tx, x, st.script())?, st.size))
}

fn arguments(tx: &Tx, args: &[Expr], st: St) -> Option<Layout> {
    let mut parts = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            parts.push(tx.text(",", st.size, Role::Operator, true));
            parts.push(Tx::space(0.22 * st.size));
        }
        parts.push(expr(tx, a, st)?);
    }
    Some(tx.delimited(Delim::Paren, &Tx::hcat(parts), st.size))
}

fn call(tx: &Tx, name: &str, args: &[Expr], st: St) -> Option<Layout> {
    match (name, args) {
        ("abs", [a]) => Some(tx.delimited(Delim::Bar, &expr(tx, a, st)?, st.size)),
        ("factorial", [a]) => {
            let inner = expr(tx, a, st)?;
            let inner = if is_atomic(a) { inner } else { tx.delimited(Delim::Paren, &inner, st.size) };
            Some(Tx::hcat(vec![inner, tx.text("!", st.size, Role::Operator, false)]))
        }
        _ => {
            let (shown, role) = function_name(name);
            Some(Tx::hcat(vec![tx.text(&shown, st.size, role, false), arguments(tx, args, st)?]))
        }
    }
}

/// A list of expressions: a matrix when every item is a list of the same length, otherwise `[a, b, c]`.
fn list(tx: &Tx, items: &[Expr], st: St) -> Option<Layout> {
    let rows: Vec<&[Expr]> = items
        .iter()
        .filter_map(|r| match r.kind() {
            Kind::List(cells) => Some(cells.as_slice()),
            _ => None,
        })
        .collect();
    let width = rows.first().map_or(0, |r| r.len());
    if !items.is_empty() && rows.len() == items.len() && width > 0 && rows.iter().all(|r| r.len() == width) {
        let mut grid = Vec::new();
        for r in rows {
            let mut row = Vec::new();
            for c in r {
                row.push(expr(tx, c, st)?);
            }
            grid.push(row);
        }
        return Some(tx.matrix(&grid, st.size));
    }
    let mut parts = Vec::new();
    for (i, a) in items.iter().enumerate() {
        tx.tick()?;
        if i > 0 {
            parts.push(tx.text(",", st.size, Role::Operator, true));
            parts.push(Tx::space(0.3 * st.size));
        }
        parts.push(expr(tx, a, st)?);
    }
    Some(tx.delimited(Delim::Bracket, &Tx::hcat(parts), st.size))
}

/// Plain upright text such as `True` or a quoted string, for values that are not expressions.
pub(crate) fn plain(tx: &Tx, s: &str, st: St) -> Layout {
    tx.text(s, st.size, Role::Text, true)
}
