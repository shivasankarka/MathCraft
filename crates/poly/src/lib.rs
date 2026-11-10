//! Sparse multivariate polynomials over the integers (layer L1): the fast path for `expand`, products, powers and
//! (later) greatest common divisors.
//!
//! A monomial is packed into one `u64`: eight variables with one byte each, the first variable in the most
//! significant byte, so comparing two monomials as integers is the lexicographic monomial order and multiplying them
//! is one integer addition. Exponents stay below 128 so the top bit of every byte is free for overflow and
//! divisibility checks. Terms are kept sorted from largest monomial down, with no zero coefficients.
//!
//! Products use a heap that merges the partial products of one operand's terms, so memory stays proportional to the
//! operand size rather than to the number of partial products.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use dashu::integer::IBig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Most variables a polynomial can have.
pub const MAX_VARS: usize = 8;
/// Largest exponent of any variable (also after a product).
pub const MAX_EXP: u32 = 127;

/// An exponent vector packed one byte per variable.
pub type Mono = u64;

const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

/// Why an operation could not be done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolyError {
    /// An exponent would exceed [`MAX_EXP`], or more than [`MAX_VARS`] variables were asked for.
    TooBig,
    /// The two polynomials use a different number of variables.
    Mismatch,
    /// The caller's check asked to stop (cancelled or out of time).
    Stopped,
}

impl std::fmt::Display for PolyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolyError::TooBig => f.write_str("polynomial exponents or variable count are too large"),
            PolyError::Mismatch => f.write_str("polynomials have different variables"),
            PolyError::Stopped => f.write_str("stopped"),
        }
    }
}

impl std::error::Error for PolyError {}

/// Shift of variable `i` in a packed monomial (variable 0 is the most significant byte).
fn shift(i: usize) -> u32 {
    (7 - i.min(7) as u32) * 8
}

/// Pack exponents (one per variable); `None` if one is above [`MAX_EXP`] or there are too many.
pub fn pack(exps: &[u32]) -> Option<Mono> {
    if exps.len() > MAX_VARS {
        return None;
    }
    let mut m = 0u64;
    for (i, e) in exps.iter().enumerate() {
        if *e > MAX_EXP {
            return None;
        }
        m |= u64::from(*e) << shift(i);
    }
    Some(m)
}

/// The exponent of variable `i`.
pub fn exponent(m: Mono, i: usize) -> u32 {
    ((m >> shift(i)) & 0xff) as u32
}

/// Does `b` divide `a` (every exponent of `b` is at most the one in `a`)?
fn divides(b: Mono, a: Mono) -> bool {
    // per byte: (a | 0x80) - b stays >= 0x80 exactly when a >= b, given all bytes are below 0x80
    ((a | HIGH_BITS).wrapping_sub(b)) & HIGH_BITS == HIGH_BITS
}

/// A sparse polynomial in `nvars` variables with integer coefficients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poly {
    nvars: usize,
    /// Sorted by monomial, largest first; coefficients are never zero.
    terms: Vec<(Mono, IBig)>,
}

impl Poly {
    pub fn zero(nvars: usize) -> Poly {
        Poly { nvars, terms: Vec::new() }
    }

    pub fn constant(nvars: usize, c: IBig) -> Poly {
        if c == IBig::ZERO { Poly::zero(nvars) } else { Poly { nvars, terms: vec![(0, c)] } }
    }

    /// The variable number `i` (a polynomial with the single term `x_i`).
    pub fn var(nvars: usize, i: usize) -> Result<Poly, PolyError> {
        if nvars > MAX_VARS || i >= nvars {
            return Err(PolyError::TooBig);
        }
        Ok(Poly { nvars, terms: vec![(1u64 << shift(i), IBig::ONE)] })
    }

