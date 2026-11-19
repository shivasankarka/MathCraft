//! Laplace transforms. `laplace` handles sums of `c * t**n * exp(a*t) * trig(w*t)` (`trig` one of `sin`, `cos`,
//! `sinh`, `cosh`; `n` a non-negative integer, or any exponent above -1 when nothing else depends on `t`) through
//! the shift rule and `t**n f <-> (-1)**n F^(n)`; products and powers of trig functions go through exponentials.
//! `inverse_laplace` goes the other way for rational functions (partial fractions, quadratics up to the square)
//! and for powers `s**p`.

use crate::calculus::diff_n;
use crate::complex::rewrite_exp;
use crate::expand::expand;
use crate::poly::{factor, linear_in, to_poly};
use crate::rational::{apart, cancel};
use crate::simplify::simplify;
use crate::util::{guard, require_symbol, sign_of, split_fraction};
use mathcraft_core::{Expr, Kind, MathResult};

/// The Laplace transform `integral_0^oo f(t) exp(-s t) dt`, or `None` when no rule applies.
pub fn laplace(f: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(t)?;
    require_symbol(s)?;
    guard(f)?;
    let expanded = expand(f)?;
    let terms = match expanded.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut total = Vec::with_capacity(terms.len());
    for term in &terms {
        let piece = match transform_term(term, t, s)? {
            Some(p) => p,
            None => match exponential_pieces(term, t, s)? {
                Some(p) => p,
                None => return Ok(None),
            },
        };
        total.push(piece);
    }
    Ok(Some(simplify(&Expr::add(total))?))
}

/// A term with several trig factors or powers of them, through `sin(x) = (e**(I x) - e**(-I x))/(2 I)`.
fn exponential_pieces(term: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    if !has_trig(term) {
        return Ok(None);
    }
    let rewritten = expand(&rewrite_exp(term)?)?;
    let pieces = match rewritten.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![rewritten.clone()],
    };
    let mut total = Vec::with_capacity(pieces.len());
    for p in &pieces {
        if has_trig(p) {
            return Ok(None);
        }
        let Some(v) = transform_term(p, t, s)? else { return Ok(None) };
        total.push(v);
    }
    Ok(Some(cancel(&Expr::add(total))?))
}

fn has_trig(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Call(n, _) if is_trig(n)) || e.children().iter().any(has_trig)
}

fn is_trig(name: &str) -> bool {
    matches!(name, "sin" | "cos" | "sinh" | "cosh")
}

/// One term `c * t**n * exp(a*t) * trig(w*t)`.
fn transform_term(term: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (mut coef, mut power, mut rate) = (Vec::new(), Vec::new(), Vec::new());
    let mut trig: Option<(String, Expr)> = None;
    for f in &factors {
        if !f.contains(t) {
            coef.push(f.clone());
            continue;
        }
        match f.kind() {
            _ if f == t => power.push(Expr::one()),
            Kind::Pow(b, p) if b == t && p.as_num().is_some_and(|n| n.is_exact()) => power.push(p.clone()),
            Kind::Pow(b, u) if *b == Expr::e() => {
                let Some((k, c)) = linear_in(u, t)? else { return Ok(None) };
                if !c.is_zero() {
                    coef.push(Expr::pow(&Expr::e(), &c));
                }
                rate.push(k);
            }
            Kind::Call(name, args) if is_trig(name) && trig.is_none() => {
                let [arg] = args.as_slice() else { return Ok(None) };
                let Some((w, c)) = linear_in(arg, t)? else { return Ok(None) };
                if !c.is_zero() {
                    return Ok(None);
                }
                trig = Some((name.to_string(), w));
            }
            _ => return Ok(None),
        }
    }
    let n = Expr::add(power);
    let a = Expr::add(rate);
    let c = Expr::mul(coef);
    let shifted = Expr::add(vec![s.clone(), a.neg()]);
    let base = match &trig {
        None if n.as_i64().is_none() => {
            // t**p alone: Gamma(p + 1)/s**(p + 1).
            let p1 = Expr::add(vec![n.clone(), Expr::one()]);
            if !a.is_zero() || !n.is_num() || sign_of(&p1) != Some(1) {
                return Ok(None);
            }
            let g = Expr::call("Gamma", vec![p1.clone()]);
            return Ok(Some(Expr::mul(vec![c, g, Expr::pow(s, &p1.neg())])));
        }
        _ if n.as_i64().is_none_or(|k| k < 0) => return Ok(None),
        None => Expr::pow(&shifted, &Expr::int(-1)),
        Some((name, w)) => {
            let w2 = Expr::pow(w, &Expr::int(2));
            let u2 = Expr::pow(&shifted, &Expr::int(2));
            let (num, den) = match name.as_str() {
                "sin" => (w.clone(), Expr::add(vec![u2, w2])),
                "cos" => (shifted.clone(), Expr::add(vec![u2, w2])),
                "sinh" => (w.clone(), Expr::add(vec![u2, w2.neg()])),
                _ => (shifted.clone(), Expr::add(vec![u2, w2.neg()])),
            };
            num.div(&den)
        }
    };
    let order = n.as_i64().unwrap_or(0);
    let g = if order == 0 { base } else { diff_n(&base, s, u32::try_from(order).unwrap_or(0))? };
    let sign = if order % 2 == 0 { Expr::one() } else { Expr::int(-1) };
    Ok(Some(Expr::mul(vec![c, sign, g])))
}

