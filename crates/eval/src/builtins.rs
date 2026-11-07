//! The builtin function library: elementary functions, algebra, calculus, linear algebra, plotting and the
//! Python builtins people expect. `DOCS` is the single list of names, so `help()` and suggestions never drift.

use crate::array::{self, DType};
use crate::interp::{Interp, numeric, order_numbers};
use crate::value::{EvalError, PlotSeries, PlotSpec, R, Value, err};
use dashu::integer::IBig;
use mathcraft_algebra as alg;
use mathcraft_algebra::linalg;
use mathcraft_amplitude as amp;
use mathcraft_core::numeric::{approximate, eval_f64};
use mathcraft_core::{Expr, Kind, Num, budget};
use std::cmp::Ordering;
use std::sync::Arc;

pub struct Args {
    pub pos: Vec<Value>,
    pub kw: Vec<(String, Value)>,
}

impl Args {
    fn limits(&self, f: &str, min: usize, max: usize) -> R<()> {
        if !self.kw.is_empty() {
            return err(format!("{f}() does not take keyword arguments"));
        }
        let n = self.pos.len();
        if n < min || n > max {
            let want = if min == max { format!("{min}") } else { format!("{min} to {max}") };
            return err(format!("{f}() takes {want} argument(s) but {n} were given (see help('{f}'))"));
        }
        Ok(())
    }

    /// Positional argument `i`, or the keyword `name`.
    fn opt(&self, i: usize, name: &str) -> Option<&Value> {
        self.pos.get(i).or_else(|| self.kw.iter().find(|(k, _)| k == name).map(|(_, v)| v))
    }

    fn get(&self, i: usize) -> R<&Value> {
        self.pos.get(i).ok_or_else(|| EvalError::new("missing argument"))
    }

    fn expr(&self, i: usize) -> R<Expr> {
        self.get(i)?.to_expr()
    }

    fn int(&self, i: usize) -> R<i64> {
        let v = self.get(i)?;
        numeric(v).and_then(|e| e.as_i64()).ok_or_else(|| EvalError::new(format!("expected an integer, got a {}", v.type_name())))
    }
}

