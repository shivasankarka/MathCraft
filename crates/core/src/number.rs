//! Exact and approximate numbers: arbitrary-precision integers and rationals (dashu), plus `f64` floats.
//!
//! Invariants: a `Rat` always has a denominator greater than one (anything else is an `Int`).

use dashu::base::BitTest;
use dashu::integer::{IBig, UBig};
use dashu::rational::RBig;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub enum Num {
    Int(IBig),
    Rat(RBig),
    Float(f64),
}

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_cmp(other) == Ordering::Equal
    }
}
impl Eq for Num {}

impl Hash for Num {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Num::Int(i) => {
                0u8.hash(state);
                i.to_string().hash(state)
            }
            Num::Rat(r) => {
                1u8.hash(state);
                r.numerator().to_string().hash(state);
                r.denominator().to_string().hash(state)
            }
            Num::Float(f) => {
                2u8.hash(state);
                f.to_bits().hash(state)
            }
        }
    }
}

impl From<i64> for Num {
    fn from(v: i64) -> Self {
        Num::Int(IBig::from(v))
    }
}

impl Num {
    pub fn int(v: i64) -> Num {
        Num::from(v)
    }

    pub fn zero() -> Num {
        Num::Int(IBig::ZERO)
    }

    pub fn one() -> Num {
        Num::Int(IBig::ONE)
    }

    /// Exact fraction `n / d`; `None` when `d` is zero.
    pub fn ratio(n: IBig, d: IBig) -> Option<Num> {
        if d == IBig::ZERO {
            return None;
        }
        let (sign, mag) = if d < IBig::ZERO { (-1, -d) } else { (1, d) };
        let n = if sign < 0 { -n } else { n };
        let den = UBig::try_from(mag).ok()?;
        Some(Num::from_rbig(RBig::from_parts(n, den)))
    }

    fn from_rbig(r: RBig) -> Num {
        if r.denominator() == &UBig::ONE { Num::Int(r.numerator().clone()) } else { Num::Rat(r) }
    }

    fn to_rbig(&self) -> Option<RBig> {
        match self {
            Num::Int(i) => Some(RBig::from(i.clone())),
            Num::Rat(r) => Some(r.clone()),
            Num::Float(_) => None,
        }
    }

