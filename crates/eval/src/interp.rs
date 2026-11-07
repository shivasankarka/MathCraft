//! The interpreter: statements, expressions, functions and control flow.
//!
//! A cell runs against a read-only view of the notebook's global variables and collects its own assignments in an
//! overlay (`top`); the overlay is committed only if the whole cell succeeds, so a failing cell leaves no half-done state.

use crate::builtins::{self, Args};
use crate::value::{EvalError, FuncBody, Function, R, Value, err};
use dashu::integer::IBig;
use mathcraft_algebra::linalg;
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, Num, budget};
use mathcraft_syntax::ast::{BinOp, CmpOp, Comp, Ex, Param, Stmt, StmtKind, Target, UnOp};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

pub const MAX_CALL_DEPTH: usize = 120;
/// Results nested deeper than this are refused, which keeps every later recursive algorithm safe.
pub const MAX_EXPR_DEPTH: u32 = 1000;
const MAX_SEQUENCE: usize = 2_000_000;

enum Flow {
    Next,
    Break,
    Continue,
    Return(Value),
}

struct Frame {
    locals: HashMap<String, Value>,
    captured: HashMap<String, Value>,
}

pub struct Interp<'g> {
    base: &'g HashMap<String, Value>,
    pub(crate) top: HashMap<String, Value>,
    frames: Vec<Frame>,
    pub(crate) stdout: String,
    pub(crate) strict: bool,
}

/// What running a piece of code produced. Assignments are committed to the globals only when `result` is `Ok`.
#[derive(Debug)]
pub struct Outcome {
    pub stdout: String,
    pub result: Result<Option<Value>, EvalError>,
    /// Names this run committed to the globals (empty when it failed).
    pub assigned: Vec<String>,
}

/// Run parsed statements against `globals`.
pub fn run(globals: &mut HashMap<String, Value>, stmts: &[Stmt], strict: bool) -> Outcome {
    let (top, stdout, result) = {
        let mut it = Interp { base: &*globals, top: HashMap::new(), frames: Vec::new(), stdout: String::new(), strict };
        let result = it.run_top(stmts);
        (std::mem::take(&mut it.top), std::mem::take(&mut it.stdout), result)
    };
    let mut assigned = Vec::new();
    if result.is_ok() {
        assigned = top.keys().cloned().collect();
        assigned.sort();
        globals.extend(top);
    }
    Outcome { stdout, result, assigned }
}

/// Parse and run source text.
pub fn run_source(globals: &mut HashMap<String, Value>, source: &str, strict: bool) -> Outcome {
    match mathcraft_syntax::parse(source) {
        Ok(stmts) => run(globals, &stmts, strict),
        Err(e) => Outcome { stdout: String::new(), result: Err(EvalError { line: Some(e.line), message: e.message }), assigned: Vec::new() },
    }
}

fn int_literal(s: &str) -> R<Expr> {
    s.parse::<IBig>().map(Expr::big).map_err(|_| EvalError::new(format!("invalid integer literal {s}")))
}

