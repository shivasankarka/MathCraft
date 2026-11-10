//! Greatest common divisors by sparse modular interpolation (Zippel's method).
//!
//! The inputs are primitive in the main variable `x_v`. For a prime `p` the gcd is rebuilt from many univariate gcds
//! of images in which every other variable is replaced by a number: the first variable is interpolated densely, and
//! each further variable only for the monomials already seen (a sparse polynomial has few), solved from geometric
//! progressions of sample points with a transposed Vandermonde solver. The image gcds are scaled so their leading
//! coefficient equals the gcd of the leading coefficients, which makes the images consistent. Residues from several
//! primes are combined by the Chinese remainder theorem until the candidate divides both inputs; that exact
//! division is the only thing that is trusted, so an unlucky choice of primes or points costs time, never a wrong
//! answer.

use crate::gcd::{gcd_rec, positive};
use crate::{Mono, Poly, PolyError, exponent, shift};
use dashu::integer::IBig;

type Stop<'a> = &'a mut dyn FnMut() -> bool;

// Reads and writes that never panic. Every index below is meant to be in range; if one were not, the value reads as
// zero and the write is dropped, which can only make a candidate wrong, and a wrong candidate fails the exact
// division at the end (it costs time, never a wrong answer).

fn at(v: &[u64], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(0)
}

/// `tabs[t][e]`: the `e`-th power in the `t`-th table.
fn pw(tabs: &[Vec<u64>], t: usize, e: usize) -> u64 {
    tabs.get(t).map_or(0, |table| at(table, e))
}

fn modify(v: &mut [u64], i: usize, op: impl FnOnce(u64) -> u64) {
    if let Some(x) = v.get_mut(i) {
        *x = op(*x);
    }
}

/// Smallest input (terms of both together) worth the modular method.
pub(crate) const MIN_TERMS: usize = 40;
/// Most primes tried before giving up.
const MAX_PRIMES: usize = 48;
/// Terms to evaluate per node (rows times terms of both inputs) from which the nodes are computed on several threads.
const PARALLEL_WORK: usize = 100_000;
/// Fresh random points tried for one prime.
const MAX_RETRIES: usize = 3;

/// Threads to spread independent nodes over.
fn parallelism() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// `f` applied to every node, on one thread each when there are several.
fn per_node<T: Send>(gammas: &[u64], f: &(impl Fn(u64) -> T + Sync)) -> Vec<Option<T>> {
    if gammas.len() < 2 {
        return gammas.iter().map(|g| Some(f(*g))).collect();
    }
    std::thread::scope(|sc| {
        let handles: Vec<_> = gammas.iter().map(|g| sc.spawn(move || f(*g))).collect();
        handles.into_iter().map(|h| h.join().ok()).collect()
    })
}

/// Arithmetic modulo an odd prime below 2^62, in Montgomery form.
#[derive(Clone, Copy)]
struct Field {
    p: u64,
    neg_inv: u64,
    r2: u64,
}

impl Field {
    fn new(p: u64) -> Field {
        let mut inv: u64 = 1;
        for _ in 0..6 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(p.wrapping_mul(inv)));
        }
        let r = ((1u128 << 64) % u128::from(p)) as u64;
        let r2 = ((u128::from(r) * u128::from(r)) % u128::from(p)) as u64;
        Field { p, neg_inv: inv.wrapping_neg(), r2 }
    }

    #[inline(always)]
    fn mul(&self, a: u64, b: u64) -> u64 {
        let t = u128::from(a) * u128::from(b);
        let m = (t as u64).wrapping_mul(self.neg_inv);
        let u = ((t + u128::from(m) * u128::from(self.p)) >> 64) as u64;
        if u >= self.p { u - self.p } else { u }
    }

    #[inline(always)]
    fn add(&self, a: u64, b: u64) -> u64 {
        let s = a + b;
        if s >= self.p { s - self.p } else { s }
    }

    #[inline(always)]
    fn sub(&self, a: u64, b: u64) -> u64 {
        if a >= b { a - b } else { a + self.p - b }
    }

    fn enc(&self, a: u64) -> u64 {
        self.mul(a % self.p, self.r2)
    }

    fn dec(&self, a: u64) -> u64 {
        self.mul(a, 1)
    }

    fn one(&self) -> u64 {
        self.enc(1)
    }

    fn pow(&self, mut a: u64, mut e: u64) -> u64 {
        let mut r = self.one();
        while e > 0 {
            if e & 1 == 1 {
                r = self.mul(r, a);
            }
            a = self.mul(a, a);
            e >>= 1;
        }
        r
    }

    fn inv(&self, a: u64) -> u64 {
        self.pow(a, self.p - 2)
    }

    fn enc_big(&self, c: &IBig) -> u64 {
        if let Ok(v) = i64::try_from(c) {
            let r = i128::from(v).rem_euclid(i128::from(self.p)) as u64;
            return self.enc(r);
        }
        let mut r = c % IBig::from(self.p);
        if r < IBig::ZERO {
            r += IBig::from(self.p);
        }
        self.enc(u64::try_from(&r).unwrap_or(0))
    }

    /// `1, a, a^2, ..., a^top`
    fn powers(&self, a: u64, top: usize) -> Vec<u64> {
        let mut t = Vec::with_capacity(top + 1);
        let mut x = self.one();
        for _ in 0..=top {
            t.push(x);
            x = self.mul(x, a);
        }
        t
    }
}

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(m)) as u64
}

