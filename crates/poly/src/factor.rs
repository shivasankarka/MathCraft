//! Factoring polynomials over the integers into irreducibles.
//!
//! One variable: Yun's squarefree decomposition, then for each squarefree part a factorisation modulo a small prime
//! (distinct-degree, then Cantor-Zassenhaus equal-degree), Hensel lifting of that factorisation to a modulus above
//! the Mignotte bound, and Zassenhaus recombination of the lifted factors into true factors. Several variables:
//! Kronecker substitution reduces the problem to one variable, and the candidate products are decoded back and
//! tested by exact division. Everything is exact; when a search would be too long the functions return `None`
//! instead of a partial answer.

#![allow(clippy::ptr_arg)]

use crate::gcd::int_gcd;
use crate::{Mono, Poly, PolyError, exponent, pack};
use dashu::integer::IBig;

type Stop<'a> = &'a mut dyn FnMut() -> bool;

/// A polynomial with integer coefficients, lowest degree first, no trailing zeros (zero is empty).
type Z = Vec<IBig>;
/// A content and irreducible factors with multiplicities.
type DenseFactors = (IBig, Vec<(Z, u32)>);
/// The same modulo a small prime.
type Fp = Vec<u64>;

/// Most recombination tests (and Kronecker candidates) before giving up.
const MAX_TESTS: usize = 1 << 17;
/// Largest dense image a Kronecker substitution may produce.
const MAX_IMAGE: usize = 4096;

fn tick(stop: Stop<'_>) -> Result<(), PolyError> {
    if stop() { Err(PolyError::Stopped) } else { Ok(()) }
}

// ---- dense polynomials over Z --------------------------------------------------------------------------------------

fn trim(mut v: Z) -> Z {
    while v.last().is_some_and(|c| *c == IBig::ZERO) {
        v.pop();
    }
    v
}

fn at(a: &[IBig], i: usize) -> IBig {
    a.get(i).cloned().unwrap_or(IBig::ZERO)
}

fn abs(c: &IBig) -> IBig {
    if *c < IBig::ZERO { -c } else { c.clone() }
}

fn z_sub(a: &Z, b: &Z) -> Z {
    trim((0..a.len().max(b.len())).map(|i| at(a, i) - at(b, i)).collect())
}

fn z_add(a: &Z, b: &Z) -> Z {
    trim((0..a.len().max(b.len())).map(|i| at(a, i) + at(b, i)).collect())
}

fn z_mul(a: &Z, b: &Z) -> Z {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![IBig::ZERO; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        if *x == IBig::ZERO {
            continue;
        }
        for (j, y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o += x * y;
            }
        }
    }
    trim(out)
}

fn z_deriv(a: &Z) -> Z {
    trim(a.iter().enumerate().skip(1).map(|(i, c)| c * IBig::from(i)).collect())
}

fn z_content(a: &Z) -> IBig {
    a.iter().fold(IBig::ZERO, |g, c| if g == IBig::ONE { g } else { int_gcd(&g, c) })
}

/// Divide out the content and make the leading coefficient positive.
fn z_prim(a: &Z) -> Z {
    let c = z_content(a);
    if c == IBig::ZERO {
        return Vec::new();
    }
    let negative = a.last().is_some_and(|l| *l < IBig::ZERO);
    a.iter().map(|x| if negative { -(x / &c) } else { x / &c }).collect()
}

/// The quotient when `b` divides `a` exactly in Z[x].
fn z_divexact(a: &Z, b: &Z) -> Option<Z> {
    let lcb = b.last()?;
    if a.len() < b.len() {
        return if a.is_empty() { Some(Vec::new()) } else { None };
    }
    let mut r = a.clone();
    let mut q = vec![IBig::ZERO; a.len() - b.len() + 1];
    for k in (0..q.len()).rev() {
        let rc = at(&r, k + b.len() - 1);
        if rc == IBig::ZERO {
            continue;
        }
        if &rc % lcb != IBig::ZERO {
            return None;
        }
        let c = &rc / lcb;
        for (j, bj) in b.iter().enumerate() {
            if let Some(slot) = r.get_mut(k + j) {
                *slot -= &c * bj;
            }
        }
        if let Some(s) = q.get_mut(k) {
            *s = c;
        }
    }
    r.iter().all(|c| *c == IBig::ZERO).then(|| trim(q))
}

