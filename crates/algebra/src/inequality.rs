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
        return periodic_inequality(&f, x, strict, upward);
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

/// `f` repeats every `2*pi` in the angle `t = a*x + b` (all its trig arguments are multiples of one linear angle).
/// Solve on one turn of `t`, then write the answer for every turn with an integer `n`: `(pi/6 + 2*pi*n, 5*pi/6 + 2*pi*n)`.
fn periodic_inequality(f: &Expr, x: &Expr, strict: bool, upward: bool) -> MathResult<Expr> {
    let unsupported =
        || MathError::unsupported("this inequality mixes trigonometric functions of x with other terms in x, so its solutions do not simply repeat");
    let angle = first_trig_argument(f).ok_or_else(unsupported)?;
    let (a, b) = crate::poly::linear_in(&angle, x)?.ok_or_else(unsupported)?;
    let a_val = value(&a).ok_or_else(unsupported)?;
    let t = Expr::sym("t");
    let g = crate::expand::expand(&crate::util::subs(f, &[(x.clone(), Expr::add(vec![t.clone(), b.neg()]).div(&a))])?)?;
    if g.contains(x) || g.free_symbols().iter().any(|s| &**s != "t") {
        return Err(unsupported());
    }
    // a pole such as tan(pi/2) evaluates to a huge finite number, so values that large count as undefined
    let at = |v: f64| eval_f64(&g, &|s| (s == "t").then_some(v)).ok().filter(|r| r.is_finite() && r.abs() < 1e12);
    // every term must repeat with the turn: compare at a few points
    for v in [0.37, 1.91, 4.2] {
        match (at(v), at(v + std::f64::consts::TAU)) {
            (Some(p), Some(q)) if (p - q).abs() <= 1e-8 * p.abs().max(1.0) => {}
            (None, None) => {}
            _ => return Err(unsupported()),
        }
    }
    let holds = |v: f64| {
        let zero = v.abs() < 1e-9;
        if zero { !strict } else { (v > 0.0) == upward }
    };
    let two_pi = Expr::mul(vec![Expr::int(2), Expr::pi()]);
    // candidate special angles in [0, 2*pi): zeros of g and of its poles, with the reflections and half turns that `solve` leaves out
    let mut sources = vec![g.clone()];
    crate::calculus::trig_denominators(&g, &t, &mut sources);
    let (_, den) = crate::rational::together(&g)?;
    if den.contains(&t) {
        sources.push(den);
    }
    let mut points: Vec<(f64, Expr)> = Vec::new();
    let mut zeros: Vec<f64> = Vec::new();
    for (si, src) in sources.iter().enumerate() {
        let Ok(roots) = solve(src, &t) else { continue };
        for r in roots.into_iter().filter(|r| !r.contains(&Expr::imag())) {
            for sign in [1, -1] {
                for half in 0..2 {
                    let cand = Expr::add(vec![Expr::mul(vec![r.clone(), Expr::int(sign)]), Expr::mul(vec![Expr::int(half), Expr::pi()])]);
                    let Some(v) = value(&cand) else { continue };
                    let turns = (v / std::f64::consts::TAU).floor();
                    let wrapped = v - turns * std::f64::consts::TAU;
                    let exact = crate::expand::expand(&Expr::add(vec![cand, Expr::mul(vec![two_pi.clone(), Expr::int(-(turns as i64))])]))?;
                    let w = value(&exact).unwrap_or(wrapped);
                    let hit = eval_f64(src, &|s| (s == "t").then_some(w)).ok().is_some_and(|z| z.abs() < 1e-9);
                    if hit && wrapped < std::f64::consts::TAU - 1e-9 {
                        if si == 0 {
                            zeros.push(wrapped);
                        }
                        if !points.iter().any(|(p, _)| close(*p, wrapped)) {
                            points.push((wrapped, exact));
                        }
                    }
                }
            }
        }
    }
    points.sort_by(|p, q| p.0.total_cmp(&q.0));
    let n = points.len();
    let is_zero = |v: f64| zeros.iter().any(|z| close(*z, v));
    // pieces around the circle: open arc before points[0] is the last one, wrapped
    let mut open = Vec::with_capacity(n);
    let mut at_point = Vec::with_capacity(n);
    for k in 0..n {
        let (Some(here), next) = (points.get(k), points.get((k + 1) % n.max(1))) else { continue };
        let upper = next.map_or(here.0 + 1.0, |q| if k + 1 < n { q.0 } else { q.0 + std::f64::consts::TAU });
        open.push(at((here.0 + upper) / 2.0).is_some_and(holds));
        at_point.push(match at(here.0) {
            Some(_) if is_zero(here.0) => !strict,
            Some(v) => holds(v),
            None => false,
        });
    }
    let nothing = || Expr::call("EmptySet", vec![]);
    let everything = || interval(&minus_infinity(), &Expr::infinity(), false, false);
    if n == 0 {
        return Ok(if at(0.5).is_some_and(holds) { everything() } else { nothing() });
    }
    // walk the circle (2n pieces: point k, then the arc after it) from a piece that fails, so runs do not wrap
    let pieces: Vec<bool> = (0..n).flat_map(|k| [at_point.get(k).copied().unwrap_or(false), open.get(k).copied().unwrap_or(false)]).collect();
    let Some(start) = pieces.iter().position(|ok| !ok) else { return Ok(everything()) };
    let m = pieces.len();
    let period = two_pi.div(&Expr::call("abs", vec![a.clone()]));
    let to_x = |angle: &Expr| -> MathResult<Expr> { crate::expand::expand(&Expr::add(vec![angle.clone(), b.neg()]).div(&a)) };
    let shift = Expr::mul(vec![period, Expr::sym("n")]);
    // the angle of the kk-th special point counted around the circle again and again
    let angle = |kk: usize, base: usize| {
        let turns = i64::try_from(kk / n).unwrap_or(0) - i64::try_from(base).unwrap_or(0);
        points.get(kk % n).map(|p| Expr::add(vec![p.1.clone(), Expr::mul(vec![two_pi.clone(), Expr::int(turns)])]))
    };
    let mut parts = Vec::new();
    let mut q = start + 1;
    while q < start + m {
        if !pieces.get(q % m).copied().unwrap_or(false) {
            q += 1;
            continue;
        }
        let first = q;
        while q + 1 < start + m && pieces.get((q + 1) % m).copied().unwrap_or(false) {
            q += 1;
        }
        let last = q;
        q += 1;
        // piece 2k is point k, piece 2k + 1 the open arc from point k to point k + 1
        let base = first / 2 / n;
        let (lo, closed_lo) = (angle(first / 2, base), first % 2 == 0);
        let (hi, closed_hi) = if last % 2 == 0 { (angle(last / 2, base), true) } else { (angle(last / 2 + 1, base), false) };
        let (Some(lo), Some(hi)) = (lo, hi) else { continue };
        let (xl, xh) = (to_x(&lo)?, to_x(&hi)?);
        let (xl, xh, cl, ch) = if a_val > 0.0 { (xl, xh, closed_lo, closed_hi) } else { (xh, xl, closed_hi, closed_lo) };
        parts.push(interval(&Expr::add(vec![xl, shift.clone()]), &Expr::add(vec![xh, shift.clone()]), cl, ch));
    }
    Ok(match parts.len() {
        0 => nothing(),
        1 => parts.remove(0),
        _ => Expr::call("Union", parts),
    })
}

