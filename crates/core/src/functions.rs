//! Elementary functions: automatic evaluation rules applied when a call is constructed.

use crate::expr::{Expr, Kind, SYM_E, SYM_PI};
use crate::number::Num;
use dashu::integer::IBig;

const UNARY: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "erf", "Gamma", "LambertW", "Si", "Ci", "Ei", "erfc",
    "log", "abs",
];

pub fn is_builtin_function(name: &str) -> bool {
    matches!(name, "sqrt" | "exp") || UNARY.contains(&name)
}

pub(crate) fn apply(name: &str, args: Vec<Expr>) -> Expr {
    match (name, args.as_slice()) {
        ("sqrt", [x]) => Expr::pow(x, &Expr::half()),
        ("erfc", [x]) => Expr::add(vec![Expr::one(), unary("erf", x).neg()]),
        ("exp", [x]) => Expr::pow(&Expr::e(), x),
        (_, [x]) if UNARY.contains(&name) => unary(name, x),
        _ => Expr::raw_call(name, args),
    }
}

#[derive(PartialEq)]
enum Parity {
    Odd,
    Even,
    Neither,
}

fn parity(name: &str) -> Parity {
    match name {
        "sin" | "tan" | "asin" | "atan" | "sinh" | "tanh" | "asinh" | "atanh" | "erf" | "Si" => Parity::Odd,
        "cos" | "cosh" | "abs" => Parity::Even,
        _ => Parity::Neither,
    }
}

/// Machine-precision value of a built-in unary function, `None` outside its real domain.
pub fn f64_unary(name: &str, v: f64) -> Option<f64> {
    let r = match name {
        "sin" => v.sin(),
        "cos" => v.cos(),
        "tan" => v.tan(),
        "asin" => v.asin(),
        "acos" => v.acos(),
        "atan" => v.atan(),
        "sinh" => v.sinh(),
        "cosh" => v.cosh(),
        "tanh" => v.tanh(),
        "asinh" => v.asinh(),
        "acosh" => v.acosh(),
        "atanh" => v.atanh(),
        "erf" => erf(v),
        "Gamma" => gamma_fn(v),
        "LambertW" => lambert_w(v),
        "Si" => sine_integral(v),
        "Ci" => cosine_integral(v),
        "Ei" => exponential_integral(v),
        "erfc" => 1.0 - erf(v),
        "log" => v.ln(),
        "abs" => v.abs(),
        _ => return None,
    };
    (!r.is_nan()).then_some(r)
}

/// The error function to machine precision: its Maclaurin series for small arguments, a continued fraction for the
/// complementary function beyond.
fn erf(x: f64) -> f64 {
    let a = x.abs();
    let r = if a < 2.5 {
        let (mut term, mut sum, mut n) = (a, a, 0.0_f64);
        while term.abs() > 1e-17 * sum.abs() && n < 200.0 {
            n += 1.0;
            term *= -a * a / n;
            sum += term / (2.0 * n + 1.0);
        }
        2.0 / std::f64::consts::PI.sqrt() * sum
    } else if a > 6.0 {
        1.0
    } else {
        // erfc(a) = exp(-a^2)/sqrt(pi) * 1/(a + (1/2)/(a + 1/(a + (3/2)/(a + ...))))
        let mut tail = a;
        for k in (1..=80).rev() {
            tail = a + f64::from(k) / 2.0 / tail;
        }
        1.0 - (-a * a).exp() / std::f64::consts::PI.sqrt() / tail
    };
    r.copysign(x)
}

/// `E ** (I*r*pi)` as `cos(r*pi) + I*sin(r*pi)` when both have closed forms (multiples of pi/6 and pi/4).
pub(crate) fn euler(exp: &Expr) -> Option<Expr> {
    let (c, rest) = exp.split_coeff();
    if !c.is_exact() {
        return None;
    }
    let Kind::Mul(fs) = rest.kind() else { return None };
    let [a, b] = fs.as_slice() else { return None };
    let ok = (a.as_sym() == Some(SYM_PI) && b.as_sym() == Some("I")) || (b.as_sym() == Some(SYM_PI) && a.as_sym() == Some("I"));
    if !ok {
        return None;
    }
    let angle = Expr::mul(vec![Expr::num(c), Expr::pi()]);
    let (co, si) = (pi_multiple("cos", &angle)?, pi_multiple("sin", &angle)?);
    Some(Expr::add(vec![co, Expr::mul(vec![si, Expr::imag()])]))
}