/// `lc(b)^k * a mod b`, with the content removed (only the gcd is wanted from it).
fn z_prem(a: &Z, b: &Z) -> Z {
    let Some(lcb) = b.last() else { return a.clone() };
    let mut r = a.clone();
    while r.len() >= b.len() && !r.is_empty() {
        let lcr = r.last().cloned().unwrap_or(IBig::ZERO);
        let shift = r.len() - b.len();
        let mut next: Z = r.iter().map(|c| c * lcb).collect();
        for (j, bj) in b.iter().enumerate() {
            if let Some(s) = next.get_mut(shift + j) {
                *s -= &lcr * bj;
            }
        }
        r = z_prim(&trim(next));
    }
    r
}

/// The gcd of two polynomials, primitive with positive leading coefficient.
fn z_gcd(a: &Z, b: &Z) -> Z {
    let (mut a, mut b) = (z_prim(a), z_prim(b));
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    loop {
        if a.len() < b.len() {
            std::mem::swap(&mut a, &mut b);
        }
        let r = z_prem(&a, &b);
        if r.is_empty() {
            return b;
        }
        a = b;
        b = r;
    }
}

/// Yun's algorithm: `f` (primitive, positive leading coefficient) as a product of coprime squarefree parts `a_i^i`.
fn squarefree(f: &Z) -> Option<Vec<(Z, u32)>> {
    let fd = z_deriv(f);
    let a = z_gcd(f, &fd);
    if a.len() <= 1 {
        return Some(vec![(f.clone(), 1)]);
    }
    let mut b = z_divexact(f, &a)?;
    let mut c = z_divexact(&fd, &a)?;
    let mut d = z_sub(&c, &z_deriv(&b));
    let mut out = Vec::new();
    let mut i = 1u32;
    while b.len() > 1 {
        let g = z_gcd(&b, &d);
        if g.len() > 1 {
            out.push((g.clone(), i));
        }
        b = z_divexact(&b, &g)?;
        c = z_divexact(&d, &g)?;
        d = z_sub(&c, &z_deriv(&b));
        i += 1;
        if usize::try_from(i).ok()? > f.len() {
            return None;
        }
    }
    Some(out)
}

// ---- dense polynomials over GF(p) ----------------------------------------------------------------------------------

fn fp_trim(mut v: Fp) -> Fp {
    while v.last().is_some_and(|c| *c == 0) {
        v.pop();
    }
    v
}

fn fp_at(a: &[u64], i: usize) -> u64 {
    a.get(i).copied().unwrap_or(0)
}

fn fp_from_z(a: &Z, p: u64) -> Fp {
    let pb = IBig::from(p);
    fp_trim(a.iter().map(|c| u64::try_from(modn(c, &pb)).unwrap_or(0)).collect())
}

fn fp_sub(a: &Fp, b: &Fp, p: u64) -> Fp {
    fp_trim((0..a.len().max(b.len())).map(|i| (fp_at(a, i) + p - fp_at(b, i)) % p).collect())
}

fn fp_mul(a: &Fp, b: &Fp, p: u64) -> Fp {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u64; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o = (*o + x * y) % p;
            }
        }
    }
    fp_trim(out)
}

fn pow_mod(mut base: u64, mut e: u64, p: u64) -> u64 {
    let mut acc = 1u64;
    base %= p;
    while e > 0 {
        if e & 1 == 1 {
            acc = acc * base % p;
        }
        base = base * base % p;
        e >>= 1;
    }
    acc
}

fn inv(a: u64, p: u64) -> u64 {
    pow_mod(a, p - 2, p)
}