/// `(name, signature, summary)` for every builtin. Python builtins first, then math.
pub const DOCS: &[(&str, &str, &str)] = &[
    ("print", "print(*values)", "show values in the output"),
    ("len", "len(x)", "number of items in a list, tuple, string or range"),
    ("range", "range(stop) | range(start, stop[, step])", "a sequence of integers"),
    ("sum", "sum(items[, start])", "add up a list"),
    ("prod", "prod(items)", "multiply a list together"),
    ("min", "min(items) | min(a, b, ...)", "smallest number"),
    ("max", "max(items) | max(a, b, ...)", "largest number"),
    ("list", "list(x)", "turn a range, tuple or string into a list"),
    ("zip", "zip(a, b, ...)", "pair up items from several lists"),
    ("enumerate", "enumerate(items)", "pairs of (index, item)"),
    ("sorted", "sorted(items, reverse=False)", "a sorted copy of a list"),
    ("reversed", "reversed(items)", "a reversed copy of a list"),
    ("any", "any(items)", "True if any item is true"),
    ("all", "all(items)", "True if every item is true"),
    ("str", "str(x)", "text form of a value"),
    ("format", "format(x, spec)", "x as text with a format spec like '.3f', ',d', '>8' or '08.2f'; what f\"{x:.3f}\" uses"),
    ("repr", "repr(x)", "the quoted, code-like text form of a value"),
    ("bool", "bool(x)", "truth value of a number or list"),
    ("int", "int(x)", "whole part of a number (towards zero)"),
    ("float", "float(x)", "machine-precision decimal value of a number or constant"),
    ("round", "round(x[, digits])", "round to the nearest integer (or to some decimals)"),
    ("floor", "floor(x)", "largest integer not above x"),
    ("ceil", "ceil(x)", "smallest integer not below x"),
    ("factorial", "factorial(n)", "n! as an exact integer"),
    ("binomial", "binomial(n, k)", "number of ways to choose k of n"),
    ("gcd", "gcd(a, b)", "greatest common divisor of integers or of integer polynomials"),
    ("lcm", "lcm(a, b)", "least common multiple of integers"),
    ("sin", "sin(x)", "sine (radians); exact for multiples of pi/6 and pi/4"),
    ("cos", "cos(x)", "cosine"),
    ("tan", "tan(x)", "tangent"),
    ("asin", "asin(x)", "inverse sine"),
    ("acos", "acos(x)", "inverse cosine"),
    ("atan", "atan(x)", "inverse tangent"),
    ("sinh", "sinh(x)", "hyperbolic sine"),
    ("cosh", "cosh(x)", "hyperbolic cosine"),
    ("tanh", "tanh(x)", "hyperbolic tangent"),
    ("exp", "exp(x)", "e to the power x"),
    ("log", "log(x[, base])", "natural logarithm (or logarithm in a base)"),
    ("sqrt", "sqrt(x)", "square root; exact, e.g. sqrt(8) is 2*sqrt(2)"),
    ("abs", "abs(x)", "absolute value (modulus of a complex number)"),
    ("re", "re(z)", "real part; symbols are taken to be real"),
    ("im", "im(z)", "imaginary part; symbols are taken to be real"),
    ("conjugate", "conjugate(z)", "complex conjugate"),
    ("symbols", "symbols('x y z')", "declare symbols (also names of unknown functions): x, y = symbols('x y')"),
    ("Eq", "Eq(lhs, rhs)", "an equation, for solve()"),
    ("expand", "expand(expr)", "multiply out products and powers of sums"),
    ("cancel", "cancel(expr)", "cancel common factors of a fraction (any number of symbols)"),
    ("factor", "factor(poly)", "factor a polynomial over the rationals (rational roots)"),
    ("simplify", "simplify(expr)", "try expanding, factoring and sin^2+cos^2=1; keep the smallest form"),
    ("N", "N(expr)", "numeric approximation; symbols are kept"),
    ("subs", "subs(expr, old, new) | subs(expr, [x == 1, y == 2])", "substitute values or expressions"),
    ("diff", "diff(f, x[, n])", "derivative with respect to x (n times)"),
    ("integrate", "integrate(f, x) | integrate(f, (x, a, b))", "antiderivative, or definite integral (numeric if no closed form)"),
    ("limit", "limit(f, x, a, dir='+-')", "limit as x approaches a (a may be oo or -oo; dir='+' or '-' for one side)"),
    ("latex", "latex(value)", "LaTeX source of an expression, list or matrix"),
    ("pycode", "pycode(expr, args=None, name='f', numpy=False)", "Python source of a function computing expr (a list gives a tuple)"),
    ("rustcode", "rustcode(expr, args=None, name='f')", "Rust source of an f64 function computing expr"),
    ("nroots", "nroots(poly, x)", "every complex root of a polynomial, numerically (for equations solve cannot do)"),
    ("nsolve", "nsolve(eq, x, guess)", "numeric root near a starting guess (Newton's method)"),
    ("apart", "apart(f, x)", "partial fractions of a rational function"),
    ("series", "series(f, x, x0=0, n=6)", "Taylor polynomial up to order n"),
    (
        "solve",
        "solve(eq, x) | solve([eq1, eq2], [x, y])",
        "solutions of one equation (polynomial, exp/log/trig), a linear system, or a polynomial system (a list of solutions)",
    ),
    ("coeffs", "coeffs(poly, x)", "coefficients, lowest degree first"),
    ("degree", "degree(poly, x)", "degree of a polynomial"),
    ("free_symbols", "free_symbols(expr)", "the symbols in an expression"),
    ("vec", "vec(items, dtype=exact)", "a numeric vector with an element type: exact (default), i8 i16 i32 i64, f32 f64"),
    ("matrix", "matrix([[1, 2], [3, 4]], dtype=exact)", "a numeric matrix with an element type"),
    ("zeros", "zeros(n | (rows, cols), dtype=exact)", "vector or matrix of zeros"),
    ("ones", "ones(n | (rows, cols), dtype=exact)", "vector or matrix of ones"),
    ("astype", "astype(a, dtype)", "convert a vector or matrix to another element type (explicit; types are never mixed implicitly)"),
    ("tolist", "tolist(a)", "a vector or matrix as a list"),
    ("shape", "shape(a)", "dimensions of a vector or matrix"),
    ("dot", "dot(u, v)", "dot product of two vectors"),
    ("norm", "norm(v)", "Euclidean length of a vector"),
    ("det", "det(matrix)", "determinant"),
    ("inv", "inv(matrix)", "inverse of a square matrix"),
    ("transpose", "transpose(matrix)", "swap rows and columns"),
    ("rank", "rank(matrix)", "rank of a matrix"),
    ("eye", "eye(n, dtype=exact)", "n by n identity matrix"),
    ("gamma", "gamma(mu)", "the Dirac matrix γ^μ; use inside gamma_trace"),
    ("slash", "slash(p)", "the Dirac matrix p-slash for a momentum p (slash(p + q) is linear); use inside gamma_trace"),
    ("gamma_trace", "gamma_trace(a, b, ...)", "trace of a product of gamma(mu) and slash(p) matrices (items may be sums, e.g. slash(p) + m)"),
    ("metric", "metric(mu, nu)", "the metric tensor g^{mu nu}"),
    ("comp", "comp(p, mu)", "component p^mu of a four-vector"),
    ("pdot", "pdot(p, q)", "Lorentz product p.q of two four-vectors"),
    ("contract", "contract(expr)", "sum over repeated Lorentz indices (metric, comp); metric(mu, mu) is 4"),
    ("plot", "plot(f, (x, a, b)) | plot([f, g], (x, a, b))", "draw one or more curves"),
    ("help", "help() | help('name')", "list the functions, or describe one"),
];

const METHODS: &[&str] = &[
    "expand",
    "cancel",
    "factor",
    "simplify",
    "subs",
    "diff",
    "integrate",
    "limit",
    "apart",
    "nroots",
    "nsolve",
    "latex",
    "pycode",
    "rustcode",
    "series",
    "solve",
    "coeffs",
    "degree",
    "free_symbols",
    "det",
    "inv",
    "transpose",
    "rank",
    "N",
    "astype",
    "tolist",
    "shape",
    "dot",
    "norm",
];

pub fn resolve(name: &str) -> Option<&'static str> {
    DOCS.iter().find(|d| d.0 == name).map(|d| d.0)
}

pub fn resolve_method(name: &str) -> Option<&'static str> {
    METHODS.iter().find(|m| **m == name).and_then(|m| resolve(m))
}

pub fn unknown_function_message(name: &str) -> String {
    let near = DOCS.iter().map(|d| d.0).filter(|c| edit_distance(name, c) <= 2.min(name.len() / 2)).min_by_key(|c| edit_distance(name, c));
    match near {
        Some(n) => format!("'{name}' is not defined. Did you mean '{n}'?"),
        None => format!("'{name}' is not defined. If it is an unknown function, declare it first: {name} = symbols('{name}')"),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.get(j).copied().unwrap_or(0) + 1;
            cur.push(sub.min(del).min(ins));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0)
}

// ---- helpers -----------------------------------------------------------------------------------------------------

fn as_symbol(v: &Value) -> R<Expr> {
    match v {
        Value::Expr(e) if e.as_sym().is_some() && !e.is_constant_symbol() => Ok(e.clone()),
        other => err(format!("expected a symbol such as x, got {other}")),
    }
}

/// The only free symbol of `e`, for calls like `diff(x**2)`.
fn only_symbol(e: &Expr, what: &str) -> R<Expr> {
    let syms = e.free_symbols();
    let mut it = syms.iter();
    match (it.next(), it.next()) {
        (Some(s), None) => Ok(Expr::sym(s)),
        (None, _) => err(format!("{what}: the expression has no symbol to work with")),
        _ => err(format!("{what}: which variable? Name it, e.g. {what}(expr, x)")),
    }
}