    /// Build from unsorted terms: sorts, merges equal monomials and drops zeros.
    pub fn from_terms(nvars: usize, mut terms: Vec<(Mono, IBig)>) -> Poly {
        terms.sort_unstable_by_key(|t| std::cmp::Reverse(t.0));
        let mut out: Vec<(Mono, IBig)> = Vec::with_capacity(terms.len());
        for (m, c) in terms {
            match out.last_mut() {
                Some((lm, lc)) if *lm == m => *lc += c,
                _ => out.push((m, c)),
            }
        }
        out.retain(|(_, c)| *c != IBig::ZERO);
        Poly { nvars, terms: out }
    }

    pub fn nvars(&self) -> usize {
        self.nvars
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn is_zero(&self) -> bool {
        self.terms.is_empty()
    }

    /// The terms, largest monomial first.
    pub fn terms(&self) -> &[(Mono, IBig)] {
        &self.terms
    }

    /// Highest exponent of variable `i` in any term.
    pub fn degree_in(&self, i: usize) -> u32 {
        self.terms.iter().map(|(m, _)| exponent(*m, i)).max().unwrap_or(0)
    }

    pub fn total_degree(&self) -> u32 {
        self.terms.iter().map(|(m, _)| (0..self.nvars).map(|i| exponent(*m, i)).sum::<u32>()).max().unwrap_or(0)
    }

    pub fn neg(&self) -> Poly {
        Poly { nvars: self.nvars, terms: self.terms.iter().map(|(m, c)| (*m, -c)).collect() }
    }

    /// Multiply every coefficient by `k`.
    pub fn scale(&self, k: &IBig) -> Poly {
        if *k == IBig::ZERO {
            return Poly::zero(self.nvars);
        }
        Poly { nvars: self.nvars, terms: self.terms.iter().map(|(m, c)| (*m, c * k)).collect() }
    }

    pub fn add(&self, other: &Poly) -> Result<Poly, PolyError> {
        self.merge(other, false)
    }

    pub fn sub(&self, other: &Poly) -> Result<Poly, PolyError> {
        self.merge(other, true)
    }

    /// Sum or difference by merging the two sorted term lists.
    fn merge(&self, other: &Poly, subtract: bool) -> Result<Poly, PolyError> {
        if self.nvars != other.nvars {
            return Err(PolyError::Mismatch);
        }
        let (a, b) = (&self.terms, &other.terms);
        let mut out = Vec::with_capacity(a.len() + b.len());
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            match (a.get(i), b.get(j)) {
                (Some(x), Some(y)) => match x.0.cmp(&y.0) {
                    Ordering::Greater => {
                        out.push(x.clone());
                        i += 1;
                    }
                    Ordering::Less => {
                        out.push((y.0, if subtract { -&y.1 } else { y.1.clone() }));
                        j += 1;
                    }
                    Ordering::Equal => {
                        let c = if subtract { &x.1 - &y.1 } else { &x.1 + &y.1 };
                        if c != IBig::ZERO {
                            out.push((x.0, c));
                        }
                        i += 1;
                        j += 1;
                    }
                },
                (Some(x), None) => {
                    out.push(x.clone());
                    i += 1;
                }
                (None, Some(y)) => {
                    out.push((y.0, if subtract { -&y.1 } else { y.1.clone() }));
                    j += 1;
                }
                (None, None) => break,
            }
        }
        Ok(Poly { nvars: self.nvars, terms: out })
    }

    /// The product, by a heap merge of the partial products.
    pub fn mul(&self, other: &Poly) -> Result<Poly, PolyError> {
        self.mul_with(other, &mut || false)
    }

