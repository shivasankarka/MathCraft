//! The bridge between expression trees and the sparse integer polynomials of `mathcraft-poly`: `expand` uses it for
//! anything that is a polynomial in plain symbols with integer coefficients (the common, and the expensive, case).

use mathcraft_core::{Expr, Kind, MathError, MathResult, Num, budget};
use mathcraft_poly::{MAX_VARS, Poly, PolyError, exponent};

/// Largest number of terms a sparse expansion may produce.
pub const MAX_SPARSE_TERMS: usize = 3_000_000;
/// Largest number of partial products one multiplication may take (about 5 seconds of work).
const MAX_PARTIAL_PRODUCTS: usize = 400_000_000;

/// The polynomial for `e` in the symbols `vars`, if `e` is one: integer coefficients, non-negative integer powers.
pub(crate) struct Converter {
    pub vars: Vec<Expr>,
    cancelled: Option<MathError>,
}

impl Converter {
    /// A converter for the free symbols of all of `es`; `None` if there are too many, or one is a constant.
    pub fn for_all(es: &[&Expr]) -> Option<Converter> {
        let names: std::collections::BTreeSet<_> = es.iter().flat_map(|e| e.free_symbols()).collect();
        if names.len() > MAX_VARS {
            return None;
        }
        let vars: Vec<Expr> = names.iter().map(|n| Expr::sym(n)).collect();
        if vars.iter().any(Expr::is_constant_symbol) {
            return None;
        }
        Some(Converter { vars, cancelled: None })
    }

    /// A converter for the free symbols of `e`; `None` if there are too many, or one is a constant like `pi` or `I`.
    pub fn for_expr(e: &Expr) -> Option<Converter> {
        let names = e.free_symbols();
        if names.len() > MAX_VARS {
            return None;
        }
        let vars: Vec<Expr> = names.iter().map(|n| Expr::sym(n)).collect();
        if vars.iter().any(Expr::is_constant_symbol) {
            return None;
        }
        Some(Converter { vars, cancelled: None })
    }

    fn stop(&mut self) -> bool {
        match budget::tick() {
            Ok(()) => false,
            Err(e) => {
                self.cancelled = Some(e);
                true
            }
        }
    }

    /// `Ok(None)`: not a polynomial of this kind, use another method. `Err`: cancelled, out of time, too big.
    pub fn convert(&mut self, e: &Expr) -> MathResult<Option<Poly>> {
        let n = self.vars.len();
        Ok(match e.kind() {
            Kind::Num(Num::Int(i)) => Some(Poly::constant(n, i.clone())),
            Kind::Sym(_) => match self.vars.iter().position(|v| v == e) {
                Some(i) => Poly::var(n, i).ok(),
                None => None,
            },
            Kind::Add(terms) => {
                let mut acc = Poly::zero(n);
                for t in terms {
                    let Some(p) = self.convert(t)? else { return Ok(None) };
                    let Some(sum) = self.lift(acc.add(&p))? else { return Ok(None) };
                    acc = sum;
                }
                Some(acc)
            }
            Kind::Mul(factors) => {
                let mut acc = Poly::constant(n, 1.into());
                for f in factors {
                    let Some(p) = self.convert(f)? else { return Ok(None) };
                    self.check_size(&acc, &p)?;
                    let mut stop = || self.stop();
                    let r = acc.mul_with(&p, &mut stop);
                    let Some(product) = self.lift(r)? else { return Ok(None) };
                    acc = product;
                }
                Some(acc)
            }
            Kind::Pow(b, x) => {
                let Some(k) = x.as_i64().filter(|k| (0..=10_000).contains(k)) else { return Ok(None) };
                let Some(base) = self.convert(b)? else { return Ok(None) };
                let k = k as u32;
                // the largest partial product count is in the last squaring: about (terms of the result)^2 / 4
                if base.len() > 1 && k > 1 {
                    let bound = estimate_terms(base.len() as u64, u64::from(k));
                    if bound > MAX_SPARSE_TERMS as u128 {
                        return Err(MathError::too_large("expansion would produce too many terms"));
                    }
                }
                let mut stop = || self.stop();
                let r = base.pow_with(k, &mut stop);
                self.lift(r)?
            }
            _ => None,
        })
    }

