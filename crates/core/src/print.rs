//! Python-syntax printing of expressions. The output parses back to the same expression.

use crate::expr::{Expr, Kind, SYM_E, interval_parts, relation_op};
use crate::number::Num;
use std::fmt;

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        write_expr(self, 0, &mut s);
        f.write_str(&s)
    }
}

// Precedence levels: 0 equation, 1 sum, 2 product/quotient, 3 unary minus, 4 power, 5 atom.
fn prec(e: &Expr) -> u8 {
    match e.kind() {
        Kind::Num(n) => {
            if n.is_negative() {
                3
            } else if matches!(n, Num::Rat(_)) {
                2
            } else {
                5
            }
        }
        Kind::Call(name, args) if args.len() == 2 && relation_op(name).is_some() => 0,
        Kind::Call(name, _) if &**name == "Union" => 1,
        Kind::Sym(_) | Kind::Call(..) | Kind::List(_) => 5,
        Kind::Add(_) => 1,
        Kind::Mul(fs) => match fs.first().map(Expr::kind) {
            Some(Kind::Num(c)) if c.is_negative() => 3,
            _ => 2,
        },
        Kind::Pow(b, x) => {
            if b.as_sym() == Some(SYM_E) || is_half(x) {
                5
            } else if x.as_num().is_some_and(Num::is_negative) {
                2
            } else {
                4
            }
        }
        Kind::Eq(..) => 0,
    }
}

fn is_half(x: &Expr) -> bool {
    Expr::half() == *x
}

fn write_expr(e: &Expr, min: u8, out: &mut String) {
    let wrap = prec(e) < min;
    if wrap {
        out.push('(');
    }
    write_inner(e, out);
    if wrap {
        out.push(')');
    }
}

fn write_list(items: &[Expr], out: &mut String) {
    for (i, a) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_expr(a, 0, out);
    }
}

fn write_inner(e: &Expr, out: &mut String) {
    match e.kind() {
        Kind::Num(n) => out.push_str(&n.to_string()),
        Kind::Sym(s) => out.push_str(s),
        Kind::Call(name, args) if args.len() == 2 && relation_op(name).is_some() => {
            let op = relation_op(name).map_or("", |o| o.0);
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(&format!(" {op} "));
                }
                write_expr(a, 1, out);
            }
        }
        Kind::Call(name, args) if &**name == "Union" && !args.is_empty() => {
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(" \u{222a} ");
                }
                write_expr(a, 5, out);
            }
        }
        Kind::Call(name, args) if &**name == "EmptySet" && args.is_empty() => out.push('\u{2205}'),
        Kind::Call(..) if interval_parts(e).is_some() => {
            if let Some((lo, hi, closed_lo, closed_hi)) = interval_parts(e) {
                if lo == hi {
                    out.push('{');
                    write_expr(lo, 0, out);
                    out.push('}');
                } else {
                    out.push(if closed_lo { '[' } else { '(' });
                    write_expr(lo, 0, out);
                    out.push_str(", ");
                    write_expr(hi, 0, out);
                    out.push(if closed_hi { ']' } else { ')' });
                }
            }
        }
        Kind::Call(name, args) => {
            out.push_str(name);
            out.push('(');
            write_list(args, out);
            out.push(')');
        }
        Kind::List(items) => {
            out.push('[');
            write_list(items, out);
            out.push(']');
        }
        Kind::Eq(a, b) => {
            write_expr(a, 1, out);
            out.push_str(" == ");
            write_expr(b, 1, out);
        }
        Kind::Add(terms) => write_add(terms, out),
        Kind::Mul(fs) => match fs.split_first() {
            Some((first, rest)) => match first.kind() {
                Kind::Num(c) => write_product(c, rest, out),
                _ => write_product(&Num::one(), fs, out),
            },
            None => out.push('1'),
        },
        Kind::Pow(b, x) => write_pow(e, b, x, out),
    }
}