fn pow_mod(mut a: u64, mut e: u64, m: u64) -> u64 {
    let mut r = 1u64;
    a %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = mul_mod(r, a, m);
        }
        a = mul_mod(a, a, m);
        e >>= 1;
    }
    r
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for q in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n.is_multiple_of(q) {
            return n == q;
        }
    }
    let (mut d, mut s) = (n - 1, 0);
    while d % 2 == 0 {
        d /= 2;
        s += 1;
    }
    'bases: for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..s {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
        }
        return false;
    }
    true
}

/// The largest prime below `n`.
fn prime_below(mut n: u64) -> u64 {
    n -= 1;
    if n.is_multiple_of(2) {
        n -= 1;
    }
    while !is_prime(n) {
        n -= 2;
    }
    n
}

/// A small deterministic generator (xorshift), so a run is reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A nonzero field element.
    fn element(&mut self, f: &Field) -> u64 {
        f.enc(self.next() % (f.p - 1) + 1)
    }
}

// ---- dense univariate polynomials over the field (Montgomery form, lowest degree first)

fn trim(a: &mut Vec<u64>) {
    while a.last() == Some(&0) {
        a.pop();
    }
}

fn urem(f: &Field, a: &mut Vec<u64>, b: &[u64]) {
    let Some(&lead) = b.last() else { return };
    let inv = f.inv(lead);
    while a.len() >= b.len() {
        let q = f.mul(*a.last().unwrap_or(&0), inv);
        let from = a.len() - b.len();
        for (ac, bc) in a.iter_mut().skip(from).zip(b) {
            *ac = f.sub(*ac, f.mul(q, *bc));
        }
        trim(a);
    }
}

/// The monic gcd (empty when both are zero).
fn ugcd(f: &Field, mut a: Vec<u64>, mut b: Vec<u64>) -> Vec<u64> {
    trim(&mut a);
    trim(&mut b);
    while !b.is_empty() {
        urem(f, &mut a, &b);
        std::mem::swap(&mut a, &mut b);
    }
    if let Some(&lead) = a.last() {
        let inv = f.inv(lead);
        for c in &mut a {
            *c = f.mul(*c, inv);
        }
    }
    a
}

// ---- inputs reduced to a prime

/// A polynomial's terms as seen from one stage: the exponent of `x_v`, the monomial in the variables still free, and
/// the coefficient with every already-fixed variable folded in. Sorted by `x_v` exponent.
struct Side {
    runs: Vec<(usize, usize, usize)>,
    rest: Vec<Mono>,
    coef: Vec<u64>,
}

struct Ctx<'a> {
    f: &'a Poly,
    g: &'a Poly,
    fc: Vec<u64>,
    gc: Vec<u64>,
    fld: Field,
    v: usize,
    /// the other variables
    ov: Vec<usize>,
    df: usize,
    dg: usize,
    delta: &'a Poly,
    delta_c: Vec<u64>,
    /// highest exponent of each of `ov` in `f`, `g` and `delta`
    top: Vec<usize>,
    top_delta: Vec<usize>,
}