    fn check_size(&self, a: &Poly, b: &Poly) -> MathResult<()> {
        if a.len().saturating_mul(b.len()) > MAX_PARTIAL_PRODUCTS {
            return Err(MathError::too_large("expansion would produce too many terms"));
        }
        Ok(())
    }

    /// A polynomial result: `None` when it cannot be represented (an exponent above the limit), so the caller falls
    /// back to the tree-based method; an error when the work was stopped.
    fn lift<T>(&mut self, r: Result<T, PolyError>) -> MathResult<Option<T>> {
        match r {
            Ok(v) => Ok(Some(v)),
            Err(PolyError::Stopped) => Err(self.cancelled.take().unwrap_or(MathError::Cancelled)),
            Err(_) => Ok(None),
        }
    }

    /// The expression for a polynomial in this converter's symbols.
    pub fn to_expr(&self, p: &Poly) -> MathResult<Expr> {
        if p.len() > MAX_SPARSE_TERMS {
            return Err(MathError::too_large("expansion would produce too many terms"));
        }
        let mut terms = Vec::with_capacity(p.len());
        for (m, c) in p.terms() {
            let mut factors = Vec::with_capacity(self.vars.len() + 1);
            factors.push(Expr::big(c.clone()));
            for (i, v) in self.vars.iter().enumerate() {
                match exponent(*m, i) {
                    0 => {}
                    1 => factors.push(v.clone()),
                    k => factors.push(Expr::pow(v, &Expr::int(i64::from(k)))),
                }
            }
            terms.push(Expr::mul(factors));
        }
        Ok(Expr::add(terms))
    }
}

/// Upper bound on the number of terms in `(k-term sum) ** n`: C(n + k - 1, k - 1), saturating.
fn estimate_terms(k: u64, n: u64) -> u128 {
    let mut c: u128 = 1;
    for i in 1..k.min(64) {
        c = c.saturating_mul(u128::from(n + i)) / u128::from(i);
        if c > u128::from(u64::MAX) {
            return u128::MAX;
        }
    }
    c
}

/// The greatest common divisor of two polynomials with integer coefficients in plain symbols, as an expression
/// (positive leading coefficient); `None` when they are not such polynomials or the gcd is too costly to find.
pub(crate) fn gcd(a: &Expr, b: &Expr) -> MathResult<Option<Expr>> {
    let Some(mut conv) = Converter::for_all(&[a, b]) else { return Ok(None) };
    let (Some(pa), Some(pb)) = (conv.convert(a)?, conv.convert(b)?) else { return Ok(None) };
    let mut stop = || conv.stop();
    let r = pa.gcd_with(&pb, &mut stop);
    let Some(g) = conv.lift(r)? else { return Ok(None) };
    Ok(Some(conv.to_expr(&g)?))
}

/// `e` factored into irreducibles over the integers, if it is a polynomial with integer coefficients in plain symbols
/// (`None` when it is not, or the search is too costly).
pub(crate) fn factor(e: &Expr) -> MathResult<Option<Expr>> {
    let Some(mut conv) = Converter::for_expr(e) else { return Ok(None) };
    if conv.vars.is_empty() {
        return Ok(None);
    }
    let Some(p) = conv.convert(e)? else { return Ok(None) };
    let mut stop = || conv.stop();
    let r = p.factor_with(&mut stop);
    let Some(Some(f)) = conv.lift(r)? else { return Ok(None) };
    let mut parts = Vec::with_capacity(f.factors.len() + 1);
    if f.content != 1.into() {
        parts.push(Expr::big(f.content.clone()));
    }
    for (g, k) in &f.factors {
        parts.push(Expr::pow(&conv.to_expr(g)?, &Expr::int(i64::from(*k))));
    }
    Ok(Some(Expr::mul(parts)))
}