fn fp_divrem(a: &Fp, b: &Fp, p: u64) -> (Fp, Fp) {
    let Some(&lb) = b.last() else { return (Vec::new(), a.clone()) };
    if a.len() < b.len() {
        return (Vec::new(), a.clone());
    }
    let inv_lb = inv(lb, p);
    let mut r = a.clone();
    let mut q = vec![0u64; a.len() - b.len() + 1];
    for k in (0..q.len()).rev() {
        let rc = fp_at(&r, k + b.len() - 1);
        if rc == 0 {
            continue;
        }
        let c = rc * inv_lb % p;
        if let Some(s) = q.get_mut(k) {
            *s = c;
        }
        for (j, bj) in b.iter().enumerate() {
            if let Some(s) = r.get_mut(k + j) {
                *s = (*s + p - c * bj % p) % p;
            }
        }
    }
    (fp_trim(q), fp_trim(r))
}

fn fp_monic(a: &Fp, p: u64) -> Fp {
    match a.last() {
        Some(&l) if l != 1 => {
            let i = inv(l, p);
            a.iter().map(|c| c * i % p).collect()
        }
        _ => a.clone(),
    }
}

fn fp_gcd(a: &Fp, b: &Fp, p: u64) -> Fp {
    let (mut a, mut b) = (a.clone(), b.clone());
    while !b.is_empty() {
        let r = fp_divrem(&a, &b, p).1;
        a = std::mem::replace(&mut b, r);
    }
    fp_monic(&a, p)
}

fn fp_deriv(a: &Fp, p: u64) -> Fp {
    fp_trim(a.iter().enumerate().skip(1).map(|(i, c)| c * (i as u64 % p) % p).collect())
}

/// `base ** e mod modulus` in GF(p)[x].
fn fp_powmod(base: &Fp, mut e: u128, modulus: &Fp, p: u64) -> Fp {
    let mut acc: Fp = vec![1];
    let mut b = fp_divrem(base, modulus, p).1;
    while e > 0 {
        if e & 1 == 1 {
            acc = fp_divrem(&fp_mul(&acc, &b, p), modulus, p).1;
        }
        b = fp_divrem(&fp_mul(&b, &b, p), modulus, p).1;
        e >>= 1;
    }
    acc
}

/// `s` with `s*a + t*b = 1` for coprime `a`, `b`.
fn fp_bezout(a: &Fp, b: &Fp, p: u64) -> Option<Fp> {
    let (mut r0, mut r1) = (a.clone(), b.clone());
    let (mut s0, mut s1): (Fp, Fp) = (vec![1], Vec::new());
    while !r1.is_empty() {
        let (q, r) = fp_divrem(&r0, &r1, p);
        let s2 = fp_sub(&s0, &fp_mul(&q, &s1, p), p);
        r0 = std::mem::replace(&mut r1, r);
        s0 = std::mem::replace(&mut s1, s2);
    }
    if r0.len() != 1 {
        return None;
    }
    let k = inv(fp_at(&r0, 0), p);
    Some(fp_trim(s0.iter().map(|c| c * k % p).collect()))
}

