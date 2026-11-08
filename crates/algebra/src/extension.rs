//! Factoring a univariate polynomial over a simple algebraic extension of the rationals (`Q(sqrt(2))`, `Q(I)`,
//! `Q(2**(1/3))`), by Trager's method: shift the variable until the norm is squarefree, factor the norm over Q and
//! recover each factor as a gcd over the extension.
//!
//! Field elements are vectors of rationals (coefficients of `1, a, a^2, ...` reduced by the minimal polynomial `m`).
#![allow(clippy::indexing_slicing, clippy::needless_range_loop)] // small dense rational linear algebra; every index is bounded by the field degree

use crate::poly::{factor, from_poly, to_poly};
use crate::util::subs;
use mathcraft_core::{Expr, Kind, MathError, MathResult, Num};

type El = Vec<Num>;
type Pol = Vec<El>;

/// Largest extension degree and polynomial degree (times the field degree) this method accepts.
const MAX_DEGREE: usize = 6;
const MAX_NORM_DEGREE: usize = 48;
const MAX_SHIFT: i64 = 12;

fn sub(a: &Num, b: &Num) -> Num {
    a.add(&b.neg())
}

struct Field {
    d: usize,
    /// monic minimal polynomial, lowest degree first, length `d + 1`
    m: Vec<Num>,
}

impl Field {
    fn zero(&self) -> El {
        vec![Num::zero(); self.d]
    }

    fn one(&self) -> El {
        let mut v = self.zero();
        v[0] = Num::one();
        v
    }

    fn constant(&self, n: &Num) -> El {
        let mut v = self.zero();
        v[0] = n.clone();
        v
    }

    fn is_zero(a: &El) -> bool {
        a.iter().all(Num::is_zero)
    }

    fn add(&self, a: &El, b: &El) -> El {
        a.iter().zip(b).map(|(x, y)| x.add(y)).collect()
    }

    fn sub(&self, a: &El, b: &El) -> El {
        a.iter().zip(b).map(|(x, y)| sub(x, y)).collect()
    }

    fn neg(&self, a: &El) -> El {
        a.iter().map(Num::neg).collect()
    }

    fn mul(&self, a: &El, b: &El) -> El {
        let mut prod = vec![Num::zero(); 2 * self.d];
        for (i, x) in a.iter().enumerate() {
            if x.is_zero() {
                continue;
            }
            for (j, y) in b.iter().enumerate() {
                prod[i + j] = prod[i + j].add(&x.mul(y));
            }
        }
        for i in (self.d..prod.len()).rev() {
            let c = prod[i].clone();
            if c.is_zero() {
                continue;
            }
            // x^i = x^(i-d) * x^d and x^d = -(m_0 + ... + m_(d-1) x^(d-1))
            for j in 0..self.d {
                prod[i - self.d + j] = sub(&prod[i - self.d + j], &c.mul(&self.m[j]));
            }
            prod[i] = Num::zero();
        }
        prod.truncate(self.d);
        prod
    }

    /// Matrix of multiplication by `a`: column `j` holds `a * alpha^j`.
    fn matrix(&self, a: &El) -> Vec<Vec<Num>> {
        let mut cols = Vec::with_capacity(self.d);
        for j in 0..self.d {
            let mut basis = self.zero();
            basis[j] = Num::one();
            cols.push(self.mul(a, &basis));
        }
        (0..self.d).map(|r| cols.iter().map(|c| c[r].clone()).collect()).collect()
    }

    fn inv(&self, a: &El) -> Option<El> {
        let mut rhs = self.zero();
        rhs[0] = Num::one();
        solve(self.matrix(a), rhs)
    }

    fn norm(&self, a: &El) -> Num {
        det(self.matrix(a))
    }
}

/// Solve `m * v = rhs` exactly by Gaussian elimination; `None` when `m` is singular.
fn solve(mut m: Vec<Vec<Num>>, mut rhs: Vec<Num>) -> Option<Vec<Num>> {
    let n = rhs.len();
    for col in 0..n {
        let pivot = (col..n).find(|r| !m[*r][col].is_zero())?;
        m.swap(col, pivot);
        rhs.swap(col, pivot);
        let inv = m[col][col].recip()?;
        for k in col..n {
            m[col][k] = m[col][k].mul(&inv);
        }
        rhs[col] = rhs[col].mul(&inv);
        for r in 0..n {
            if r != col && !m[r][col].is_zero() {
                let f = m[r][col].clone();
                for k in col..n {
                    let t = f.mul(&m[col][k]);
                    m[r][k] = sub(&m[r][k], &t);
                }
                let t = f.mul(&rhs[col]);
                rhs[r] = sub(&rhs[r], &t);
            }
        }
    }
    Some(rhs)
}