fn field_mask(vars: &[usize]) -> u64 {
    vars.iter().fold(0u64, |m, v| m | (0xffu64 << shift(*v)))
}

impl Ctx<'_> {
    fn reduce(&self, p: &Poly, coefs: &[u64], j: usize, beta: &[Vec<u64>]) -> Side {
        let keep = field_mask(self.ov.get(..=j).unwrap_or(&[]));
        let mut rows: Vec<(usize, Mono, u64)> = Vec::with_capacity(p.terms.len());
        for ((m, _), c) in p.terms.iter().zip(coefs) {
            let mut val = *c;
            for (var, table) in self.ov.iter().zip(beta).skip(j + 1) {
                let e = exponent(*m, *var) as usize;
                if e > 0 {
                    val = self.fld.mul(val, at(table, e));
                }
            }
            if val != 0 {
                rows.push((exponent(*m, self.v) as usize, *m & keep, val));
            }
        }
        rows.sort_unstable_by_key(|r| (r.0, r.1));
        let mut runs: Vec<(usize, usize, usize)> = Vec::new();
        let mut rest: Vec<Mono> = Vec::new();
        let mut coef: Vec<u64> = Vec::new();
        for (e, mono, val) in rows {
            if let (Some(&last), Some(c)) = (rest.last(), coef.last_mut())
                && last == mono
                && runs.last().is_some_and(|r| r.0 == e)
            {
                *c = self.fld.add(*c, val);
                continue;
            }
            if runs.last().is_none_or(|r| r.0 != e) {
                runs.push((e, rest.len(), rest.len()));
            }
            rest.push(mono);
            coef.push(val);
            if let Some(r) = runs.last_mut() {
                r.2 = rest.len();
            }
        }
        Side { runs, rest, coef }
    }

    /// Image of `p` with every other variable replaced by `tabs`' power tables.
    fn image(&self, p: &Poly, coefs: &[u64], tabs: &[Vec<u64>], deg: usize) -> Vec<u64> {
        let mut out = vec![0u64; deg + 1];
        for ((m, _), c) in p.terms.iter().zip(coefs) {
            let mut val = *c;
            for (t, var) in self.ov.iter().enumerate() {
                let e = exponent(*m, *var) as usize;
                if e > 0 {
                    val = self.fld.mul(val, pw(tabs, t, e));
                }
            }
            let e = exponent(*m, self.v) as usize;
            modify(&mut out, e, |o| self.fld.add(o, val));
        }
        out
    }

    fn delta_at(&self, tabs: &[Vec<u64>]) -> u64 {
        let mut s = 0u64;
        for ((m, _), c) in self.delta.terms.iter().zip(&self.delta_c) {
            let mut val = *c;
            for (t, var) in self.ov.iter().enumerate() {
                let e = exponent(*m, *var) as usize;
                if e > 0 {
                    val = self.fld.mul(val, pw(tabs, t, e));
                }
            }
            s = self.fld.add(s, val);
        }
        s
    }

    /// The image gcd of `f` and `g` for the point, scaled so its leading coefficient is `delta(point)`.
    fn image_gcd(&self, fi: Vec<u64>, gi: Vec<u64>, d: Option<usize>, delta: u64) -> Option<Vec<u64>> {
        let (mut fi, mut gi) = (fi, gi);
        trim(&mut fi);
        trim(&mut gi);
        if fi.len() != self.df + 1 || gi.len() != self.dg + 1 || delta == 0 {
            return None;
        }
        let mut h = ugcd(&self.fld, fi, gi);
        if d.is_some_and(|d| h.len() != d + 1) {
            return None;
        }
        for c in &mut h {
            *c = self.fld.mul(*c, delta);
        }
        Some(h)
    }

    fn tables(&self, points: &[u64], tops: &[usize]) -> Vec<Vec<u64>> {
        points.iter().zip(tops).map(|(x, t)| self.fld.powers(*x, *t)).collect()
    }
}

/// Solves `sum_l a_l w_l^k = y_k` for `k = 1..=n` (the transposed Vandermonde system) in O(n^2).
struct Vandermonde {
    n: usize,
    /// row `l`: `a_l = sum_k q[l][k] * y[k + 1]`
    q: Vec<u64>,
}