/// Apply `f` to a number/formula, or to every element of a (nested) list.
fn broadcast(v: &Value, f: &mut dyn FnMut(&Expr) -> R<Expr>) -> R<Value> {
    match v {
        Value::List(l) | Value::Tuple(l) => Ok(Value::list(l.iter().map(|x| broadcast(x, f)).collect::<R<_>>()?)),
        Value::Array(a) => Ok(Value::Array(Arc::new(a.map(f)?))),
        other => match numeric(other) {
            Some(e) => Ok(Value::from_expr(f(&e)?)),
            None => err(format!("expected a number or formula, got a {}", other.type_name())),
        },
    }
}

fn unary_math(name: &str, a: &Args, it: &Interp<'_>) -> R<Value> {
    a.limits(name, 1, 1)?;
    let out = broadcast(a.get(0)?, &mut |e| Ok(Expr::call(name, vec![e.clone()])))?;
    check_depth(it, out)
}

fn check_depth(it: &Interp<'_>, v: Value) -> R<Value> {
    match &v {
        Value::Expr(e) => it.checked(e.clone()),
        _ => Ok(v),
    }
}

fn eq_to_zero_form(e: &Expr) -> Expr {
    match e.kind() {
        Kind::Eq(l, r) => l.sub(r),
        _ => e.clone(),
    }
}

fn rules_from(v: &Value, out: &mut Vec<(Expr, Expr)>) -> R<()> {
    match v {
        Value::Expr(e) => match e.kind() {
            Kind::Eq(l, r) => {
                out.push((l.clone(), r.clone()));
                Ok(())
            }
            _ => err("substitution rules are written as x == value, or (x, value)"),
        },
        Value::List(l) | Value::Tuple(l) => {
            if let [a, b] = l.as_slice()
                && !matches!(a, Value::List(_) | Value::Tuple(_))
                && !is_equation(a)
                && !is_equation(b)
            {
                out.push((a.to_expr()?, b.to_expr()?));
                return Ok(());
            }
            l.iter().try_for_each(|x| rules_from(x, out))
        }
        other => err(format!("cannot use a {} as a substitution rule", other.type_name())),
    }
}

fn is_equation(v: &Value) -> bool {
    matches!(v, Value::Expr(e) if matches!(e.kind(), Kind::Eq(..)))
}

fn floor_div(n: &IBig, d: &IBig) -> IBig {
    let q = n / d;
    if (n - &q * d) * d.clone() < IBig::ZERO { q - IBig::ONE } else { q }
}

/// Floor and ceiling of an exact or float number as an integer expression.
fn floor_ceil(e: &Expr, ceil: bool) -> R<Expr> {
    let Some(n) = e.as_num() else {
        if e.free_symbols().is_empty() {
            let v = eval_f64(e, &|_| None)?;
            return float_to_int(if ceil { v.ceil() } else { v.floor() });
        }
        return err("floor/ceil need a number");
    };
    match n.parts() {
        Some((p, q)) => {
            let d = IBig::from(q);
            let f = floor_div(&p, &d);
            Ok(Expr::big(if ceil && &f * &d != p { f + IBig::ONE } else { f }))
        }
        None => float_to_int(if ceil { n.to_f64().ceil() } else { n.to_f64().floor() }),
    }
}

fn float_to_int(v: f64) -> R<Expr> {
    if !v.is_finite() || v.abs() > 1e30 {
        return err("the number is too large to turn into an integer");
    }
    Ok(Expr::big(IBig::from(v as i128)))
}

fn parse_symbol_names(s: &str) -> R<Vec<String>> {
    let names: Vec<String> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()).map(str::to_string).collect();
    for n in &names {
        let mut chars = n.chars();
        let ok = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_');
        if !ok {
            return err(format!("'{n}' is not a valid symbol name"));
        }
        if matches!(n.as_str(), "E" | "pi" | "I") {
            return err(format!("'{n}' is already the constant {n}"));
        }
    }
    if names.is_empty() { err("symbols() needs at least one name") } else { Ok(names) }
}

