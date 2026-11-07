//! Source code for an expression in another language, so a result can leave MathCraft: Python (`math`/`cmath`),
//! NumPy and Rust (`f64`). Pure text generation; the expression is printed with the minimum parentheses.

use crate::expr::{Expr, Kind, SYM_E, SYM_I, SYM_OO, SYM_PI};
use crate::{MathError, MathResult, Num};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Python,
    NumPy,
    Rust,
}

impl Target {
    pub fn parse(name: &str) -> Option<Target> {
        match name.to_ascii_lowercase().as_str() {
            "python" | "py" => Some(Target::Python),
            "numpy" | "np" => Some(Target::NumPy),
            "rust" | "rs" => Some(Target::Rust),
            _ => None,
        }
    }
}

// precedence levels
const ADD: u8 = 1;
const MUL: u8 = 2;
const UNARY: u8 = 3;
const POW: u8 = 4;
const ATOM: u8 = 5;

struct Gen {
    target: Target,
    /// Python target: the expression has `I`, so use `cmath`.
    complex: bool,
}

fn contains_i(e: &Expr) -> bool {
    e.as_sym() == Some(SYM_I) || e.children().iter().any(contains_i)
}

/// The expression as a single source-code expression.
pub fn expression(e: &Expr, target: Target) -> MathResult<String> {
    let g = Gen { target, complex: contains_i(e) };
    if g.complex && target == Target::Rust {
        return Err(MathError::unsupported("complex numbers cannot be exported to Rust (f64) yet"));
    }
    Ok(g.emit(e)?.0)
}

/// A complete function. `body` is one expression, or a list (returned as a tuple / array).
pub fn function(body: &Expr, args: &[Expr], name: &str, target: Target) -> MathResult<String> {
    let names: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (complex, items): (bool, Vec<&Expr>) = match body.kind() {
        Kind::List(v) => (v.iter().any(contains_i), v.iter().collect()),
        _ => (contains_i(body), vec![body]),
    };
    let is_list = matches!(body.kind(), Kind::List(_));
    let codes = items.iter().map(|e| expression(e, target)).collect::<MathResult<Vec<_>>>()?;
    Ok(match target {
        Target::Python | Target::NumPy => {
            let import = match (target, complex) {
                (Target::NumPy, _) => "import numpy as np",
                (_, true) => "import cmath",
                _ => "import math",
            };
            let ret = if is_list { format!("({}{})", codes.join(", "), if codes.len() == 1 { "," } else { "" }) } else { codes.join("") };
            format!("{import}\n\n\ndef {name}({}):\n    return {ret}\n", names.join(", "))
        }
        Target::Rust => {
            let params: Vec<String> = names.iter().map(|n| format!("{n}: f64")).collect();
            let (ty, ret) = if is_list { (format!("[f64; {}]", codes.len()), format!("[{}]", codes.join(", "))) } else { ("f64".to_owned(), codes.join("")) };
            format!("fn {name}({}) -> {ty} {{\n    {ret}\n}}\n", params.join(", "))
        }
    })
}

impl Gen {
    fn paren(&self, (s, p): (String, u8), need: u8) -> String {
        if p < need { format!("({s})") } else { s }
    }