    /// The product; `stop` is called now and then and returns `true` to abandon the work (`Err(Stopped)`).
    pub fn mul_with(&self, other: &Poly, stop: &mut dyn FnMut() -> bool) -> Result<Poly, PolyError> {
        if self.nvars != other.nvars {
            return Err(PolyError::Mismatch);
        }
        if self.is_zero() || other.is_zero() {
            return Ok(Poly::zero(self.nvars));
        }
        for v in 0..self.nvars {
            if self.degree_in(v) + other.degree_in(v) > MAX_EXP {
                return Err(PolyError::TooBig);
            }
        }
        if let Some(terms) = mul_small(&self.terms, &other.terms, stop) {
            return terms.map(|terms| Poly { nvars: self.nvars, terms });
        }
        // the smaller operand drives the heap (fewer heap entries, longer runs along the other)
        let (a, b) = if self.len() <= other.len() { (&self.terms, &other.terms) } else { (&other.terms, &self.terms) };
        let (n, m) = (a.len(), b.len());
        let mut heap: BinaryHeap<Entry> = BinaryHeap::with_capacity(n.min(1 << 16));
        if let (Some(x), Some(y)) = (a.first(), b.first()) {
            heap.push(Entry { mono: x.0 + y.0, i: 0, j: 0 });
        }
        let mut out: Vec<(Mono, IBig)> = Vec::with_capacity(n.saturating_mul(m).min(1 << 22));
        let mut groups = 0u32;
        while let Some(top) = heap.peek() {
            groups = groups.wrapping_add(1);
            if groups.is_multiple_of(4096) && stop() {
                return Err(PolyError::Stopped);
            }
            let mono = top.mono;
            let mut acc = IBig::ZERO;
            while heap.peek().is_some_and(|e| e.mono == mono) {
                let Some(Entry { i, j, .. }) = heap.pop() else { break };
                if let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
                    acc += &x.1 * &y.1;
                }
                if j + 1 < m
                    && let (Some(x), Some(y)) = (a.get(i), b.get(j + 1))
                {
                    heap.push(Entry { mono: x.0 + y.0, i, j: j + 1 });
                }
                // the first partial product of the next row enters when this row's first is consumed
                if j == 0
                    && i + 1 < n
                    && let (Some(x), Some(y)) = (a.get(i + 1), b.first())
                {
                    heap.push(Entry { mono: x.0 + y.0, i: i + 1, j: 0 });
                }
            }
            if acc != IBig::ZERO {
                out.push((mono, acc));
            }
        }
        Ok(Poly { nvars: self.nvars, terms: out })
    }

    /// `self ** n` by repeated squaring.
    pub fn pow(&self, n: u32) -> Result<Poly, PolyError> {
        self.pow_with(n, &mut || false)
    }

    pub fn pow_with(&self, mut n: u32, stop: &mut dyn FnMut() -> bool) -> Result<Poly, PolyError> {
        let mut result = Poly::constant(self.nvars, IBig::ONE);
        let mut base = self.clone();
        while n > 0 {
            if n & 1 == 1 {
                result = result.mul_with(&base, stop)?;
            }
            n >>= 1;
            if n > 0 {
                base = base.mul_with(&base, stop)?;
            }
        }
        Ok(result)
    }

    /// The quotient when `divisor` divides `self` exactly, else `None`. A heap merges the pending products
    /// `quotient term * divisor term`, so the work is proportional to the product sizes, not to repeated subtraction.
    pub fn div_exact(&self, divisor: &Poly) -> Result<Option<Poly>, PolyError> {
        if self.nvars != divisor.nvars {
            return Err(PolyError::Mismatch);
        }
        let Some((lm, lc)) = divisor.terms.first() else { return Ok(None) };
        if let Some(r) = self.div_exact_words(divisor) {
            return Ok(r);
        }
        let d = &divisor.terms;
        let mut quotient: Vec<(Mono, IBig)> = Vec::new();
        let mut heap: BinaryHeap<Entry> = BinaryHeap::new();
        let mut ip = 0usize;
        loop {
            let next = self.terms.get(ip).map(|t| t.0);
            let Some(m) = next.into_iter().chain(heap.peek().map(|e| e.mono)).max() else { break };
            let mut s = IBig::ZERO;
            if let Some((_, c)) = self.terms.get(ip).filter(|t| t.0 == m) {
                s = c.clone();
                ip += 1;
            }
            while heap.peek().is_some_and(|e| e.mono == m) {
                let Some(Entry { i, j, .. }) = heap.pop() else { break };
                if let (Some(q), Some(dj)) = (quotient.get(i), d.get(j)) {
                    s -= &q.1 * &dj.1;
                    if let Some(next) = d.get(j + 1) {
                        heap.push(Entry { mono: q.0 + next.0, i, j: j + 1 });
                    }
                }
            }
            if s == IBig::ZERO {
                continue;
            }
            if !divides(*lm, m) || &s % lc != IBig::ZERO {
                return Ok(None);
            }
            let qm = m - lm;
            quotient.push((qm, &s / lc));
            if let Some(second) = d.get(1) {
                heap.push(Entry { mono: qm + second.0, i: quotient.len() - 1, j: 1 });
            }
        }
        Ok(Some(Poly { nvars: self.nvars, terms: quotient }))
    }

    /// `div_exact` for coefficients that fit in 128 bits. Pending sums live in a hash table and the heap holds only
    /// the distinct monomials, so each partial product costs one table update instead of a heap operation. A
    /// monomial is final once it is the largest pending one, because every later product is strictly smaller.
    /// `None` when the inputs do not qualify or a sum overflows; the caller then uses big integers.
    fn div_exact_words(&self, divisor: &Poly) -> Option<Option<Poly>> {
        let word = |t: &(Mono, IBig)| i128::try_from(&t.1).ok().map(|c| (t.0, c));
        let a: Vec<(Mono, i128)> = self.terms.iter().map(word).collect::<Option<_>>()?;
        let d: Vec<(Mono, i128)> = divisor.terms.iter().map(word).collect::<Option<_>>()?;
        let (lm, lc) = *d.first()?;
        let rest = d.get(1..)?;
        let mut table = Table::with_capacity(a.len() * 2);
        let mut heap: BinaryHeap<Mono> = BinaryHeap::with_capacity(a.len());
        for (m, c) in &a {
            table.add(*m, *c)?;
            heap.push(*m);
        }
        let mut quotient: Vec<(Mono, IBig)> = Vec::new();
        while let Some(m) = heap.pop() {
            let s = table.take(m);
            if s == 0 {
                continue;
            }
            if !divides(lm, m) || s % lc != 0 {
                return Some(None);
            }
            let (qm, qc) = (m - lm, s / lc);
            for (dm, dc) in rest {
                if table.add(qm + dm, qc.checked_mul(*dc)?.checked_neg()?)? {
                    heap.push(qm + dm);
                }
            }
            quotient.push((qm, IBig::from(qc)));
        }
        Some(Some(Poly { nvars: self.nvars, terms: quotient }))
    }
}