impl Vandermonde {
    fn new(f: &Field, w: &[u64]) -> Option<Vandermonde> {
        let n = w.len();
        if n == 0 {
            return Some(Vandermonde { n, q: Vec::new() });
        }
        let mut master = vec![f.one()];
        for wl in w {
            let mut next = vec![0u64; master.len() + 1];
            for (k, c) in master.iter().enumerate() {
                modify(&mut next, k + 1, |x| f.add(x, *c));
                modify(&mut next, k, |x| f.sub(x, f.mul(*c, *wl)));
            }
            master = next;
        }
        let mut q = vec![0u64; n * n];
        for (row, wl) in q.chunks_exact_mut(n).zip(w) {
            let mut carry = at(&master, n);
            for (k, slot) in row.iter_mut().enumerate().rev() {
                *slot = carry;
                carry = f.add(at(&master, k), f.mul(*wl, carry));
            }
            let mut at_w = 0u64;
            for c in row.iter().rev() {
                at_w = f.add(f.mul(at_w, *wl), *c);
            }
            let denom = f.mul(at_w, *wl);
            if denom == 0 {
                return None;
            }
            let inv = f.inv(denom);
            for c in row.iter_mut() {
                *c = f.mul(*c, inv);
            }
        }
        Some(Vandermonde { n, q })
    }

    fn solve(&self, f: &Field, y: &[u64]) -> Vec<u64> {
        self.q.chunks_exact(self.n.max(1)).take(self.n).map(|row| row.iter().zip(y).fold(0u64, |s, (a, b)| f.add(s, f.mul(*a, *b)))).collect()
    }
}

/// A polynomial in one variable kept in Newton form over the nodes seen so far.
struct Newton {
    coef: Vec<u64>,
}

impl Newton {
    fn eval(&self, f: &Field, nodes: &[u64], x: u64) -> u64 {
        let mut r = 0u64;
        for (k, c) in self.coef.iter().enumerate().rev() {
            let step = if k + 1 < self.coef.len() { f.sub(x, at(nodes, k)) } else { f.one() };
            r = f.add(f.mul(r, step), *c);
        }
        r
    }

    /// The coefficients of `1, x, x^2, ...`.
    fn monomial(&self, f: &Field, nodes: &[u64]) -> Vec<u64> {
        let mut p: Vec<u64> = Vec::new();
        for (k, ck) in self.coef.iter().enumerate().rev() {
            // p := p * (x - nodes[k]) + coef[k]
            let node = at(nodes, k);
            let mut next = vec![0u64; p.len() + 1];
            for (i, c) in p.iter().enumerate() {
                modify(&mut next, i + 1, |x| f.add(x, *c));
                modify(&mut next, i, |x| f.sub(x, f.mul(*c, node)));
            }
            modify(&mut next, 0, |x| f.add(x, *ck));
            p = next;
        }
        p
    }
}

/// The product of the table powers for `mono`'s exponents in the first `upto` of the variables `ov`.
fn monomial_value(f: &Field, mono: Mono, ov: &[usize], tabs: &[Vec<u64>], upto: usize) -> u64 {
    ov.iter().take(upto).enumerate().fold(f.one(), |a, (t, var)| f.mul(a, pw(tabs, t, exponent(mono, *var) as usize)))
}

fn stage_points(rng: &mut Rng, f: &Field, n: usize) -> Vec<u64> {
    (0..n).map(|_| rng.element(f)).collect()
}

/// The degree in `x_v` of the gcd modulo this prime: the smallest seen over two random points (never below the true
/// degree, equal to it unless the prime or the points are unlucky). `None` when a point is unusable.
fn probe_degree(cx: &Ctx<'_>, rng: &mut Rng) -> Option<usize> {
    let m = cx.ov.len();
    let mut d: Option<usize> = None;
    for _ in 0..2 {
        let pt = stage_points(rng, &cx.fld, m);
        let tabs = cx.tables(&pt, &cx.top);
        let (fi, gi) = (cx.image(cx.f, &cx.fc, &tabs, cx.df), cx.image(cx.g, &cx.gc, &tabs, cx.dg));
        let dt = cx.tables(&pt, &cx.top_delta);
        let h = cx.image_gcd(fi, gi, None, cx.delta_at(&dt))?;
        d = Some(d.map_or(h.len() - 1, |d| d.min(h.len() - 1)));
    }
    d
}

