//! Pattern matching and rewrite rules.
//!
//! A symbol whose name ends in `_` (`a_`, `u_`) is a wildcard: it matches any subexpression, and the same wildcard
//! must match the same thing everywhere in a pattern. Sums and products match in any order, and a pattern
//! with fewer terms than a sum or product matches part of it, so `sin(a_)**2 + cos(a_)**2 -> 1` fires inside
//! `sin(t)**2 + cos(t)**2 + 5`.

use crate::util::{guard, subs};
use mathcraft_core::{Expr, Kind, MathResult, budget};
use std::sync::Arc;

/// What each wildcard matched.
pub type Bindings = Vec<(Arc<str>, Expr)>;

fn wild_name(e: &Expr) -> Option<&Arc<str>> {
    match e.kind() {
        Kind::Sym(s) if s.len() > 1 && s.ends_with('_') => Some(s),
        _ => None,
    }
}

fn bind(b: &mut Bindings, name: &Arc<str>, e: &Expr) -> bool {
    match b.iter().find(|(n, _)| n == name) {
        Some((_, bound)) => bound == e,
        None => {
            b.push((name.clone(), e.clone()));
            true
        }
    }
}

/// Match `e` against `pat` as a whole. With `loose`, a missing term or factor may be matched by a wildcard
/// (`x` matches `a_*x` with `a_ = 1`, and `x` matches `x**n_` with `n_ = 1`).
pub fn match_expr(e: &Expr, pat: &Expr, loose: bool) -> MathResult<Option<Bindings>> {
    let mut b = Vec::new();
    Ok(go(e, pat, &mut b, loose)?.then_some(b))
}

fn go(e: &Expr, pat: &Expr, b: &mut Bindings, loose: bool) -> MathResult<bool> {
    budget::tick()?;
    if let Some(w) = wild_name(pat) {
        return Ok(bind(b, w, e));
    }
    match (e.kind(), pat.kind()) {
        (Kind::Add(es), Kind::Add(ps)) | (Kind::Mul(es), Kind::Mul(ps)) => {
            Ok(match_ac(es, ps, b, Mode { leftover: false, additive: matches!(e.kind(), Kind::Add(_)), loose })?.is_some())
        }
        // `x` matches `x + a_` (a_ = 0) and `a_*x` (a_ = 1)
        (_, Kind::Add(ps)) if loose => Ok(match_ac(std::slice::from_ref(e), ps, b, Mode { leftover: false, additive: true, loose })?.is_some()),
        (_, Kind::Mul(ps)) if loose => Ok(match_ac(std::slice::from_ref(e), ps, b, Mode { leftover: false, additive: false, loose })?.is_some()),
        (Kind::Pow(eb, ek), Kind::Pow(pb, pk)) => Ok(go(eb, pb, b, loose)? && go(ek, pk, b, loose)?),
        // `x` matches `x**n_` with n_ = 1
        (_, Kind::Pow(pb, pk)) if loose => Ok(go(e, pb, b, loose)? && go(&Expr::one(), pk, b, loose)?),
        (Kind::Call(n1, a1), Kind::Call(n2, a2)) => {
            if n1 != n2 || a1.len() != a2.len() {
                return Ok(false);
            }
            all_match(a1, a2, b, loose)
        }
        (Kind::List(a1), Kind::List(a2)) => {
            if a1.len() != a2.len() {
                return Ok(false);
            }
            all_match(a1, a2, b, loose)
        }
        (Kind::Eq(l1, r1), Kind::Eq(l2, r2)) => Ok(go(l1, l2, b, loose)? && go(r1, r2, b, loose)?),
        _ => Ok(e == pat),
    }
}

fn all_match(es: &[Expr], ps: &[Expr], b: &mut Bindings, loose: bool) -> MathResult<bool> {
    for (e, p) in es.iter().zip(ps) {
        if !go(e, p, b, loose)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Clone, Copy)]
struct Mode {
    /// Members of the sum or product that no pattern matched may remain (and are returned).
    leftover: bool,
    additive: bool,
    loose: bool,
}

/// Match the terms (or factors) `ps` against distinct members of `es`, in any order. Returns the members left over.
/// Without `leftover`, a lone bare wildcard takes everything not matched by the other patterns and nothing may remain.
fn match_ac(es: &[Expr], ps: &[Expr], b: &mut Bindings, mode: Mode) -> MathResult<Option<Vec<Expr>>> {
    // specific patterns first, bare wildcards last
    let mut order: Vec<&Expr> = ps.iter().filter(|p| wild_name(p).is_none()).collect();
    let specific = order.len();
    order.extend(ps.iter().filter(|p| wild_name(p).is_some()));
    let lone_wild = ps.len() - specific == 1 && !mode.leftover;
    let mut used = vec![false; es.len()];
    rec(es, &order, 0, &mut used, b, lone_wild, mode)
}

fn rec(es: &[Expr], order: &[&Expr], i: usize, used: &mut [bool], b: &mut Bindings, lone_wild: bool, mode: Mode) -> MathResult<Option<Vec<Expr>>> {
    let remaining = |used: &[bool]| es.iter().zip(used).filter(|(_, u)| !**u).map(|(e, _)| e.clone()).collect::<Vec<_>>();
    let Some(p) = order.get(i) else {
        let rest = remaining(used);
        return Ok(if mode.leftover || rest.is_empty() { Some(rest) } else { None });
    };
    if lone_wild && let Some(w) = wild_name(p) {
        let rest = remaining(used);
        if rest.is_empty() && !mode.loose {
            return Ok(None);
        }
        let whole = if mode.additive { Expr::add(rest) } else { Expr::mul(rest) };
        return Ok(bind(b, w, &whole).then(Vec::new));
    }
    for (j, ej) in es.iter().enumerate() {
        if used.get(j).copied().unwrap_or(true) {
            continue;
        }
        let snapshot = b.len();
        if go(ej, p, b, mode.loose)? {
            mark(used, j, true);
            if let Some(r) = rec(es, order, i + 1, used, b, lone_wild, mode)? {
                return Ok(Some(r));
            }
            mark(used, j, false);
        }
        b.truncate(snapshot);
    }
    Ok(None)
}

