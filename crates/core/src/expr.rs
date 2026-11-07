//! Immutable symbolic expressions with canonical automatic simplification.
//!
//! Every constructor returns an expression in canonical form: sums and products are flattened and sorted, numbers are
//! folded, like terms and like powers are combined, and trivial identities (`x + 0`, `x * 1`, `x ** 1`, ...) vanish.
//! Canonical form makes structural equality meaningful and results deterministic across runs.
//!
//! Nodes are shared (`Arc`), so cloning is cheap. Exponentials are `E ** x`; square roots are `x ** (1/2)`.

use crate::functions;
use crate::number::{Num, small_factors};
use dashu::integer::IBig;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const SYM_E: &str = "E";
pub const SYM_PI: &str = "pi";
/// The imaginary unit. `I ** 2` is `-1`; complex numbers are ordinary sums such as `1 + 2*I`.
pub const SYM_I: &str = "I";
/// Positive infinity (`-oo` is `-1 * oo`). A plain constant symbol: arithmetic on it is not simplified.
pub const SYM_OO: &str = "oo";

#[derive(Clone)]
pub struct Expr(Arc<Node>);

struct Node {
    kind: Kind,
    depth: u32,
}

#[derive(Debug)]
pub enum Kind {
    Num(Num),
    Sym(Arc<str>),
    /// Canonical sum: sorted, at most one leading number, no nested sums.
    Add(Vec<Expr>),
    /// Canonical product: an optional leading numeric coefficient (never 1), then sorted non-numeric factors.
    Mul(Vec<Expr>),
    Pow(Expr, Expr),
    Call(Arc<str>, Vec<Expr>),
    List(Vec<Expr>),
    /// An equation `lhs == rhs`.
    Eq(Expr, Expr),
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Expr {}
impl PartialOrd for Expr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn rank(k: &Kind) -> u8 {
    match k {
        Kind::Num(_) => 0,
        Kind::Sym(_) => 1,
        Kind::Pow(..) => 2,
        Kind::Mul(_) => 3,
        Kind::Add(_) => 4,
        Kind::Call(..) => 5,
        Kind::List(_) => 6,
        Kind::Eq(..) => 7,
    }
}

fn cmp_slices(a: &[Expr], b: &[Expr]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let o = x.cmp(y);
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Expr {
    fn cmp(&self, other: &Self) -> Ordering {
        if Arc::ptr_eq(&self.0, &other.0) {
            return Ordering::Equal;
        }
        let (a, b) = (&self.0.kind, &other.0.kind);
        match rank(a).cmp(&rank(b)) {
            Ordering::Equal => {}
            o => return o,
        }
        match (a, b) {
            (Kind::Num(x), Kind::Num(y)) => x.canonical_cmp(y),
            (Kind::Sym(x), Kind::Sym(y)) => x.cmp(y),
            (Kind::Pow(b1, e1), Kind::Pow(b2, e2)) | (Kind::Eq(b1, e1), Kind::Eq(b2, e2)) => b1.cmp(b2).then_with(|| e1.cmp(e2)),
            (Kind::Add(x), Kind::Add(y)) | (Kind::Mul(x), Kind::Mul(y)) | (Kind::List(x), Kind::List(y)) => cmp_slices(x, y),
            (Kind::Call(n1, x), Kind::Call(n2, y)) => n1.cmp(n2).then_with(|| cmp_slices(x, y)),
            _ => Ordering::Equal,
        }
    }
}

/// Integers in this range are cached by [`Expr::num`].
const SMALL_MIN: i64 = -64;
const SMALL_MAX: i64 = 4096;
static SMALL: std::sync::OnceLock<Vec<Expr>> = std::sync::OnceLock::new();

impl Expr {
    fn raw(kind: Kind) -> Expr {
        let child_depth = match &kind {
            Kind::Num(_) | Kind::Sym(_) => 0,
            Kind::Pow(a, b) | Kind::Eq(a, b) => a.depth().max(b.depth()),
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v.iter().map(Expr::depth).max().unwrap_or(0),
        };
        Expr(Arc::new(Node { kind, depth: child_depth.saturating_add(1) }))
    }

    pub fn kind(&self) -> &Kind {
        &self.0.kind
    }

    /// Nesting depth (a symbol or number has depth 1). Used to refuse absurdly deep expressions.
    pub fn depth(&self) -> u32 {
        self.0.depth
    }

    // ---- atoms -------------------------------------------------------------------------------------------------

    /// A number. Small integers are shared (a table built on first use), so the loop counters and literals of a
    /// program do not allocate a node each time.
    pub fn num(n: Num) -> Expr {
        if let Num::Int(i) = &n
            && let Ok(v) = i64::try_from(i)
            && (SMALL_MIN..=SMALL_MAX).contains(&v)
        {
            let table = SMALL.get_or_init(|| (SMALL_MIN..=SMALL_MAX).map(|k| Expr::raw(Kind::Num(Num::int(k)))).collect());
            if let Some(e) = usize::try_from(v - SMALL_MIN).ok().and_then(|at| table.get(at)) {
                return e.clone();
            }
        }
        Expr::raw(Kind::Num(n))
    }
    pub fn int(v: i64) -> Expr {
        Expr::num(Num::int(v))
    }
    pub fn big(v: IBig) -> Expr {
        Expr::num(Num::Int(v))
    }
    pub fn float(v: f64) -> Expr {
        Expr::num(Num::Float(v))
    }
    /// Exact fraction; `None` when `d == 0`.
    pub fn rational(n: i64, d: i64) -> Option<Expr> {
        Num::ratio(IBig::from(n), IBig::from(d)).map(Expr::num)
    }
    pub fn sym(name: &str) -> Expr {
        Expr::raw(Kind::Sym(Arc::from(name)))
    }
    pub fn zero() -> Expr {
        Expr::int(0)
    }
    pub fn one() -> Expr {
        Expr::int(1)
    }
    pub fn e() -> Expr {
        Expr::sym(SYM_E)
    }
    pub fn pi() -> Expr {
        Expr::sym(SYM_PI)
    }
    pub fn infinity() -> Expr {
        Expr::sym(SYM_OO)
    }

    pub fn imag() -> Expr {
        Expr::sym(SYM_I)
    }
    pub fn half() -> Expr {
        Expr::rational(1, 2).unwrap_or_else(Expr::zero)
    }
    pub fn list(items: Vec<Expr>) -> Expr {
        Expr::raw(Kind::List(items))
    }
    pub fn eq(lhs: Expr, rhs: Expr) -> Expr {
        Expr::raw(Kind::Eq(lhs, rhs))
    }

    // ---- accessors ---------------------------------------------------------------------------------------------

    pub fn as_num(&self) -> Option<&Num> {
        if let Kind::Num(n) = self.kind() { Some(n) } else { None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        self.as_num().and_then(Num::as_i64)
    }
    pub fn as_sym(&self) -> Option<&str> {
        if let Kind::Sym(s) = self.kind() { Some(s) } else { None }
    }
    pub fn is_zero(&self) -> bool {
        self.as_num().is_some_and(Num::is_zero)
    }
    pub fn is_one(&self) -> bool {
        self.as_num().is_some_and(Num::is_one)
    }
    pub fn is_num(&self) -> bool {
        matches!(self.kind(), Kind::Num(_))
    }
    pub fn is_constant_symbol(&self) -> bool {
        matches!(self.as_sym(), Some(SYM_E | SYM_PI | SYM_I | SYM_OO))
    }

    /// `(a, b)` when this is the exact or float complex number `a + b*I` with numeric `a` and `b`.
    pub fn as_complex(&self) -> Option<(Num, Num)> {
        fn imag_part(t: &Expr) -> Option<Num> {
            match t.kind() {
                Kind::Sym(s) if &**s == SYM_I => Some(Num::one()),
                Kind::Mul(fs) => match fs.as_slice() {
                    [c, i] if i.as_sym() == Some(SYM_I) => c.as_num().cloned(),
                    _ => None,
                },
                _ => None,
            }
        }
        match self.kind() {
            Kind::Num(n) => Some((n.clone(), Num::zero())),
            Kind::Add(terms) => {
                let (mut re, mut im) = (Num::zero(), Num::zero());
                for t in terms {
                    match t.kind() {
                        Kind::Num(n) => re = re.add(n),
                        _ => im = im.add(&imag_part(t)?),
                    }
                }
                Some((re, im))
            }
            _ => imag_part(self).map(|b| (Num::zero(), b)),
        }
    }

    pub fn args(&self) -> &[Expr] {
        match self.kind() {
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v,
            _ => &[],
        }
    }

    /// Split into `(numeric coefficient, rest)`: `3*x*y` is `(3, x*y)`, `x` is `(1, x)`, `5` is `(5, 1)`.
    pub fn split_coeff(&self) -> (Num, Expr) {
        match self.kind() {
            Kind::Num(n) => (n.clone(), Expr::one()),
            Kind::Mul(fs) => match fs.split_first() {
                Some((first, rest)) => match first.kind() {
                    Kind::Num(c) => {
                        let r = match rest {
                            [single] => single.clone(),
                            _ => Expr::raw(Kind::Mul(rest.to_vec())),
                        };
                        (c.clone(), r)
                    }
                    _ => (Num::one(), self.clone()),
                },
                None => (Num::one(), self.clone()),
            },
            _ => (Num::one(), self.clone()),
        }
    }

    fn scaled(c: Num, rest: Expr) -> Expr {
        if c.is_one() {
            return rest;
        }
        match rest.kind() {
            Kind::Mul(fs) => {
                let mut v = Vec::with_capacity(fs.len() + 1);
                v.push(Expr::num(c));
                v.extend(fs.iter().cloned());
                Expr::raw(Kind::Mul(v))
            }
            _ => Expr::raw(Kind::Mul(vec![Expr::num(c), rest])),
        }
    }

    // ---- canonical arithmetic ----------------------------------------------------------------------------------

    pub fn add(terms: Vec<Expr>) -> Expr {
        let mut konst = Num::zero();
        let mut map: BTreeMap<Expr, Num> = BTreeMap::new();
        let mut put = |t: &Expr| {
            if let Kind::Num(n) = t.kind() {
                konst = konst.add(n);
            } else {
                let (c, rest) = t.split_coeff();
                match map.get_mut(&rest) {
                    Some(slot) => *slot = slot.add(&c),
                    None => {
                        map.insert(rest, c);
                    }
                }
            }
        };
        for t in &terms {
            match t.kind() {
                Kind::Add(inner) => inner.iter().for_each(&mut put),
                _ => put(t),
            }
        }
        let mut out: Vec<Expr> = Vec::with_capacity(map.len() + 1);
        if !konst.is_zero() {
            out.push(Expr::num(konst));
        }
        for (rest, c) in map {
            if !c.is_zero() {
                out.push(Expr::scaled(c, rest));
            }
        }
        match out.len() {
            0 => Expr::zero(),
            1 => out.swap_remove(0),
            _ => Expr::raw(Kind::Add(out)),
        }
    }

    pub fn mul(factors: Vec<Expr>) -> Expr {
        let mut coeff = Num::one();
        let mut groups: BTreeMap<Expr, Vec<Expr>> = BTreeMap::new();
        let mut put = |f: &Expr| match f.kind() {
            Kind::Num(n) => coeff = coeff.mul(n),
            Kind::Pow(b, e) => groups.entry(b.clone()).or_default().push(e.clone()),
            _ => groups.entry(f.clone()).or_default().push(Expr::one()),
        };
        for f in &factors {
            match f.kind() {
                Kind::Mul(inner) => inner.iter().for_each(&mut put),
                _ => put(f),
            }
        }
        if coeff.is_zero() {
            // `0 * (1/0)` is undefined, not zero: keep the division by zero visible so callers can detect it.
            let singular = groups.iter().find_map(|(b, es)| {
                let neg = es.iter().find(|e| e.as_num().is_some_and(Num::is_negative))?;
                b.is_zero().then(|| Expr::raw(Kind::Pow(b.clone(), neg.clone())))
            });
            return singular.unwrap_or_else(Expr::zero);
        }
        let mut again = false;
        let mut out: Vec<Expr> = Vec::with_capacity(groups.len() + 1);
        for (base, mut exps) in groups {
            let e = if exps.len() == 1 { exps.swap_remove(0) } else { Expr::add(exps) };
            let p = if e.is_one() { base } else { Expr::pow(&base, &e) };
            match p.kind() {
                Kind::Num(n) => coeff = coeff.mul(n),
                Kind::Mul(_) => {
                    again = true;
                    out.push(p);
                }
                _ => out.push(p),
            }
        }
        if again {
            out.push(Expr::num(coeff));
            return Expr::mul(out);
        }
        if coeff.is_zero() {
            return Expr::zero();
        }
        out.sort();
        match (out.len(), coeff.is_one()) {
            (0, _) => Expr::num(coeff),
            (1, true) => out.swap_remove(0),
            (_, true) => Expr::raw(Kind::Mul(out)),
            _ => {
                let mut v = Vec::with_capacity(out.len() + 1);
                v.push(Expr::num(coeff));
                v.extend(out);
                Expr::raw(Kind::Mul(v))
            }
        }
    }

    pub fn neg(&self) -> Expr {
        Expr::mul(vec![Expr::int(-1), self.clone()])
    }
    pub fn sub(&self, o: &Expr) -> Expr {
        Expr::add(vec![self.clone(), o.neg()])
    }
    /// `self / o`. Division by exact zero stays unevaluated (`o ** -1`); callers that need an error check first.
    pub fn div(&self, o: &Expr) -> Expr {
        Expr::mul(vec![self.clone(), Expr::pow(o, &Expr::int(-1))])
    }

    pub fn pow(base: &Expr, exp: &Expr) -> Expr {
        if let Some(en) = exp.as_num() {
            if en.is_zero() {
                return Expr::one();
            }
            if en.is_one() {
                return base.clone();
            }
            if let Some(bn) = base.as_num() {
                if let Some(r) = num_pow(bn, en) {
                    return r;
                }
                // sqrt of a negative float is imaginary: sqrt(-4.0) = 2.0*I
                if let Num::Float(f) = bn
                    && *f < 0.0
                    && exp == &Expr::half()
                {
                    return Expr::mul(vec![Expr::float((-f).sqrt()), Expr::imag()]);
                }
            } else if base.as_sym() == Some(SYM_I)
                && en.is_exact()
                && let Some(n) = en.as_i64()
            {
                return match n.rem_euclid(4) {
                    0 => Expr::one(),
                    1 => base.clone(),
                    2 => Expr::int(-1),
                    _ => Expr::mul(vec![Expr::int(-1), base.clone()]),
                };
            } else if en.is_exact()
                && en.is_negative()
                && en.is_integer()
                && let Some((a, b)) = base.as_complex()
                && !b.is_zero()
                && let Some(inv_norm) = a.mul(&a).add(&b.mul(&b)).recip()
            {
                // 1 / (a + b*I) = (a - b*I) / (a**2 + b**2); the exponent is a negative integer.
                let inv = Expr::add(vec![Expr::num(a.mul(&inv_norm)), Expr::mul(vec![Expr::num(b.neg().mul(&inv_norm)), Expr::imag()])]);
                return Expr::pow(&inv, &Expr::num(en.neg()));
            } else {
                match base.kind() {
                    Kind::Pow(b2, e2) if en.is_integer() && en.is_exact() => return Expr::pow(b2, &Expr::mul(vec![e2.clone(), exp.clone()])),
                    Kind::Mul(fs) if en.is_integer() && en.is_exact() => return Expr::mul(fs.iter().map(|f| Expr::pow(f, exp)).collect()),
                    _ => {}
                }
            }
        }
        if base.is_one() {
            return Expr::one();
        }
        // E ** log(x) == x
        if base.as_sym() == Some(SYM_E)
            && let Kind::Call(name, args) = exp.kind()
            && &**name == "log"
            && let [x] = args.as_slice()
        {
            return x.clone();
        }
        Expr::raw(Kind::Pow(base.clone(), exp.clone()))
    }

    pub fn call(name: &str, args: Vec<Expr>) -> Expr {
        functions::apply(name, args)
    }

    pub(crate) fn raw_call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::raw(Kind::Call(Arc::from(name), args))
    }

    // ---- structure ---------------------------------------------------------------------------------------------

    /// Rebuild an expression of the same kind from new children, re-simplifying.
    pub fn rebuild(&self, c: Vec<Expr>) -> Expr {
        match self.kind() {
            Kind::Add(_) => Expr::add(c),
            Kind::Mul(_) => Expr::mul(c),
            Kind::Pow(..) => match <[Expr; 2]>::try_from(c) {
                Ok([b, e]) => Expr::pow(&b, &e),
                Err(_) => self.clone(),
            },
            Kind::Eq(..) => match <[Expr; 2]>::try_from(c) {
                Ok([l, r]) => Expr::eq(l, r),
                Err(_) => self.clone(),
            },
            Kind::Call(name, _) => Expr::call(name, c),
            Kind::List(_) => Expr::list(c),
            Kind::Num(_) | Kind::Sym(_) => self.clone(),
        }
    }

    pub fn children(&self) -> Vec<Expr> {
        match self.kind() {
            Kind::Pow(a, b) | Kind::Eq(a, b) => vec![a.clone(), b.clone()],
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v.clone(),
            Kind::Num(_) | Kind::Sym(_) => Vec::new(),
        }
    }

    /// Apply `f` to each child and rebuild.
    pub fn map_children(&self, mut f: impl FnMut(&Expr) -> crate::MathResult<Expr>) -> crate::MathResult<Expr> {
        let kids = self.children();
        if kids.is_empty() {
            return Ok(self.clone());
        }
        let mapped = kids.iter().map(&mut f).collect::<Result<Vec<_>, _>>()?;
        Ok(self.rebuild(mapped))
    }

    /// Does `needle` occur anywhere inside `self` (including `self`)?
    pub fn contains(&self, needle: &Expr) -> bool {
        if self == needle {
            return true;
        }
        match self.kind() {
            Kind::Pow(a, b) | Kind::Eq(a, b) => a.contains(needle) || b.contains(needle),
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v.iter().any(|x| x.contains(needle)),
            Kind::Num(_) | Kind::Sym(_) => false,
        }
    }

    /// Free symbols, excluding the constants `E` and `pi`.
    pub fn free_symbols(&self) -> BTreeSet<Arc<str>> {
        let mut set = BTreeSet::new();
        self.collect_symbols(&mut set);
        set
    }

    fn collect_symbols(&self, set: &mut BTreeSet<Arc<str>>) {
        match self.kind() {
            Kind::Sym(s) => {
                if !matches!(&**s, SYM_E | SYM_PI | SYM_I | SYM_OO) {
                    set.insert(s.clone());
                }
            }
            Kind::Num(_) => {}
            Kind::Pow(a, b) | Kind::Eq(a, b) => {
                a.collect_symbols(set);
                b.collect_symbols(set);
            }
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v.iter().for_each(|x| x.collect_symbols(set)),
        }
    }

    /// Number of nodes; a cheap measure of expression size.
    pub fn size(&self) -> usize {
        1 + match self.kind() {
            Kind::Pow(a, b) | Kind::Eq(a, b) => a.size() + b.size(),
            Kind::Add(v) | Kind::Mul(v) | Kind::List(v) | Kind::Call(_, v) => v.iter().map(Expr::size).sum(),
            Kind::Num(_) | Kind::Sym(_) => 0,
        }
    }
}

/// Numeric power with exact simplification. `None` leaves `base ** exp` unevaluated.
fn num_pow(base: &Num, exp: &Num) -> Option<Expr> {
    if base.is_one() {
        return Some(Expr::one());
    }
    if !base.is_exact() || !exp.is_exact() {
        let v = base.to_f64().powf(exp.to_f64());
        return v.is_finite().then(|| Expr::float(v));
    }
    if let Some(n) = exp.as_i64() {
        return base.powi(n).map(Expr::num);
    }
    // Rational exponent p/q with q > 1 on an exact base.
    let (p, q) = exp.parts()?;
    let p = i64::try_from(&p).ok()?;
    let q = i64::try_from(&dashu::integer::IBig::from(q)).ok()?;
    if p.unsigned_abs() > 64 || q > 64 {
        return None;
    }
    if base.is_zero() {
        return if p > 0 { Some(Expr::zero()) } else { None };
    }
    if base.is_negative() {
        // (-a) ** (p/2) = a ** (p/2) * I ** p; other roots of negative numbers stay unevaluated.
        if q != 2 {
            return None;
        }
        let pos = num_pow(&base.neg(), exp)?;
        return Some(Expr::mul(vec![pos, Expr::pow(&Expr::imag(), &Expr::int(p))]));
    }
    let (bn, bd) = base.parts()?;
    let bn = u64::try_from(&bn).ok()?;
    let bd = u64::try_from(&dashu::integer::IBig::from(bd)).ok()?;
    let a = root_part(bn, p, q)?;
    let b = root_part(bd, -p, q)?;
    let out = a.0.mul(&b.0);
    let rad = a.1.checked_mul(b.1)?;
    let rad_e = if rad == 1 { None } else { Some(Expr::raw(Kind::Pow(Expr::big(IBig::from(rad)), Expr::rational(1, q)?))) };
    Some(match rad_e {
        None => Expr::num(out),
        Some(r) => Expr::scaled(out, r),
    })
}

/// `n ** (p / q)` as `(rational factor outside, integer radicand r)` meaning `factor * r ** (1/q)`.
fn root_part(n: u64, p: i64, q: i64) -> Option<(Num, u64)> {
    let mut outside = Num::one();
    let mut radicand: u64 = 1;
    for (prime, k) in small_factors(n) {
        let total = i64::from(k).checked_mul(p)?;
        let whole = total.div_euclid(q);
        let frac = total.rem_euclid(q);
        let pw = Num::Int(IBig::from(prime)).powi(whole)?;
        outside = outside.mul(&pw);
        radicand = radicand.checked_mul(prime.checked_pow(u32::try_from(frac).ok()?)?)?;
    }
    Some((outside, radicand))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }
    fn i(v: i64) -> Expr {
        Expr::int(v)
    }

    #[test]
    fn like_terms_combine() {
        assert_eq!(Expr::add(vec![x(), x()]).to_string(), "2*x");
        assert_eq!(Expr::add(vec![x(), x().neg()]), Expr::zero());
        assert_eq!(Expr::add(vec![i(1), x(), i(2)]).to_string(), "x + 3");
    }

    #[test]
    fn like_powers_combine() {
        assert_eq!(Expr::mul(vec![x(), x()]).to_string(), "x**2");
        assert_eq!(Expr::mul(vec![x(), x().div(&y())]).to_string(), "x**2/y");
        assert_eq!(Expr::mul(vec![x(), Expr::pow(&x(), &i(-1))]), Expr::one());
    }

    #[test]
    fn canonical_order_is_deterministic() {
        assert_eq!(Expr::add(vec![x(), y()]), Expr::add(vec![y(), x()]));
        assert_eq!(Expr::mul(vec![x(), y()]), Expr::mul(vec![y(), x()]));
    }

    #[test]
    fn exact_roots() {
        let two = i(2);
        assert_eq!(Expr::pow(&i(4), &Expr::half()), two);
        assert_eq!(Expr::pow(&i(8), &Expr::half()).to_string(), "2*sqrt(2)");
        assert_eq!(Expr::mul(vec![Expr::pow(&two, &Expr::half()), Expr::pow(&two, &Expr::half())]), two);
        assert_eq!(Expr::pow(&Expr::rational(1, 4).unwrap(), &Expr::half()).to_string(), "1/2");
    }

    #[test]
    fn integer_powers_distribute() {
        assert_eq!(Expr::pow(&Expr::mul(vec![i(2), x()]), &i(3)).to_string(), "8*x**3");
        assert_eq!(Expr::pow(&Expr::pow(&x(), &i(2)), &i(3)).to_string(), "x**6");
        // not valid for non-integer exponents: sqrt(x**2) is |x|
        assert_eq!(Expr::pow(&Expr::pow(&x(), &i(2)), &Expr::half()).to_string(), "sqrt(x**2)");
    }

    #[test]
    fn floats_are_contagious() {
        assert_eq!(Expr::add(vec![Expr::float(0.5), i(1)]).to_string(), "1.5");
    }
}