const EULER_GAMMA: f64 = std::f64::consts::EULER_GAMMA;

/// `(Si(t), Ci(t))` for `t >= 2`: Lentz's method for the continued fraction of `E1(i t)`.
fn cisi_large(t: f64) -> (f64, f64) {
    let tiny = 1e-300;
    let cdiv = |p: (f64, f64), q: (f64, f64)| {
        let m = q.0 * q.0 + q.1 * q.1;
        ((p.0 * q.0 + p.1 * q.1) / m, (p.1 * q.0 - p.0 * q.1) / m)
    };
    let cmul = |p: (f64, f64), q: (f64, f64)| (p.0 * q.0 - p.1 * q.1, p.0 * q.1 + p.1 * q.0);
    let (mut b, mut c) = ((1.0, t), (1.0 / tiny, 0.0));
    let mut d = cdiv((1.0, 0.0), b);
    let mut h = d;
    for i in 2..10_000 {
        let a = -f64::from((i - 1) * (i - 1));
        b = (b.0 + 2.0, b.1);
        d = cdiv((1.0, 0.0), (a * d.0 + b.0, a * d.1 + b.1));
        let q = cdiv((a, 0.0), c);
        c = (b.0 + q.0, b.1 + q.1);
        let del = cmul(c, d);
        h = cmul(h, del);
        if (del.0 - 1.0).abs() + del.1.abs() < 1e-16 {
            break;
        }
    }
    let h = cmul((t.cos(), -t.sin()), h);
    (std::f64::consts::FRAC_PI_2 + h.1, -h.0)
}

/// The sine integral `Si(x)`: its Maclaurin series for small arguments, a continued fraction beyond.
fn sine_integral(x: f64) -> f64 {
    let t = x.abs();
    let r = if t < 2.0 {
        let (mut term, mut sum, mut n) = (t, t, 0.0_f64);
        while term.abs() > 1e-17 * sum.abs() && n < 100.0 {
            n += 1.0;
            term *= -t * t / ((2.0 * n) * (2.0 * n + 1.0));
            sum += term / (2.0 * n + 1.0);
        }
        sum
    } else {
        cisi_large(t).0
    };
    if x < 0.0 { -r } else { r }
}

/// The cosine integral `Ci(x)` for `x > 0` (`NaN` otherwise).
fn cosine_integral(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    if x < 2.0 {
        let (mut term, mut sum, mut n) = (1.0, 0.0, 0.0_f64);
        while n < 100.0 {
            n += 1.0;
            term *= -x * x / ((2.0 * n - 1.0) * (2.0 * n));
            let add = term / (2.0 * n);
            sum += add;
            if add.abs() < 1e-17 * sum.abs() {
                break;
            }
        }
        EULER_GAMMA + x.ln() + sum
    } else {
        cisi_large(x).1
    }
}

/// The exponential integral `Ei(x)` (principal value for `x > 0`).
fn exponential_integral(x: f64) -> f64 {
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x < 0.0 {
        return -e1(-x);
    }
    if x <= 40.0 {
        let (mut term, mut sum, mut k) = (1.0, 0.0, 0.0_f64);
        while k < 500.0 {
            k += 1.0;
            term *= x / k;
            let add = term / k;
            sum += add;
            if add < 1e-17 * sum {
                break;
            }
        }
        EULER_GAMMA + x.ln() + sum
    } else {
        // asymptotic series, summed until its terms stop shrinking
        let (mut term, mut sum, mut k) = (1.0, 1.0, 0.0_f64);
        loop {
            k += 1.0;
            let next = term * k / x;
            if next.abs() >= term.abs() || next.abs() < 1e-17 {
                break;
            }
            term = next;
            sum += term;
        }
        x.exp() / x * sum
    }
}