fn det(mut m: Vec<Vec<Num>>) -> Num {
    let n = m.len();
    let mut d = Num::one();
    for col in 0..n {
        let Some(pivot) = (col..n).find(|r| !m[*r][col].is_zero()) else { return Num::zero() };
        if pivot != col {
            m.swap(col, pivot);
            d = d.neg();
        }
        d = d.mul(&m[col][col]);
        let Some(inv) = m[col][col].recip() else { return Num::zero() };
        for r in col + 1..n {
            if m[r][col].is_zero() {
                continue;
            }
            let f = m[r][col].mul(&inv);
            for k in col..n {
                let t = f.mul(&m[col][k]);
                m[r][k] = sub(&m[r][k], &t);
            }
        }
    }
    d
}

// ---- polynomials over the extension ------------------------------------------------------------------------------

fn trim(f: &mut Pol) {
    while f.last().is_some_and(Field::is_zero) {
        f.pop();
    }
}

fn pmul_linear(k: &Field, f: &Pol, c: &El) -> Pol {
    // f * (x + c)
    let mut out = vec![k.zero(); f.len() + 1];
    for (i, a) in f.iter().enumerate() {
        out[i + 1] = k.add(&out[i + 1], a);
        out[i] = k.add(&out[i], &k.mul(a, c));
    }
    out
}

/// `f(x + c)`.
fn shift(k: &Field, f: &Pol, c: &El) -> Pol {
    let mut out: Pol = Vec::new();
    for a in f.iter().rev() {
        out = if out.is_empty() { Vec::new() } else { pmul_linear(k, &out, c) };
        if out.is_empty() {
            out.push(a.clone());
        } else {
            out[0] = k.add(&out[0], a);
        }
    }
    trim(&mut out);
    out
}

fn divrem(k: &Field, f: &Pol, g: &Pol) -> Option<(Pol, Pol)> {
    let lead_inv = k.inv(g.last()?)?;
    let mut r = f.clone();
    let mut q = vec![k.zero(); (f.len() + 1).saturating_sub(g.len()).max(1)];
    while r.len() >= g.len() {
        let c = k.mul(r.last()?, &lead_inv);
        let at = r.len() - g.len();
        for (i, gi) in g.iter().enumerate() {
            r[at + i] = k.sub(&r[at + i], &k.mul(&c, gi));
        }
        q[at] = c;
        r.pop();
        trim(&mut r);
    }
    trim(&mut q);
    Some((q, r))
}

fn monic(k: &Field, f: &Pol) -> Option<Pol> {
    let inv = k.inv(f.last()?)?;
    Some(f.iter().map(|c| k.mul(c, &inv)).collect())
}

fn gcd(k: &Field, a: &Pol, b: &Pol) -> Option<Pol> {
    let (mut a, mut b) = (a.clone(), b.clone());
    trim(&mut a);
    trim(&mut b);
    while !b.is_empty() {
        let (_, r) = divrem(k, &a, &b)?;
        a = b;
        b = r;
    }
    monic(k, &a)
}

fn derivative(f: &Pol) -> Pol {
    let mut out: Pol = f
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, c)| {
            let n = Num::int(i as i64);
            c.iter().map(|v| v.mul(&n)).collect::<El>()
        })
        .collect();
    trim(&mut out);
    out
}

/// Squarefree decomposition `f = c * prod s_i ** i` (all `s_i` monic and squarefree).
fn squarefree(k: &Field, f: &Pol) -> Option<Vec<(Pol, usize)>> {
    let f = monic(k, f)?;
    let mut c = gcd(k, &f, &derivative(&f))?;
    let mut w = divrem(k, &f, &c)?.0;
    let mut out = Vec::new();
    let mut i = 1;
    while w.len() > 1 {
        let y = gcd(k, &w, &c)?;
        let z = divrem(k, &w, &y)?.0;
        if z.len() > 1 {
            out.push((monic(k, &z)?, i));
        }
        c = divrem(k, &c, &y)?.0;
        w = y;
        i += 1;
    }
    Some(out)
}

// ---- the norm and its factors over Q ------------------------------------------------------------------------------

fn eval_at(k: &Field, f: &Pol, x0: i64) -> El {
    let x = k.constant(&Num::int(x0));
    f.iter().rev().fold(k.zero(), |acc, c| k.add(&k.mul(&acc, &x), c))
}