/// Rebuild the gcd modulo one prime, given its degree `d >= 1` in `x_v`. `Ok(None)`: the points were unlucky.
fn image_mod_prime(cx: &Ctx<'_>, d: usize, rng: &mut Rng, stop: Stop<'_>) -> Result<Option<Vec<(Mono, u64)>>, PolyError> {
    let fld = &cx.fld;
    let m = cx.ov.len();
    let beta = stage_points(rng, fld, m);
    let beta_tabs: Vec<Vec<u64>> = beta.iter().zip(&cx.top).map(|(b, t)| fld.powers(*b, *t)).collect();
    // support found so far, per power of x_v: monomials in ov[..j]
    let mut skel: Vec<Vec<(Mono, u64)>> = (0..=d).map(|_| vec![(0, 0)]).collect();
    for j in 0..m {
        if stop() {
            return Err(PolyError::Stopped);
        }
        let Some(&xj) = cx.ov.get(j) else { return Ok(None) };
        let (fs, gs) = (cx.reduce(cx.f, &cx.fc, j, &beta_tabs), cx.reduce(cx.g, &cx.gc, j, &beta_tabs));
        let rows = skel.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let s = stage_points(rng, fld, j);
        let s_tabs: Vec<Vec<u64>> = s.iter().zip(&cx.top).map(|(x, t)| fld.powers(*x, *t)).collect();
        // solvers for the monomial values at the sample points s^k
        let mut solvers = Vec::with_capacity(d + 1);
        for monos in &skel {
            let w: Vec<u64> = monos.iter().map(|(mono, _)| monomial_value(fld, *mono, &cx.ov, &s_tabs, j)).collect();
            let Some(vm) = Vandermonde::new(fld, &w) else { return Ok(None) };
            solvers.push(vm);
        }
        let step_of = |side: &Side| -> Vec<u64> { side.rest.iter().map(|mono| monomial_value(fld, *mono, &cx.ov, &s_tabs, j)).collect() };
        let (fstep, gstep) = (step_of(&fs), step_of(&gs));
        let dk = cx.top.get(j).copied().unwrap_or(0);
        let mut nodes: Vec<u64> = Vec::new();
        // Newton form per (power of x_v, monomial)
        let mut newton: Vec<Vec<Newton>> = skel.iter().map(|sk| sk.iter().map(|_| Newton { coef: Vec::new() }).collect()).collect();
        // the images for one node `gamma` of `x_j`: independent of every other node
        let node_images = |gamma: u64| -> Option<Vec<Vec<u64>>> {
            let gpow = fld.powers(gamma, dk);
            let init =
                |side: &Side| -> Vec<u64> { side.rest.iter().zip(&side.coef).map(|(mono, c)| fld.mul(*c, at(&gpow, exponent(*mono, xj) as usize))).collect() };
            let (mut fcur, mut gcur) = (init(&fs), init(&gs));
            // the point for the fixed variables, as power tables for delta
            let mut xs: Vec<u64> = vec![fld.one(); j];
            let mut images: Vec<Vec<u64>> = Vec::with_capacity(rows);
            for _ in 0..rows {
                for (x, sv) in xs.iter_mut().zip(&s) {
                    *x = fld.mul(*x, *sv);
                }
                let fi = advance(fld, &fs, &mut fcur, &fstep, cx.df);
                let gi = advance(fld, &gs, &mut gcur, &gstep, cx.dg);
                let mut point = xs.clone();
                point.push(gamma);
                point.extend(beta.iter().skip(j + 1));
                let dt = cx.tables(&point, &cx.top_delta);
                images.push(cx.image_gcd(fi, gi, Some(d), cx.delta_at(&dt))?);
            }
            Some(images)
        };
        // enough work per node to be worth a thread each
        let threads = if rows * (fs.coef.len() + gs.coef.len()) >= PARALLEL_WORK { parallelism() } else { 1 };
        'nodes: while nodes.len() <= dk {
            if stop() {
                return Err(PolyError::Stopped);
            }
            let mut gammas: Vec<u64> = Vec::new();
            while gammas.len() < (dk + 1 - nodes.len()).min(threads) {
                let gamma = rng.element(fld);
                if !nodes.contains(&gamma) && !gammas.contains(&gamma) {
                    gammas.push(gamma);
                }
            }
            let batch = per_node(&gammas, &node_images);
            for (gamma, images) in gammas.into_iter().zip(batch) {
                let Some(Some(images)) = images else { return Ok(None) };
                // values of every coefficient at this node
                let mut values: Vec<Vec<u64>> = Vec::with_capacity(d + 1);
                for (i, solver) in solvers.iter().enumerate() {
                    let y: Vec<u64> = images.iter().take(solver.n).map(|h| at(h, i)).collect();
                    values.push(solver.solve(fld, &y));
                }
                let converged =
                    !nodes.is_empty() && newton.iter().zip(&values).all(|(row, vals)| row.iter().zip(vals).all(|(nw, y)| nw.eval(fld, &nodes, gamma) == *y));
                if converged {
                    break 'nodes;
                }
                let factor = nodes.iter().fold(fld.one(), |a, x| fld.mul(a, fld.sub(gamma, *x)));
                let finv = fld.inv(factor);
                for (row, vals) in newton.iter_mut().zip(&values) {
                    for (nw, y) in row.iter_mut().zip(vals) {
                        let c = if nw.coef.is_empty() { *y } else { fld.mul(fld.sub(*y, nw.eval(fld, &nodes, gamma)), finv) };
                        nw.coef.push(c);
                    }
                }
                nodes.push(gamma);
            }
        }
        // the next support: every (monomial, power of x_j) with a nonzero coefficient
        let var_shift = shift(xj);
        for (sk, row) in skel.iter_mut().zip(&newton) {
            let mut next = Vec::new();
            for ((mono, _), nw) in sk.iter().zip(row) {
                for (e, c) in nw.monomial(fld, &nodes).into_iter().enumerate() {
                    if c != 0 {
                        next.push((*mono + ((e as u64) << var_shift), c));
                    }
                }
            }
            *sk = next;
        }
        if skel.iter().all(Vec::is_empty) {
            return Ok(None);
        }
    }
    let mut out = Vec::new();
    for (i, sk) in skel.iter().enumerate() {
        for (mono, c) in sk {
            out.push((*mono + ((i as u64) << shift(cx.v)), fld.dec(*c)));
        }
    }
    Ok(Some(out))
}