/// `E1(t)` for `t > 0`: series below 1, continued fraction above.
fn e1(t: f64) -> f64 {
    if t <= 1.0 {
        let (mut term, mut sum, mut k) = (1.0, 0.0, 0.0_f64);
        while k < 100.0 {
            k += 1.0;
            term *= -t / k;
            let add = -term / k;
            sum += add;
            if add.abs() < 1e-17 * sum.abs() {
                break;
            }
        }
        -EULER_GAMMA - t.ln() + sum
    } else {
        let tiny = 1e-300;
        let (mut b, mut c) = (t + 1.0, 1.0 / tiny);
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..10_000 {
            let a = -f64::from(i * i);
            b += 2.0;
            d = 1.0 / (a * d + b);
            c = b + a / c;
            let del = c * d;
            h *= del;
            if (del - 1.0).abs() < 1e-16 {
                break;
            }
        }
        h * (-t).exp()
    }
}

/// The principal branch of the Lambert W function (the inverse of `w * exp(w)`), by Halley iteration; `NaN` below `-1/e`.
fn lambert_w(x: f64) -> f64 {
    let branch = -(-1.0_f64).exp();
    if x < branch - 1e-15 {
        return f64::NAN;
    }
    let x = x.max(branch);
    let mut w = if x < -0.3 {
        // near the branch point, the series in p = sqrt(2 (e x + 1))
        let p = (2.0 * (std::f64::consts::E * x + 1.0)).max(0.0).sqrt();
        -1.0 + p - p * p / 3.0 + 11.0 / 72.0 * p * p * p
    } else if x < 3.0 {
        0.5 * (1.0 + x).ln()
    } else {
        x.ln() - x.ln().ln()
    };
    for _ in 0..64 {
        let ew = w.exp();
        let f = w * ew - x;
        let denom = ew * (w + 1.0) - (w + 2.0) * f / (2.0 * w + 2.0);
        if denom == 0.0 {
            break;
        }
        let step = f / denom;
        w -= step;
        if step.abs() <= 1e-16 * (1.0 + w.abs()) {
            break;
        }
    }
    w
}

/// The Gamma function: Lanczos approximation (g = 7) with the reflection formula for `x < 1/2`; `None` at the poles.
fn gamma_fn(x: f64) -> f64 {
    use std::f64::consts::PI;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return PI / ((PI * x).sin() * gamma_fn(1.0 - x));
    }
    let x = x - 1.0;
    let t = x + 7.5;
    let sum = C.iter().enumerate().skip(1).fold(C[0], |acc, (i, c)| acc + c / (x + i as f64));
    (2.0 * PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * sum
}

/// Exact `Gamma` of a positive integer (a factorial) and of a half-integer (a rational multiple of sqrt(pi)).
fn gamma_exact(n: &Num) -> Option<Expr> {
    let (p, q) = n.parts()?;
    let p = i64::try_from(&p).ok()?;
    let q = i64::try_from(&IBig::from(q)).ok()?;
    let fact = |k: i64| (1..=k).fold(IBig::from(1), |a, b| a * IBig::from(b));
    if q == 1 && (1..=2000).contains(&p) {
        return Some(Expr::big(fact(p - 1)));
    }
    if q == 2 && p.abs() <= 400 {
        // x = p/2 = 1/2 + k with k integer
        let k = (p - 1) / 2;
        let sqrt_pi = Expr::pow(&Expr::pi(), &Expr::half());
        let coeff = if k >= 0 {
            Expr::big(fact(2 * k)).div(&Expr::mul(vec![Expr::big(IBig::from(4).pow(usize::try_from(k).ok()?)), Expr::big(fact(k))]))
        } else {
            let m = -k;
            let sign = if m % 2 == 0 { 1 } else { -1 };
            Expr::mul(vec![Expr::int(sign), Expr::big(IBig::from(4).pow(usize::try_from(m).ok()?)), Expr::big(fact(m))]).div(&Expr::big(fact(2 * m)))
        };
        return Some(Expr::mul(vec![coeff, sqrt_pi]));
    }
    None
}

