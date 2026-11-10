//! LaTeX source for an expression, so a result can be pasted into a paper, a slide or a Markdown note.
//! Term and factor order come from the printer (`sum_terms`, `product_of`) so the LaTeX reads like the text form.

use crate::expr::{Expr, Kind, SYM_E, SYM_I, SYM_OO, SYM_PI};
use crate::{Num, product_of, sum_terms};

// precedence levels
const ADD: u8 = 1;
const MUL: u8 = 2;
const UNARY: u8 = 3;
const POW: u8 = 4;
const ATOM: u8 = 5;

const GREEK: [&str; 35] = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "rho", "sigma", "tau",
    "upsilon", "phi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Phi", "Psi", "Omega", "Upsilon", "varphi",
];

/// Functions LaTeX has a command for (the key is our name, the value the command without the backslash).
const NAMED: [(&str, &str); 11] = [
    ("sin", "sin"),
    ("cos", "cos"),
    ("tan", "tan"),
    ("asin", "arcsin"),
    ("acos", "arccos"),
    ("atan", "arctan"),
    ("sinh", "sinh"),
    ("cosh", "cosh"),
    ("tanh", "tanh"),
    ("log", "ln"),
    ("exp", "exp"),
];

/// The expression as LaTeX (math mode, no `$` delimiters).
pub fn latex(e: &Expr) -> String {
    emit(e).0
}

/// The expression as a display-math block, `\[ ... \]`.
pub fn latex_block(e: &Expr) -> String {
    format!("\\[\n{}\n\\]", latex(e))
}

fn paren((s, p): (String, u8), need: u8) -> String {
    if p < need { format!("\\left({s}\\right)") } else { s }
}

fn symbol(name: &str) -> String {
    match name {
        SYM_PI => return "\\pi".to_owned(),
        SYM_E => return "e".to_owned(),
        SYM_I => return "i".to_owned(),
        SYM_OO => return "\\infty".to_owned(),
        _ => {}
    }
    if let Some((base, sub)) = name.split_once('_') {
        return format!("{}_{{{}}}", symbol(base), sub.replace('_', ","));
    }
    if GREEK.contains(&name) {
        return format!("\\{name}");
    }
    let mut chars = name.chars();
    match (chars.next(), chars.clone().next()) {
        (Some(c), None) => c.to_string(),
        // x1, y2: a letter and digits read as a subscript
        (Some(c), Some(_)) if c.is_alphabetic() && chars.all(|d| d.is_ascii_digit()) => format!("{c}_{{{}}}", &name[c.len_utf8()..]),
        _ => format!("\\mathrm{{{}}}", name.replace('_', "\\_")),
    }
}

/// A float like `1.5e-7` becomes `1.5\times10^{-7}`.
fn float(s: &str) -> String {
    match s.split_once(['e', 'E']) {
        Some((m, x)) => format!("{m}\\times10^{{{}}}", x.trim_start_matches('+')),
        None => s.to_owned(),
    }
}

fn call(name: &str, args: &[Expr]) -> (String, u8) {
    let list = |args: &[Expr]| args.iter().map(latex).collect::<Vec<_>>().join(",\\,");
    match (name, args) {
        ("sqrt", [a]) => (format!("\\sqrt{{{}}}", latex(a)), ATOM),
        ("abs", [a]) => (format!("\\left|{}\\right|", latex(a)), ATOM),
        ("Integral" | "integrate", [f, x]) => (format!("\\int {}\\,d{}", latex(f), latex(x)), MUL),
        ("Integral", [f, x, lo, hi]) => (format!("\\int_{{{}}}^{{{}}} {}\\,d{}", latex(lo), latex(hi), latex(f), latex(x)), MUL),
        ("Sum", [f, k, lo, hi]) => (format!("\\sum_{{{}={}}}^{{{}}} {}", latex(k), latex(lo), latex(hi), paren(emit(f), MUL)), MUL),
        ("Limit", [f, x, a]) => (format!("\\lim_{{{} \\to {}}} {}", latex(x), latex(a), paren(emit(f), MUL)), MUL),
        (n, [a]) if NAMED.iter().any(|(k, _)| *k == n) => {
            let cmd = NAMED.iter().find(|(k, _)| *k == n).map_or(n, |(_, c)| *c);
            (format!("\\{cmd}\\left({}\\right)", latex(a)), ATOM)
        }
        (n, a) => (format!("\\operatorname{{{}}}\\left({}\\right)", n.replace('_', "\\_"), list(a)), ATOM),
    }
}

fn pow(base: &Expr, exp: &Expr) -> (String, u8) {
    if base.as_sym() == Some(SYM_E) {
        return (format!("e^{{{}}}", latex(exp)), POW);
    }
    if exp == &Expr::half() {
        return (format!("\\sqrt{{{}}}", latex(base)), ATOM);
    }
    // sin(x)^2 reads better as \sin^{2}(x)
    if let Kind::Call(name, args) = base.kind()
        && let [a] = args.as_slice()
        && let Some((_, cmd)) = NAMED.iter().find(|(k, _)| *k == &**name)
        && *cmd != "exp"
        && *cmd != "ln"
    {
        return (format!("\\{cmd}^{{{}}}\\left({}\\right)", latex(exp), latex(a)), ATOM);
    }
    (format!("{}^{{{}}}", paren(emit(base), ATOM), latex(exp)), POW)
}