/// The least common multiple, `a / gcd * b` (positive leading coefficient).
pub(crate) fn lcm(a: &Expr, b: &Expr) -> MathResult<Option<Expr>> {
    let Some(mut conv) = Converter::for_all(&[a, b]) else { return Ok(None) };
    let (Some(pa), Some(pb)) = (conv.convert(a)?, conv.convert(b)?) else { return Ok(None) };
    if pa.is_zero() || pb.is_zero() {
        return Ok(Some(Expr::zero()));
    }
    let mut stop = || conv.stop();
    let g = pa.gcd_with(&pb, &mut stop);
    let Some(g) = conv.lift(g)? else { return Ok(None) };
    let Ok(Some(q)) = pa.div_exact(&g) else { return Ok(None) };
    let mut stop = || conv.stop();
    let r = q.mul_with(&pb, &mut stop);
    let Some(l) = conv.lift(r)? else { return Ok(None) };
    let l = if l.terms().first().is_some_and(|(_, c)| *c < 0.into()) { l.neg() } else { l };
    Ok(Some(conv.to_expr(&l)?))
}

/// Cancel the common factor of a numerator and denominator that are integer polynomials; `None` when there is
/// nothing to cancel or they are not such polynomials.
pub(crate) fn cancel_pair(n: &Expr, d: &Expr) -> MathResult<Option<(Expr, Expr)>> {
    let Some(mut conv) = Converter::for_all(&[n, d]) else { return Ok(None) };
    let (Some(pn), Some(pd)) = (conv.convert(n)?, conv.convert(d)?) else { return Ok(None) };
    if pn.is_zero() || pd.is_zero() {
        return Ok(None);
    }
    let mut stop = || conv.stop();
    let r = pn.gcd_with(&pd, &mut stop);
    let Some(g) = conv.lift(r)? else { return Ok(None) };
    if g.len() == 1 && g.terms().first().is_some_and(|(m, c)| *m == 0 && *c == 1.into()) {
        return Ok(None);
    }
    let (Ok(Some(qn)), Ok(Some(qd))) = (pn.div_exact(&g), pd.div_exact(&g)) else { return Ok(None) };
    Ok(Some((conv.to_expr(&qn)?, conv.to_expr(&qd)?)))
}