/// The polynomial of degree `<= pts.len() - 1` through the points `(i, ys[i])`, lowest degree first.
fn interpolate(ys: &[Num]) -> Vec<Num> {
    // Newton divided differences at the nodes 0, 1, 2, ...
    let n = ys.len();
    let mut coef = ys.to_vec();
    for j in 1..n {
        for i in (j..n).rev() {
            let step = Num::int(j as i64).recip().unwrap_or_else(Num::one);
            coef[i] = sub(&coef[i], &coef[i - 1]).mul(&step);
        }
    }
    let mut poly = vec![Num::zero()];
    for i in (0..n).rev() {
        // poly = poly * (x - i) + coef[i]
        let mut next = vec![Num::zero(); poly.len() + 1];
        for (d, c) in poly.iter().enumerate() {
            next[d + 1] = next[d + 1].add(c);
            next[d] = sub(&next[d], &c.mul(&Num::int(i as i64)));
        }
        next[0] = next[0].add(&coef[i]);
        poly = next;
    }
    while poly.len() > 1 && poly.last().is_some_and(Num::is_zero) {
        poly.pop();
    }
    poly
}

fn norm_poly(k: &Field, t: &Pol) -> Vec<Num> {
    let degree = (t.len() - 1) * k.d;
    let ys: Vec<Num> = (0..=degree as i64).map(|x0| k.norm(&eval_at(k, t, x0))).collect();
    interpolate(&ys)
}

/// Irreducible factors over Q of a rational polynomial, each monic; `None` when it has a repeated factor.
fn rational_factors(n: &[Num], x: &Expr) -> MathResult<Option<Vec<Vec<Num>>>> {
    let expr = from_poly(&n.iter().map(|c| Expr::num(c.clone())).collect::<Vec<_>>(), x);
    let factored = factor(&expr)?;
    let parts = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![factored.clone()],
    };
    let mut out = Vec::new();
    for p in parts {
        let base = match p.kind() {
            Kind::Pow(b, e) if b.contains(x) => {
                if e.as_i64() != Some(1) {
                    return Ok(None);
                }
                b.clone()
            }
            _ => p.clone(),
        };
        if !base.contains(x) {
            continue;
        }
        let Some(c) = to_poly(&base, x)? else { return Err(MathError::unsupported("cannot factor the norm over the rationals")) };
        let nums: Option<Vec<Num>> = c.iter().map(|e| e.as_num().cloned()).collect();
        let Some(mut nums) = nums else { return Err(MathError::unsupported("cannot factor the norm over the rationals")) };
        let Some(lead) = nums.last().and_then(Num::recip) else { continue };
        nums.iter_mut().for_each(|v| *v = v.mul(&lead));
        out.push(nums);
    }
    Ok(Some(out))
}

/// Irreducible factors over the extension of a monic squarefree polynomial.
fn split(k: &Field, s: &Pol, x: &Expr, alpha_el: &El) -> MathResult<Vec<Pol>> {
    if s.len() <= 2 {
        return Ok(vec![s.clone()]);
    }
    for shift_by in 0..=MAX_SHIFT {
        let c = k.mul(&k.constant(&Num::int(-shift_by)), alpha_el);
        let t = shift(k, s, &c);
        if (t.len() - 1) * k.d > MAX_NORM_DEGREE {
            return Err(MathError::too_large("the polynomial is too large to factor over this extension"));
        }
        let n = norm_poly(k, &t);
        let Some(factors) = rational_factors(&n, x)? else { continue };
        let mut out = Vec::new();
        for f in factors {
            let embedded: Pol = f.iter().map(|v| k.constant(v)).collect();
            let Some(g) = gcd(k, &t, &embedded) else { return Err(MathError::unsupported("gcd over the extension failed")) };
            if g.len() > 1 {
                // undo the shift: x -> x - c
                out.push(shift(k, &g, &k.neg(&c)));
            }
        }
        return Ok(out);
    }
    Err(MathError::unsupported("could not find a squarefree norm for this polynomial"))
}

// ---- entry point -------------------------------------------------------------------------------------------------