fn unary(name: &str, x: &Expr) -> Expr {
    let raw = || Expr::raw_call(name, vec![x.clone()]);
    if let Some(n) = x.as_num() {
        if !n.is_exact() {
            return match f64_unary(name, n.to_f64()) {
                Some(v) if v.is_finite() => Expr::float(v),
                _ => raw(),
            };
        }
        return exact_value(name, n).unwrap_or_else(raw);
    }
    // Parity: f(-u) = ±f(u).
    let (c, rest) = x.split_coeff();
    if c.is_negative() && !rest.is_one() && parity(name) != Parity::Neither {
        let flipped = Expr::mul(vec![Expr::num(c.neg()), rest]);
        let inner = unary(name, &flipped);
        return if parity(name) == Parity::Odd { inner.neg() } else { inner };
    }
    if matches!(name, "sin" | "cos" | "tan")
        && let Some(v) = pi_multiple(name, x)
    {
        return v;
    }
    // a purely imaginary argument turns circular functions into hyperbolic ones and back: sin(I*r) = I*sinh(r)
    if let Some(r) = imaginary_multiple(x) {
        let hyper = |f: &str| unary(f, &r);
        let times_i = |v: Expr| Expr::mul(vec![Expr::imag(), v]);
        match name {
            "sin" => return times_i(hyper("sinh")),
            "cos" => return hyper("cosh"),
            "tan" => return times_i(hyper("tanh")),
            "sinh" => return times_i(hyper("sin")),
            "cosh" => return hyper("cos"),
            "tanh" => return times_i(hyper("tan")),
            _ => {}
        }
    }
    match (name, x.kind()) {
        ("log", Kind::Sym(s)) if &**s == SYM_E => Expr::one(),
        ("log", Kind::Pow(b, y)) if b.as_sym() == Some(SYM_E) => y.clone(),
        ("abs", Kind::Call(n, _)) if &**n == "abs" => x.clone(),
        ("LambertW", Kind::Sym(s)) if &**s == SYM_E => Expr::one(),
        _ => raw(),
    }
}

/// `r` when `x` is `I*r` and `r` has no `I` in it.
fn imaginary_multiple(x: &Expr) -> Option<Expr> {
    let unit = Expr::imag();
    match x.kind() {
        Kind::Sym(_) if *x == unit => Some(Expr::one()),
        Kind::Mul(fs) => {
            let rest: Vec<Expr> = fs.iter().filter(|f| **f != unit).cloned().collect();
            (rest.len() + 1 == fs.len() && !rest.iter().any(|f| f.contains(&unit))).then(|| Expr::mul(rest))
        }
        _ => None,
    }
}

fn exact_value(name: &str, n: &Num) -> Option<Expr> {
    let zero = n.is_zero();
    let one = n.is_one();
    match name {
        "sin" | "tan" | "asin" | "atan" | "sinh" | "tanh" | "asinh" | "atanh" | "erf" | "Si" if zero => Some(Expr::zero()),
        "cos" | "cosh" if zero => Some(Expr::one()),
        "acos" | "acosh" | "log" if one => Some(Expr::zero()),
        "atan" if one => Some(Expr::mul(vec![Expr::rational(1, 4)?, Expr::pi()])),
        "atan" if n.neg().is_one() => Some(Expr::mul(vec![Expr::rational(-1, 4)?, Expr::pi()])),
        "asin" | "acos" => {
            // arcsin/arccos at 0, ±1/2, ±1: the angle as a multiple of pi
            let (p, q) = n.parts().and_then(|(p, q)| Some((i64::try_from(&p).ok()?, i64::try_from(&IBig::from(q)).ok()?)))?;
            let sixths = match (p, q) {
                (0, _) => 0,
                (1, 1) => 3,
                (-1, 1) => -3,
                (1, 2) => 1,
                (-1, 2) => -1,
                _ => return None,
            };
            let s = if name == "asin" { sixths } else { 3 - sixths };
            Some(Expr::mul(vec![Expr::rational(s, 6)?, Expr::pi()]))
        }
        "Gamma" => gamma_exact(n),
        "LambertW" if zero => Some(Expr::zero()),
        "abs" => Some(Expr::num(if n.is_negative() { n.neg() } else { n.clone() })),
        // log of a negative number: log(|n|) + I*pi
        "log" if n.is_negative() => Some(Expr::add(vec![unary("log", &Expr::num(n.neg())), Expr::mul(vec![Expr::imag(), Expr::pi()])])),
        _ => None,
    }
}