/// Rebuild the gcd modulo another prime when the support (monomials per power of `x_v`) is already known from the
/// first prime: one pass over sample points `s^k`, no interpolation in single variables.
fn image_known(cx: &Ctx<'_>, support: &[Vec<Mono>], rng: &mut Rng, stop: Stop<'_>) -> Result<Option<Vec<(Mono, u64)>>, PolyError> {
    let fld = &cx.fld;
    let m = cx.ov.len();
    let d = support.len().saturating_sub(1);
    let (fs, gs) = (cx.reduce(cx.f, &cx.fc, m - 1, &[]), cx.reduce(cx.g, &cx.gc, m - 1, &[]));
    let s = stage_points(rng, fld, m);
    let s_tabs: Vec<Vec<u64>> = s.iter().zip(&cx.top).map(|(x, t)| fld.powers(*x, *t)).collect();
    let value_at = |mono: Mono| monomial_value(fld, mono, &cx.ov, &s_tabs, m);
    let mut solvers = Vec::with_capacity(d + 1);
    for monos in support {
        let w: Vec<u64> = monos.iter().map(|mono| value_at(*mono)).collect();
        let Some(vm) = Vandermonde::new(fld, &w) else { return Ok(None) };
        solvers.push(vm);
    }
    let rows = support.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let (fstep, gstep): (Vec<u64>, Vec<u64>) = (fs.rest.iter().map(|x| value_at(*x)).collect(), gs.rest.iter().map(|x| value_at(*x)).collect());
    let (mut fcur, mut gcur) = (fs.coef.clone(), gs.coef.clone());
    let mut xs: Vec<u64> = vec![fld.one(); m];
    let mut images: Vec<Vec<u64>> = Vec::with_capacity(rows);
    for _ in 0..rows {
        if stop() {
            return Err(PolyError::Stopped);
        }
        for (x, sv) in xs.iter_mut().zip(&s) {
            *x = fld.mul(*x, *sv);
        }
        let fi = advance(fld, &fs, &mut fcur, &fstep, cx.df);
        let gi = advance(fld, &gs, &mut gcur, &gstep, cx.dg);
        let dt = cx.tables(&xs, &cx.top_delta);
        let Some(h) = cx.image_gcd(fi, gi, Some(d), cx.delta_at(&dt)) else { return Ok(None) };
        images.push(h);
    }
    let mut out = Vec::new();
    for (i, (monos, solver)) in support.iter().zip(&solvers).enumerate() {
        let y: Vec<u64> = images.iter().take(solver.n).map(|h| at(h, i)).collect();
        for (mono, a) in monos.iter().zip(solver.solve(fld, &y)) {
            if a != 0 {
                out.push((*mono + ((i as u64) << shift(cx.v)), fld.dec(a)));
            }
        }
    }
    Ok(Some(out))
}

