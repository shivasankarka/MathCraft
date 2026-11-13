//! Inequalities in one real unknown: `x**2 > 4`, `(x - 1)/(x + 2) <= 0`, `log(x) < 1`, `abs(x - 1) >= 2`.
//!
//! The relation is moved to `f op 0`. The sign of `f` can only change at its zeros, at its poles, and where a
//! logarithm, an even root or an absolute value changes behaviour, so those points are found with `solve`, sorted, and
//! `f` is tested once between each pair and at each point. Neighbouring pieces that satisfy the relation merge into
//! one interval. The answer is `Interval(lo, hi, closed_lo, closed_hi)`, a `Union` of them, or `EmptySet`.

use crate::solve::solve;
use crate::util::guard;
use mathcraft_core::expr::relation_op;
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, MathError, MathResult};
use std::cmp::Ordering;

/// Functions whose solutions repeat: `solve` returns one period only, so the answer here would silently be wrong.
const PERIODIC: [&str; 6] = ["sin", "cos", "tan", "cot", "sec", "csc"];

/// The set of real `x` satisfying `rel`, a relation built with `<`, `<=`, `>` or `>=`.
pub fn solve_inequality(rel: &Expr, x: &Expr) -> MathResult<Expr> {
    guard(rel)?;
    let Kind::Call(name, args) = rel.kind() else { return Err(MathError::unsupported("this is not an inequality")) };
    let ([lhs, rhs], Some(_)) = (args.as_slice(), relation_op(name)) else { return Err(MathError::unsupported("this is not an inequality")) };
    let f = lhs.sub(rhs);
    let (strict, upward) = match &**name {
        "Lt" => (true, false),
        "Le" => (false, false),
        "Gt" => (true, true),
        _ => (false, true),
    };
    let holds = |v: f64| {
        let zero = v.abs() < 1e-12;
        match (zero, v > 0.0) {
            (true, _) => !strict,
            (false, up) => up == upward,
        }
    };
    let Some(xname) = x.as_sym().map(str::to_owned) else { return Err(MathError::unsupported("solve for a plain symbol")) };
    if f.free_symbols().iter().any(|s| **s != *xname) {
        return Err(MathError::unsupported("an inequality can have only the unknown as a symbol; give the other symbols values"));
    }
    if has_periodic(&f) {
        return Err(MathError::unsupported("inequalities with sin, cos or tan are not supported yet (their solutions repeat forever)"));
    }
    let at = |v: f64| eval_f64(&f, &|s| (s == xname).then_some(v)).ok().filter(|r| r.is_finite());
    if !f.contains(x) {
        let all = at(0.0).is_some_and(holds);
        return Ok(if all { interval(&minus_infinity(), &Expr::infinity(), false, false) } else { Expr::call("EmptySet", vec![]) });
    }

    let zeros = real_roots(&f, x)?;
    let mut points = zeros.clone();
    for g in pole_and_edge_expressions(&f, x)? {
        points.extend(real_roots(&g, x)?);
    }
    let mut points: Vec<(f64, Expr)> = points.into_iter().filter_map(|p| Some((value(&p)?, p))).collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.dedup_by(|a, b| close(a.0, b.0));
    let is_zero = |v: f64| zeros.iter().any(|z| value(z).is_some_and(|zv| close(zv, v)));

    // pieces: open interval 0, point 1, open interval 1, point 2, ..., open interval n
    let n = points.len();
    let mut keep = Vec::with_capacity(2 * n + 1);
    for k in 0..=n {
        let sample = match (k.checked_sub(1).and_then(|i| points.get(i)), points.get(k)) {
            (Some(a), Some(b)) => (a.0 + b.0) / 2.0,
            (Some(a), None) => a.0 + 1.0,
            (None, Some(b)) => b.0 - 1.0,
            (None, None) => 0.0,
        };
        keep.push(at(sample).is_some_and(holds));
        if let Some((p, _)) = points.get(k) {
            keep.push(match at(*p) {
                Some(_) if is_zero(*p) => !strict,
                Some(v) => holds(v),
                None => false,
            });
        }
    }

    let end = |k: usize| points.get(k).map(|p| p.1.clone());
    let mut parts = Vec::new();
    let mut i = 0;
    while i < keep.len() {
        if !keep.get(i).copied().unwrap_or(false) {
            i += 1;
            continue;
        }
        let start = i;
        while keep.get(i + 1).copied().unwrap_or(false) {
            i += 1;
        }
        // a run starting on an open piece begins at the point before it, which it does not contain
        let (lo, closed_lo) = if start % 2 == 1 { (end(start / 2), true) } else { (start.checked_sub(2).map(|s| s / 2).and_then(end), false) };
        let (hi, closed_hi) = if i % 2 == 1 { (end(i / 2), true) } else { (end(i / 2), false) };
        parts.push(interval(&lo.unwrap_or_else(minus_infinity), &hi.unwrap_or_else(Expr::infinity), closed_lo, closed_hi));
        i += 1;
    }
    Ok(match parts.len() {
        0 => Expr::call("EmptySet", vec![]),
        1 => parts.remove(0),
        _ => Expr::call("Union", parts),
    })
}

fn minus_infinity() -> Expr {
    Expr::infinity().neg()
}

fn interval(lo: &Expr, hi: &Expr, closed_lo: bool, closed_hi: bool) -> Expr {
    Expr::call("Interval", vec![lo.clone(), hi.clone(), Expr::int(i64::from(closed_lo)), Expr::int(i64::from(closed_hi))])
}

fn value(e: &Expr) -> Option<f64> {
    eval_f64(e, &|_| None).ok().filter(|v| v.is_finite())
}

fn close(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(Ordering::Equal) || (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}

fn has_periodic(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Call(name, _) if PERIODIC.contains(&&**name)) || e.children().iter().any(has_periodic)
}

/// The real solutions of `g == 0`: those without an imaginary part that have a numeric value.
fn real_roots(g: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    Ok(solve(g, x)?.into_iter().filter(|r| !r.contains(&Expr::imag()) && value(r).is_some()).collect())
}

/// Expressions whose zeros are poles or edges of the domain: denominators, arguments of `log`, bases of fractional
/// powers, and arguments of `abs` (where the function bends).
fn pole_and_edge_expressions(f: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    let mut out = Vec::new();
    collect_edges(f, x, &mut out);
    let (_, den) = crate::rational::together(f)?;
    if den.contains(x) {
        out.push(den);
    }
    out.dedup();
    Ok(out)
}

fn collect_edges(e: &Expr, x: &Expr, out: &mut Vec<Expr>) {
    match e.kind() {
        Kind::Call(name, args) if matches!(&**name, "log" | "abs") => out.extend(args.iter().filter(|a| a.contains(x)).cloned()),
        Kind::Pow(b, p) if b.contains(x) && p.as_i64().is_none() => {
            out.push(b.clone());
        }
        Kind::Pow(b, p) if b.contains(x) && p.as_i64().is_some_and(|k| k < 0) => out.push(b.clone()),
        _ => {}
    }
    for c in e.children() {
        collect_edges(&c, x, out);
    }
}