impl Interp<'_> {
    // ---- scopes ------------------------------------------------------------------------------------------------

    fn lookup(&self, name: &str) -> Option<Value> {
        if let Some(fr) = self.frames.last()
            && let Some(v) = fr.locals.get(name).or_else(|| fr.captured.get(name))
        {
            return Some(v.clone());
        }
        self.top.get(name).or_else(|| self.base.get(name)).cloned()
    }

    fn set(&mut self, name: &str, v: Value) {
        match self.frames.last_mut() {
            Some(fr) => fr.locals.insert(name.to_string(), v),
            None => self.top.insert(name.to_string(), v),
        };
    }

    fn eval_name(&mut self, name: &str) -> R<Value> {
        if let Some(v) = self.lookup(name) {
            return Ok(v);
        }
        match name {
            "pi" => return Ok(Value::Expr(Expr::pi())),
            "E" => return Ok(Value::Expr(Expr::e())),
            _ if crate::array::DType::parse(name).is_some() => return Ok(crate::array::DType::parse(name).map_or(Value::None, Value::DType)),
            "I" => return Ok(Value::Expr(Expr::imag())),
            "oo" => return Ok(Value::Expr(Expr::infinity())),
            _ => {}
        }
        if let Some(b) = builtins::resolve(name) {
            return Ok(Value::Builtin(b));
        }
        if self.strict {
            return err(format!("name '{name}' is not defined (declare it with symbols('{name}'))"));
        }
        Ok(Value::Expr(Expr::sym(name)))
    }

    // ---- statements --------------------------------------------------------------------------------------------

    fn run_top(&mut self, stmts: &[Stmt]) -> R<Option<Value>> {
        let mut last = None;
        for (i, s) in stmts.iter().enumerate() {
            budget::tick().map_err(|e| EvalError::from(e).at(s.line))?;
            if let (StmtKind::Expr(e), true) = (&s.kind, i + 1 == stmts.len()) {
                last = Some(self.eval(e).map_err(|e| e.at(s.line))?);
                continue;
            }
            match self.exec(s).map_err(|e| e.at(s.line))? {
                Flow::Next => {}
                Flow::Return(_) => return Err(EvalError::new("`return` outside a function").at(s.line)),
                Flow::Break | Flow::Continue => return Err(EvalError::new("`break`/`continue` outside a loop").at(s.line)),
            }
        }
        Ok(last.filter(|v| *v != Value::None))
    }

    fn block(&mut self, stmts: &[Stmt]) -> R<Flow> {
        for s in stmts {
            budget::tick().map_err(|e| EvalError::from(e).at(s.line))?;
            match self.exec(s).map_err(|e| e.at(s.line))? {
                Flow::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    fn exec(&mut self, s: &Stmt) -> R<Flow> {
        match &s.kind {
            StmtKind::Expr(e) => {
                self.eval(e)?;
            }
            StmtKind::Assign { targets, value } => {
                let v = self.eval(value)?;
                for t in targets {
                    self.bind(t, v.clone())?;
                }
            }
            StmtKind::AugAssign { target, op, value } => {
                let Target::Name(name) = target else { return err("augmented assignment needs a single name") };
                let cur = self.eval_name(name)?;
                let rhs = self.eval(value)?;
                let new = self.binop(cur, *op, rhs)?;
                self.set(name, new);
            }
            StmtKind::Def { name, params, body } => {
                let f = self.make_function(name, params, FuncBody::Block(body.clone()))?;
                self.set(name, f);
            }
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::None,
                };
                return Ok(Flow::Return(v));
            }
            StmtKind::If { test, body, orelse } => {
                let t = self.eval(test)?;
                return if self.truthy(&t)? { self.block(body) } else { self.block(orelse) };
            }
            StmtKind::For { target, iter, body } => {
                let it = self.eval(iter)?;
                if let Value::Range { start, stop, step } = it {
                    let mut i = start;
                    while (step > 0 && i < stop) || (step < 0 && i > stop) {
                        budget::tick()?;
                        self.bind(target, Value::int(i))?;
                        match self.block(body)? {
                            Flow::Break => break,
                            Flow::Return(v) => return Ok(Flow::Return(v)),
                            Flow::Next | Flow::Continue => {}
                        }
                        i = i.saturating_add(step);
                    }
                } else {
                    for v in self.iter_values(&it)? {
                        budget::tick()?;
                        self.bind(target, v)?;
                        match self.block(body)? {
                            Flow::Break => break,
                            Flow::Return(v) => return Ok(Flow::Return(v)),
                            Flow::Next | Flow::Continue => {}
                        }
                    }
                }
            }
            StmtKind::While { test, body } => loop {
                budget::tick()?;
                let t = self.eval(test)?;
                if !self.truthy(&t)? {
                    break;
                }
                match self.block(body)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Next | Flow::Continue => {}
                }
            },
            StmtKind::Pass => {}
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
        }
        Ok(Flow::Next)
    }

    fn bind(&mut self, t: &Target, v: Value) -> R<()> {
        match t {
            Target::Name(n) => {
                self.set(n, v);
                Ok(())
            }
            Target::Tuple(ts) => {
                let items = self.iter_values(&v)?;
                if items.len() != ts.len() {
                    return err(format!("cannot unpack {} values into {} names", items.len(), ts.len()));
                }
                for (t, v) in ts.iter().zip(items) {
                    self.bind(t, v)?;
                }
                Ok(())
            }
        }
    }

    fn make_function(&mut self, name: &str, params: &[Param], body: FuncBody) -> R<Value> {
        let defaults = params.iter().map(|p| p.default.as_ref().map(|d| self.eval(d)).transpose()).collect::<R<Vec<_>>>()?;
        let captured = match self.frames.last() {
            Some(fr) => fr.captured.iter().chain(fr.locals.iter()).map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => HashMap::new(),
        };
        Ok(Value::Func(Arc::new(Function { name: name.to_string(), params: params.to_vec(), defaults, body, captured })))
    }

    // ---- expressions -------------------------------------------------------------------------------------------

    pub(crate) fn eval(&mut self, e: &Ex) -> R<Value> {
        budget::tick()?;
        match e {
            Ex::Int(s) => Ok(Value::Expr(int_literal(s)?)),
            Ex::Float(f) => Ok(Value::Expr(Expr::float(*f))),
            Ex::Str(s) => Ok(Value::Str(Arc::from(s.as_str()))),
            Ex::Bool(b) => Ok(Value::Bool(*b)),
            Ex::None => Ok(Value::None),
            Ex::Name(n) => self.eval_name(n),
            Ex::Bin(a, op, b) => {
                let x = self.eval(a)?;
                let y = self.eval(b)?;
                self.binop(x, *op, y)
            }
            Ex::Unary(op, x) => {
                let v = self.eval(x)?;
                match op {
                    UnOp::Not => Ok(Value::Bool(!self.truthy(&v)?)),
                    UnOp::Neg if matches!(v, Value::Array(_)) => match &v {
                        Value::Array(a) => Ok(Value::Array(Arc::new(a.neg()?))),
                        _ => err("bad operand for unary -"),
                    },
                    UnOp::Pos if matches!(v, Value::Array(_)) => Ok(v),
                    UnOp::Pos => match numeric(&v) {
                        Some(e) => Ok(Value::Expr(e)),
                        None => err(format!("bad operand for unary +: {}", v.type_name())),
                    },
                    UnOp::Neg => match numeric(&v) {
                        Some(e) => self.checked(e.neg()),
                        None => err(format!("bad operand for unary -: {}", v.type_name())),
                    },
                }
            }
            Ex::Compare(first, rest) => {
                let mut left = self.eval(first)?;
                let mut result = Value::Bool(true);
                for (op, rhs) in rest {
                    let right = self.eval(rhs)?;
                    result = self.compare(&left, *op, &right)?;
                    // Only a chain like `a < b < c` needs each link decided; a lone `x == 1` may stay an equation.
                    if rest.len() > 1 && !self.truthy(&result)? {
                        return Ok(result);
                    }
                    left = right;
                }
                Ok(result)
            }
            Ex::And(vs) => {
                let mut last = Value::Bool(true);
                for v in vs {
                    last = self.eval(v)?;
                    if !self.truthy(&last)? {
                        return Ok(last);
                    }
                }
                Ok(last)
            }
            Ex::Or(vs) => {
                let mut last = Value::Bool(false);
                for v in vs {
                    last = self.eval(v)?;
                    if self.truthy(&last)? {
                        return Ok(last);
                    }
                }
                Ok(last)
            }
            Ex::Call { func, args, kwargs } => self.eval_call(func, args, kwargs),
            Ex::Attr(..) => err("attributes are not supported; call a method like expr.expand() or use a function like expand(expr)"),
            Ex::Index(v, i) => {
                let v = self.eval(v)?;
                let i = self.eval(i)?;
                self.index(&v, &i)
            }
            Ex::List(xs) => Ok(Value::list(xs.iter().map(|x| self.eval(x)).collect::<R<_>>()?)),
            Ex::Tuple(xs) => Ok(Value::Tuple(Arc::new(xs.iter().map(|x| self.eval(x)).collect::<R<_>>()?))),
            Ex::Lambda { params, body } => self.make_function("<lambda>", params, FuncBody::Lambda((**body).clone())),
            Ex::IfExp { test, body, orelse } => {
                let t = self.eval(test)?;
                if self.truthy(&t)? { self.eval(body) } else { self.eval(orelse) }
            }
            Ex::ListComp { elt, gens } => {
                let captured = match self.frames.last() {
                    Some(fr) => fr.captured.iter().chain(fr.locals.iter()).map(|(k, v)| (k.clone(), v.clone())).collect(),
                    None => HashMap::new(),
                };
                self.frames.push(Frame { locals: HashMap::new(), captured });
                let mut out = Vec::new();
                let r = self.comprehend(gens, elt, &mut out);
                self.frames.pop();
                r?;
                Ok(Value::list(out))
            }
        }
    }

    fn comprehend(&mut self, gens: &[Comp], elt: &Ex, out: &mut Vec<Value>) -> R<()> {
        let Some((g, rest)) = gens.split_first() else {
            out.push(self.eval(elt)?);
            if out.len() > MAX_SEQUENCE {
                return err("list is too long");
            }
            return Ok(());
        };
        let it = self.eval(&g.iter)?;
        'items: for v in self.iter_values(&it)? {
            budget::tick()?;
            self.bind(&g.target, v)?;
            for cond in &g.ifs {
                let c = self.eval(cond)?;
                if !self.truthy(&c)? {
                    continue 'items;
                }
            }
            self.comprehend(rest, elt, out)?;
        }
        Ok(())
    }

    pub(crate) fn truthy(&self, v: &Value) -> R<bool> {
        Ok(match v {
            Value::Bool(b) => *b,
            Value::None => false,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) | Value::Tuple(l) => !l.is_empty(),
            Value::Range { start, stop, step } => (*step > 0 && start < stop) || (*step < 0 && start > stop),
            Value::Expr(e) => match e.kind() {
                Kind::Num(n) => !n.is_zero(),
                Kind::Eq(..) => return err("an equation can't be used as a condition; solve it or compare numbers"),
                _ if e.free_symbols().is_empty() => eval_f64(e, &|_| None).map(|x| x != 0.0).unwrap_or(true),
                _ => return err(format!("cannot decide whether `{e}` is true or false because it contains symbols")),
            },
            Value::Func(_) | Value::Builtin(_) | Value::Plot(_) | Value::DType(_) => true,
            Value::Array(_) => return err("the truth value of an array is ambiguous; compare it with == or test its elements"),
        })
    }

    pub(crate) fn iter_values(&mut self, v: &Value) -> R<Vec<Value>> {
        match v {
            Value::List(l) | Value::Tuple(l) => Ok(l.as_ref().clone()),
            Value::Str(s) => Ok(s.chars().map(|c| Value::Str(Arc::from(c.to_string().as_str()))).collect()),
            Value::Array(a) => Ok((0..a.rows).filter_map(|i| a.index(i)).collect()),
            Value::Range { start, stop, step } => {
                let mut out = Vec::new();
                let mut i = *start;
                while (*step > 0 && i < *stop) || (*step < 0 && i > *stop) {
                    budget::tick()?;
                    out.push(Value::int(i));
                    if out.len() > MAX_SEQUENCE {
                        return err("range is too long to turn into a list");
                    }
                    i = i.saturating_add(*step);
                }
                Ok(out)
            }
            other => err(format!("a {} is not iterable", other.type_name())),
        }
    }

    fn index(&self, v: &Value, i: &Value) -> R<Value> {
        if let (Value::Array(a), Value::Tuple(ij)) = (v, i) {
            let pair = match ij.as_slice() {
                [r, c] => numeric(r).and_then(|e| e.as_i64()).zip(numeric(c).and_then(|e| e.as_i64())),
                _ => None,
            };
            let (Some((r, c)), Some(cols)) = (pair, a.cols) else { return err("a matrix is indexed A[row, column]; a vector has a single index") };
            let fix = |k: i64, n: usize| usize::try_from(if k < 0 { k + i64::try_from(n).unwrap_or(0) } else { k }).ok().filter(|x| *x < n);
            let (Some(r), Some(c)) = (fix(r, a.rows), fix(c, cols)) else { return err(format!("index out of range for a {}", a.describe())) };
            return Ok(a.index2(r, c).map_or(Value::None, Value::from_expr));
        }
        let Some(idx) = numeric(i).and_then(|e| e.as_i64()) else { return err("an index must be an integer") };
        let pick = |len: usize| -> R<usize> {
            let n = i64::try_from(len).unwrap_or(i64::MAX);
            let real = if idx < 0 { idx + n } else { idx };
            usize::try_from(real).ok().filter(|r| *r < len).ok_or_else(|| EvalError::new(format!("index {idx} is out of range (length {len})")))
        };
        match v {
            Value::List(l) | Value::Tuple(l) => Ok(l.get(pick(l.len())?).cloned().unwrap_or(Value::None)),
            Value::Array(a) => Ok(a.index(pick(a.rows)?).unwrap_or(Value::None)),
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                Ok(Value::Str(Arc::from(chars.get(pick(chars.len())?).map(char::to_string).unwrap_or_default().as_str())))
            }
            Value::Range { start, stop, step } => {
                let len = if *step > 0 { (stop - start + step - 1).max(0) / step } else { (start - stop - step - 1).max(0) / -step };
                let k = pick(usize::try_from(len).unwrap_or(0))?;
                Ok(Value::int(start + step * i64::try_from(k).unwrap_or(0)))
            }
            other => err(format!("a {} cannot be indexed", other.type_name())),
        }
    }

    // ---- calls -------------------------------------------------------------------------------------------------

    fn eval_call(&mut self, func: &Ex, args: &[Ex], kwargs: &[(String, Ex)]) -> R<Value> {
        let argv = args.iter().map(|a| self.eval(a)).collect::<R<Vec<_>>>()?;
        let kw = kwargs.iter().map(|(k, v)| Ok((k.clone(), self.eval(v)?))).collect::<R<Vec<_>>>()?;
        match func {
            Ex::Name(n) => {
                if let Some(v) = self.lookup(n) {
                    return self.call_value(v, Some(n), argv, kw);
                }
                if let Some(b) = builtins::resolve(n) {
                    return builtins::call(self, b, Args { pos: argv, kw });
                }
                err(builtins::unknown_function_message(n))
            }
            Ex::Attr(obj, method) => {
                let o = self.eval(obj)?;
                let Some(b) = builtins::resolve_method(method) else { return err(format!("a {} has no method '{method}'", o.type_name())) };
                let mut all = vec![o];
                all.extend(argv);
                builtins::call(self, b, Args { pos: all, kw })
            }
            other => {
                let f = self.eval(other)?;
                self.call_value(f, None, argv, kw)
            }
        }
    }

    pub(crate) fn call_value(&mut self, f: Value, name: Option<&str>, args: Vec<Value>, kw: Vec<(String, Value)>) -> R<Value> {
        match f {
            Value::Func(func) => self.call_function(&func, args, kw),
            Value::Builtin(b) => builtins::call(self, b, Args { pos: args, kw }),
            // A declared symbol applied to arguments is an unknown function: f = symbols('f'); f(x)
            Value::Expr(e) if e.as_sym().is_some() && name == e.as_sym() => {
                if !kw.is_empty() {
                    return err("a symbolic function does not take keyword arguments");
                }
                let a = args.iter().map(Value::to_expr).collect::<R<Vec<_>>>()?;
                self.checked(Expr::call(&e.to_string(), a))
            }
            other => err(format!("a {} is not callable", other.type_name())),
        }
    }

    pub(crate) fn call_function(&mut self, f: &Arc<Function>, args: Vec<Value>, kw: Vec<(String, Value)>) -> R<Value> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            return err("maximum recursion depth exceeded");
        }
        if args.len() > f.params.len() {
            return err(format!("{}() takes {} argument(s) but {} were given", f.name, f.params.len(), args.len()));
        }
        let mut locals: HashMap<String, Value> = HashMap::new();
        if f.name != "<lambda>" {
            locals.insert(f.name.clone(), Value::Func(f.clone()));
        }
        let mut positional = args.into_iter();
        let mut kwargs = kw;
        for (p, default) in f.params.iter().zip(f.defaults.iter()) {
            let value = if let Some(v) = positional.next() {
                v
            } else if let Some(pos) = kwargs.iter().position(|(k, _)| *k == p.name) {
                kwargs.swap_remove(pos).1
            } else if let Some(d) = default {
                d.clone()
            } else {
                return err(format!("{}() is missing the argument '{}'", f.name, p.name));
            };
            locals.insert(p.name.clone(), value);
        }
        if let Some((k, _)) = kwargs.first() {
            return err(format!("{}() got an unexpected argument '{k}'", f.name));
        }
        self.frames.push(Frame { locals, captured: f.captured.clone() });
        let result = match &f.body {
            FuncBody::Block(stmts) => self.block(stmts).map(|flow| match flow {
                Flow::Return(v) => v,
                _ => Value::None,
            }),
            FuncBody::Lambda(e) => self.eval(e),
        };
        self.frames.pop();
        result
    }

    // ---- operators ---------------------------------------------------------------------------------------------

    pub(crate) fn checked(&self, e: Expr) -> R<Value> {
        if e.depth() > MAX_EXPR_DEPTH { err("the expression became too deeply nested") } else { Ok(Value::from_expr(e)) }
    }

    pub(crate) fn binop(&mut self, a: Value, op: BinOp, b: Value) -> R<Value> {
        if matches!(a, Value::Array(_)) || matches!(b, Value::Array(_)) {
            return crate::array::binop(&a, op, &b, &numeric);
        }
        if let (Some(x), Some(y)) = (numeric(&a), numeric(&b)) {
            return self.arith(x, op, y);
        }
        let sym = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::FloorDiv => "//",
            BinOp::Mod => "%",
            BinOp::Pow => "**",
            BinOp::MatMul => "@",
        };
        match (&a, op, &b) {
            (Value::List(x), BinOp::Add, Value::List(y)) => {
                if x.len() + y.len() > MAX_SEQUENCE {
                    return err("list is too long");
                }
                Ok(Value::list(x.iter().chain(y.iter()).cloned().collect()))
            }
            (Value::Tuple(x), BinOp::Add, Value::Tuple(y)) => Ok(Value::Tuple(Arc::new(x.iter().chain(y.iter()).cloned().collect()))),
            (Value::Str(x), BinOp::Add, Value::Str(y)) => Ok(Value::Str(Arc::from(format!("{x}{y}").as_str()))),
            (Value::List(_) | Value::Str(_), BinOp::Mul, other) | (other, BinOp::Mul, Value::List(_) | Value::Str(_)) => {
                let (seq, n) = if matches!(a, Value::List(_) | Value::Str(_)) { (&a, other) } else { (&b, other) };
                let Some(n) = numeric(n).and_then(|e| e.as_i64()) else { return err("a list or string can only be repeated an integer number of times") };
                let n = usize::try_from(n.max(0)).unwrap_or(0);
                match seq {
                    Value::List(l) => {
                        if l.len().saturating_mul(n) > MAX_SEQUENCE {
                            return err("list is too long");
                        }
                        Ok(Value::list((0..n).flat_map(|_| l.iter().cloned()).collect()))
                    }
                    Value::Str(s) => {
                        if s.len().saturating_mul(n) > MAX_SEQUENCE {
                            return err("string is too long");
                        }
                        Ok(Value::Str(Arc::from(s.repeat(n).as_str())))
                    }
                    _ => err("cannot repeat this value"),
                }
            }
            (Value::List(_), BinOp::MatMul, Value::List(_)) => {
                let (x, y) = (linalg::to_matrix(&a.to_expr()?)?, linalg::to_matrix(&b.to_expr()?)?);
                self.checked(linalg::from_matrix(&linalg::matmul(&x, &y)?))
            }
            _ => err(format!("unsupported operand types for {sym}: {} and {}", a.type_name(), b.type_name())),
        }
    }

    fn arith(&self, a: Expr, op: BinOp, b: Expr) -> R<Value> {
        let r = match op {
            BinOp::Add => Expr::add(vec![a, b]),
            BinOp::Sub => a.sub(&b),
            BinOp::Mul => Expr::mul(vec![a, b]),
            BinOp::Div => {
                if b.is_zero() {
                    return err("division by zero");
                }
                a.div(&b)
            }
            BinOp::Pow => {
                if a.is_zero() && b.as_num().is_some_and(Num::is_negative) {
                    return err("zero cannot be raised to a negative power");
                }
                Expr::pow(&a, &b)
            }
            BinOp::FloorDiv | BinOp::Mod => {
                let (Some(x), Some(y)) = (a.as_num(), b.as_num()) else { return err("`//` and `%` need plain numbers") };
                if y.is_zero() {
                    return err("division by zero");
                }
                match (x.as_i64(), y.as_i64()) {
                    (Some(p), Some(q)) => {
                        let (p, q) = (i128::from(p), i128::from(q));
                        let (mut d, mut m) = (p / q, p % q);
                        if m != 0 && ((m < 0) != (q < 0)) {
                            d -= 1;
                            m += q;
                        }
                        let v = if op == BinOp::FloorDiv { d } else { m };
                        Expr::big(IBig::from(v))
                    }
                    _ => {
                        let (p, q) = (x.to_f64(), y.to_f64());
                        let d = (p / q).floor();
                        Expr::float(if op == BinOp::FloorDiv { d } else { p - d * q })
                    }
                }
            }
            BinOp::MatMul => return err("`@` multiplies matrices (lists of lists)"),
        };
        self.checked(r)
    }

    fn compare(&self, a: &Value, op: CmpOp, b: &Value) -> R<Value> {
        match op {
            CmpOp::Is => Ok(Value::Bool(a == b)),
            CmpOp::IsNot => Ok(Value::Bool(a != b)),
            CmpOp::In | CmpOp::NotIn => {
                let found = match b {
                    Value::List(l) | Value::Tuple(l) => l.iter().any(|x| x == a),
                    Value::Str(s) => matches!(a, Value::Str(t) if s.contains(&**t)),
                    other => return err(format!("`in` needs a list, tuple or string, not a {}", other.type_name())),
                };
                Ok(Value::Bool(found == (op == CmpOp::In)))
            }
            CmpOp::Eq | CmpOp::NotEq => {
                let eq = match (numeric(a), numeric(b)) {
                    (Some(x), Some(y)) => self.equal_numbers(&x, &y),
                    _ => Value::Bool(a == b),
                };
                match (&eq, op) {
                    (Value::Bool(v), CmpOp::NotEq) => Ok(Value::Bool(!v)),
                    (Value::Expr(_), CmpOp::NotEq) => err("cannot decide whether these expressions differ because they contain symbols"),
                    _ => Ok(eq),
                }
            }
            CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE => {
                let ord = match (a, b) {
                    (Value::Str(x), Value::Str(y)) => x.cmp(y),
                    _ => match (numeric(a), numeric(b)) {
                        (Some(x), Some(y)) => order_numbers(&x, &y)?,
                        _ => return err(format!("cannot compare a {} with a {}", a.type_name(), b.type_name())),
                    },
                };
                Ok(Value::Bool(match op {
                    CmpOp::Lt => ord == Ordering::Less,
                    CmpOp::LtE => ord != Ordering::Greater,
                    CmpOp::Gt => ord == Ordering::Greater,
                    _ => ord != Ordering::Less,
                }))
            }
        }
    }

    /// `==` on numbers and formulas: a decided answer when possible, otherwise a symbolic equation.
    fn equal_numbers(&self, x: &Expr, y: &Expr) -> Value {
        if x == y {
            return Value::Bool(true);
        }
        if x.free_symbols().is_empty() && y.free_symbols().is_empty() {
            return Value::Bool(order_numbers(x, y).map(|o| o == Ordering::Equal).unwrap_or(false));
        }
        Value::Expr(Expr::eq(x.clone(), y.clone()))
    }
}

/// Numbers, formulas and booleans as expressions.
pub(crate) fn numeric(v: &Value) -> Option<Expr> {
    match v {
        Value::Expr(e) => Some(e.clone()),
        Value::Bool(b) => Some(Expr::int(i64::from(*b))),
        _ => None,
    }
}

pub(crate) fn order_numbers(x: &Expr, y: &Expr) -> R<Ordering> {
    if let (Some(a), Some(b)) = (x.as_num(), y.as_num()) {
        return Ok(a.canonical_cmp(b));
    }
    let syms: Vec<String> = x.free_symbols().into_iter().chain(y.free_symbols()).map(|s| s.to_string()).collect();
    if !syms.is_empty() {
        return err(format!("cannot compare expressions that contain symbols ({})", syms.join(", ")));
    }
    let (a, b) = (eval_f64(x, &|_| None)?, eval_f64(y, &|_| None)?);
    // Values this close are treated as equal: they are the same number up to rounding.
    if (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0) { Ok(Ordering::Equal) } else { Ok(a.total_cmp(&b)) }
}