/// `expand` for polynomials with integer coefficients in plain symbols; `None` for anything else.
pub(crate) fn expand(e: &Expr) -> MathResult<Option<Expr>> {
    if !matches!(e.kind(), Kind::Add(_) | Kind::Mul(_) | Kind::Pow(..)) {
        return Ok(None);
    }
    let Some(mut conv) = Converter::for_expr(e) else { return Ok(None) };
    // a symbol raised to more than MAX_EXP cannot be packed; the tree-based expansion handles those
    let Some(p) = conv.convert(e)? else { return Ok(None) };
    Ok(Some(conv.to_expr(&p)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::expand;
    use mathcraft_core::budget::{self, Budget};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn sym(n: &str) -> Expr {
        Expr::sym(n)
    }

    fn sample() -> Vec<Expr> {
        let (x, y, z) = (sym("x"), sym("y"), sym("z"));
        let sum = |a: &Expr, b: &Expr| Expr::add(vec![a.clone(), b.clone()]);
        vec![
            Expr::pow(&sum(&x, &y), &Expr::int(5)),
            Expr::mul(vec![Expr::pow(&sum(&x, &Expr::int(1)), &Expr::int(3)), Expr::add(vec![y.clone(), Expr::mul(vec![Expr::int(-2), z.clone()])])]),
            Expr::mul(vec![sum(&x, &y), sum(&y, &z), sum(&z, &x), Expr::int(-3)]),
            Expr::pow(&Expr::add(vec![x.clone(), Expr::mul(vec![Expr::int(2), y.clone()]), Expr::int(-7)]), &Expr::int(4)),
            Expr::add(vec![Expr::pow(&sum(&x, &y), &Expr::int(2)), Expr::mul(vec![Expr::int(-1), Expr::pow(&sum(&x, &y), &Expr::int(2))])]),
        ]
    }

    #[test]
    fn sparse_expansion_matches_the_tree_method() {
        for e in sample() {
            let fast = expand(&e).unwrap();
            let slow = crate::expand::expand_tree(&e).unwrap();
            assert_eq!(fast, slow, "{e}");
        }
    }

    #[test]
    fn falls_back_for_things_it_cannot_represent() {
        let x = sym("x");
        assert!(expand_sparse(&Expr::pow(&Expr::add(vec![x.clone(), Expr::int(1)]), &Expr::int(-2))).is_none());
        assert!(expand_sparse(&Expr::pow(&Expr::add(vec![x.clone(), Expr::rational(1, 2).unwrap()]), &Expr::int(2))).is_none());
        assert!(expand_sparse(&Expr::add(vec![Expr::call("sin", vec![x.clone()]), x.clone()])).is_none());
        assert!(expand_sparse(&Expr::add(vec![Expr::imag(), x.clone()])).is_none(), "I is not a plain symbol");
        // an exponent above the packing limit: fall back rather than fail
        let big = Expr::mul(vec![Expr::pow(&x, &Expr::int(100)), Expr::pow(&x, &Expr::int(100)), Expr::add(vec![x.clone(), Expr::int(1)])]);
        assert!(expand(&big).is_ok());
    }

    fn expand_sparse(e: &Expr) -> Option<Expr> {
        super::expand(e).unwrap()
    }

    #[test]
    fn a_huge_expansion_is_refused_and_a_cancel_stops_it() {
        let vars: Vec<Expr> = (1..=7).map(|i| sym(&format!("x{i}"))).collect();
        let base = Expr::add(std::iter::once(Expr::one()).chain(vars).collect());
        let e = Expr::pow(&base, &Expr::int(60));
        assert!(expand(&e).is_err(), "far too many terms");
        // a cancelled budget stops a real (but affordable) expansion
        let cancel = Arc::new(AtomicBool::new(true));
        let _g = budget::install(Budget::new(None, cancel, u64::MAX));
        let affordable = Expr::pow(&Expr::add((1..=6).map(|i| sym(&format!("y{i}"))).chain([Expr::one()]).collect()), &Expr::int(12));
        let r = (0..300).map(|_| expand(&affordable)).find(Result::is_err);
        assert!(r.is_some(), "the cancel flag is noticed");
    }
}

#[cfg(test)]
mod timing {
    use super::*;
    use std::time::Instant;

    fn poly(k: i64, sign: i64, shift: i64) -> Expr {
        let mut terms = vec![Expr::one()];
        for (i, c) in [3, 5, 7, 9, 11, 13, 15].iter().enumerate() {
            terms.push(Expr::mul(vec![Expr::int(if i == 6 { sign * c } else { *c }), Expr::sym(&format!("x{}", i + 1))]));
        }
        Expr::add(vec![Expr::pow(&Expr::add(terms), &Expr::int(7)), Expr::int(k + shift)])
    }

    #[test]
    #[ignore = "timing; run with --release --ignored --nocapture"]
    fn stages_of_a_large_expansion() {
        let (a, g) = (poly(-1, 1, 0), poly(3, -1, 0));
        let e = Expr::mul(vec![a, g]);
        let t = Instant::now();
        let mut conv = Converter::for_expr(&e).unwrap();
        let p = conv.convert(&e).unwrap().unwrap();
        println!("to poly: {} terms in {:?}", p.len(), t.elapsed());
        let t = Instant::now();
        let out = conv.to_expr(&p).unwrap();
        println!("to expr: {:?} ({} terms)", t.elapsed(), out.args().len());
    }
}