/// Minimal polynomial (monic, lowest degree first) of the radical `alpha`, which is `I` or `b ** (p/q)` with `b` a
/// positive rational.
fn minimal_polynomial(alpha: &Expr) -> MathResult<Vec<Num>> {
    let unsupported = || MathError::unsupported("the extension must be I or a radical such as sqrt(2) or 2**(1/3)");
    if alpha == &Expr::imag() {
        return Ok(vec![Num::one(), Num::zero(), Num::one()]);
    }
    let Kind::Pow(b, e) = alpha.kind() else { return Err(unsupported()) };
    let (Some(b), Some(e)) = (b.as_num().filter(|n| n.is_exact() && !n.is_negative() && !n.is_zero()), e.as_num().filter(|n| n.is_exact())) else {
        return Err(unsupported());
    };
    let (p, q) = e.parts().ok_or_else(unsupported)?;
    let (p, q) = (i64::try_from(&p).map_err(|_| unsupported())?, i64::try_from(&dashu::integer::IBig::from(q)).map_err(|_| unsupported())?);
    if q < 2 || q as usize > MAX_DEGREE || p == 0 {
        return Err(unsupported());
    }
    let c = b.powi(p).ok_or_else(unsupported)?;
    let mut m = vec![Num::zero(); q as usize + 1];
    m[0] = c.neg();
    m[q as usize] = Num::one();
    // y**q - c must be irreducible, or the radical generates a smaller field
    let y = Expr::sym("_y");
    let poly = from_poly(&m.iter().map(|v| Expr::num(v.clone())).collect::<Vec<_>>(), &y);
    let f = factor(&poly)?;
    let pieces = match f.kind() {
        Kind::Mul(v) => v.iter().filter(|t| t.contains(&y)).count(),
        _ => 1,
    };
    if pieces != 1 || matches!(f.kind(), Kind::Pow(..)) {
        return Err(MathError::unsupported("that radical is not of full degree (it simplifies); use a simpler extension"));
    }
    Ok(m)
}

/// Factor `e`, a polynomial in one symbol, into irreducibles over `Q(alpha)`.
pub fn factor_over(e: &Expr, alpha: &Expr) -> MathResult<Expr> {
    let m = minimal_polynomial(alpha)?;
    let d = m.len() - 1;
    let k = Field { d, m };
    let a = Expr::sym("_alpha");
    let replaced = subs(e, &[(alpha.clone(), a.clone())])?;
    let syms: Vec<String> = replaced.free_symbols().into_iter().filter(|s| &**s != "_alpha").map(|s| s.to_string()).collect();
    let [name] = syms.as_slice() else { return Err(MathError::unsupported("factoring over an extension needs a polynomial in exactly one symbol")) };
    let x = Expr::sym(name);
    let Some(coeffs) = to_poly(&replaced, &x)? else { return Err(MathError::unsupported("factoring over an extension needs a polynomial")) };
    let mut f: Pol = Vec::new();
    for c in &coeffs {
        let Some(ac) = to_poly(c, &a)? else { return Err(MathError::unsupported("coefficients must be rational combinations of the extension")) };
        let nums: Option<Vec<Num>> = ac.iter().map(|v| v.as_num().filter(|n| n.is_exact()).cloned()).collect();
        let Some(nums) = nums else { return Err(MathError::unsupported("coefficients must be rational combinations of the extension")) };
        // reduce the polynomial in alpha modulo m
        let mut el = k.zero();
        let mut power = k.one();
        let alpha_el = {
            let mut v = k.zero();
            if d > 1 {
                v[1] = Num::one();
            }
            v
        };
        for n in &nums {
            el = k.add(&el, &k.mul(&k.constant(n), &power));
            power = k.mul(&power, &alpha_el);
        }
        f.push(el);
    }
    trim(&mut f);
    if f.len() < 2 {
        return Ok(e.clone());
    }
    let lead = f.last().cloned().unwrap_or_else(|| k.one());
    let alpha_el = {
        let mut v = k.zero();
        if d > 1 {
            v[1] = Num::one();
        }
        v
    };
    let Some(parts) = squarefree(&k, &f) else { return Err(MathError::unsupported("could not factor over this extension")) };
    let to_expr = |el: &El| -> Expr {
        Expr::add(el.iter().enumerate().map(|(i, c)| Expr::mul(vec![Expr::num(c.clone()), Expr::pow(alpha, &Expr::int(i as i64))])).collect())
    };
    let poly_expr = |p: &Pol| -> Expr { from_poly(&p.iter().map(&to_expr).collect::<Vec<_>>(), &x) };
    let mut out: Vec<Expr> = Vec::new();
    for (s, mult) in parts {
        for g in split(&k, &s, &x, &alpha_el)? {
            out.push(Expr::pow(&poly_expr(&g), &Expr::int(mult as i64)));
        }
    }
    if lead != k.one() {
        out.insert(0, to_expr(&lead));
    }
    Ok(Expr::mul(out))
}