/// Products below this many partial products always use the heap: building a hash table would cost more.
const SMALL_PRODUCT: usize = 256;

/// The product when every coefficient is a machine word and the sums cannot overflow `i128`: each partial product is
/// added into an open-addressing table keyed by the monomial, then the table is sorted. Much faster than the heap
/// when the result is large. `None` when the coefficients are too big (the heap handles those).
fn mul_small(a: &[(Mono, IBig)], b: &[(Mono, IBig)], stop: &mut dyn FnMut() -> bool) -> Option<Result<Vec<(Mono, IBig)>, PolyError>> {
    if a.len().saturating_mul(b.len()) < SMALL_PRODUCT {
        return None;
    }
    let words = |p: &[(Mono, IBig)]| -> Option<(Vec<(Mono, i64)>, u128)> {
        let mut big = 0u128;
        let v = p.iter().map(|(m, c)| i64::try_from(c).ok().map(|c| (*m, c))).collect::<Option<Vec<_>>>()?;
        for (_, c) in &v {
            big = big.max(u128::from(c.unsigned_abs()));
        }
        Some((v, big))
    };
    let (wa, ma) = words(a)?;
    let (wb, mb) = words(b)?;
    // a result coefficient is a sum of at most min(len) partial products
    let bound = ma.checked_mul(mb)?.checked_mul(a.len().min(b.len()) as u128)?;
    if bound >= 1u128 << 126 {
        return None;
    }
    let mut table = Table::with_capacity(a.len().max(b.len()) * 4);
    for (m1, c1) in &wa {
        if stop() {
            return Some(Err(PolyError::Stopped));
        }
        let c1 = i128::from(*c1);
        for (m2, c2) in &wb {
            table.add(m1 + m2, c1 * i128::from(*c2))?;
        }
    }
    let mut out: Vec<(Mono, i128)> = table.into_entries();
    out.sort_unstable_by_key(|t| std::cmp::Reverse(t.0));
    Some(Ok(out.into_iter().map(|(m, c)| (m, IBig::from(c))).collect()))
}

