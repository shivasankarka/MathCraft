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
    let inner = gamma_ratio(&pull_squares(&e.map_children(simplify)?)?)?;
    let expanded = expand(&inner)?;
    let mut candidates = vec![inner.clone(), expanded.clone(), trig(&inner)?, trig(&expanded)?, factor(&inner)?, factor(&expanded)?, reduce_trig(&expanded)?];
    if has_complex_exp(&expanded) {
        // `(e**(I*x) + e**(-I*x))/2` is `cos(x)`: go through the trigonometric form
        let t = expand(&crate::complex::rewrite_trig(&expanded)?)?;
        candidates.push(t.clone());
        candidates.push(reduce_trig(&t)?);
    }
    for base in [&inner, &expanded] {
        candidates.push(double_angle(base)?);
    }
    let multiple = crate::trig::expand_trig(&expanded)?;
    if multiple != expanded {
        // sin(2*x)/sin(x) is 2*cos(x): through the addition formulas, then cancel
        let reduced = reduce_trig(&multiple)?;
        candidates.push(crate::rational::cancel(&multiple)?);
        candidates.push(crate::rational::cancel(&reduced)?);
        candidates.push(multiple);
        candidates.push(reduced);
    }
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
    // a single fraction is preferred to a nested one of the same size: 1/(1 + 1/x) is x/(x + 1)
    if has_denominator(&best) {
        let (n, d) = crate::rational::together_deep(&best)?;
        let one = crate::rational::cancel(&n.div(&d))?;
        if one.size() <= best.size() && !has_nested_denominator(&one) {
            best = one;
        }
    }
    Ok(best)
}

/// Normal form modulo `sin**2 + cos**2 == 1`: every `cos(u)**k` with `k >= 2` becomes `cos(u)**(k % 2) * (1 - sin(u)**2)**(k / 2)`,
/// so any trigonometric polynomial identity reduces to zero.
fn reduce_trig(e: &Expr) -> MathResult<Expr> {
    fn go(e: &Expr) -> MathResult<Expr> {
        guard(e)?;
        if let Kind::Pow(b, k) = e.kind()
            && let Kind::Call(name, args) = b.kind()
            && matches!(name.as_ref(), "cos" | "cosh")
            && let [u] = args.as_slice()
            && let Some(n) = k.as_i64()
            && n >= 2
        {
            // cos^2 = 1 - sin^2, cosh^2 = 1 + sinh^2
            let (partner, sign) = if name.as_ref() == "cos" { ("sin", -1) } else { ("sinh", 1) };
            let s2 = Expr::pow(&Expr::call(partner, vec![u.clone()]), &Expr::int(2));
            let rest = Expr::add(vec![Expr::one(), Expr::mul(vec![Expr::int(sign), s2])]);
            return Ok(Expr::mul(vec![Expr::pow(b, &Expr::int(n % 2)), Expr::pow(&rest, &Expr::int(n / 2))]));
        }
        e.map_children(go)
    }
    if !has_trig_square(e) {
        return Ok(e.clone());
    }
    expand(&go(e)?)
}

fn has_complex_exp(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Pow(b, x) if *b == Expr::e() && x.contains(&Expr::imag())) || e.children().iter().any(has_complex_exp)
}

fn has_trig_square(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Pow(b, k) if matches!(b.kind(), Kind::Call(n, _) if matches!(n.as_ref(), "cos" | "cosh")) && k.as_i64().is_some_and(|n| n >= 2))
        || e.children().iter().any(has_trig_square)
}