fn emit(e: &Expr) -> (String, u8) {
    match e.kind() {
        Kind::Sym(s) => (symbol(s), ATOM),
        Kind::Num(_) | Kind::Mul(_) => product(e),
        Kind::Pow(b, x) if x.as_num().is_some_and(Num::is_negative) => product(e),
        Kind::Pow(b, x) => pow(b, x),
        Kind::Call(name, args) => call(name, args),
        Kind::Add(terms) => {
            let mut out = String::new();
            for (i, t) in sum_terms(terms).iter().enumerate() {
                let magnitude = Expr::mul(vec![Expr::num(t.coeff.clone()), t.rest.clone()]);
                out.push_str(match (i, t.negative) {
                    (0, false) => "",
                    (0, true) => "-",
                    (_, false) => " + ",
                    (_, true) => " - ",
                });
                out.push_str(&paren(emit(&magnitude), MUL));
            }
            (out, ADD)
        }
        Kind::List(items) => (format!("\\left[{}\\right]", items.iter().map(latex).collect::<Vec<_>>().join(",\\,")), ATOM),
        Kind::Eq(a, b) => (format!("{} = {}", latex(a), latex(b)), ADD),
    }
}

/// A product or a lone number, split into a numerator and denominator (`\frac`).
fn product(e: &Expr) -> (String, u8) {
    let p = product_of(e);
    let factors = |num: &Option<String>, rest: &[Expr]| -> Vec<(String, u8)> { num.iter().map(|n| (float(n), ATOM)).chain(rest.iter().map(emit)).collect() };
    let top = factors(&p.numer_num, &p.numer);
    let bottom = factors(&p.denom_num, &p.denom);
    // Alone (inside a fraction bar or a lone term) a factor needs no parentheses; beside others a sum does.
    let join = |parts: Vec<(String, u8)>| -> String {
        let alone = parts.len() == 1;
        let mut out = String::new();
        for (i, part) in parts.into_iter().enumerate() {
            // two plain numbers side by side would read as one: 2 \cdot 3
            let numeral = part.0.starts_with(|c: char| c.is_ascii_digit());
            if i > 0 {
                out.push_str(if numeral { " \\cdot " } else { " " });
            }
            out.push_str(&if alone { part.0 } else { paren(part, MUL + 1) });
        }
        out
    };
    let single = top.len() <= 1 && bottom.is_empty();
    let num = if top.is_empty() { "1".to_owned() } else { join(top) };
    let body = if bottom.is_empty() { num } else { format!("\\frac{{{num}}}{{{}}}", join(bottom)) };
    match (p.negative, single) {
        (true, _) => (format!("-{body}"), UNARY),
        (false, true) => (body, ATOM.min(MUL + 1)),
        (false, false) => (body, MUL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn sums_and_fractions() {
        let e = Expr::add(vec![Expr::pow(&x(), &Expr::int(2)), Expr::mul(vec![Expr::int(-3), x()]), Expr::int(2)]);
        assert_eq!(latex(&e), "x^{2} - 3 x + 2");
        assert_eq!(latex(&x().div(&Expr::add(vec![x(), Expr::int(1)]))), "\\frac{x}{x + 1}");
        assert_eq!(latex(&Expr::mul(vec![Expr::rational(1, 2).unwrap(), x()])), "\\frac{x}{2}");
        assert_eq!(latex(&Expr::rational(3, 4).unwrap()), "\\frac{3}{4}");
        assert_eq!(latex(&Expr::int(-5)), "-5");
    }

    #[test]
    fn powers_roots_and_functions() {
        assert_eq!(latex(&Expr::pow(&x(), &Expr::half())), "\\sqrt{x}");
        assert_eq!(latex(&Expr::pow(&Expr::e(), &x())), "e^{x}");
        assert_eq!(latex(&Expr::call("sin", vec![x()])), "\\sin\\left(x\\right)");
        assert_eq!(latex(&Expr::pow(&Expr::call("sin", vec![x()]), &Expr::int(2))), "\\sin^{2}\\left(x\\right)");
        assert_eq!(latex(&Expr::pow(&Expr::add(vec![x(), Expr::int(1)]), &Expr::int(2))), "\\left(x + 1\\right)^{2}");
        assert_eq!(latex(&Expr::call("asin", vec![x()])), "\\arcsin\\left(x\\right)");
        assert_eq!(latex(&Expr::call("foo", vec![x(), Expr::int(2)])), "\\operatorname{foo}\\left(x,\\,2\\right)");
    }

    #[test]
    fn names_and_constants() {
        assert_eq!(latex(&Expr::mul(vec![Expr::int(2), Expr::pi()])), "2 \\pi");
        assert_eq!(latex(&Expr::sym("alpha")), "\\alpha");
        assert_eq!(latex(&Expr::sym("x_1")), "x_{1}");
        assert_eq!(latex(&Expr::sym("x2")), "x_{2}");
        assert_eq!(latex(&Expr::sym("speed")), "\\mathrm{speed}");
        assert_eq!(latex(&Expr::imag()), "i");
    }

    #[test]
    fn lists_equations_and_floats() {
        assert_eq!(latex(&Expr::list(vec![x(), Expr::int(1)])), "\\left[x,\\,1\\right]");
        assert_eq!(float("1.5e-7"), "1.5\\times10^{-7}");
        assert_eq!(latex_block(&x()), "\\[\nx\n\\]");
    }
}
