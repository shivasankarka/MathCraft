//! Fourier transforms `F(w) = integral f(t) exp(-I w t) dt` over the real line, for sums of `c * t**n * core(t)` with
//! `core` a Gaussian `exp(a t**2 + b t + c)` (`a < 0`), `exp(-a*abs(t))` or `1/(p t**2 + r)`. The factor `t**n` is
//! `(I d/dw)**n` of the transform. A symbolic coefficient is taken to be positive where the sign decides convergence.

use crate::calculus::diff_n;
use crate::expand::expand;
use crate::poly::to_poly;
use crate::simplify::simplify;
use crate::util::{guard, require_symbol, sign_of, subs};
use mathcraft_core::{Expr, Kind, MathResult};

/// Is `e` negative, as far as its numeric coefficient shows?
fn negative(e: &Expr) -> bool {
    sign_of(e) == Some(-1) || (!e.free_symbols().is_empty() && e.split_coeff().0.is_negative())
}

fn positive(e: &Expr) -> bool {
    sign_of(e) == Some(1) || (!e.free_symbols().is_empty() && !e.split_coeff().0.is_negative() && !e.split_coeff().0.is_zero())
}

fn abs_of(t: &Expr) -> Expr {
    Expr::call("abs", vec![t.clone()])
}

/// The Fourier transform of `f(t)` as a function of `w`, or `None` when no rule applies.
pub fn fourier(f: &Expr, t: &Expr, w: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(t)?;
    require_symbol(w)?;
    guard(f)?;
    let expanded = expand(f)?;
    let terms = match expanded.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut total = Vec::with_capacity(terms.len());
    for term in &terms {
        match transform_term(term, t, w)? {
            Some(v) => total.push(v),
            None => return Ok(None),
        }
    }
    Ok(Some(simplify(&Expr::add(total))?))
}

/// The inverse transform `1/(2 pi) integral F(w) exp(I w t) dw`, by duality with `fourier`.
pub fn inverse_fourier(f: &Expr, w: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(w)?;
    require_symbol(t)?;
    let Some(g) = fourier(f, w, t)? else { return Ok(None) };
    let flipped = subs(&g, &[(t.clone(), t.neg())])?;
    Ok(Some(simplify(&Expr::mul(vec![flipped, Expr::pow(&Expr::mul(vec![Expr::int(2), Expr::pi()]), &Expr::int(-1))]))?))
}

fn transform_term(term: &Expr, t: &Expr, w: &Expr) -> MathResult<Option<Expr>> {
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (mut coef, mut n, mut core): (Vec<Expr>, i64, Option<Expr>) = (Vec::new(), 0, None);
    for f in &factors {
        if !f.contains(t) {
            coef.push(f.clone());
        } else if f == t {
            n += 1;
        } else if let Kind::Pow(b, k) = f.kind()
            && b == t
            && let Some(k) = k.as_i64().filter(|k| *k > 0)
        {
            n += k;
        } else if core.is_none() {
            core = Some(f.clone());
        } else {
            return Ok(None);
        }
    }
    let Some(core) = core else { return Ok(None) };
    let Some(base) = core_transform(&core, t, w)? else { return Ok(None) };
    let Ok(order) = u32::try_from(n) else { return Ok(None) };
    let g = if order == 0 { base } else { Expr::mul(vec![Expr::pow(&Expr::imag(), &Expr::int(n)), diff_n(&base, w, order)?]) };
    Ok(Some(Expr::mul(vec![Expr::mul(coef), g])))
}

fn core_transform(core: &Expr, t: &Expr, w: &Expr) -> MathResult<Option<Expr>> {
    let pi = Expr::pi();
    match core.kind() {
        Kind::Pow(b, u) if *b == Expr::e() => {
            // exp(-a*abs(t))
            if let Some(a) = abs_coefficient(u, t) {
                if !positive(&a.neg()) {
                    return Ok(None);
                }
                let a = a.neg();
                let den = Expr::add(vec![Expr::pow(&a, &Expr::int(2)), Expr::pow(w, &Expr::int(2))]);
                return Ok(Some(Expr::mul(vec![Expr::int(2), a, Expr::pow(&den, &Expr::int(-1))])));
            }
            let Some(c) = to_poly(u, t)? else { return Ok(None) };
            if c.len() != 3 {
                return Ok(None);
            }
            let at = |i: usize| c.get(i).cloned().unwrap_or_else(Expr::zero);
            let (c0, b1, a2) = (at(0), at(1), at(2));
            if !negative(&a2) {
                return Ok(None);
            }
            // sqrt(pi/(-a)) * exp(c0 + (b - I w)**2/(-4 a))
            let na = a2.neg();
            let shift = Expr::add(vec![b1, Expr::mul(vec![Expr::imag(), w.clone()]).neg()]);
            let exponent =
                Expr::add(vec![c0, Expr::mul(vec![Expr::pow(&shift, &Expr::int(2)), Expr::pow(&Expr::mul(vec![Expr::int(4), na.clone()]), &Expr::int(-1))])]);
            let root = Expr::pow(&Expr::mul(vec![pi, Expr::pow(&na, &Expr::int(-1))]), &Expr::half());
            Ok(Some(Expr::mul(vec![root, Expr::pow(&Expr::e(), &expand(&exponent)?)])))
        }
        Kind::Pow(den, k) if k.as_i64() == Some(-1) => {
            let Some(c) = to_poly(den, t)? else { return Ok(None) };
            let at = |i: usize| c.get(i).cloned().unwrap_or_else(Expr::zero);
            if c.len() != 3 || !at(1).is_zero() || !positive(&at(2)) || !positive(&at(0)) {
                return Ok(None);
            }
            // pi/(p a) exp(-a |w|) with a = sqrt(r/p)
            let a = Expr::pow(&at(0).div(&at(2)), &Expr::half());
            let decay = Expr::pow(&Expr::e(), &Expr::mul(vec![a.clone(), abs_of(w)]).neg());
            Ok(Some(Expr::mul(vec![Expr::pi(), Expr::pow(&Expr::mul(vec![at(2), a]), &Expr::int(-1)), decay])))
        }
        _ => Ok(None),
    }
}

/// `c` when `u` is `c * abs(t)`.
fn abs_coefficient(u: &Expr, t: &Expr) -> Option<Expr> {
    let target = abs_of(t);
    if *u == target {
        return Some(Expr::one());
    }
    let Kind::Mul(fs) = u.kind() else { return None };
    let rest: Vec<Expr> = fs.iter().filter(|f| **f != target).cloned().collect();
    (rest.len() + 1 == fs.len() && !rest.iter().any(|f| f.contains(t))).then(|| Expr::mul(rest))
}