fn plot_points(f: &Expr, x: &Expr, lo: f64, hi: f64) -> R<Vec<Vec<[f64; 2]>>> {
    const N: usize = 600;
    let name = x.as_sym().unwrap_or("x").to_string();
    let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for i in 0..=N {
        budget::tick()?;
        let t = lo + (hi - lo) * (i as f64) / (N as f64);
        match eval_f64(f, &|n| (n == name).then_some(t)) {
            Ok(y) if y.is_finite() && y.abs() < 1e12 => cur.push([t, y]),
            _ => {
                if !cur.is_empty() {
                    segments.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        segments.push(cur);
    }
    Ok(segments)
}

fn dtype_arg(a: &Args, at: usize) -> R<DType> {
    if let Some((k, _)) = a.kw.iter().find(|(k, _)| k != "dtype") {
        return err(format!("unknown keyword '{k}'; the only keyword is dtype="));
    }
    match a.opt(at, "dtype") {
        None => Ok(DType::Exact),
        Some(Value::DType(d)) => Ok(*d),
        Some(other) => err(format!("dtype must be one of exact, i8, i16, i32, i64, f32, f64, not {other}")),
    }
}

fn size_of(v: &Value) -> R<usize> {
    match numeric(v).and_then(|e| e.as_i64()).and_then(|n| usize::try_from(n).ok()) {
        Some(n) if n <= 100_000 => Ok(n),
        _ => err("a size must be a whole number between 0 and 100000"),
    }
}

fn matrix_arg(a: &Args) -> R<linalg::Matrix> {
    Ok(linalg::to_matrix(&a.expr(0)?)?)
}

// ---- dispatch ----------------------------------------------------------------------------------------------------

pub fn call(it: &mut Interp<'_>, name: &'static str, a: Args) -> R<Value> {
    budget::tick()?;
    match name {
        // ---- python builtins ----
        "print" => {
            let line = a.pos.iter().map(Value::to_text).collect::<Vec<_>>().join(" ");
            if it.stdout.len() + line.len() > 2_000_000 {
                return err("too much output");
            }
            it.stdout.push_str(&line);
            it.stdout.push('\n');
            Ok(Value::None)
        }
        "len" => {
            a.limits(name, 1, 1)?;
            let n = match a.get(0)? {
                Value::List(l) | Value::Tuple(l) => l.len(),
                Value::Str(s) => s.chars().count(),
                Value::Array(arr) => arr.rows,
                v @ Value::Range { .. } => it.iter_values(v)?.len(),
                other => return err(format!("a {} has no length", other.type_name())),
            };
            Ok(Value::int(i64::try_from(n).unwrap_or(i64::MAX)))
        }
        "range" => {
            a.limits(name, 1, 3)?;
            let (start, stop, step) = match a.pos.len() {
                1 => (0, a.int(0)?, 1),
                2 => (a.int(0)?, a.int(1)?, 1),
                _ => (a.int(0)?, a.int(1)?, a.int(2)?),
            };
            if step == 0 {
                return err("range() step cannot be zero");
            }
            Ok(Value::Range { start, stop, step })
        }
        "sum" | "prod" => {
            a.limits(name, 1, 2)?;
            let op = if name == "sum" { mathcraft_syntax::ast::BinOp::Add } else { mathcraft_syntax::ast::BinOp::Mul };
            let mut acc = match a.pos.get(1) {
                Some(v) => v.clone(),
                None => Value::int(i64::from(name == "prod")),
            };
            for v in it.iter_values(a.get(0)?)? {
                budget::tick()?;
                acc = it.binop(acc, op, v)?;
            }
            Ok(acc)
        }
        "min" | "max" => {
            let items = if a.pos.len() == 1 { it.iter_values(a.get(0)?)? } else { a.pos.clone() };
            let mut best: Option<(Expr, Value)> = None;
            for v in items {
                let Some(e) = numeric(&v) else { return err(format!("{name}() needs numbers")) };
                best = match best {
                    None => Some((e, v)),
                    Some((be, bv)) => {
                        let ord = order_numbers(&e, &be)?;
                        if (name == "min" && ord == Ordering::Less) || (name == "max" && ord == Ordering::Greater) { Some((e, v)) } else { Some((be, bv)) }
                    }
                };
            }
            best.map(|b| b.1).ok_or_else(|| EvalError::new(format!("{name}() of an empty sequence")))
        }
        "list" => {
            a.limits(name, 0, 1)?;
            match a.pos.first() {
                Some(v) => Ok(Value::list(it.iter_values(v)?)),
                None => Ok(Value::list(Vec::new())),
            }
        }
        "zip" => {
            let lists = a.pos.iter().map(|v| it.iter_values(v)).collect::<R<Vec<_>>>()?;
            let n = lists.iter().map(Vec::len).min().unwrap_or(0);
            Ok(Value::list((0..n).map(|i| Value::Tuple(Arc::new(lists.iter().filter_map(|l| l.get(i).cloned()).collect()))).collect()))
        }
        "enumerate" => {
            a.limits(name, 1, 1)?;
            let items = it.iter_values(a.get(0)?)?;
            Ok(Value::list(items.into_iter().enumerate().map(|(i, v)| Value::Tuple(Arc::new(vec![Value::int(i64::try_from(i).unwrap_or(0)), v]))).collect()))
        }
        "sorted" | "reversed" => {
            let mut items = it.iter_values(a.get(0)?)?;
            if name == "reversed" {
                items.reverse();
                return Ok(Value::list(items));
            }
            let mut failure = None;
            items.sort_by(|x, y| match (numeric(x), numeric(y), x, y) {
                (Some(p), Some(q), _, _) => order_numbers(&p, &q).unwrap_or_else(|e| {
                    failure = Some(e);
                    Ordering::Equal
                }),
                (_, _, Value::Str(p), Value::Str(q)) => p.cmp(q),
                _ => {
                    failure = Some(EvalError::new("sorted() needs numbers or strings"));
                    Ordering::Equal
                }
            });
            if let Some(e) = failure {
                return Err(e);
            }
            if let Some(v) = a.opt(1, "reverse")
                && it.truthy(v)?
            {
                items.reverse();
            }
            Ok(Value::list(items))
        }
        "any" | "all" => {
            a.limits(name, 1, 1)?;
            let mut result = name == "all";
            for v in it.iter_values(a.get(0)?)? {
                let t = it.truthy(&v)?;
                if name == "any" && t {
                    result = true;
                    break;
                }
                if name == "all" && !t {
                    result = false;
                    break;
                }
            }
            Ok(Value::Bool(result))
        }
        "format" => {
            a.limits(name, 1, 2)?;
            let spec = match a.pos.get(1) {
                Some(Value::Str(s)) => s.to_string(),
                Some(_) => return err("format() needs the spec as a string, like format(x, '.3f')"),
                None => String::new(),
            };
            Ok(Value::Str(Arc::from(crate::format::format_value(a.get(0)?, &spec)?.as_str())))
        }
        "repr" => {
            a.limits(name, 1, 1)?;
            Ok(Value::Str(Arc::from(a.get(0)?.to_string().as_str())))
        }
        "str" => {
            a.limits(name, 1, 1)?;
            Ok(Value::Str(Arc::from(a.get(0)?.to_text().as_str())))
        }
        "bool" => {
            a.limits(name, 1, 1)?;
            Ok(Value::Bool(it.truthy(a.get(0)?)?))
        }
        "float" => {
            a.limits(name, 1, 1)?;
            check_depth(it, broadcast(a.get(0)?, &mut |e| Ok(Expr::float(eval_f64(e, &|_| None)?)))?)
        }
        "int" => {
            a.limits(name, 1, 1)?;
            broadcast(a.get(0)?, &mut |e| match e.as_num().and_then(Num::parts) {
                Some((p, q)) => Ok(Expr::big(&p / IBig::from(q))),
                None => float_to_int(eval_f64(e, &|_| None)?.trunc()),
            })
        }
        "floor" | "ceil" => {
            a.limits(name, 1, 1)?;
            broadcast(a.get(0)?, &mut |e| floor_ceil(e, name == "ceil"))
        }
        "round" => {
            a.limits(name, 1, 2)?;
            let digits = a
                .opt(1, "digits")
                .map(|v| numeric(v).and_then(|e| e.as_i64()).ok_or_else(|| EvalError::new("round() digits must be an integer")))
                .transpose()?;
            broadcast(a.get(0)?, &mut |e| {
                let x = eval_f64(e, &|_| None)?;
                match digits {
                    None => {
                        let r = x.round();
                        // Python rounds half to even.
                        let r = if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - x.signum() } else { r };
                        float_to_int(r)
                    }
                    Some(d) => {
                        let m = 10f64.powi(i32::try_from(d).unwrap_or(0));
                        Ok(Expr::float((x * m).round() / m))
                    }
                }
            })
        }
        "factorial" => {
            a.limits(name, 1, 1)?;
            let n = a.int(0)?;
            if !(0..=20_000).contains(&n) {
                return err("factorial() needs an integer from 0 to 20000");
            }
            let mut acc = IBig::ONE;
            for k in 2..=n {
                budget::tick()?;
                acc *= IBig::from(k);
            }
            Ok(Value::Expr(Expr::big(acc)))
        }
        "binomial" => {
            a.limits(name, 2, 2)?;
            let (n, k) = (a.int(0)?, a.int(1)?);
            if n < 0 || k < 0 || k > n {
                return Ok(Value::int(0));
            }
            let k = k.min(n - k);
            let mut acc = IBig::ONE;
            for i in 0..k {
                budget::tick()?;
                acc = acc * IBig::from(n - i) / IBig::from(i + 1);
            }
            Ok(Value::Expr(Expr::big(acc)))
        }
        "gcd" | "lcm" => {
            a.limits(name, 2, 2)?;
            if let (Some(Value::Expr(p)), Some(Value::Expr(q))) = (a.pos.first(), a.pos.get(1))
                && !(p.as_i64().is_some() && q.as_i64().is_some())
            {
                let r = if name == "gcd" { alg::poly_gcd(p, q)? } else { alg::poly_lcm(p, q)? };
                return match r {
                    Some(e) => Ok(Value::Expr(e)),
                    None => err(format!("{name}() of expressions needs polynomials with integer coefficients")),
                };
            }
            let (mut x, mut y) = (i128::from(a.int(0)?).abs(), i128::from(a.int(1)?).abs());
            let product = x * y;
            while y != 0 {
                (x, y) = (y, x % y);
            }
            Ok(Value::Expr(Expr::big(IBig::from(if name == "gcd" {
                x
            } else if x == 0 {
                0
            } else {
                product / x
            }))))
        }
        // ---- elementary functions ----
        "abs" => {
            a.limits(name, 1, 1)?;
            let out = broadcast(a.get(0)?, &mut |e| if e.contains(&Expr::imag()) { Ok(alg::complex_abs(e)?) } else { Ok(Expr::call("abs", vec![e.clone()])) })?;
            check_depth(it, out)
        }
        "re" | "im" | "conjugate" => {
            a.limits(name, 1, 1)?;
            let out = broadcast(a.get(0)?, &mut |e| {
                Ok(match name {
                    "re" | "im" => {
                        let (re, im) = alg::complex_parts(e)?;
                        if name == "re" { re } else { im }
                    }
                    _ => alg::conjugate(e)?,
                })
            })?;
            check_depth(it, out)
        }
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "sqrt" => unary_math(name, &a, it),
        "log" => {
            a.limits(name, 1, 2)?;
            let base = a.pos.get(1).map(Value::to_expr).transpose()?;
            let out = broadcast(a.get(0)?, &mut |e| {
                let ln = Expr::call("log", vec![e.clone()]);
                Ok(match &base {
                    Some(b) => ln.div(&Expr::call("log", vec![b.clone()])),
                    None => ln,
                })
            })?;
            check_depth(it, out)
        }
        // ---- algebra ----
        "symbols" => {
            a.limits(name, 1, 1)?;
            let Value::Str(s) = a.get(0)? else { return err("symbols() takes a string such as 'x y z'") };
            let names = parse_symbol_names(s)?;
            let mut vals: Vec<Value> = names.iter().map(|n| Value::Expr(Expr::sym(n))).collect();
            if vals.len() == 1 { Ok(vals.swap_remove(0)) } else { Ok(Value::Tuple(Arc::new(vals))) }
        }
        "Eq" => {
            a.limits(name, 2, 2)?;
            Ok(Value::Expr(Expr::eq(a.expr(0)?, a.expr(1)?)))
        }
        "gamma" | "slash" | "comp" | "metric" | "pdot" => {
            let (lo, hi) = if matches!(name, "gamma" | "slash") { (1, 1) } else { (2, 2) };
            a.limits(name, lo, hi)?;
            let x = a.expr(0)?;
            Ok(Value::Expr(match name {
                "gamma" => amp::gamma(&x),
                "slash" => amp::slash(&x),
                _ => {
                    let y = a.expr(1)?;
                    match name {
                        "comp" => amp::comp(&x, &y),
                        "metric" => amp::metric(&x, &y),
                        _ => amp::pdot(&x, &y),
                    }
                }
            }))
        }
        "gamma_trace" => {
            let items = (0..a.pos.len()).map(|i| a.expr(i)).collect::<R<Vec<_>>>()?;
            check_depth(it, Value::Expr(amp::gamma_trace(&items)?))
        }
        "contract" => {
            a.limits(name, 1, 1)?;
            check_depth(it, Value::Expr(amp::contract(&a.expr(0)?)?))
        }
        "expand" | "simplify" | "factor" | "cancel" => {
            a.limits(name, 1, 1)?;
            let out = broadcast(a.get(0)?, &mut |e| {
                Ok(match name {
                    "expand" => alg::expand(e)?,
                    "simplify" => alg::simplify(e)?,
                    "cancel" => alg::cancel(e)?,
                    _ => alg::factor(e)?,
                })
            })?;
            check_depth(it, out)
        }
        "N" => {
            a.limits(name, 1, 1)?;
            check_depth(
                it,
                broadcast(a.get(0)?, &mut |e| {
                    let v = approximate(e)?;
                    // A complex result is shown as `re + im*I`.
                    if v.contains(&Expr::imag()) { Ok(alg::expand(&v)?) } else { Ok(v) }
                })?,
            )
        }
        "subs" => {
            a.limits(name, 2, 3)?;
            let mut rules = Vec::new();
            if a.pos.len() == 3 {
                rules.push((a.expr(1)?, a.expr(2)?));
            } else {
                rules_from(a.get(1)?, &mut rules)?;
            }
            let out = broadcast(a.get(0)?, &mut |e| Ok(alg::subs(e, &rules)?))?;
            check_depth(it, out)
        }
        "diff" => {
            if a.pos.is_empty() {
                return err("diff() needs an expression: diff(f, x)");
            }
            let f = a.expr(0)?;
            let mut cur = f.clone();
            if a.pos.len() == 1 {
                cur = alg::diff(&cur, &only_symbol(&f, "diff")?)?;
            }
            let mut last: Option<Expr> = None;
            for arg in a.pos.iter().skip(1) {
                if let Some(n) = numeric(arg).and_then(|e| e.as_i64()) {
                    let Some(x) = &last else { return err("diff(f, x, n): name the variable (a symbol such as x) before the order") };
                    if !(1..=50).contains(&n) {
                        return err("the derivative order must be between 1 and 50");
                    }
                    cur = alg::diff_n(&cur, x, u32::try_from(n - 1).unwrap_or(0))?;
                } else {
                    let x = as_symbol(arg)?;
                    cur = alg::diff(&cur, &x)?;
                    last = Some(x);
                }
            }
            it.checked(cur)
        }
        "integrate" => {
            if a.pos.is_empty() {
                return err("integrate() needs an expression: integrate(f, x)");
            }
            let mut cur = a.expr(0)?;
            let mut specs: Vec<Value> = a.pos.iter().skip(1).cloned().collect();
            if specs.is_empty() {
                specs.push(Value::Expr(only_symbol(&cur, "integrate")?));
            }
            for spec in &specs {
                match spec {
                    Value::Tuple(t) | Value::List(t) => {
                        let [x, lo, hi] = t.as_slice() else { return err("a definite integral is written integrate(f, (x, a, b))") };
                        cur = alg::integrate_definite(&cur, &as_symbol(x)?, &lo.to_expr()?, &hi.to_expr()?)?;
                    }
                    other => {
                        let x = as_symbol(other)?;
                        cur = match alg::integrate(&cur, &x)? {
                            Some(f) => f,
                            None => Expr::call("integrate", vec![cur, x]),
                        };
                    }
                }
            }
            it.checked(cur)
        }
        "limit" => {
            if a.pos.len() < 3 || a.pos.len() > 4 {
                return err("limit(f, x, a, dir='+-')");
            }
            let side = match a.opt(3, "dir") {
                None => alg::Side::Both,
                Some(Value::Str(s)) => match &**s {
                    "+" => alg::Side::Right,
                    "-" => alg::Side::Left,
                    "+-" => alg::Side::Both,
                    other => return err(format!("dir must be '+', '-' or '+-', not '{other}'")),
                },
                Some(_) => return err("dir must be the string '+', '-' or '+-'"),
            };
            it.checked(alg::limit_side(&a.expr(0)?, &as_symbol(a.get(1)?)?, &a.expr(2)?, side)?)
        }
        "latex" => {
            a.limits(name, 1, 1)?;
            match a.get(0)?.latex() {
                Some(s) => Ok(Value::Str(s.into())),
                None => err("latex() takes an expression, a list or a matrix"),
            }
        }
        "pycode" | "rustcode" => {
            if a.pos.is_empty() || a.pos.len() > 4 {
                return err(format!("{name}(expr, args=None, name='f'{})", if name == "pycode" { ", numpy=False" } else { "" }));
            }
            let body = a.expr(0)?;
            let args: Vec<Expr> = match a.opt(1, "args") {
                None | Some(Value::None) => {
                    let mut all: Vec<Expr> = body.free_symbols().iter().map(|s| Expr::sym(s)).filter(|s| !s.is_constant_symbol()).collect();
                    all.sort();
                    all
                }
                Some(Value::List(v) | Value::Tuple(v)) => v.iter().map(as_symbol).collect::<R<_>>()?,
                Some(single) => vec![as_symbol(single)?],
            };
            let fname = match a.opt(2, "name") {
                None => "f".to_owned(),
                Some(Value::Str(s)) => s.to_string(),
                Some(_) => return err("name must be a string"),
            };
            let numpy = matches!(a.opt(3, "numpy"), Some(Value::Bool(true)));
            let target = if name == "rustcode" {
                mathcraft_core::codegen::Target::Rust
            } else if numpy {
                mathcraft_core::codegen::Target::NumPy
            } else {
                mathcraft_core::codegen::Target::Python
            };
            let code = mathcraft_core::codegen::function(&body, &args, &fname, target)?;
            Ok(Value::Str(code.into()))
        }
        "nsolve" => {
            a.limits(name, 3, 3)?;
            let f = eq_to_zero_form(&a.expr(0)?);
            let x = as_symbol(a.get(1)?)?;
            let df = alg::diff(&f, &x)?;
            let at = |e: &Expr, v: f64| eval_f64(e, &|n| (Some(n) == x.as_sym()).then_some(v));
            let mut v = eval_f64(&a.expr(2)?, &|_| None)?;
            for _ in 0..100 {
                it.checked(Expr::zero())?;
                let (fv, dv) = (at(&f, v)?, at(&df, v)?);
                if dv == 0.0 || !dv.is_finite() || !fv.is_finite() {
                    return err("nsolve: the derivative vanished; try another starting guess");
                }
                let step = fv / dv;
                v -= step;
                if step.abs() <= 1e-14 * (1.0 + v.abs()) {
                    return Ok(Value::Expr(Expr::float(v)));
                }
            }
            err("nsolve did not converge; try another starting guess")
        }
        "apart" => {
            a.limits(name, 1, 2)?;
            let e = a.expr(0)?;
            let x = match a.pos.get(1) {
                Some(v) => as_symbol(v)?,
                None => only_symbol(&e, "apart")?,
            };
            it.checked(alg::apart(&e, &x)?)
        }
        "series" => {
            if a.pos.len() < 2 || a.pos.len() > 4 {
                return err("series(f, x, x0=0, n=6)");
            }
            let x0 = a.opt(2, "x0").map(Value::to_expr).transpose()?.unwrap_or_else(Expr::zero);
            let n = a
                .opt(3, "n")
                .map(|v| numeric(v).and_then(|e| e.as_i64()).ok_or_else(|| EvalError::new("the series order must be an integer")))
                .transpose()?
                .unwrap_or(6);
            let order = u32::try_from(n).map_err(|_| EvalError::new("the series order must not be negative"))?;
            let s = alg::series(&a.expr(0)?, &as_symbol(a.get(1)?)?, &x0, order)?;
            it.checked(alg::expand(&s)?)
        }
        "solve" => solve(it, &a),
        "nroots" => {
            a.limits(name, 1, 2)?;
            let f = eq_to_zero_form(&a.expr(0)?);
            let x = match a.pos.get(1) {
                Some(v) => as_symbol(v)?,
                None => only_symbol(&f, name)?,
            };
            Ok(Value::list(alg::nroots(&f, &x)?.into_iter().map(Value::from_expr).collect()))
        }
        "coeffs" | "degree" => {
            a.limits(name, 1, 2)?;
            let f = a.expr(0)?;
            let x = match a.pos.get(1) {
                Some(v) => as_symbol(v)?,
                None => only_symbol(&f, name)?,
            };
            let Some(c) = alg::to_poly(&f, &x)? else { return err("this is not a polynomial in that variable") };
            if name == "degree" {
                return Ok(Value::int(i64::try_from(c.len().saturating_sub(1)).unwrap_or(0)));
            }
            Ok(Value::list(c.into_iter().map(Value::Expr).collect()))
        }
        "free_symbols" => {
            a.limits(name, 1, 1)?;
            Ok(Value::list(a.expr(0)?.free_symbols().iter().map(|s| Value::Expr(Expr::sym(s))).collect()))
        }
        // ---- linear algebra ----
        "det" | "inv" | "transpose" | "rank" if matches!(a.pos.first(), Some(Value::Array(_))) => {
            a.limits(name, 1, 1)?;
            let Value::Array(arr) = a.get(0)? else { return err("expected a matrix") };
            match name {
                "det" => arr.det(),
                "inv" => Ok(Value::Array(Arc::new(arr.inverse()?))),
                "transpose" => Ok(Value::Array(Arc::new(arr.transpose()))),
                _ => Ok(Value::int(i64::try_from(arr.rank()?).unwrap_or(0))),
            }
        }
        "vec" | "matrix" => {
            let dt = dtype_arg(&a, 1)?;
            if a.pos.len() != 1 {
                return err(format!("{name}() takes the items and an optional dtype, e.g. {name}(..., dtype=f64)"));
            }
            Ok(Value::Array(Arc::new(array::build(dt, a.get(0)?, name == "matrix")?)))
        }
        "zeros" | "ones" | "eye" => {
            let dt = dtype_arg(&a, 1)?;
            if a.pos.len() != 1 {
                return err(format!("{name}() takes a size and an optional dtype, e.g. {name}(3, dtype=f64)"));
            }
            let (rows, cols) = match a.get(0)? {
                Value::Tuple(t) | Value::List(t) => match t.as_slice() {
                    [r, c] => (size_of(r)?, Some(size_of(c)?)),
                    _ => return err("a shape is (rows, columns)"),
                },
                v => (size_of(v)?, if name == "eye" { Some(size_of(v)?) } else { None }),
            };
            let one = i64::from(name != "zeros");
            Ok(Value::Array(Arc::new(array::filled(dt, rows, cols, &|i, j| if name == "eye" { i64::from(i == j) } else { one })?)))
        }
        "astype" => {
            a.limits(name, 2, 2)?;
            let (Value::Array(arr), Value::DType(d)) = (a.get(0)?, a.get(1)?) else {
                return err("astype(a, dtype) converts a vector or matrix, e.g. astype(A, f64)");
            };
            Ok(Value::Array(Arc::new(arr.cast(*d)?)))
        }
        "tolist" => {
            a.limits(name, 1, 1)?;
            match a.get(0)? {
                Value::Array(arr) => Ok(arr.to_list()),
                other => Ok(other.clone()),
            }
        }
        "shape" => {
            a.limits(name, 1, 1)?;
            let Value::Array(arr) = a.get(0)? else { return err("shape() takes a vector or matrix") };
            Ok(Value::Tuple(Arc::new(arr.shape().into_iter().map(|n| Value::int(i64::try_from(n).unwrap_or(0))).collect())))
        }
        "dot" | "norm" => {
            let want = if name == "dot" { 2 } else { 1 };
            a.limits(name, want, want)?;
            let (Value::Array(x), y) = (a.get(0)?, a.pos.get(1)) else { return err(format!("{name}() takes vectors; make one with vec([...])")) };
            if x.is_matrix() {
                return err(format!("{name}() takes vectors, not a matrix"));
            }
            let y = match y {
                Some(Value::Array(y)) if !y.is_matrix() => y.clone(),
                Some(_) => return err("dot() takes two vectors"),
                None => x.clone(),
            };
            let d = x.matmul(&y)?;
            if name == "dot" {
                return Ok(d);
            }
            match d {
                Value::Expr(e) => it.checked(Expr::pow(&e, &Expr::half())),
                other => Ok(other),
            }
        }
        "det" => {
            a.limits(name, 1, 1)?;
            it.checked(linalg::det(&matrix_arg(&a)?)?)
        }
        "inv" => {
            a.limits(name, 1, 1)?;
            it.checked(linalg::from_matrix(&linalg::inverse(&matrix_arg(&a)?)?))
        }
        "transpose" => {
            a.limits(name, 1, 1)?;
            it.checked(linalg::from_matrix(&linalg::transpose(&matrix_arg(&a)?)))
        }
        "rank" => {
            a.limits(name, 1, 1)?;
            Ok(Value::int(i64::try_from(linalg::rank(&matrix_arg(&a)?)?).unwrap_or(0)))
        }
        "plot" => plot(&a),
        "help" => {
            a.limits(name, 0, 1)?;
            let text = match a.pos.first() {
                Some(Value::Str(n)) => match DOCS.iter().find(|d| d.0 == &**n) {
                    Some((_, sig, summary)) => format!("{sig}\n    {summary}"),
                    None => return err(unknown_function_message(n)),
                },
                Some(Value::Builtin(n)) => DOCS.iter().find(|d| d.0 == *n).map(|(_, sig, summary)| format!("{sig}\n    {summary}")).unwrap_or_default(),
                Some(_) => return err("help('name') takes the name of a function"),
                None => DOCS.iter().map(|(_, sig, summary)| format!("{sig:<52} {summary}")).collect::<Vec<_>>().join("\n"),
            };
            it.stdout.push_str(&text);
            it.stdout.push('\n');
            Ok(Value::None)
        }
        other => err(format!("'{other}' is not implemented")),
    }
}

fn solve(it: &Interp<'_>, a: &Args) -> R<Value> {
    a.limits("solve", 1, 2)?;
    match a.get(0)? {
        Value::List(items) | Value::Tuple(items) => {
            let eqs = items.iter().map(Value::to_expr).collect::<R<Vec<_>>>()?;
            let vars: Vec<Expr> = match a.pos.get(1) {
                Some(Value::List(v) | Value::Tuple(v)) => v.iter().map(as_symbol).collect::<R<_>>()?,
                Some(single) => vec![as_symbol(single)?],
                None => {
                    let mut all = std::collections::BTreeSet::new();
                    for e in &eqs {
                        all.extend(e.free_symbols());
                    }
                    all.iter().map(|s| Expr::sym(s)).collect()
                }
            };
            match linalg::solve_linear(&eqs, &vars) {
                Ok(Some(sol)) => Ok(Value::list(sol.into_iter().map(Value::Expr).collect())),
                Ok(None) => Ok(Value::list(Vec::new())),
                // not linear: eliminate variables; the answer is a list of solutions, each a list of `var == value`
                Err(e) if e.to_string().contains("only linear systems") => {
                    let sols = alg::solve_system(&eqs, &vars)?;
                    Ok(Value::list(
                        sols.into_iter().map(|s| Value::list(vars.iter().zip(s).map(|(v, val)| Value::Expr(Expr::eq(v.clone(), val))).collect())).collect(),
                    ))
                }
                Err(e) => Err(e.into()),
            }
        }
        single => {
            let eq = single.to_expr()?;
            let f = eq_to_zero_form(&eq);
            let x = match a.pos.get(1) {
                Some(v) => as_symbol(v)?,
                None => only_symbol(&f, "solve")?,
            };
            let roots = alg::solve(&f, &x)?;
            roots.into_iter().map(|r| it.checked(r)).collect::<R<Vec<_>>>().map(Value::list)
        }
    }
}

fn plot(a: &Args) -> R<Value> {
    if a.pos.is_empty() || a.pos.len() > 2 {
        return err("plot(f, (x, a, b)) draws f for x from a to b");
    }
    let curves: Vec<Expr> = match a.get(0)? {
        Value::List(l) => l.iter().map(Value::to_expr).collect::<R<_>>()?,
        single => vec![single.to_expr()?],
    };
    let (x, lo, hi) = match a.pos.get(1) {
        Some(Value::Tuple(t) | Value::List(t)) => {
            let [x, lo, hi] = t.as_slice() else { return err("the range is written (x, a, b)") };
            let (lo, hi) = (eval_f64(&lo.to_expr()?, &|_| None)?, eval_f64(&hi.to_expr()?, &|_| None)?);
            (as_symbol(x)?, lo, hi)
        }
        Some(_) => return err("the range is written (x, a, b)"),
        None => {
            let first = curves.first().ok_or_else(|| EvalError::new("nothing to plot"))?;
            (only_symbol(first, "plot")?, -10.0, 10.0)
        }
    };
    if lo.partial_cmp(&hi) != Some(Ordering::Less) || !lo.is_finite() || !hi.is_finite() {
        return err("the plot range must satisfy a < b");
    }
    let mut series = Vec::new();
    for c in &curves {
        if let Some(extra) = c.free_symbols().iter().find(|s| Some(&***s) != x.as_sym()) {
            return err(format!("cannot plot: '{extra}' has no value (plot only varies {x})"));
        }
        series.push(PlotSeries { label: c.to_string(), segments: plot_points(c, &x, lo, hi)? });
    }
    Ok(Value::Plot(Arc::new(PlotSpec { series, x_label: x.to_string() })))
}
