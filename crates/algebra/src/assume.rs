//! Assumptions about symbols (`positive`, `integer`, ...) and the simplifications they allow.
//!
//! Every symbol is real unless said otherwise. [`Assumptions`] maps symbol names to [`Facts`]; [`sign_of`] and
//! [`is_integer`] infer what follows for a whole expression, and [`refine`] rewrites with those facts
//! (`sqrt(x**2) -> x` for positive `x`, `log(x**k) -> k*log(x)`, `sin(n*pi) -> 0` for integer `n`, ...).

use crate::util::guard;
use mathcraft_core::{Expr, Kind, MathResult, Num};
use std::collections::BTreeMap;
use std::sync::Arc;

/// What is known about the sign of a quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Pos,
    Neg,
    NonNeg,
    NonPos,
    Zero,
}

impl Sign {
    fn nonneg(self) -> bool {
        matches!(self, Sign::Pos | Sign::NonNeg | Sign::Zero)
    }

    fn nonpos(self) -> bool {
        matches!(self, Sign::Neg | Sign::NonPos | Sign::Zero)
    }

    fn nonzero(self) -> bool {
        matches!(self, Sign::Pos | Sign::Neg)
    }
}

/// The facts declared for one symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub sign: Option<Sign>,
    pub nonzero: bool,
    pub integer: bool,
}

impl Facts {
    /// Parse one assumption name; `Err` carries the unknown name.
    fn set(&mut self, name: &str) -> Result<(), String> {
        match name {
            "positive" => self.sign = Some(Sign::Pos),
            "negative" => self.sign = Some(Sign::Neg),
            "nonnegative" => self.sign = Some(Sign::NonNeg),
            "nonpositive" => self.sign = Some(Sign::NonPos),
            "nonzero" => self.nonzero = true,
            "integer" => self.integer = true,
            "real" => {}
            other => return Err(other.to_string()),
        }
        Ok(())
    }

    /// Facts from a comma-separated list such as `positive,integer`.
    pub fn parse(text: &str) -> Result<Facts, String> {
        let mut f = Facts::default();
        for part in text.split(',').filter(|p| !p.is_empty()) {
            f.set(part)?;
        }
        Ok(f)
    }

    /// Facts from names (the `positive=True` keywords).
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Facts, String> {
        let mut f = Facts::default();
        for n in names {
            f.set(n)?;
        }
        Ok(f)
    }

    /// The comma-separated form that [`Facts::parse`] reads back.
    pub fn encode(&self) -> String {
        let mut parts = Vec::new();
        match self.sign {
            Some(Sign::Pos) => parts.push("positive"),
            Some(Sign::Neg) => parts.push("negative"),
            Some(Sign::NonNeg) => parts.push("nonnegative"),
            Some(Sign::NonPos) => parts.push("nonpositive"),
            _ => {}
        }
        if self.nonzero {
            parts.push("nonzero");
        }
        if self.integer {
            parts.push("integer");
        }
        parts.join(",")
    }

    pub fn is_empty(&self) -> bool {
        *self == Facts::default()
    }
}

/// Facts about the symbols of an expression.
#[derive(Clone, Debug, Default)]
pub struct Assumptions {
    map: BTreeMap<Arc<str>, Facts>,
}