/// The argument of the first trigonometric call in `e`, in reading order.
fn first_trig_argument(e: &Expr) -> Option<Expr> {
    if let Kind::Call(name, args) = e.kind()
        && PERIODIC.contains(&&**name)
        && let [u] = args.as_slice()
    {
        return Some(u.clone());
    }
    e.children().iter().find_map(first_trig_argument)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn rel(op: &str, f: Expr, rhs: Expr) -> String {
        solve_inequality(&Expr::call(op, vec![f, rhs]), &x()).unwrap().to_string()
    }

    #[test]
    fn trigonometric_inequalities_repeat_with_an_integer() {
        let sin = Expr::call("sin", vec![x()]);
        let half = Expr::rational(1, 2).unwrap();
        assert_eq!(rel("Gt", sin.clone(), half.clone()), "(2*n*pi + pi/6, 2*n*pi + 5*pi/6)");
        assert_eq!(rel("Ge", sin.clone(), half), "[2*n*pi + pi/6, 2*n*pi + 5*pi/6]");
        assert_eq!(rel("Gt", sin.clone(), Expr::int(2)), "∅");
        let double = Expr::call("sin", vec![Expr::mul(vec![Expr::int(2), x()])]);
        assert_eq!(rel("Gt", double, Expr::zero()), "(n*pi, n*pi + pi/2)");
        // tan has poles, which stay open
        let tan = Expr::call("tan", vec![x()]);
        assert!(rel("Gt", tan, Expr::one()).contains("2*n*pi + pi/2)"));
        // a plain x next to sin(x) does not repeat
        let mixed = Expr::add(vec![sin, x()]);
        assert!(solve_inequality(&Expr::call("Gt", vec![mixed, Expr::zero()]), &x()).is_err());
    }
}
