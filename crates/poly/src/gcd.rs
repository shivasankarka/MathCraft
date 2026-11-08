//! Greatest common divisors of sparse polynomials over the integers.
//!
//! The method is the textbook Euclidean algorithm over a unique factorisation domain: view both polynomials as
//! polynomials in one variable whose coefficients are polynomials in the others, take out the content (a gcd one
//! variable down), run pseudo-remainders on the primitive parts, and multiply the contents' gcd back in. It is exact
//! and works for any number of variables, but intermediate coefficients can grow, so very large inputs are slow.

use crate::{Mono, Poly, PolyError, exponent, shift};
use dashu::integer::IBig;

/// The integer gcd (never negative).
pub(crate) fn int_gcd(a: &IBig, b: &IBig) -> IBig {
    let (mut x, mut y) = (if *a < IBig::ZERO { -a } else { a.clone() }, if *b < IBig::ZERO { -b } else { b.clone() });
    while y != IBig::ZERO {
        let r = &x % &y;
        x = std::mem::replace(&mut y, r);
    }
    x
}

impl Poly {
    /// The gcd of all coefficients (never negative; zero for the zero polynomial).
    pub fn content(&self) -> IBig {
        self.terms.iter().fold(IBig::ZERO, |g, (_, c)| if g == IBig::ONE { g } else { int_gcd(&g, c) })
    }

    /// This polynomial as a polynomial in variable `v`: entry `i` is the coefficient of `x_v^i`, itself free of `x_v`.
    fn coeffs_in(&self, v: usize) -> Vec<Poly> {
        let mut out: Vec<Vec<(Mono, IBig)>> = Vec::new();
        for (m, c) in &self.terms {
            let e = exponent(*m, v) as usize;
            if out.len() <= e {
                out.resize_with(e + 1, Vec::new);
            }
            if let Some(bucket) = out.get_mut(e) {
                // removing one field keeps the order of the remaining monomials
                bucket.push((*m - ((e as u64) << shift(v)), c.clone()));
            }
        }
        out.into_iter().map(|terms| Poly { nvars: self.nvars, terms }).collect()
    }

    fn from_coeffs_in(nvars: usize, v: usize, coeffs: &[Poly]) -> Poly {
        let mut terms = Vec::new();
        for (i, c) in coeffs.iter().enumerate() {
            for (m, k) in &c.terms {
                terms.push((*m + ((i as u64) << shift(v)), k.clone()));
            }
        }
        Poly::from_terms(nvars, terms)
    }

    /// The greatest common divisor, with a positive leading coefficient (`gcd(0, 0) = 0`).
    pub fn gcd(&self, other: &Poly) -> Result<Poly, PolyError> {
        self.gcd_with(other, &mut || false)
    }

    pub fn gcd_with(&self, other: &Poly, stop: &mut dyn FnMut() -> bool) -> Result<Poly, PolyError> {
        if self.nvars != other.nvars {
            return Err(PolyError::Mismatch);
        }
        let g = gcd_rec(self, other, stop)?;
        Ok(positive(g))
    }
}

/// Negate so that the leading coefficient is positive.
fn positive(p: Poly) -> Poly {
    if p.terms.first().is_some_and(|(_, c)| *c < IBig::ZERO) { p.neg() } else { p }
}

fn gcd_rec(a: &Poly, b: &Poly, stop: &mut dyn FnMut() -> bool) -> Result<Poly, PolyError> {
    if stop() {
        return Err(PolyError::Stopped);
    }
    if a.is_zero() {
        return Ok(b.clone());
    }
    if b.is_zero() {
        return Ok(a.clone());
    }
    let n = a.nvars;
    // the first variable that occurs; with none, both are integers
    let Some(v) = (0..n).find(|v| a.degree_in(*v) > 0 || b.degree_in(*v) > 0) else {
        let g = int_gcd(&a.content(), &b.content());
        return Ok(Poly::constant(n, g));
    };
    // a polynomial that does not involve x_v is a "constant" here: the gcd is the gcd of its content with the other's
    let (ca, cb) = (a.coeffs_in(v), b.coeffs_in(v));
    let content = |cs: &[Poly], stop: &mut dyn FnMut() -> bool| -> Result<Poly, PolyError> {
        let mut g = Poly::zero(n);
        for c in cs {
            g = gcd_rec(&g, c, stop)?;
            if g.terms.len() == 1 && g.terms.first().is_some_and(|(m, k)| *m == 0 && *k == IBig::ONE) {
                break;
            }
        }
        Ok(g)
    };
    let (cont_a, cont_b) = (content(&ca, stop)?, content(&cb, stop)?);
    let cont = gcd_rec(&cont_a, &cont_b, stop)?;
    if ca.len() <= 1 || cb.len() <= 1 {
        // one of them is free of x_v: the common part cannot involve x_v beyond the contents
        let (lone, other) = if ca.len() <= 1 { (&cont_a, &cb) } else { (&cont_b, &ca) };
        let mut g = Poly::zero(n);
        for c in other {
            g = gcd_rec(&g, c, stop)?;
        }
        return gcd_rec(lone, &g, stop);
    }
    let primitive = |p: &Poly, c: &Poly| -> Result<Poly, PolyError> { p.div_exact(c)?.ok_or(PolyError::TooBig) };
    let (mut f, mut g) = (primitive(a, &cont_a)?, primitive(b, &cont_b)?);
    if f.coeffs_in(v).len() < g.coeffs_in(v).len() {
        std::mem::swap(&mut f, &mut g);
    }
    while !g.is_zero() {
        let r = pseudo_remainder(&f, &g, v, stop)?;
        if r.is_zero() {
            break;
        }
        f = g;
        let cr = content(&r.coeffs_in(v), stop)?;
        g = r.div_exact(&cr)?.ok_or(PolyError::TooBig)?;
    }
    // g is now the primitive gcd in x_v (or a constant in x_v: the contents' gcd is the answer)
    let result = if g.coeffs_in(v).len() <= 1 {
        cont
    } else {
        let cg = content(&g.coeffs_in(v), stop)?;
        let pg = g.div_exact(&cg)?.ok_or(PolyError::TooBig)?;
        pg.mul_with(&cont, stop)?
    };
    Ok(positive(result))
}

/// `lc(g)^k * f` reduced modulo `g`, as polynomials in `x_v`: the remainder of the division with no fractions.
fn pseudo_remainder(f: &Poly, g: &Poly, v: usize, stop: &mut dyn FnMut() -> bool) -> Result<Poly, PolyError> {
    let n = f.nvars;
    let gc = g.coeffs_in(v);
    let dg = gc.len().saturating_sub(1);
    let Some(lg) = gc.last() else { return Ok(f.clone()) };
    let mut r = f.coeffs_in(v);
    while r.len() > dg {
        if stop() {
            return Err(PolyError::Stopped);
        }
        let shift_by = r.len() - 1 - dg;
        let Some(lr) = r.last().cloned() else { break };
        // r := lc(g) * r - lc(r) * x^shift * g
        let mut next: Vec<Poly> = Vec::with_capacity(r.len());
        for c in &r {
            next.push(c.mul_with(lg, stop)?);
        }
        for (i, c) in gc.iter().enumerate() {
            if let Some(slot) = next.get_mut(i + shift_by) {
                *slot = slot.sub(&lr.mul_with(c, stop)?)?;
            }
        }
        next.pop();
        while next.last().is_some_and(Poly::is_zero) {
            next.pop();
        }
        r = next;
    }
    Ok(Poly::from_coeffs_in(n, v, &r))
}