/// Advance every term to the next sample point and collect the image in `x_v`.
fn advance(f: &Field, side: &Side, cur: &mut [u64], step: &[u64], deg: usize) -> Vec<u64> {
    let mut out = vec![0u64; deg + 1];
    for &(e, lo, hi) in &side.runs {
        let mut acc = 0u128;
        for (c, st) in cur.iter_mut().zip(step).take(hi).skip(lo) {
            *c = f.mul(*c, *st);
            acc += u128::from(*c);
        }
        if e <= deg {
            modify(&mut out, e, |_| (acc % u128::from(f.p)) as u64);
        }
    }
    out
}

/// Add the residues `new` modulo `p` to the integers `acc` known modulo `modulus`.
fn combine(acc: &mut Vec<(Mono, IBig)>, modulus: &IBig, new: &[(Mono, u64)], p: u64) {
    let mut new: Vec<(Mono, u64)> = new.to_vec();
    new.sort_unstable_by_key(|t| t.0);
    let m_mod = u64::try_from(modulus % IBig::from(p)).unwrap_or(0);
    let m_inv = pow_mod(m_mod, p - 2, p);
    let mut out: Vec<(Mono, IBig)> = Vec::with_capacity(acc.len().max(new.len()));
    let (mut i, mut j) = (0, 0);
    while i < acc.len() || j < new.len() {
        let (mono, a, r) = match (acc.get(i), new.get(j)) {
            (Some((ma, a)), Some((mn, r))) if ma == mn => {
                i += 1;
                j += 1;
                (*ma, a.clone(), *r)
            }
            (Some((ma, a)), Some((mn, _))) if ma < mn => {
                i += 1;
                (*ma, a.clone(), 0)
            }
            (Some((ma, a)), None) => {
                i += 1;
                (*ma, a.clone(), 0)
            }
            (_, Some((mn, r))) => {
                j += 1;
                (*mn, IBig::ZERO, *r)
            }
            (None, None) => break,
        };
        let a_mod = u64::try_from(&a % IBig::from(p)).unwrap_or(0);
        let diff = (r + p - a_mod) % p;
        let t = mul_mod(diff, m_inv, p);
        out.push((mono, a + modulus * IBig::from(t)));
    }
    *acc = out;
}

/// Inputs with at least this many terms are checked on two threads (a thread costs about as much as dividing a few
/// hundred terms).
const PARALLEL_CHECK: usize = 4000;

/// Does `cand` divide both `f` and `g` exactly? The two divisions are independent, so big ones run side by side.
fn divides_both(f: &Poly, g: &Poly, cand: &Poly) -> bool {
    let divides = |x: &Poly| matches!(x.div_exact(cand), Ok(Some(_)));
    if f.terms.len() + g.terms.len() < PARALLEL_CHECK {
        return divides(f) && divides(g);
    }
    std::thread::scope(|s| {
        let second = s.spawn(|| divides(g));
        divides(f) && second.join().unwrap_or(false)
    })
}