    pub fn is_exact(&self) -> bool {
        !matches!(self, Num::Float(_))
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Num::Int(i) => *i == IBig::ZERO,
            Num::Rat(_) => false,
            Num::Float(f) => *f == 0.0,
        }
    }

    pub fn is_one(&self) -> bool {
        match self {
            Num::Int(i) => *i == IBig::ONE,
            Num::Rat(_) => false,
            Num::Float(f) => *f == 1.0,
        }
    }

    pub fn is_integer(&self) -> bool {
        match self {
            Num::Int(_) => true,
            Num::Rat(_) => false,
            Num::Float(f) => f.is_finite() && f.fract() == 0.0,
        }
    }

    pub fn is_negative(&self) -> bool {
        match self {
            Num::Int(i) => *i < IBig::ZERO,
            Num::Rat(r) => r.numerator() < &IBig::ZERO,
            Num::Float(f) => *f < 0.0,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Num::Int(i) => i64::try_from(i).ok(),
            _ => None,
        }
    }

    pub fn to_f64(&self) -> f64 {
        match self {
            Num::Int(i) => i.to_f64().value(),
            Num::Rat(r) => r.to_f64().value(),
            Num::Float(f) => *f,
        }
    }

    pub fn neg(&self) -> Num {
        match self {
            Num::Int(i) => Num::Int(-i.clone()),
            Num::Rat(r) => Num::Rat(-r.clone()),
            Num::Float(f) => Num::Float(-f),
        }
    }

    pub fn add(&self, o: &Num) -> Num {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a + b),
            (Num::Float(_), _) | (_, Num::Float(_)) => Num::Float(self.to_f64() + o.to_f64()),
            _ => match (self.to_rbig(), o.to_rbig()) {
                (Some(a), Some(b)) => Num::from_rbig(a + b),
                _ => Num::Float(self.to_f64() + o.to_f64()),
            },
        }
    }

    pub fn mul(&self, o: &Num) -> Num {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a * b),
            (Num::Float(_), _) | (_, Num::Float(_)) => Num::Float(self.to_f64() * o.to_f64()),
            _ => match (self.to_rbig(), o.to_rbig()) {
                (Some(a), Some(b)) => Num::from_rbig(a * b),
                _ => Num::Float(self.to_f64() * o.to_f64()),
            },
        }
    }

    /// Multiplicative inverse; `None` for exact zero.
    pub fn recip(&self) -> Option<Num> {
        match self {
            Num::Float(f) => Some(Num::Float(1.0 / f)),
            _ => {
                if self.is_zero() {
                    return None;
                }
                let (n, d) = self.parts()?;
                Num::ratio(IBig::from(d), n)
            }
        }
    }

    /// Numerator and denominator of an exact number.
    pub fn parts(&self) -> Option<(IBig, UBig)> {
        match self {
            Num::Int(i) => Some((i.clone(), UBig::ONE)),
            Num::Rat(r) => Some((r.numerator().clone(), r.denominator().clone())),
            Num::Float(_) => None,
        }
    }

    /// Integer power of an exact or float number. `None` when it would be undefined (0^-n) or too large.
    pub fn powi(&self, n: i64) -> Option<Num> {
        if let Num::Float(f) = self {
            let e = i32::try_from(n).ok()?;
            return Some(Num::Float(f.powi(e)));
        }
        if n == 0 {
            return Some(Num::one());
        }
        let mag = usize::try_from(n.unsigned_abs()).ok()?;
        let bits = match self {
            Num::Int(i) => i.bit_len(),
            Num::Rat(r) => r.numerator().bit_len().max(r.denominator().bit_len()),
            Num::Float(_) => 0,
        };
        if bits.saturating_mul(mag) > MAX_RESULT_BITS {
            return None;
        }
        let base = if n < 0 { self.recip()? } else { self.clone() };
        match base {
            Num::Int(i) => Some(Num::Int(i.pow(mag))),
            Num::Rat(r) => Some(Num::from_rbig(r.pow(isize::try_from(mag).ok()?))),
            Num::Float(f) => Some(Num::Float(f.powi(i32::try_from(n).ok()?))),
        }
    }

    /// Total order used for canonical sorting and comparison of values (exact where possible).
    pub fn canonical_cmp(&self, o: &Num) -> Ordering {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => a.cmp(b),
            (Num::Float(a), Num::Float(b)) => a.total_cmp(b),
            (Num::Float(a), _) => a.total_cmp(&o.to_f64()),
            (_, Num::Float(b)) => self.to_f64().total_cmp(b),
            _ => match (self.to_rbig(), o.to_rbig()) {
                (Some(a), Some(b)) => a.cmp(&b),
                _ => Ordering::Equal,
            },
        }
    }
}

/// Exact results larger than this many bits are left unevaluated.
pub const MAX_RESULT_BITS: usize = 4_000_000;

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Num::Int(i) => write!(f, "{i}"),
            Num::Rat(r) => write!(f, "{}/{}", r.numerator(), r.denominator()),
            Num::Float(x) => write!(f, "{}", format_float(*x)),
        }
    }
}

pub fn format_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{x}");
    if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
}

/// Prime factorisation by trial division over small primes. The returned cofactor (if above one) is treated as a
/// single opaque factor: it may be composite, which only means some perfect powers are not extracted.
pub fn small_factors(mut n: u64) -> Vec<(u64, u32)> {
    let mut out = Vec::new();
    let mut p = 2u64;
    while p <= 100_000 && p.saturating_mul(p) <= n {
        if n.is_multiple_of(p) {
            let mut k = 0;
            while n.is_multiple_of(p) {
                n /= p;
                k += 1;
            }
            out.push((p, k));
        }
        p += if p == 2 { 1 } else { 2 };
    }
    if n > 1 {
        out.push((n, 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationals_normalise() {
        let half = Num::ratio(IBig::from(2), IBig::from(4)).unwrap();
        assert_eq!(half.to_string(), "1/2");
        assert_eq!(half.add(&half), Num::one());
        assert_eq!(Num::ratio(IBig::from(3), IBig::from(-6)).unwrap().to_string(), "-1/2");
        assert!(Num::ratio(IBig::ONE, IBig::ZERO).is_none());
    }

    #[test]
    fn powers() {
        assert_eq!(Num::int(2).powi(10).unwrap(), Num::int(1024));
        assert_eq!(Num::int(2).powi(-2).unwrap().to_string(), "1/4");
        assert!(Num::zero().powi(-1).is_none());
        assert!(Num::int(10).powi(10_000_000).is_none());
    }

    #[test]
    fn factors() {
        assert_eq!(small_factors(360), vec![(2, 3), (3, 2), (5, 1)]);
    }
}