fn mark(used: &mut [bool], j: usize, v: bool) {
    if let Some(u) = used.get_mut(j) {
        *u = v;
    }
}

fn fill(repl: &Expr, b: &Bindings) -> MathResult<Expr> {
    let rules: Vec<(Expr, Expr)> = b.iter().map(|(n, v)| (Expr::sym(n), v.clone())).collect();
    subs(repl, &rules)
}

/// Rewrite every match of `pat` inside `e` (innermost first) to `repl`, with the wildcards filled in.
pub fn replace(e: &Expr, pat: &Expr, repl: &Expr) -> MathResult<Expr> {
    guard(e)?;
    let inner = e.map_children(|c| replace(c, pat, repl))?;
    if let Some(b) = match_expr(&inner, pat, false)? {
        return fill(repl, &b);
    }
    // the pattern may match some of the terms (or factors) of a longer sum (or product)
    let parts = match (inner.kind(), pat.kind()) {
        (Kind::Add(es), Kind::Add(ps)) if es.len() > ps.len() => Some((es, ps, true)),
        (Kind::Mul(es), Kind::Mul(ps)) if es.len() > ps.len() => Some((es, ps, false)),
        _ => None,
    };
    if let Some((es, ps, additive)) = parts {
        let mut b = Vec::new();
        if let Some(mut rest) = match_ac(es, ps, &mut b, Mode { leftover: true, additive, loose: false })? {
            rest.push(fill(repl, &b)?);
            return Ok(if additive { Expr::add(rest) } else { Expr::mul(rest) });
        }
    }
    Ok(inner)
}

/// Every subexpression of `e` that matches `pat` (outermost first, without repeats).
pub fn find_all(e: &Expr, pat: &Expr) -> MathResult<Vec<Expr>> {
    fn walk(e: &Expr, pat: &Expr, out: &mut Vec<Expr>) -> MathResult<()> {
        guard(e)?;
        if match_expr(e, pat, false)?.is_some() && !out.contains(e) {
            out.push(e.clone());
        }
        for c in e.children() {
            walk(&c, pat, out)?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(e, pat, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str) -> Expr {
        Expr::sym(n)
    }

    fn call(n: &str, a: &Expr) -> Expr {
        Expr::call(n, vec![a.clone()])
    }

    fn sq(e: &Expr) -> Expr {
        Expr::pow(e, &Expr::int(2))
    }

    fn text(r: MathResult<Expr>) -> String {
        r.map(|e| e.to_string()).unwrap_or_else(|e| e.to_string())
    }

    #[test]
    fn wildcards_bind_consistently() {
        let (a, x, y) = (s("a_"), s("x"), s("y"));
        let pat = Expr::mul(vec![a.clone(), call("sin", &a)]);
        assert!(match_expr(&Expr::mul(vec![x.clone(), call("sin", &x)]), &pat, false).ok().flatten().is_some());
        assert!(match_expr(&Expr::mul(vec![y, call("sin", &x)]), &pat, false).ok().flatten().is_none());
    }

    #[test]
    fn pythagorean_pattern_fires_inside_a_longer_sum() {
        let (a, t) = (s("a_"), s("t"));
        let pat = Expr::add(vec![sq(&call("sin", &a)), sq(&call("cos", &a))]);
        let e = Expr::add(vec![sq(&call("sin", &t)), sq(&call("cos", &t)), Expr::int(5)]);
        assert_eq!(text(replace(&e, &pat, &Expr::one())), "6");
        let nested = call("exp", &Expr::add(vec![sq(&call("sin", &s("u"))), sq(&call("cos", &s("u")))]));
        assert_eq!(text(replace(&nested, &pat, &Expr::one())), "E");
    }

    #[test]
    fn a_lone_wildcard_takes_the_rest() {
        let (a, x, y) = (s("a_"), s("x"), s("y"));
        let pat = Expr::mul(vec![Expr::int(2), a]);
        let e = Expr::mul(vec![Expr::int(2), x.clone(), y.clone()]);
        let b = match_expr(&e, &pat, false).ok().flatten().unwrap_or_default();
        assert_eq!(b.first().map(|(_, v)| v.to_string()), Some("x*y".into()));
        let bare = Expr::add(vec![s("b_"), Expr::int(1)]);
        assert_eq!(text(replace(&Expr::add(vec![x, Expr::int(1)]), &bare, &sq(&s("b_")))), "x**2");
    }

    #[test]
    fn exponent_wildcards_match_a_bare_power() {
        let pat = Expr::pow(&s("x"), &s("n_"));
        let b = match_expr(&s("x"), &pat, true).ok().flatten().unwrap_or_default();
        assert_eq!(b.first().map(|(_, v)| v.to_string()), Some("1".into()));
    }

    #[test]
    fn finds_every_match() {
        let (x, y) = (s("x"), s("y"));
        let e = Expr::add(vec![call("sin", &x), call("sin", &y), call("cos", &x)]);
        let found = find_all(&e, &call("sin", &s("u_"))).unwrap_or_default();
        assert_eq!(found.len(), 2);
    }
}