/// The gcd of two polynomials that are primitive in `x_v`, or `None` when this method does not apply or gives up.
pub(crate) fn gcd_primitive(f: &Poly, g: &Poly, v: usize, min_terms: usize, stop: Stop<'_>) -> Result<Option<Poly>, PolyError> {
    let n = f.nvars;
    let ov: Vec<usize> = (0..n).filter(|i| *i != v && (f.degree_in(*i) > 0 || g.degree_in(*i) > 0)).collect();
    if ov.is_empty() || f.terms.len() + g.terms.len() < min_terms {
        return Ok(None);
    }
    let (df, dg) = (f.degree_in(v) as usize, g.degree_in(v) as usize);
    let lc = |p: &Poly| p.coeffs_in(v).pop().unwrap_or_else(|| Poly::zero(n));
    let delta = gcd_rec(&lc(f), &lc(g), stop)?;
    let top = |p: &Poly| -> Vec<usize> { ov.iter().map(|i| p.degree_in(*i) as usize).collect() };
    let (tf, tg, td) = (top(f), top(g), top(&delta));
    let tops: Vec<usize> = tf.iter().zip(&tg).map(|(a, b)| *a.max(b)).collect();
    let mut p = 1u64 << 62;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut acc: Vec<(Mono, IBig)> = Vec::new();
    let mut modulus = IBig::ONE;
    let mut tried_last: Option<Poly> = None;
    let mut prev: Option<Poly> = None;
    let mut support: Option<Vec<Vec<Mono>>> = None;
    let mut degree: Option<usize> = None;
    let mut primes_used = 0;
    let mut attempts = 0;
    while primes_used < MAX_PRIMES && attempts < MAX_PRIMES * MAX_RETRIES {
        attempts += 1;
        p = prime_below(p);
        let fld = Field::new(p);
        let cx = Ctx {
            f,
            g,
            fc: f.terms.iter().map(|(_, c)| fld.enc_big(c)).collect(),
            gc: g.terms.iter().map(|(_, c)| fld.enc_big(c)).collect(),
            fld,
            v,
            ov: ov.clone(),
            df,
            dg,
            delta: &delta,
            delta_c: delta.terms.iter().map(|(_, c)| fld.enc_big(c)).collect(),
            top: tops.clone(),
            top_delta: td.clone(),
        };
        let Some(dp) = probe_degree(&cx, &mut rng) else { continue };
        if dp == 0 {
            return Ok(Some(Poly::constant(n, IBig::ONE)));
        }
        // a prime that gives a smaller degree shows the earlier ones were unlucky; a larger one is unlucky itself
        match degree {
            Some(d) if dp > d => continue,
            Some(d) if dp < d => {
                (acc, modulus, support, tried_last, prev, primes_used) = (Vec::new(), IBig::ONE, None, None, None, 0);
                degree = Some(dp);
                let _ = d;
            }
            None => degree = Some(dp),
            _ => {}
        }
        let terms = match &support {
            Some(sup) => match image_known(&cx, sup, &mut rng, stop)? {
                Some(t) => t,
                None => continue,
            },
            None => match image_mod_prime(&cx, dp, &mut rng, stop)? {
                None => continue,
                Some(t) => {
                    let mut sup: Vec<Vec<Mono>> = vec![Vec::new(); dp + 1];
                    let low = !(0xffu64 << shift(v));
                    for (m, _) in &t {
                        if let Some(slot) = sup.get_mut(exponent(*m, v) as usize) {
                            slot.push(*m & low);
                        }
                    }
                    support = Some(sup);
                    t
                }
            },
        };
        combine(&mut acc, &modulus, &terms, p);
        modulus *= IBig::from(p);
        primes_used += 1;
        let half = &modulus >> 1;
        let lifted: Vec<(Mono, IBig)> = acc.iter().map(|(m, c)| (*m, if *c > half { c - &modulus } else { c.clone() })).collect();
        let h = Poly::from_terms(n, lifted);
        // the exact trial division is costly: only try once another prime has left the lifted result unchanged
        let stable = prev.as_ref() == Some(&h);
        prev = Some(h.clone());
        if !stable || tried_last.as_ref() == Some(&h) || h.is_zero() {
            continue;
        }
        tried_last = Some(h.clone());
        let mut cont = Poly::zero(n);
        for c in h.coeffs_in(v) {
            cont = gcd_rec(&cont, &c, stop)?;
        }
        let Ok(Some(cand)) = h.div_exact(&cont) else { continue };
        let cand = positive(cand);
        if divides_both(f, g, &cand) {
            return Ok(Some(cand));
        }
    }
    Ok(None)
}