/// `sin`, `cos`, `tan` of a rational multiple of pi, for the angles with closed forms (multiples of pi/6 and pi/4).
fn pi_multiple(name: &str, x: &Expr) -> Option<Expr> {
    let (c, rest) = x.split_coeff();
    if rest.as_sym() != Some(SYM_PI) || !c.is_exact() {
        return None;
    }
    let (p, q) = c.parts()?;
    let p = i64::try_from(&p).ok()?;
    let q = i64::try_from(&IBig::from(q)).ok()?;
    match name {
        "sin" => sin_pi(i128::from(p), i128::from(q)),
        "cos" => sin_pi(i128::from(p) * 2 + i128::from(q), i128::from(q) * 2),
        "tan" => {
            let s = sin_pi(i128::from(p), i128::from(q))?;
            let c = sin_pi(i128::from(p) * 2 + i128::from(q), i128::from(q) * 2)?;
            if c.is_zero() { None } else { Some(s.div(&c)) }
        }
        _ => None,
    }
}

/// `sin(p/q * pi)` when it has a simple closed form.
fn sin_pi(p: i128, q: i128) -> Option<Expr> {
    if q <= 0 {
        return None;
    }
    let mut n = p.rem_euclid(2 * q);
    let mut negative = false;
    if n > q {
        n -= q;
        negative = true;
    }
    if 2 * n > q {
        n = q - n;
    }
    let half = Expr::half();
    let v = if n == 0 {
        Expr::zero()
    } else if 6 * n == q {
        half
    } else if 4 * n == q {
        Expr::mul(vec![half, Expr::pow(&Expr::int(2), &Expr::half())])
    } else if 3 * n == q {
        Expr::mul(vec![half, Expr::pow(&Expr::int(3), &Expr::half())])
    } else if 2 * n == q {
        Expr::one()
    } else {
        return None;
    };
    Some(if negative { v.neg() } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn pi_times(n: i64, d: i64) -> Expr {
        Expr::mul(vec![Expr::rational(n, d).unwrap(), Expr::pi()])
    }

    #[test]
    fn special_angles() {
        assert_eq!(Expr::call("sin", vec![Expr::pi()]), Expr::zero());
        assert_eq!(Expr::call("cos", vec![Expr::pi()]), Expr::int(-1));
        assert_eq!(Expr::call("sin", vec![pi_times(1, 6)]).to_string(), "1/2");
        assert_eq!(Expr::call("cos", vec![pi_times(1, 4)]).to_string(), "sqrt(2)/2");
        assert_eq!(Expr::call("sin", vec![pi_times(7, 6)]).to_string(), "-1/2");
        assert_eq!(Expr::call("tan", vec![pi_times(1, 4)]), Expr::one());
        assert_eq!(Expr::call("sin", vec![pi_times(1, 7)]).to_string(), "sin(pi/7)");
    }

    #[test]
    fn parity_rules() {
        assert_eq!(Expr::call("sin", vec![x().neg()]).to_string(), "-sin(x)");
        assert_eq!(Expr::call("cos", vec![x().neg()]).to_string(), "cos(x)");
    }

    #[test]
    fn exp_and_log() {
        let ex = Expr::call("exp", vec![x()]);
        assert_eq!(Expr::call("log", vec![ex.clone()]), x());
        assert_eq!(Expr::call("exp", vec![Expr::call("log", vec![x()])]), x());
        assert_eq!(Expr::mul(vec![ex.clone(), ex]).to_string(), "exp(2*x)");
        assert_eq!(Expr::call("log", vec![Expr::e()]), Expr::one());
        // exp(c*log(a)) is a power; powers of exp(number) scale the exponent
        assert_eq!(Expr::call("exp", vec![Expr::mul(vec![Expr::int(3), Expr::call("log", vec![Expr::int(2)])])]), Expr::int(8));
        assert_eq!(Expr::pow(&Expr::call("exp", vec![Expr::int(4)]), &Expr::half()).to_string(), "exp(2)");
    }

    #[test]
    fn float_arguments_evaluate() {
        let v = Expr::call("sin", vec![Expr::float(0.0)]);
        assert!(v.is_zero());
        let w = Expr::call("cos", vec![Expr::float(0.0)]);
        assert!(w.is_one());
    }
}