impl Assumptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: &str, facts: Facts) {
        if !facts.is_empty() {
            self.map.insert(Arc::from(name), facts);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Facts> {
        self.map.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn num_sign(n: &Num) -> Sign {
    if n.is_zero() {
        Sign::Zero
    } else if n.is_negative() {
        Sign::Neg
    } else {
        Sign::Pos
    }
}

/// What can be said about the sign of `e`; `None` when nothing.
pub fn sign_of(e: &Expr, asm: &Assumptions) -> Option<Sign> {
    match e.kind() {
        Kind::Num(n) => Some(num_sign(n)),
        Kind::Sym(s) => match &**s {
            "pi" | "E" | "oo" => Some(Sign::Pos),
            name => {
                let f = asm.get(name)?;
                match (f.sign, f.nonzero) {
                    (Some(Sign::NonNeg), true) => Some(Sign::Pos),
                    (Some(Sign::NonPos), true) => Some(Sign::Neg),
                    (s, _) => s,
                }
            }
        },
        Kind::Add(ts) => {
            let signs: Vec<Sign> = ts.iter().map(|t| sign_of(t, asm)).collect::<Option<_>>()?;
            if signs.iter().all(|s| s.nonneg()) {
                Some(if signs.contains(&Sign::Pos) { Sign::Pos } else { Sign::NonNeg })
            } else if signs.iter().all(|s| s.nonpos()) {
                Some(if signs.contains(&Sign::Neg) { Sign::Neg } else { Sign::NonPos })
            } else {
                None
            }
        }
        Kind::Mul(fs) => {
            let mut negative = false;
            let mut all_nonzero = true;
            for f in fs {
                match sign_of(f, asm)? {
                    Sign::Zero => return Some(Sign::Zero),
                    Sign::Pos => {}
                    Sign::Neg => negative = !negative,
                    Sign::NonNeg => all_nonzero = false,
                    Sign::NonPos => {
                        all_nonzero = false;
                        negative = !negative;
                    }
                }
            }
            Some(match (negative, all_nonzero) {
                (false, true) => Sign::Pos,
                (false, false) => Sign::NonNeg,
                (true, true) => Sign::Neg,
                (true, false) => Sign::NonPos,
            })
        }
        Kind::Pow(b, k) => {
            let bs = sign_of(b, asm);
            let even = k.as_i64().is_some_and(|n| n % 2 == 0);
            let odd = k.as_i64().is_some_and(|n| n % 2 != 0);
            match bs {
                Some(Sign::Pos) => Some(Sign::Pos),
                Some(s) if s.nonneg() && k.as_num().is_some_and(|n| !n.is_negative() && !n.is_zero()) => {
                    Some(if s.nonzero() { Sign::Pos } else { Sign::NonNeg })
                }
                Some(s) if s.nonpos() && even => Some(if s.nonzero() { Sign::Pos } else { Sign::NonNeg }),
                Some(s) if s.nonpos() && odd => Some(if s.nonzero() { Sign::Neg } else { Sign::NonPos }),
                None if even && k.as_i64().is_some_and(|n| n > 0) => Some(Sign::NonNeg),
                _ => None,
            }
        }
        Kind::Call(name, args) => match (&**name, args.as_slice()) {
            ("abs", _) => Some(Sign::NonNeg),
            ("exp" | "cosh", _) => Some(Sign::Pos),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `e` is known to be an integer.
pub fn is_integer(e: &Expr, asm: &Assumptions) -> bool {
    match e.kind() {
        Kind::Num(n) => n.is_exact() && n.is_integer(),
        Kind::Sym(s) => asm.get(s).is_some_and(|f| f.integer),
        Kind::Add(ts) | Kind::Mul(ts) => ts.iter().all(|t| is_integer(t, asm)),
        Kind::Pow(b, k) => is_integer(b, asm) && k.as_i64().is_some_and(|n| n >= 0),
        _ => false,
    }
}

/// Whether `e` is known to be nonzero.
pub fn is_nonzero(e: &Expr, asm: &Assumptions) -> bool {
    sign_of(e, asm).is_some_and(Sign::nonzero) || matches!(e.kind(), Kind::Sym(s) if asm.get(s).is_some_and(|f| f.nonzero))
}

/// Rewrite `e` using what the assumptions allow.
pub fn refine(e: &Expr, asm: &Assumptions) -> MathResult<Expr> {
    refine_with(e, asm, 4)
}

/// `fuel` bounds how often a rewrite may feed another one.
fn refine_with(e: &Expr, asm: &Assumptions, fuel: u32) -> MathResult<Expr> {
    guard(e)?;
    let inner = e.map_children(|c| refine_with(c, asm, fuel))?;
    match refine_node(&inner, asm) {
        Some(next) if fuel > 0 && next != inner => refine_with(&next, asm, fuel - 1),
        Some(next) => Ok(next),
        None => Ok(inner),
    }
}

fn refine_node(e: &Expr, asm: &Assumptions) -> Option<Expr> {
    match e.kind() {
        Kind::Call(name, args) => match (&**name, args.as_slice()) {
            ("abs", [u]) => match sign_of(u, asm)? {
                s if s.nonneg() => Some(u.clone()),
                s if s.nonpos() => Some(u.neg()),
                _ => None,
            },
            ("log", [u]) => refine_log(u, asm),
            ("sin" | "tan", [u]) if integer_multiple_of_pi(u, asm) => Some(Expr::zero()),
            ("cos", [u]) if integer_multiple_of_pi(u, asm) => Some(Expr::pow(&Expr::int(-1), &u.div(&Expr::pi()))),
            _ => None,
        },
        Kind::Pow(b, k) => {
            // (-1)**(2*n) with integer n
            if b.as_i64() == Some(-1) {
                let (c, rest) = k.split_coeff();
                if c.is_integer() && c.as_i64().is_some_and(|c| c % 2 == 0) && is_integer(&rest, asm) {
                    return Some(Expr::one());
                }
            }
            // positive factors come out of a power: (p*u)**k = p**k * u**k
            if let Kind::Mul(fs) = b.kind() {
                let (pos, rest): (Vec<&Expr>, Vec<&Expr>) = fs.iter().partition(|f| sign_of(f, asm) == Some(Sign::Pos));
                if !pos.is_empty() && !rest.is_empty() {
                    let mut out: Vec<Expr> = pos.iter().map(|p| Expr::pow(p, k)).collect();
                    out.push(Expr::pow(&Expr::mul(rest.into_iter().cloned().collect()), k));
                    return Some(Expr::mul(out));
                }
            }
            // abs(u)**even is u**even for real u
            if let Kind::Call(n, a) = b.kind()
                && &**n == "abs"
                && let [u] = a.as_slice()
                && k.as_i64().is_some_and(|n| n % 2 == 0)
            {
                return Some(Expr::pow(u, k));
            }
            // (c**m)**k is c**(m*k) for positive c, and for nonnegative c when both exponents are positive numbers
            if let Kind::Pow(c, m) = b.kind() {
                let positive_exps = [m, k].iter().all(|x| x.as_num().is_some_and(|n| !n.is_negative() && !n.is_zero()));
                match sign_of(c, asm) {
                    Some(Sign::Pos) => return Some(Expr::pow(c, &Expr::mul(vec![m.clone(), k.clone()]))),
                    Some(Sign::NonNeg) if positive_exps => return Some(Expr::pow(c, &Expr::mul(vec![m.clone(), k.clone()]))),
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn integer_multiple_of_pi(u: &Expr, asm: &Assumptions) -> bool {
    let (c, rest) = u.split_coeff();
    if rest == Expr::pi() {
        return c.is_exact() && c.is_integer();
    }
    // n*pi with a symbolic integer n
    match rest.kind() {
        Kind::Mul(fs) if fs.iter().any(|f| *f == Expr::pi()) => {
            let others: Vec<Expr> = fs.iter().filter(|f| **f != Expr::pi()).cloned().collect();
            c.is_exact() && c.is_integer() && others.iter().all(|f| is_integer(f, asm)) && !others.is_empty()
        }
        _ => false,
    }
}

/// `log(x**k) = k*log(x)` and `log(a*b) = log(a) + log(b)` for positive factors.
fn refine_log(u: &Expr, asm: &Assumptions) -> Option<Expr> {
    match u.kind() {
        Kind::Pow(b, k) if sign_of(b, asm) == Some(Sign::Pos) => Some(Expr::mul(vec![k.clone(), Expr::call("log", vec![b.clone()])])),
        Kind::Mul(fs) if fs.len() > 1 && fs.iter().all(|f| sign_of(f, asm) == Some(Sign::Pos)) => Some(Expr::add(
            fs.iter().map(|f| refine_node(&Expr::call("log", vec![f.clone()]), asm).unwrap_or_else(|| Expr::call("log", vec![f.clone()]))).collect(),
        )),
        _ => None,
    }
}

/// `simplify` that also uses the assumptions.
pub fn simplify_with(e: &Expr, asm: &Assumptions) -> MathResult<Expr> {
    if asm.is_empty() {
        return crate::simplify::simplify(e);
    }
    let pre = refine(e, asm)?;
    let out = crate::simplify::simplify(&pre)?;
    refine(&out, asm)
}

/// Drop solutions of `x` that the assumptions about `x` rule out.
pub fn admissible(root: &Expr, x: &Expr, asm: &Assumptions) -> bool {
    let Some(name) = x.as_sym() else { return true };
    let Some(f) = asm.get(name) else { return true };
    if let (Some(want), Some(have)) = (f.sign, sign_of(root, asm)) {
        let clash = match want {
            Sign::Pos => have.nonpos(),
            Sign::Neg => have.nonneg(),
            Sign::NonNeg => have == Sign::Neg,
            Sign::NonPos => have == Sign::Pos,
            Sign::Zero => have.nonzero(),
        };
        if clash {
            return false;
        }
    }
    if f.nonzero && root.is_zero() {
        return false;
    }
    if f.integer && root.as_num().is_some_and(|n| n.is_exact() && !n.is_integer()) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(name: &str, facts: &str) -> Assumptions {
        let mut a = Assumptions::new();
        a.insert(name, Facts::parse(facts).unwrap_or_default());
        a
    }

    fn sym(n: &str) -> Expr {
        Expr::sym(n)
    }

    #[test]
    fn square_root_of_a_square() {
        let x = sym("x");
        let e = Expr::pow(&Expr::pow(&x, &Expr::int(2)), &Expr::half());
        assert_eq!(simplify_with(&e, &asm("x", "positive")).map(|r| r.to_string()).ok(), Some("x".into()));
        assert_eq!(simplify_with(&e, &Assumptions::new()).map(|r| r.to_string()).ok(), Some("abs(x)".into()));
    }

    #[test]
    fn positive_factors_leave_a_root() {
        let (x, y) = (sym("x"), sym("y"));
        let e = Expr::pow(&Expr::mul(vec![Expr::pow(&x, &Expr::int(2)), Expr::pow(&y, &Expr::int(2))]), &Expr::half());
        assert_eq!(simplify_with(&e, &asm("x", "positive")).map(|r| r.to_string()).ok(), Some("x*abs(y)".into()));
    }

    #[test]
    fn abs_follows_the_sign() {
        let x = sym("x");
        let e = Expr::call("abs", vec![x.clone()]);
        assert_eq!(refine(&e, &asm("x", "negative")).map(|r| r.to_string()).ok(), Some("-x".into()));
        assert_eq!(refine(&e, &asm("x", "nonnegative")).map(|r| r.to_string()).ok(), Some("x".into()));
        assert_eq!(refine(&e, &Assumptions::new()).map(|r| r.to_string()).ok(), Some("abs(x)".into()));
    }

    #[test]
    fn logs_of_positive_factors() {
        let (x, y) = (sym("x"), sym("y"));
        let mut a = asm("x", "positive");
        a.insert("y", Facts::parse("positive").unwrap_or_default());
        let e = Expr::call("log", vec![Expr::mul(vec![Expr::pow(&x, &Expr::int(3)), y])]);
        assert_eq!(refine(&e, &a).map(|r| r.to_string()).ok(), Some("3*log(x) + log(y)".into()));
    }

    #[test]
    fn trig_at_integer_multiples_of_pi() {
        let n = sym("n");
        let a = asm("n", "integer");
        let arg = Expr::mul(vec![n.clone(), Expr::pi()]);
        assert_eq!(refine(&Expr::call("sin", vec![arg.clone()]), &a).map(|r| r.to_string()).ok(), Some("0".into()));
        assert_eq!(refine(&Expr::call("cos", vec![arg]), &a).map(|r| r.to_string()).ok(), Some("(-1)**n".into()));
        assert_eq!(refine(&Expr::pow(&Expr::int(-1), &Expr::mul(vec![Expr::int(2), n])), &a).map(|r| r.to_string()).ok(), Some("1".into()));
    }

    #[test]
    fn roots_outside_the_assumption_are_dropped() {
        let x = sym("x");
        let a = asm("x", "positive");
        assert!(admissible(&Expr::int(2), &x, &a));
        assert!(!admissible(&Expr::int(-2), &x, &a));
        assert!(!admissible(&Expr::zero(), &x, &a));
        assert!(!admissible(&Expr::rational(1, 2).unwrap_or_else(Expr::zero), &x, &asm("x", "integer")));
    }
}