fn write_pow(whole: &Expr, b: &Expr, x: &Expr, out: &mut String) {
    if b.as_sym() == Some(SYM_E) {
        out.push_str("exp(");
        write_expr(x, 0, out);
        out.push(')');
    } else if is_half(x) {
        out.push_str("sqrt(");
        write_expr(b, 0, out);
        out.push(')');
    } else if x.as_num().is_some_and(Num::is_negative) {
        write_product(&Num::one(), std::slice::from_ref(whole), out);
    } else {
        write_expr(b, 5, out);
        out.push_str("**");
        write_expr(x, 4, out);
    }
}

/// Polynomial-style degree used only to order the terms of a sum for display (highest first, constants last).
fn degree(e: &Expr) -> i64 {
    match e.kind() {
        Kind::Num(_) => 0,
        Kind::Pow(b, x) => x.as_i64().map_or(1, |n| n.saturating_mul(degree(b).max(1))),
        Kind::Mul(fs) => fs.iter().map(degree).fold(0i64, i64::saturating_add),
        _ => 1,
    }
}

/// Variables and exponents of a monomial, by variable name, for ordering equal-degree terms (`x**2` before `x*y`).
fn monomial(e: &Expr) -> Vec<(String, i64)> {
    let factors: &[Expr] = match e.kind() {
        Kind::Mul(fs) => fs,
        _ => std::slice::from_ref(e),
    };
    let mut v: Vec<(String, i64)> = factors
        .iter()
        .filter_map(|f| match f.kind() {
            Kind::Sym(s) => Some((s.to_string(), 1)),
            Kind::Pow(b, k) => Some((b.as_sym()?.to_string(), k.as_i64()?)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn display_order(a: &Expr, b: &Expr) -> std::cmp::Ordering {
    let by_mono = |x: &Vec<(String, i64)>, y: &Vec<(String, i64)>| {
        for (p, q) in x.iter().zip(y.iter()) {
            let o = p.0.cmp(&q.0).then_with(|| q.1.cmp(&p.1));
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        y.len().cmp(&x.len())
    };
    degree(b).cmp(&degree(a)).then_with(|| by_mono(&monomial(a), &monomial(b))).then_with(|| a.cmp(b))
}

/// One term of a sum in display order: `±coeff * rest` (`rest` is 1 for a plain number).
#[derive(Debug, Clone)]
pub struct SumTerm {
    pub negative: bool,
    /// Magnitude of the numeric coefficient.
    pub coeff: Num,
    pub rest: Expr,
}

/// The terms of a sum in the order they are displayed: highest degree first, constants last, imaginary terms after those.
pub fn sum_terms(terms: &[Expr]) -> Vec<SumTerm> {
    let mut items: Vec<(Num, Expr)> = terms.iter().map(Expr::split_coeff).collect();
    // The imaginary part is written last: `3 - 4*I`, `x**2 + 1 + 2*I`.
    let imaginary = |e: &Expr| e.as_sym() == Some(crate::SYM_I) || matches!(e.kind(), Kind::Mul(fs) if fs.iter().any(|f| f.as_sym() == Some(crate::SYM_I)));
    items.sort_by(|(_, a), (_, b)| imaginary(a).cmp(&imaginary(b)).then_with(|| display_order(a, b)));
    items
        .into_iter()
        .map(|(c, rest)| {
            let negative = c.is_negative();
            SumTerm { negative, coeff: if negative { c.neg() } else { c }, rest }
        })
        .collect()
}

/// A product split for display: sign, numeric numerator and denominator, then the factors above and below the bar.
/// Factors in `denom` already have their exponent made positive.
#[derive(Debug, Clone)]
pub struct Product {
    pub negative: bool,
    pub numer_num: Option<String>,
    pub denom_num: Option<String>,
    pub numer: Vec<Expr>,
    pub denom: Vec<Expr>,
}

/// Split `coeff * factors` for display (negative-power factors move below the division bar).
pub fn split_product(coeff: &Num, factors: &[Expr]) -> Product {
    let negative = coeff.is_negative();
    let abs = if negative { coeff.neg() } else { coeff.clone() };
    let mut numer_num = None;
    let mut denom_num = None;
    match abs.parts() {
        Some((n, d)) => {
            if n != dashu::integer::IBig::ONE {
                numer_num = Some(n.to_string());
            }
            if d != dashu::integer::UBig::ONE {
                denom_num = Some(d.to_string());
            }
        }
        None => {
            if !abs.is_one() {
                numer_num = Some(abs.to_string());
            }
        }
    }
    let mut ordered: Vec<&Expr> = factors.iter().collect();
    ordered.sort_by(|a, b| factor_key(a).cmp(&factor_key(b)).then_with(|| a.cmp(b)));
    let mut numer = Vec::new();
    let mut denom = Vec::new();
    for f in ordered {
        match f.kind() {
            Kind::Pow(b, x) if x.as_num().is_some_and(Num::is_negative) => {
                let flipped = x.as_num().map(Num::neg).map(Expr::num).unwrap_or_else(Expr::one);
                denom.push(Expr::pow(b, &flipped));
            }
            _ => numer.push(f.clone()),
        }
    }
    Product { negative, numer_num, denom_num, numer, denom }
}

/// Split any expression as a product for display (a lone negative power is `1/...`).
pub fn product_of(e: &Expr) -> Product {
    match e.kind() {
        Kind::Mul(fs) => match fs.split_first() {
            Some((first, rest)) => match first.kind() {
                Kind::Num(c) => split_product(c, rest),
                _ => split_product(&Num::one(), fs),
            },
            None => split_product(&Num::one(), &[]),
        },
        Kind::Num(c) => split_product(c, &[]),
        _ => split_product(&Num::one(), std::slice::from_ref(e)),
    }
}

fn write_add(terms: &[Expr], out: &mut String) {
    for (i, t) in sum_terms(terms).iter().enumerate() {
        match (i, t.negative) {
            (0, true) => out.push('-'),
            (0, false) => {}
            (_, true) => out.push_str(" - "),
            (_, false) => out.push_str(" + "),
        }
        if t.rest.is_one() {
            out.push_str(&t.coeff.to_string());
        } else {
            match t.rest.kind() {
                Kind::Mul(fs) => write_product(&t.coeff, fs, out),
                _ => write_product(&t.coeff, std::slice::from_ref(&t.rest), out),
            }
        }
    }
}

/// `coeff * factors`, with negative-power factors moved below a division bar.
fn write_product(coeff: &Num, factors: &[Expr], out: &mut String) {
    let p = split_product(coeff, factors);
    if p.negative {
        out.push('-');
    }
    let mut numer: Vec<String> = p.numer_num.into_iter().collect();
    numer.extend(p.numer.iter().map(|f| render(f, 3)));
    let mut denom: Vec<String> = p.denom_num.into_iter().collect();
    denom.extend(p.denom.iter().map(|f| render(f, 3)));
    let top = if numer.is_empty() { "1".to_string() } else { numer.join("*") };
    out.push_str(&top);
    match denom.len() {
        0 => {}
        1 => {
            out.push('/');
            out.extend(denom);
        }
        _ => {
            out.push_str("/(");
            out.push_str(&denom.join("*"));
            out.push(')');
        }
    }
}

/// Display order of the factors of a product: plain variables and their powers by name (`x*y**2`), the rest after.
fn factor_key(f: &Expr) -> (u8, String) {
    match f.kind() {
        Kind::Sym(s) => (0, s.to_string()),
        Kind::Pow(b, _) => match b.as_sym() {
            Some(s) if s != SYM_E => (0, s.to_string()),
            _ => (1, String::new()),
        },
        _ => (1, String::new()),
    }
}

fn render(e: &Expr, min: u8) -> String {
    let mut s = String::new();
    write_expr(e, min, &mut s);
    s
}
