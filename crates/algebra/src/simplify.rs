//! A pragmatic `simplify`: tries expansion, factoring and the Pythagorean identity, and keeps the smallest result.

use crate::expand::expand;
use crate::poly::factor;
use crate::util::{guard, split_fraction};
use mathcraft_core::{Expr, Kind, MathResult, Num};

const MAX_SIZE: usize = 3000;

pub fn simplify(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    if e.size() > MAX_SIZE {
        return Ok(e.clone());
    }
    let inner = e.map_children(simplify)?;
    let expanded = expand(&inner)?;
    let mut candidates = vec![inner.clone(), expanded.clone(), trig(&inner)?, trig(&expanded)?, factor(&inner)?, factor(&expanded)?];
    if has_denominator(&inner) {
        let cancelled = crate::rational::cancel(&inner)?;
        let (n, d) = split_fraction(&cancelled);
        candidates.push(Expr::mul(vec![factor(&n)?, Expr::pow(&factor(&d)?, &Expr::int(-1))]));
        candidates.push(cancelled);
    }
    let mut best = inner;
    for c in candidates {
        if c.size() < best.size() {
            best = c;
        }
    }
    Ok(best)
}

fn has_denominator(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Pow(_, k) if k.as_num().is_some_and(Num::is_negative)) || e.children().iter().any(has_denominator)
}

/// `sin(u)/cos(u)` is `tan(u)` (and powers of it); `sqrt(u**2)` is `abs(u)` because symbols are real.
fn rewrite_products(e: &Expr) -> Expr {
    match e.kind() {
        Kind::Pow(b, k) if k == &Expr::half() => match b.kind() {
            Kind::Pow(c, m) if m.as_i64().is_some_and(|m| m > 0 && m % 2 == 0) => {
                Expr::pow(&Expr::call("abs", vec![c.clone()]), &Expr::int(m.as_i64().unwrap_or(2) / 2))
            }
            _ => e.clone(),
        },
        Kind::Mul(fs) => {
            let mut out: Vec<Expr> = fs.clone();
            for f in fs {
                if let Kind::Pow(b, k) = f.kind()
                    && let Kind::Call(name, args) = b.kind()
                    && &**name == "cos"
                    && let Some(n) = k.as_i64()
                    && n < 0
                    && let [u] = args.as_slice()
                {
                    let sin_k = Expr::pow(&Expr::call("sin", vec![u.clone()]), &Expr::int(-n));
                    if let Some(pos) = out.iter().position(|o| *o == sin_k) {
                        out.remove(pos);
                        if let Some(pos) = out.iter().position(|o| o == f) {
                            out.remove(pos);
                        }
                        out.push(Expr::pow(&Expr::call("tan", vec![u.clone()]), &Expr::int(-n)));
                    }
                }
            }
            Expr::mul(out)
        }
        _ => e.clone(),
    }
}

/// Apply `sin(u)**2 + cos(u)**2 == 1` (and its rearrangements) in every sum.
fn trig(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    let e = e.map_children(trig)?;
    match e.kind() {
        Kind::Add(terms) => Ok(pythagorean(terms)),
        _ => Ok(rewrite_products(&e)),
    }
}

struct Term {
    coeff: Num,
    rest: Expr,
    /// `(is_sin, u)` when the term contains `sin(u)**2` or `cos(u)**2`; `rest` is then everything else.
    trig: Option<(bool, Expr)>,
}

fn classify(t: &Expr) -> Term {
    let (coeff, rest) = t.split_coeff();
    let factors: Vec<Expr> = match rest.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![rest.clone()],
    };
    for (i, f) in factors.iter().enumerate() {
        if let Kind::Pow(b, k) = f.kind()
            && k.as_i64() == Some(2)
            && let Kind::Call(name, args) = b.kind()
            && let ([u], "sin" | "cos") = (args.as_slice(), name.as_ref())
        {
            let others: Vec<Expr> = factors.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, f)| f.clone()).collect();
            return Term { coeff, rest: Expr::mul(others), trig: Some((name.as_ref() == "sin", u.clone())) };
        }
    }
    Term { coeff, rest, trig: None }
}