fn next_rand(state: &mut u64) -> u64 {
    // xorshift64: deterministic, so factoring is reproducible
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Split a monic squarefree `f` whose irreducible factors all have degree `d` (Cantor-Zassenhaus).
fn equal_degree(f: &Fp, d: usize, p: u64, rng: &mut u64, out: &mut Vec<Fp>, stop: Stop<'_>) -> Result<Option<()>, PolyError> {
    let n = f.len().saturating_sub(1);
    if n == d {
        out.push(f.clone());
        return Ok(Some(()));
    }
    let Some(pd) = u128::from(p).checked_pow(d as u32) else { return Ok(None) };
    let e = (pd - 1) / 2;
    for _ in 0..200 {
        tick(stop)?;
        let a = fp_trim((0..n).map(|_| next_rand(rng) % p).collect());
        if a.len() < 2 {
            continue;
        }
        let t = fp_powmod(&a, e, f, p);
        let g = fp_gcd(f, &fp_sub(&t, &vec![1], p), p);
        if g.len() > 1 && g.len() < f.len() {
            let (h, _) = fp_divrem(f, &g, p);
            if equal_degree(&g, d, p, rng, out, stop)?.is_none() {
                return Ok(None);
            }
            return equal_degree(&fp_monic(&h, p), d, p, rng, out, stop);
        }
    }
    Ok(None)
}

/// Irreducible factors of a monic squarefree polynomial modulo `p`.
fn factor_mod_p(f: &Fp, p: u64, stop: Stop<'_>) -> Result<Option<Vec<Fp>>, PolyError> {
    let x: Fp = vec![0, 1];
    let mut fstar = f.clone();
    let mut h = x.clone();
    let mut classes: Vec<(Fp, usize)> = Vec::new();
    let mut d = 0usize;
    while fstar.len().saturating_sub(1) >= 2 * (d + 1) {
        tick(stop)?;
        d += 1;
        h = fp_powmod(&h, u128::from(p), &fstar, p);
        let g = fp_gcd(&fstar, &fp_sub(&h, &x, p), p);
        if g.len() > 1 {
            classes.push((g.clone(), d));
            fstar = fp_monic(&fp_divrem(&fstar, &g, p).0, p);
            h = fp_divrem(&h, &fstar, p).1;
        }
    }
    if fstar.len() > 1 {
        let dd = fstar.len() - 1;
        classes.push((fstar, dd));
    }
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let mut out = Vec::new();
    for (g, d) in classes {
        if equal_degree(&g, d, p, &mut rng, &mut out, stop)?.is_none() {
            return Ok(None);
        }
    }
    Ok(Some(out))
}

// ---- Hensel lifting ------------------------------------------------------------------------------------------------

/// `c` reduced into `[0, m)`.
fn modn(c: &IBig, m: &IBig) -> IBig {
    let r = c % m;
    if r < IBig::ZERO { r + m } else { r }
}

/// `c` reduced into `(-m/2, m/2]`.
fn symmetric(c: &IBig, m: &IBig) -> IBig {
    let r = modn(c, m);
    if &r * IBig::from(2) > *m { r - m } else { r }
}

fn inv_mod(a: &IBig, m: &IBig) -> Option<IBig> {
    let (mut r0, mut r1) = (m.clone(), modn(a, m));
    let (mut t0, mut t1) = (IBig::ZERO, IBig::ONE);
    while r1 != IBig::ZERO {
        let q = &r0 / &r1;
        let r2 = &r0 - &q * &r1;
        let t2 = &t0 - &q * &t1;
        r0 = std::mem::replace(&mut r1, r2);
        t0 = std::mem::replace(&mut t1, t2);
    }
    (r0 == IBig::ONE).then(|| modn(&t0, m))
}

fn z_mod(a: &Z, m: &IBig) -> Z {
    trim(a.iter().map(|c| modn(c, m)).collect())
}

fn fp_to_z(a: &Fp) -> Z {
    a.iter().map(|c| IBig::from(*c)).collect()
}

/// Lift `f = g0 * h0 (mod p)` with `h0` monic to `f = G * H (mod p^k)`, linearly, one power of `p` at a time.
fn lift_pair(f: &Z, g0: &Fp, h0: &Fp, p: u64, k: u32, stop: Stop<'_>) -> Result<Option<(Z, Z)>, PolyError> {
    let Some(s) = fp_bezout(g0, h0, p) else { return Ok(None) };
    let mut g = fp_to_z(g0);
    let mut h = fp_to_z(h0);
    let mut m = IBig::from(p);
    let pb = IBig::from(p);
    for _ in 1..k {
        tick(stop)?;
        let diff = z_sub(f, &z_mul(&g, &h));
        let mut e: Fp = Vec::with_capacity(diff.len());
        for c in &diff {
            if c % &m != IBig::ZERO {
                return Ok(None);
            }
            let Ok(v) = u64::try_from(modn(&(c / &m), &pb)) else { return Ok(None) };
            e.push(v);
        }
        let e = fp_trim(e);
        let h1 = fp_divrem(&fp_mul(&s, &e, p), h0, p).1;
        let (g1, rem) = fp_divrem(&fp_sub(&e, &fp_mul(&h1, g0, p), p), h0, p);
        if !rem.is_empty() {
            return Ok(None);
        }
        g = z_add(&g, &z_mul(&fp_to_z(&g1), &vec![m.clone()]));
        h = z_add(&h, &z_mul(&fp_to_z(&h1), &vec![m.clone()]));
        m *= &pb;
    }
    Ok(Some((z_mod(&g, &m), z_mod(&h, &m))))
}

/// Lift the factorisation `f = lc * u_1 ... u_r (mod p)` (the `u_i` monic) to `p^k`; the factors come back in order.
fn lift_all(f: &Z, us: &[Fp], p: u64, k: u32, m: &IBig, stop: Stop<'_>) -> Result<Option<Vec<Z>>, PolyError> {
    let Some((first, _)) = us.split_first() else { return Ok(None) };
    if us.len() == 1 {
        let _ = first;
        return Ok(Some(vec![z_mod(f, m)]));
    }
    let (left, right) = us.split_at(us.len() / 2);
    let h0 = right.iter().fold(vec![1u64], |acc, u| fp_mul(&acc, u, p));
    let (g0, rem) = fp_divrem(&fp_from_z(f, p), &h0, p);
    if !rem.is_empty() {
        return Ok(None);
    }
    let Some((g, h)) = lift_pair(f, &g0, &h0, p, k, stop)? else { return Ok(None) };
    let Some(mut out) = lift_all(&g, left, p, k, m, stop)? else { return Ok(None) };
    let Some(rest) = lift_all(&h, right, p, k, m, stop)? else { return Ok(None) };
    out.extend(rest);
    Ok(Some(out))
}

// ---- recombination -------------------------------------------------------------------------------------------------

struct Search<'a, 'b> {
    vs: &'a [Z],
    avail: &'a [usize],
    m: &'a IBig,
    f: &'a Z,
    tests: &'a mut usize,
    stop: Stop<'b>,
}

type Found = (Vec<usize>, Z, Z);

impl Search<'_, '_> {
    fn dfs(&mut self, start: usize, left: usize, prod: &Z, chosen: &mut Vec<usize>) -> Result<Option<Found>, PolyError> {
        if left == 0 {
            *self.tests += 1;
            if *self.tests > MAX_TESTS {
                return Err(PolyError::TooBig);
            }
            tick(&mut *self.stop)?;
            let cand = z_prim(&trim(prod.iter().map(|c| symmetric(c, self.m)).collect()));
            return Ok(z_divexact(self.f, &cand).map(|q| (chosen.clone(), cand, q)));
        }
        for pos in start..self.avail.len() {
            if self.avail.len() - pos < left {
                break;
            }
            let Some(&idx) = self.avail.get(pos) else { break };
            let Some(v) = self.vs.get(idx) else { break };
            let next = z_mod(&z_mul(prod, v), self.m);
            chosen.push(idx);
            if let Some(found) = self.dfs(pos + 1, left - 1, &next, chosen)? {
                return Ok(Some(found));
            }
            chosen.pop();
        }
        Ok(None)
    }
}

/// Combine the lifted factors `vs` (monic mod `m`) into the true irreducible factors of `f`.
fn recombine(f: &Z, vs: &[Z], m: &IBig, stop: Stop<'_>) -> Result<Vec<Z>, PolyError> {
    let mut f_cur = f.clone();
    let mut avail: Vec<usize> = (0..vs.len()).collect();
    let mut out = Vec::new();
    let mut s = 1usize;
    let mut tests = 0usize;
    while avail.len() >= 2 * s {
        let lcf = f_cur.last().cloned().unwrap_or(IBig::ONE);
        let found = {
            let mut search = Search { vs, avail: &avail, m, f: &f_cur, tests: &mut tests, stop: &mut *stop };
            search.dfs(0, s, &vec![modn(&lcf, m)], &mut Vec::new())?
        };
        match found {
            Some((idxs, cand, quotient)) => {
                out.push(cand);
                f_cur = quotient;
                avail.retain(|i| !idxs.contains(i));
            }
            None => s += 1,
        }
    }
    if f_cur.len() > 1 {
        out.push(f_cur);
    }
    Ok(out)
}

fn small_primes() -> impl Iterator<Item = u64> {
    (3u64..4000).filter(|n| (2..).take_while(|d| d * d <= *n).all(|d| n % d != 0))
}

/// The irreducible factors of a primitive squarefree `g` with positive leading coefficient and degree at least 2.
fn factor_squarefree(g: &Z, stop: Stop<'_>) -> Result<Option<Vec<Z>>, PolyError> {
    let lc = g.last().cloned().unwrap_or(IBig::ONE);
    let mut best: Option<(u64, Vec<Fp>)> = None;
    let mut tried = 0;
    for p in small_primes() {
        let gp = fp_from_z(g, p);
        if gp.len() != g.len() || fp_gcd(&gp, &fp_deriv(&gp, p), p).len() > 1 {
            continue;
        }
        let Some(us) = factor_mod_p(&fp_monic(&gp, p), p, stop)? else { continue };
        tried += 1;
        if best.as_ref().is_none_or(|(_, b)| us.len() < b.len()) {
            best = Some((p, us));
        }
        if best.as_ref().is_some_and(|(_, b)| b.len() == 1) || tried >= 5 {
            break;
        }
    }
    let Some((p, us)) = best else { return Ok(None) };
    if us.len() == 1 {
        return Ok(Some(vec![g.clone()]));
    }
    // a modulus above twice the Mignotte bound times the leading coefficient
    let l1 = g.iter().fold(IBig::ZERO, |s, c| s + abs(c));
    let bound = (IBig::ONE << (g.len() - 1)) * l1 * abs(&lc) * IBig::from(2);
    let (mut m, mut k) = (IBig::from(p), 1u32);
    while m <= bound {
        m *= IBig::from(p);
        k += 1;
    }
    let Some(lifted) = lift_all(g, &us, p, k, &m, stop)? else { return Ok(None) };
    let mut monic = Vec::with_capacity(lifted.len());
    for v in &lifted {
        let Some(lead) = v.last() else { return Ok(None) };
        let Some(i) = inv_mod(lead, &m) else { return Ok(None) };
        monic.push(z_mod(&z_mul(v, &vec![i]), &m));
    }
    match recombine(g, &monic, &m, stop) {
        Ok(fs) => Ok(Some(fs)),
        Err(PolyError::TooBig) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `f` as `content * product of irreducible^multiplicity` (`None` when too costly). The content carries the sign.
fn factor_dense(f: &Z, stop: Stop<'_>) -> Result<Option<DenseFactors>, PolyError> {
    if f.is_empty() {
        return Ok(None);
    }
    let mut content = z_content(f);
    if f.last().is_some_and(|l| *l < IBig::ZERO) {
        content = -content;
    }
    let prim = z_prim(f);
    let Some(parts) = squarefree(&prim) else { return Ok(None) };
    let mut out = Vec::new();
    for (sf, mult) in parts {
        if sf.len() <= 2 {
            out.push((sf, mult));
            continue;
        }
        let Some(fs) = factor_squarefree(&sf, stop)? else { return Ok(None) };
        out.extend(fs.into_iter().map(|g| (g, mult)));
    }
    Ok(Some((content, out)))
}

// ---- polynomials ---------------------------------------------------------------------------------------------------

/// The result of factoring: `content * product of factor^multiplicity`. Every factor is irreducible, primitive, with
/// a positive leading coefficient; the content carries the sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Factorization {
    pub content: IBig,
    pub factors: Vec<(Poly, u32)>,
}

fn to_dense(p: &Poly, v: usize) -> Z {
    let mut z = vec![IBig::ZERO; p.degree_in(v) as usize + 1];
    for (m, c) in p.terms() {
        if let Some(slot) = z.get_mut(exponent(*m, v) as usize) {
            *slot = c.clone();
        }
    }
    z
}

fn from_dense(nvars: usize, v: usize, z: &Z) -> Option<Poly> {
    let mut terms = Vec::new();
    for (i, c) in z.iter().enumerate() {
        if *c == IBig::ZERO {
            continue;
        }
        let mut exps = vec![0u32; nvars];
        *exps.get_mut(v)? = u32::try_from(i).ok()?;
        terms.push((pack(&exps)?, c.clone()));
    }
    Some(Poly::from_terms(nvars, terms))
}

fn leading_negative(p: &Poly) -> bool {
    p.terms().first().is_some_and(|(_, c)| *c < IBig::ZERO)
}

impl Poly {
    /// Factor into irreducibles over the integers; `None` if that would take too long.
    pub fn factor(&self) -> Result<Option<Factorization>, PolyError> {
        self.factor_with(&mut || false)
    }

    pub fn factor_with(&self, stop: Stop<'_>) -> Result<Option<Factorization>, PolyError> {
        let n = self.nvars();
        if self.is_zero() {
            return Ok(None);
        }
        let mut content = self.content();
        let mut factors: Vec<(Poly, u32)> = Vec::new();
        // the monomial every term shares
        let mut common = vec![0u32; n];
        for (i, slot) in common.iter_mut().enumerate() {
            *slot = self.terms().iter().map(|(m, _)| exponent(*m, i)).min().unwrap_or(0);
        }
        let Some(common_mono) = pack(&common) else { return Ok(None) };
        for (i, e) in common.iter().enumerate() {
            if *e > 0 {
                factors.push((Poly::var(n, i)?, *e));
            }
        }
        let rest = Poly::from_terms(n, self.terms().iter().map(|(m, c)| (*m - common_mono, c / &content)).collect());
        let rest = if leading_negative(&rest) {
            content = -content;
            rest.neg()
        } else {
            rest
        };
        let used: Vec<usize> = (0..n).filter(|v| rest.degree_in(*v) > 0).collect();
        match used.as_slice() {
            [] => {}
            [v] => {
                let Some((_, fs)) = factor_dense(&to_dense(&rest, *v), stop)? else { return Ok(None) };
                for (z, mult) in fs {
                    let Some(p) = from_dense(n, *v, &z) else { return Ok(None) };
                    factors.push((p, mult));
                }
            }
            _ => {
                let Some((sign, fs)) = kronecker(&rest, &used, stop)? else { return Ok(None) };
                if sign {
                    content = -content;
                }
                factors.extend(fs);
            }
        }
        factors.sort_by(|a, b| (a.0.total_degree(), a.0.terms().first().map(|t| t.0)).cmp(&(b.0.total_degree(), b.0.terms().first().map(|t| t.0))));
        Ok(Some(Factorization { content, factors }))
    }
}

/// Factor a primitive polynomial in the variables `used` (at least two) by Kronecker substitution: variable `i` is
/// replaced by `x ** weight_i`, the one-variable image is factored, and products of image factors are decoded back
/// and tested by exact division. Returns whether the sign flipped, and the factors.
#[allow(clippy::type_complexity)]
fn kronecker(p: &Poly, used: &[usize], stop: Stop<'_>) -> Result<Option<(bool, Vec<(Poly, u32)>)>, PolyError> {
    let n = p.nvars();
    let degs: Vec<u32> = used.iter().map(|v| p.degree_in(*v)).collect();
    let mut weights: Vec<usize> = Vec::with_capacity(used.len());
    let mut w = 1usize;
    for d in &degs {
        weights.push(w);
        w = w.saturating_mul(*d as usize + 1);
        if w > MAX_IMAGE {
            return Ok(None);
        }
    }
    let mut image = vec![IBig::ZERO; w];
    for (m, c) in p.terms() {
        let e: usize = used.iter().zip(&weights).map(|(v, wt)| exponent(*m, *v) as usize * wt).sum();
        if let Some(slot) = image.get_mut(e) {
            *slot = c.clone();
        }
    }
    let image = trim(image);
    let Some((_, parts)) = factor_dense(&image, stop)? else { return Ok(None) };
    let mut irreducible: Vec<Z> = Vec::new();
    for (z, mult) in parts {
        for _ in 0..mult {
            irreducible.push(z.clone());
        }
    }
    if irreducible.len() > 16 {
        return Ok(None);
    }
    let decode = |z: &Z| -> Option<Poly> {
        let mut terms: Vec<(Mono, IBig)> = Vec::new();
        for (e, c) in z.iter().enumerate() {
            if *c == IBig::ZERO {
                continue;
            }
            let mut exps = vec![0u32; n];
            for (i, (v, wt)) in used.iter().zip(&weights).enumerate() {
                let digit = if i + 1 == used.len() { e / wt } else { (e / wt) % (degs.get(i).copied()? as usize + 1) };
                *exps.get_mut(*v)? = u32::try_from(digit).ok()?;
            }
            if used.iter().zip(&degs).any(|(v, d)| exps.get(*v).is_none_or(|x| x > d)) {
                return None;
            }
            terms.push((pack(&exps)?, c.clone()));
        }
        Some(Poly::from_terms(n, terms))
    };
    let mut cur = p.clone();
    let mut avail: Vec<usize> = (0..irreducible.len()).collect();
    let mut found: Vec<Poly> = Vec::new();
    let mut flipped = false;
    let mut tests = 0usize;
    let mut s = 1usize;
    while avail.len() >= 2 * s {
        let hit = kron_dfs(&cur, &irreducible, &avail, 0, s, &vec![IBig::ONE], &mut Vec::new(), &decode, &mut tests, stop)?;
        match hit {
            Some((idxs, cand, quotient)) => {
                let (cand, quotient) = if leading_negative(&cand) {
                    flipped = !flipped;
                    (cand.neg(), quotient.neg())
                } else {
                    (cand, quotient)
                };
                found.push(cand);
                cur = quotient;
                avail.retain(|i| !idxs.contains(i));
            }
            None => s += 1,
        }
        if tests > MAX_TESTS {
            return Ok(None);
        }
    }
    if cur.total_degree() > 0 {
        if leading_negative(&cur) {
            flipped = !flipped;
            cur = cur.neg();
        }
        found.push(cur);
    }
    found.sort_by(|a, b| a.terms().first().map(|t| t.0).cmp(&b.terms().first().map(|t| t.0)));
    let mut grouped: Vec<(Poly, u32)> = Vec::new();
    for f in found {
        match grouped.iter_mut().find(|(g, _)| *g == f) {
            Some((_, k)) => *k += 1,
            None => grouped.push((f, 1)),
        }
    }
    Ok(Some((flipped, grouped)))
}

type KronFound = (Vec<usize>, Poly, Poly);

#[allow(clippy::too_many_arguments)]
fn kron_dfs(
    cur: &Poly,
    parts: &[Z],
    avail: &[usize],
    start: usize,
    left: usize,
    prod: &Z,
    chosen: &mut Vec<usize>,
    decode: &dyn Fn(&Z) -> Option<Poly>,
    tests: &mut usize,
    stop: Stop<'_>,
) -> Result<Option<KronFound>, PolyError> {
    if left == 0 {
        *tests += 1;
        if *tests > MAX_TESTS {
            return Ok(None);
        }
        tick(stop)?;
        let Some(cand) = decode(prod) else { return Ok(None) };
        if cand.total_degree() == 0 {
            return Ok(None);
        }
        return Ok(cur.div_exact(&cand)?.map(|q| (chosen.clone(), cand, q)));
    }
    for pos in start..avail.len() {
        if avail.len() - pos < left {
            break;
        }
        let Some(&idx) = avail.get(pos) else { break };
        let Some(part) = parts.get(idx) else { break };
        chosen.push(idx);
        let next = z_mul(prod, part);
        if let Some(hit) = kron_dfs(cur, parts, avail, pos + 1, left - 1, &next, chosen, decode, tests, stop)? {
            return Ok(Some(hit));
        }
        chosen.pop();
        if *tests > MAX_TESTS {
            return Ok(None);
        }
    }
    Ok(None)
}