/// A negative power whose base contains another negative power, as in `1/(1 + 1/x)`.
fn has_nested_denominator(e: &Expr) -> bool {
    match e.kind() {
        Kind::Pow(b, k) if k.as_num().is_some_and(Num::is_negative) && has_denominator(b) => true,
        _ => e.children().iter().any(has_nested_denominator),
    }
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

/// `sqrt(4 - 4*x**2)` is `2*sqrt(1 - x**2)`: take the largest square out of the integer content under a square root.
pub fn pull_squares(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    let e = e.map_children(pull_squares)?;
    if let Kind::Pow(b, k) = e.kind()
        && let Kind::Add(terms) = b.kind()
        && (*k == Expr::half() || *k == Expr::half().neg())
    {
        let mut g = 0u64;
        for t in terms {
            let Some(n) = t.split_coeff().0.as_i64() else { return Ok(e) };
            let (mut a, mut c) = (g, n.unsigned_abs());
            while c != 0 {
                (a, c) = (c, a % c);
            }
            g = a;
        }
        let side = (2..).take_while(|s: &u64| s.saturating_mul(*s) <= g).filter(|s| g.is_multiple_of(s * s)).last();
        if let Some(s) = side
            && let Ok(si) = i64::try_from(s)
            && let Some(inv) = Expr::rational(1, si * si)
        {
            let rest = expand(&Expr::mul(vec![inv, b.clone()]))?;
            let root = Expr::pow(&rest, k);
            return Ok(if *k == Expr::half() {
                Expr::mul(vec![Expr::int(si), root])
            } else {
                Expr::mul(vec![Expr::rational(1, si).unwrap_or_else(Expr::one), root])
            });
        }
    }
    Ok(e)
}

/// Contract `2*sin(u)*cos(u)` to `sin(2*u)`, and `cos(u)**2 - sin(u)**2`, `1 - 2*sin(u)**2`, `2*cos(u)**2 - 1` to `cos(2*u)`.
fn double_angle(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    let e = e.map_children(double_angle)?;
    match e.kind() {
        Kind::Mul(fs) => {
            let mut out: Vec<Expr> = fs.clone();
            for f in fs {
                if let Kind::Call(n, a) = f.kind()
                    && &**n == "sin"
                    && let [u] = a.as_slice()
                    && let Some(c) = out.iter().position(|o| *o == Expr::call("cos", vec![u.clone()]))
                    && let Some(sn) = out.iter().position(|o| o == f)
                {
                    let (hi, lo) = (c.max(sn), c.min(sn));
                    out.remove(hi);
                    out.remove(lo);
                    out.push(Expr::mul(vec![Expr::half(), Expr::call("sin", vec![Expr::mul(vec![Expr::int(2), u.clone()])])]));
                    break;
                }
            }
            Ok(Expr::mul(out))
        }
        Kind::Add(terms) => Ok(cos_double(terms)),
        _ => Ok(e),
    }
}

fn cos_double(terms: &[Expr]) -> Expr {
    let info: Vec<Term> = terms.iter().map(classify).collect();
    let mut used = vec![false; info.len()];
    let mut extra: Vec<Expr> = Vec::new();
    for i in 0..info.len() {
        for j in (i + 1)..info.len() {
            if used.get(i) == Some(&true) || used.get(j) == Some(&true) {
                continue;
            }
            let (Some(a), Some(b)) = (info.get(i), info.get(j)) else { continue };
            let two = Num::int(2);
            let combined = match (&a.trig, &b.trig) {
                // c*R*cos(u)^2 - c*R*sin(u)^2
                (Some((sa, ua)), Some((sb, ub))) if sa != sb && ua == ub && a.rest == b.rest && a.coeff == b.coeff.neg() => {
                    let c = if *sa { &b.coeff } else { &a.coeff };
                    Some((c.clone(), a.rest.clone(), ua.clone()))
                }
                (None, Some((is_sin, u))) | (Some((is_sin, u)), None) => {
                    let (plain, t) = if a.trig.is_none() { (a, b) } else { (b, a) };
                    if plain.rest == t.rest && t.coeff == plain.coeff.mul(&two).neg() {
                        let c = if *is_sin { plain.coeff.clone() } else { plain.coeff.neg() };
                        Some((c, plain.rest.clone(), u.clone()))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some((c, rest, u)) = combined {
                for k in [i, j] {
                    if let Some(slot) = used.get_mut(k) {
                        *slot = true;
                    }
                }
                extra.push(Expr::mul(vec![Expr::num(c), rest, Expr::call("cos", vec![Expr::mul(vec![Expr::int(2), u])])]));
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
    fn double_angles_and_square_content() {
        let (s, c) = (Expr::call("sin", vec![x()]), Expr::call("cos", vec![x()]));
        assert_eq!(show(Expr::mul(vec![Expr::int(2), s.clone(), c.clone()])), "sin(2*x)");
        let (s2, c2) = (Expr::pow(&s, &Expr::int(2)), Expr::pow(&c, &Expr::int(2)));
        assert_eq!(show(Expr::add(vec![c2.clone(), s2.clone().neg()])), "cos(2*x)");
        assert_eq!(show(Expr::add(vec![Expr::one(), Expr::mul(vec![Expr::int(-2), s2])])), "cos(2*x)");
        assert_eq!(show(Expr::add(vec![Expr::mul(vec![Expr::int(2), c2]), Expr::int(-1)])), "cos(2*x)");
        let u = Expr::add(vec![Expr::mul(vec![Expr::int(-4), Expr::pow(&x(), &Expr::int(2))]), Expr::int(4)]);
        assert_eq!(show(Expr::pow(&u, &Expr::half()).div(&Expr::int(2))), "sqrt(-x**2 + 1)");
    }

    #[test]
    fn trig_quotients_and_even_roots() {
        let (s, c) = (Expr::call("sin", vec![x()]), Expr::call("cos", vec![x()]));
        assert_eq!(show(s.div(&c)), "tan(x)");
        assert_eq!(show(Expr::pow(&Expr::pow(&x(), &Expr::int(2)), &Expr::half())), "abs(x)");
    }
}

/// `Gamma(n + 2)/Gamma(n + 1)` is `n + 1`: a Gamma factor over another whose argument differs by a small integer becomes
/// the product of the arguments in between (up to four repeats of each factor).
pub fn gamma_ratio(e: &Expr) -> MathResult<Expr> {
    let e = e.map_children(gamma_ratio)?;
    let Kind::Mul(factors) = e.kind() else { return Ok(e) };
    let mut top: Vec<Expr> = Vec::new();
    let mut bottom: Vec<Expr> = Vec::new();
    let mut rest: Vec<Expr> = Vec::new();
    for t in factors {
        let (g, power) = match t.kind() {
            Kind::Pow(g, p) => (g.clone(), p.as_i64()),
            _ => (t.clone(), Some(1)),
        };
        match (g.kind(), power) {
            (Kind::Call(name, args), Some(p)) if &**name == "Gamma" && args.len() == 1 && (1..=4).contains(&p.abs()) => {
                let arg = args.first().cloned().unwrap_or_else(Expr::zero);
                for _ in 0..p.abs() {
                    if p > 0 { top.push(arg.clone()) } else { bottom.push(arg.clone()) }
                }
            }
            _ => rest.push(t.clone()),
        }
    }
    let mut changed = false;
    let mut i = 0;
    while i < top.len() {
        let hit = bottom.iter().enumerate().find_map(|(j, b)| {
            let d = expand(&Expr::add(vec![top.get(i)?.clone(), b.neg()])).ok()?.as_i64()?;
            (d.abs() <= 20).then_some((j, d))
        });
        let Some((j, d)) = hit else {
            i += 1;
            continue;
        };
        let (a, b) = (top.remove(i), bottom.remove(j));
        changed = true;
        // Gamma(b + d)/Gamma(b) = b (b + 1) ... (b + d - 1); for d < 0 the reciprocal of (a)...(a - d - 1)
        if d >= 0 {
            rest.extend((0..d).map(|m| Expr::add(vec![b.clone(), Expr::int(m)])));
        } else {
            rest.extend((0..-d).map(|m| Expr::pow(&Expr::add(vec![a.clone(), Expr::int(m)]), &Expr::int(-1))));
        }
    }
    if !changed {
        return Ok(e);
    }
    rest.extend(top.into_iter().map(|a| Expr::call("Gamma", vec![a])));
    rest.extend(bottom.into_iter().map(|a| Expr::pow(&Expr::call("Gamma", vec![a]), &Expr::int(-1))));
    Ok(Expr::mul(rest))
}