/// Open-addressing hash table from monomial to accumulated coefficient (linear probing, grows at half full).
struct Table {
    keys: Vec<Mono>,
    vals: Vec<i128>,
    used: usize,
    bits: u32,
}

/// No valid monomial has every byte set (exponents stay below 128).
const EMPTY: Mono = u64::MAX;

impl Table {
    fn with_capacity(n: usize) -> Table {
        let bits = n.max(16).next_power_of_two().trailing_zeros();
        Table { keys: vec![EMPTY; 1usize << bits], vals: vec![0; 1usize << bits], used: 0, bits }
    }

    fn slot(&self, key: Mono) -> usize {
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - self.bits)) as usize
    }

    /// Adds `v` to the entry for `key`; `Some(true)` when the key is new, `None` when the sum overflows.
    fn add(&mut self, key: Mono, v: i128) -> Option<bool> {
        if self.used * 2 >= self.keys.len() {
            self.grow()?;
        }
        let mask = self.keys.len() - 1;
        let mut i = self.slot(key);
        loop {
            match self.keys.get(i) {
                Some(&k) if k == key => {
                    let slot = self.vals.get_mut(i)?;
                    *slot = slot.checked_add(v)?;
                    return Some(false);
                }
                Some(&k) if k == EMPTY => {
                    *self.keys.get_mut(i)? = key;
                    *self.vals.get_mut(i)? = v;
                    self.used += 1;
                    return Some(true);
                }
                _ => i = (i + 1) & mask,
            }
        }
    }

    /// The value for `key`, which is reset to zero (the key stays, so probing chains are not broken).
    fn take(&mut self, key: Mono) -> i128 {
        let mask = self.keys.len() - 1;
        let mut i = self.slot(key);
        loop {
            match self.keys.get(i) {
                Some(&k) if k == key => return self.vals.get_mut(i).map_or(0, std::mem::take),
                Some(&k) if k == EMPTY => return 0,
                _ => i = (i + 1) & mask,
            }
        }
    }

    fn grow(&mut self) -> Option<()> {
        let old = std::mem::replace(self, Table::with_capacity(self.keys.len() * 2));
        for (k, v) in old.keys.into_iter().zip(old.vals) {
            if k != EMPTY {
                self.add(k, v)?;
            }
        }
        Some(())
    }

    fn into_entries(self) -> Vec<(Mono, i128)> {
        self.keys.into_iter().zip(self.vals).filter(|(k, v)| *k != EMPTY && *v != 0).collect()
    }
}

/// One pending partial product `a[i] * b[j]` in the heap, ordered by its monomial.
struct Entry {
    mono: Mono,
    i: usize,
    j: usize,
}

impl PartialEq for Entry {
    fn eq(&self, o: &Self) -> bool {
        self.mono == o.mono
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry {
    fn cmp(&self, o: &Self) -> Ordering {
        self.mono.cmp(&o.mono)
    }
}

mod factor;
mod gcd;
mod modgcd;
pub use factor::Factorization;

#[cfg(test)]
mod tests;