/// The inverse Laplace transform of `f(s)` as a function of `t` (for `t > 0`), or `None` when no rule applies.
pub fn inverse_laplace(f: &Expr, s: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(s)?;
    require_symbol(t)?;
    guard(f)?;
    let parted = apart(f, s)?;
    let terms = match parted.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![parted.clone()],
    };
    let mut total = Vec::with_capacity(terms.len());
    for term in &terms {
        match inverse_term(term, s, t)? {
            Some(v) => total.push(v),
            None => return Ok(None),
        }
    }
    Ok(Some(expand(&Expr::add(total))?))
}

/// `c / B**m` (with `B` linear, or quadratic over `m <= 2`) or `c * s**p`.
fn inverse_term(term: &Expr, s: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    if !term.contains(s) {
        return Ok(None); // a constant would be a Dirac delta
    }
    // c * s**p with p a negative non-integer: t**(-p - 1)/Gamma(-p).
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (free, dep): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| !f.contains(s));
    if let [only] = dep.as_slice()
        && let Kind::Pow(b, p) = only.kind()
        && b == s
        && let Some(pn) = p.as_num()
        && pn.is_exact()
        && !pn.is_integer()
        && pn.is_negative()
    {
        let q = p.neg();
        let value = Expr::mul(vec![
            Expr::mul(free),
            Expr::pow(t, &Expr::add(vec![q.clone(), Expr::int(-1)])),
            Expr::pow(&Expr::call("Gamma", vec![q]), &Expr::int(-1)),
        ]);
        return Ok(Some(value));
    }
    let (num, den) = split_fraction(term);
    if num.contains(s) && to_poly(&num, s)?.is_none_or(|c| c.len() > 2) {
        return Ok(None);
    }
    let factored = factor(&den)?;
    let parts = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![factored.clone()],
    };
    let (mut lead, mut base, mut m) = (Vec::new(), None, 1i64);
    for p in &parts {
        if !p.contains(s) {
            lead.push(p.clone());
            continue;
        }
        let (b, k) = match p.kind() {
            Kind::Pow(b, k) => (b.clone(), k.as_i64()),
            _ => (p.clone(), Some(1)),
        };
        let Some(k) = k.filter(|k| *k >= 1) else { return Ok(None) };
        if base.is_some() {
            return Ok(None);
        }
        base = Some(b);
        m = k;
    }
    let Some(b) = base else { return Ok(None) };
    let Some(bc) = to_poly(&b, s)? else { return Ok(None) };
    let scale = Expr::mul(lead);
    let at = |i: usize| bc.get(i).cloned().unwrap_or_else(Expr::zero);
    let (p0, q0, r0) = (at(2), at(1), at(0));
    let Some(np) = to_poly(&num, s)? else { return Ok(None) };
    let (n1, n0) = (np.get(1).cloned().unwrap_or_else(Expr::zero), np.first().cloned().unwrap_or_else(Expr::zero));
    match bc.len() {
        2 => {
            // c/(q0 s + r0)**m, a numerator constant after `apart`.
            if !n1.is_zero() {
                return Ok(None);
            }
            let a = r0.neg().div(&q0);
            let k = Expr::mul(vec![n0, Expr::pow(&Expr::mul(vec![scale, Expr::pow(&q0, &Expr::int(m))]), &Expr::int(-1))]);
            let value = Expr::mul(vec![
                k,
                Expr::pow(t, &Expr::int(m - 1)),
                Expr::pow(&Expr::e(), &Expr::mul(vec![a, t.clone()])),
                Expr::rational(1, factorial(m - 1)?).unwrap_or_else(Expr::one),
            ]);
            Ok(Some(value))
        }
        3 if m <= 2 => {
            let a = q0.neg().div(&Expr::mul(vec![Expr::int(2), p0.clone()]));
            let w2 = expand(&Expr::add(vec![r0.div(&p0), Expr::pow(&a, &Expr::int(2)).neg()]))?;
            let trig = match sign_of(&w2) {
                Some(1) => true,
                Some(-1) => false,
                _ if !w2.is_num() => true, // a symbolic frequency: assume it is real
                _ => return Ok(None),
            };
            let w = match w2.kind() {
                Kind::Pow(b, k) if trig && k.as_i64() == Some(2) => b.clone(),
                _ => Expr::pow(&if trig { w2.clone() } else { w2.neg() }, &Expr::half()),
            };
            let k = Expr::pow(&Expr::mul(vec![scale, Expr::pow(&p0, &Expr::int(m))]), &Expr::int(-1));
            // (n1 s + n0) = n1 (s - a) + (n0 + n1 a)
            let (u_coef, one_coef) = (n1.clone(), expand(&Expr::add(vec![n0, Expr::mul(vec![n1, a.clone()])]))?);
            let wt = Expr::mul(vec![w.clone(), t.clone()]);
            let (c_fn, s_fn) = if trig {
                (Expr::call("cos", vec![wt.clone()]), Expr::call("sin", vec![wt.clone()]))
            } else {
                (Expr::call("cosh", vec![wt.clone()]), Expr::call("sinh", vec![wt.clone()]))
            };
            // L^-1 of u/(u^2 +- w^2)^m and 1/(u^2 +- w^2)^m, before the shift.
            let (f_u, f_one) = if m == 1 {
                (c_fn, Expr::mul(vec![s_fn, Expr::pow(&w, &Expr::int(-1))]))
            } else {
                let half_w = Expr::pow(&Expr::mul(vec![Expr::int(2), w.clone()]), &Expr::int(-1));
                let fu = Expr::mul(vec![t.clone(), s_fn.clone(), half_w]);
                let cube = Expr::pow(&Expr::mul(vec![Expr::int(2), Expr::pow(&w, &Expr::int(3))]), &Expr::int(-1));
                let fo = if trig {
                    Expr::mul(vec![Expr::add(vec![s_fn, Expr::mul(vec![wt, c_fn]).neg()]), cube])
                } else {
                    Expr::mul(vec![Expr::add(vec![Expr::mul(vec![wt, c_fn]), s_fn.neg()]), cube])
                };
                (fu, fo)
            };
            let inner = Expr::add(vec![Expr::mul(vec![u_coef, f_u]), Expr::mul(vec![one_coef, f_one])]);
            let shift = Expr::pow(&Expr::e(), &Expr::mul(vec![a, t.clone()]));
            Ok(Some(Expr::mul(vec![k, shift, inner])))
        }
        _ => Ok(None),
    }
}

fn factorial(n: i64) -> MathResult<i64> {
    Ok((1..=n.clamp(0, 20)).product())
}