fn pythagorean(terms: &[Expr]) -> Expr {
    let info: Vec<Term> = terms.iter().map(classify).collect();
    let mut used = vec![false; info.len()];
    let mut extra: Vec<Expr> = Vec::new();
    for i in 0..info.len() {
        for j in (i + 1)..info.len() {
            if used.get(i) == Some(&true) || used.get(j) == Some(&true) {
                continue;
            }
            let (Some(a), Some(b)) = (info.get(i), info.get(j)) else { continue };
            let combined = match (&a.trig, &b.trig) {
                // c*R*sin(u)^2 + c*R*cos(u)^2 = c*R
                (Some((sa, ua)), Some((sb, ub))) if sa != sb && ua == ub && a.rest == b.rest && a.coeff == b.coeff => {
                    Some(Expr::mul(vec![Expr::num(a.coeff.clone()), a.rest.clone()]))
                }
                // c*R - c*R*sin(u)^2 = c*R*cos(u)^2 (and the mirror image)
                (None, Some((is_sin, u))) | (Some((is_sin, u)), None) => {
                    let (plain, trig_term) = if a.trig.is_none() { (a, b) } else { (b, a) };
                    if plain.rest == trig_term.rest && plain.coeff == trig_term.coeff.neg() {
                        let other = Expr::call(if *is_sin { "cos" } else { "sin" }, vec![u.clone()]);
                        Some(Expr::mul(vec![Expr::num(plain.coeff.clone()), plain.rest.clone(), Expr::pow(&other, &Expr::int(2))]))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(c) = combined {
                if let Some(slot) = used.get_mut(i) {
                    *slot = true;
                }
                if let Some(slot) = used.get_mut(j) {
                    *slot = true;
                }
                extra.push(c);
            }
        }
    }
    let mut out: Vec<Expr> = terms.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(t, _)| t.clone()).collect();
    out.extend(extra);
    Expr::add(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn sq(name: &str) -> Expr {
        Expr::pow(&Expr::call(name, vec![x()]), &Expr::int(2))
    }

    #[test]
    fn pythagorean_identity() {
        assert_eq!(simplify(&Expr::add(vec![sq("sin"), sq("cos")])).unwrap(), Expr::one());
        assert_eq!(simplify(&Expr::add(vec![Expr::one(), sq("sin").neg()])).unwrap().to_string(), "cos(x)**2");
        assert_eq!(simplify(&Expr::add(vec![Expr::mul(vec![Expr::int(3), sq("sin")]), Expr::mul(vec![Expr::int(3), sq("cos")])])).unwrap(), Expr::int(3));
    }

    #[test]
    fn expands_or_factors_whichever_is_smaller() {
        let e = Expr::mul(vec![Expr::add(vec![x(), Expr::int(1)]), Expr::add(vec![x(), Expr::int(-1)])]);
        assert_eq!(simplify(&e).unwrap().to_string(), "x**2 - 1");
        let big = Expr::add(vec![Expr::pow(&x(), &Expr::int(2)), Expr::mul(vec![Expr::int(2), x()]), Expr::one()]);
        assert_eq!(simplify(&big).unwrap().to_string(), "(x + 1)**2");
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    fn show(e: Expr) -> String {
        simplify(&e).unwrap().to_string()
    }
    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn cancels_common_factors() {
        let p = |c: &[i64]| crate::poly::from_poly(&c.iter().map(|v| Expr::int(*v)).collect::<Vec<_>>(), &x());
        assert_eq!(show(p(&[-1, 0, 1]).div(&p(&[-1, 1]))), "x + 1");
        assert_eq!(show(p(&[-1, 0, 1]).div(&p(&[1, 2, 1]))), "(x - 1)/(x + 1)");
    }

    #[test]
    fn trig_quotients_and_even_roots() {
        let (s, c) = (Expr::call("sin", vec![x()]), Expr::call("cos", vec![x()]));
        assert_eq!(show(s.div(&c)), "tan(x)");
        assert_eq!(show(Expr::pow(&Expr::pow(&x(), &Expr::int(2)), &Expr::half())), "abs(x)");
    }
}