    fn func(&self, name: &str) -> MathResult<String> {
        let known = ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "log"];
        if !known.contains(&name) && name != "abs" && name != "sqrt" && name != "exp" {
            return Err(MathError::unsupported(format!("cannot export a call to `{name}`")));
        }
        Ok(match (self.target, name) {
            (Target::Python, "abs") => "abs".to_owned(),
            (Target::Python, n) => format!("{}.{n}", if self.complex { "cmath" } else { "math" }),
            (Target::NumPy, "abs") => "np.abs".to_owned(),
            (Target::NumPy, n) => format!("np.{n}"),
            (Target::Rust, n) => n.to_owned(),
        })
    }

    /// A call `name(arg)` in the target language.
    fn call(&self, name: &str, arg: &Expr) -> MathResult<(String, u8)> {
        let a = self.emit(arg)?;
        if self.target == Target::Rust {
            let method = match name {
                "log" => "ln",
                n => n,
            };
            return Ok((format!("{}.{method}()", self.paren(a, ATOM)), ATOM));
        }
        Ok((format!("{}({})", self.func(name)?, a.0), ATOM))
    }

    fn number(&self, n: &Num) -> (String, u8) {
        let float = |s: String| if self.target == Target::Rust && !s.contains(['.', 'e', 'E']) { format!("{s}.0") } else { s };
        match n.parts() {
            Some((p, q)) if q == dashu::integer::UBig::ONE => {
                let s = float(p.to_string());
                let prec = if p < dashu::integer::IBig::ZERO { UNARY } else { ATOM };
                (s, prec)
            }
            Some((p, q)) => (format!("{}/{}", float(p.to_string()), float(q.to_string())), MUL),
            None => {
                let f = n.to_f64();
                let s = float(format!("{f:?}"));
                (s, if f < 0.0 { UNARY } else { ATOM })
            }
        }
    }

    fn constant(&self, name: &str) -> MathResult<Option<String>> {
        Ok(Some(match (name, self.target) {
            (SYM_PI, Target::Python) => if self.complex { "cmath.pi" } else { "math.pi" }.to_owned(),
            (SYM_PI, Target::NumPy) => "np.pi".to_owned(),
            (SYM_PI, Target::Rust) => "std::f64::consts::PI".to_owned(),
            (SYM_E, Target::Python) => if self.complex { "cmath.e" } else { "math.e" }.to_owned(),
            (SYM_E, Target::NumPy) => "np.e".to_owned(),
            (SYM_E, Target::Rust) => "std::f64::consts::E".to_owned(),
            (SYM_I, Target::Rust) => return Err(MathError::unsupported("complex numbers cannot be exported to Rust")),
            (SYM_I, _) => "1j".to_owned(),
            (SYM_OO, Target::Python) => "math.inf".to_owned(),
            (SYM_OO, Target::NumPy) => "np.inf".to_owned(),
            (SYM_OO, Target::Rust) => "f64::INFINITY".to_owned(),
            _ => return Ok(None),
        }))
    }

    fn pow(&self, base: &Expr, exp: &Expr) -> MathResult<(String, u8)> {
        if base.as_sym() == Some(SYM_E) {
            return match self.target {
                Target::Rust => Ok((format!("{}.exp()", self.paren(self.emit(exp)?, ATOM)), ATOM)),
                _ => {
                    let f = match (self.target, self.complex) {
                        (Target::NumPy, _) => "np.exp",
                        (_, true) => "cmath.exp",
                        _ => "math.exp",
                    };
                    Ok((format!("{f}({})", self.emit(exp)?.0), ATOM))
                }
            };
        }
        if exp == &Expr::half() {
            return match self.target {
                Target::Rust => Ok((format!("{}.sqrt()", self.paren(self.emit(base)?, ATOM)), ATOM)),
                _ => {
                    let f = match (self.target, self.complex) {
                        (Target::NumPy, _) => "np.sqrt",
                        (_, true) => "cmath.sqrt",
                        _ => "math.sqrt",
                    };
                    Ok((format!("{f}({})", self.emit(base)?.0), ATOM))
                }
            };
        }
        let b = self.emit(base)?;
        if self.target == Target::Rust {
            let b = self.paren(b, ATOM);
            return Ok(match exp.as_i64().and_then(|n| i32::try_from(n).ok()) {
                Some(n) => (format!("{b}.powi({n})"), ATOM),
                None => (format!("{b}.powf({})", self.emit(exp)?.0), ATOM),
            });
        }
        let b = self.paren(b, POW + 1);
        let e = self.paren(self.emit(exp)?, POW);
        Ok((format!("{b}**{e}"), POW))
    }

    fn emit(&self, e: &Expr) -> MathResult<(String, u8)> {
        match e.kind() {
            Kind::Num(n) => Ok(self.number(n)),
            Kind::Sym(s) => Ok((self.constant(s)?.unwrap_or_else(|| s.to_string()), ATOM)),
            Kind::Pow(b, x) => {
                if x.as_num().is_some_and(Num::is_negative) {
                    // 1 / b**k
                    let den = self.emit(&Expr::pow(b, &x.neg()))?;
                    return Ok((format!("1/{}", self.paren(den, UNARY + 1)), MUL));
                }
                self.pow(b, x)
            }
            Kind::Call(name, args) => match args.as_slice() {
                [a] => self.call(name, a),
                _ => Err(MathError::unsupported(format!("cannot export a call to `{name}`"))),
            },
            Kind::Add(terms) => {
                // terms in the order the printer shows them: highest degree first
                let mut out = String::new();
                for (i, t) in crate::sum_terms(terms).iter().enumerate() {
                    let magnitude = Expr::mul(vec![Expr::num(t.coeff.clone()), t.rest.clone()]);
                    let s = self.paren(self.emit(&magnitude)?, MUL);
                    out.push_str(match (i, t.negative) {
                        (0, false) => "",
                        (0, true) => "-",
                        (_, false) => " + ",
                        (_, true) => " - ",
                    });
                    out.push_str(&s);
                }
                Ok((out, ADD))
            }
            Kind::Mul(factors) => {
                let (mut num, mut den): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
                let mut negative = false;
                for f in factors {
                    match f.kind() {
                        Kind::Num(n) if n.is_negative() => {
                            negative = !negative;
                            let pos = n.neg();
                            match pos.parts() {
                                Some((p, q)) if q != dashu::integer::UBig::ONE => {
                                    if p != dashu::integer::IBig::ONE {
                                        num.push(self.number(&Num::Int(p)).0);
                                    }
                                    den.push(self.number(&Num::Int(q.into())).0);
                                }
                                _ if pos.is_one() => {}
                                _ => num.push(self.number(&pos).0),
                            }
                        }
                        Kind::Num(n) => match n.parts() {
                            Some((p, q)) if q != dashu::integer::UBig::ONE => {
                                if p != dashu::integer::IBig::ONE {
                                    num.push(self.number(&Num::Int(p)).0);
                                }
                                den.push(self.number(&Num::Int(q.into())).0);
                            }
                            _ => num.push(self.number(n).0),
                        },
                        Kind::Pow(b, k) if k.as_num().is_some_and(Num::is_negative) => {
                            let d = self.emit(&Expr::pow(b, &k.neg()))?;
                            den.push(self.paren(d, UNARY + 1));
                        }
                        _ => {
                            let g = self.emit(f)?;
                            num.push(self.paren(g, MUL + 1));
                        }
                    }
                }
                let top = if num.is_empty() { self.one() } else { num.join("*") };
                let body = match den.len() {
                    0 => top,
                    1 => format!("{top}/{}", den.join("")),
                    _ => format!("{top}/({})", den.join("*")),
                };
                let single = den.is_empty() && num.len() <= 1;
                if negative { Ok((format!("-{body}"), UNARY)) } else { Ok((body, if single { ATOM.min(MUL + 1) } else { MUL })) }
            }
            Kind::List(items) => {
                let parts = items.iter().map(|i| self.emit(i).map(|x| x.0)).collect::<MathResult<Vec<_>>>()?;
                Ok((format!("[{}]", parts.join(", ")), ATOM))
            }
            Kind::Eq(..) => Err(MathError::unsupported("cannot export an equation; export one side")),
        }
    }

    fn one(&self) -> String {
        if self.target == Target::Rust { "1.0".to_owned() } else { "1".to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn py(e: &Expr) -> String {
        expression(e, Target::Python).unwrap()
    }

    #[test]
    fn polynomials_and_fractions() {
        let e = Expr::add(vec![Expr::pow(&x(), &Expr::int(2)), Expr::mul(vec![Expr::int(-3), x()]), Expr::int(2)]);
        assert_eq!(py(&e), "x**2 - 3*x + 2");
        assert_eq!(py(&x().div(&Expr::add(vec![x(), Expr::int(1)]))), "x/(x + 1)");
        assert_eq!(py(&Expr::mul(vec![Expr::rational(1, 2).unwrap(), x()])), "x/2");
        assert_eq!(py(&Expr::pow(&Expr::add(vec![x(), Expr::int(1)]), &Expr::int(-2))), "1/(x + 1)**2");
        assert_eq!(py(&Expr::mul(vec![Expr::int(-1), Expr::add(vec![x(), Expr::int(1)])])), "-(x + 1)");
    }

    #[test]
    fn functions_and_constants() {
        let sin = Expr::call("sin", vec![x()]);
        assert_eq!(py(&sin), "math.sin(x)");
        assert_eq!(expression(&sin, Target::NumPy).unwrap(), "np.sin(x)");
        assert_eq!(expression(&sin, Target::Rust).unwrap(), "x.sin()");
        assert_eq!(py(&Expr::call("exp", vec![x()])), "math.exp(x)");
        assert_eq!(py(&Expr::pow(&x(), &Expr::half())), "math.sqrt(x)");
        assert_eq!(py(&Expr::mul(vec![Expr::int(2), Expr::pi()])), "2*math.pi");
        assert_eq!(expression(&Expr::pow(&x(), &Expr::int(3)), Target::Rust).unwrap(), "x.powi(3)");
        assert_eq!(expression(&Expr::add(vec![x(), Expr::int(1)]), Target::Rust).unwrap(), "x + 1.0");
        assert_eq!(py(&Expr::call("log", vec![x()])), "math.log(x)");
        assert_eq!(expression(&Expr::call("log", vec![x()]), Target::Rust).unwrap(), "x.ln()");
    }

    #[test]
    fn complex_numbers_use_cmath() {
        let e = Expr::add(vec![Expr::int(1), Expr::mul(vec![Expr::int(2), Expr::imag()])]);
        assert_eq!(py(&e), "1 + 2*1j");
        assert_eq!(py(&Expr::call("sqrt", vec![Expr::int(-4)])), "2*1j");
        assert!(expression(&e, Target::Rust).is_err());
        let r = Expr::add(vec![Expr::pow(&x(), &Expr::half()), Expr::imag()]);
        assert!(py(&r).starts_with("cmath.sqrt"));
    }

    #[test]
    fn whole_functions() {
        let e = Expr::add(vec![x(), Expr::int(1)]);
        assert_eq!(function(&e, &[x()], "f", Target::Python).unwrap(), "import math\n\n\ndef f(x):\n    return x + 1\n");
        assert_eq!(function(&e, &[x()], "f", Target::Rust).unwrap(), "fn f(x: f64) -> f64 {\n    x + 1.0\n}\n");
        let roots = Expr::list(vec![x(), e]);
        assert!(function(&roots, &[x()], "roots", Target::Python).unwrap().contains("return (x, x + 1)"));
        assert!(function(&roots, &[x()], "roots", Target::Rust).unwrap().contains("-> [f64; 2]"));
    }

    #[test]
    fn unsupported_things_are_errors() {
        assert!(expression(&Expr::call("foo", vec![x()]), Target::Python).is_err());
        assert!(expression(&Expr::eq(x(), Expr::one()), Target::Python).is_err());
    }
}
