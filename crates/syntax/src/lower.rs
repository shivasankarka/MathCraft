//! Lowering from ruff's Python AST to ours.

use crate::SyntaxError;
use crate::ast::{BinOp, CmpOp, Comp, Ex, Param, Stmt, StmtKind, Target, UnOp};
use ruff_python_ast as py;
use ruff_text_size::Ranged;

/// Deepest nesting of statements/expressions accepted from source.
const MAX_NESTING: usize = 150;

/// Parse a cell or script into statements.
pub fn parse(source: &str) -> Result<Vec<Stmt>, SyntaxError> {
    if let Some(offset) = too_deep(source) {
        return Err(SyntaxError { line: line_of(source, offset), message: "this code is nested too deeply".into() });
    }
    // `^` is a power, as in maths (Python's XOR is not useful here); it must bind like `**`, so rewrite it up front.
    let rewritten = caret_to_pow(source);
    let source = rewritten.as_str();
    let parsed = ruff_python_parser::parse_module(source)
        .map_err(|e| SyntaxError { line: line_of(source, e.location.start().to_usize()), message: format!("invalid syntax: {}", e.error) })?;
    let lower = Lower { source };
    lower.suite(&parsed.syntax().body, 0)
}

/// `x^2` as `x**2`: every `^` outside strings and comments becomes `**`. Line numbers do not change.
fn caret_to_pow(source: &str) -> String {
    if !source.contains('^') {
        return source.to_owned();
    }
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len() + 8);
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '#' => {
                while let Some(&d) = chars.get(i).filter(|d| **d != '\n') {
                    out.push(d);
                    i += 1;
                }
                continue;
            }
            '\'' | '"' => {
                let triple = chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c);
                let width = if triple { 3 } else { 1 };
                out.extend(std::iter::repeat_n(c, width));
                i += width;
                while let Some(&d) = chars.get(i) {
                    if d == '\\' {
                        out.push(d);
                        if let Some(&n) = chars.get(i + 1) {
                            out.push(n);
                        }
                        i += 2;
                        continue;
                    }
                    if d == c && (!triple || (chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c))) {
                        out.extend(std::iter::repeat_n(c, width));
                        i += width;
                        break;
                    }
                    if d == '\n' && !triple {
                        break;
                    }
                    out.push(d);
                    i += 1;
                }
                continue;
            }
            '^' => out.push_str("**"),
            other => out.push(other),
        }
        i += 1;
    }
    out
}

/// Cheap guard run before parsing: the byte offset where brackets nest deeper than allowed, if any.
/// It ignores quoting, so it can only be wrong for absurd inputs, and it keeps the real parser away from them.
fn too_deep(source: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in source.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Some(i);
                }
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

struct Lower<'a> {
    source: &'a str,
}

fn line_of(source: &str, offset: usize) -> u32 {
    let end = offset.min(source.len());
    let n = source.as_bytes().iter().take(end).filter(|b| **b == b'\n').count();
    u32::try_from(n + 1).unwrap_or(u32::MAX)
}

impl Lower<'_> {
    fn err<T>(&self, at: impl Ranged, msg: impl Into<String>) -> Result<T, SyntaxError> {
        Err(SyntaxError { line: line_of(self.source, at.range().start().to_usize()), message: msg.into() })
    }

    fn deep(&self, at: impl Ranged, depth: usize) -> Result<(), SyntaxError> {
        if depth > MAX_NESTING { self.err::<()>(at, "this code is nested too deeply") } else { Ok(()) }
    }

    fn suite(&self, body: &[py::Stmt], depth: usize) -> Result<Vec<Stmt>, SyntaxError> {
        body.iter().map(|s| self.stmt(s, depth)).collect()
    }

    fn stmt(&self, s: &py::Stmt, depth: usize) -> Result<Stmt, SyntaxError> {
        self.deep(s, depth)?;
        let line = line_of(self.source, s.range().start().to_usize());
        let d = depth + 1;
        let kind = match s {
            py::Stmt::Expr(e) => StmtKind::Expr(self.expr(&e.value, d)?),
            py::Stmt::Assign(a) => {
                let targets = a.targets.iter().map(|t| self.target(t)).collect::<Result<_, _>>()?;
                StmtKind::Assign { targets, value: self.expr(&a.value, d)? }
            }
            py::Stmt::AugAssign(a) => StmtKind::AugAssign { target: self.target(&a.target)?, op: self.binop(a.op, s)?, value: self.expr(&a.value, d)? },
            py::Stmt::AnnAssign(a) => match &a.value {
                Some(v) => StmtKind::Assign { targets: vec![self.target(&a.target)?], value: self.expr(v, d)? },
                None => return self.err(s, "a bare type annotation does nothing here; write `x = value`"),
            },
            py::Stmt::FunctionDef(f) => {
                if f.is_async || !f.decorator_list.is_empty() {
                    return self.err(s, "async functions and decorators are not supported yet");
                }
                StmtKind::Def { name: f.name.to_string(), params: self.params(Some(&f.parameters), s, d)?, body: self.suite(&f.body, d)? }
            }
            py::Stmt::Return(r) => StmtKind::Return(r.value.as_deref().map(|v| self.expr(v, d)).transpose()?),
            py::Stmt::If(i) => {
                let mut orelse: Vec<Stmt> = Vec::new();
                // elif/else chain, built from the last clause backwards.
                for clause in i.elif_else_clauses.iter().rev() {
                    let body = self.suite(&clause.body, d)?;
                    orelse = match &clause.test {
                        Some(t) => vec![Stmt {
                            kind: StmtKind::If { test: self.expr(t, d)?, body, orelse },
                            line: line_of(self.source, clause.range.start().to_usize()),
                        }],
                        None => body,
                    };
                }
                StmtKind::If { test: self.expr(&i.test, d)?, body: self.suite(&i.body, d)?, orelse }
            }
            py::Stmt::For(f) => {
                if f.is_async || !f.orelse.is_empty() {
                    return self.err(s, "`for ... else` and `async for` are not supported yet");
                }
                StmtKind::For { target: self.target(&f.target)?, iter: self.expr(&f.iter, d)?, body: self.suite(&f.body, d)? }
            }
            py::Stmt::While(w) => {
                if !w.orelse.is_empty() {
                    return self.err(s, "`while ... else` is not supported yet");
                }
                StmtKind::While { test: self.expr(&w.test, d)?, body: self.suite(&w.body, d)? }
            }
            py::Stmt::Pass(_) => StmtKind::Pass,
            py::Stmt::Break(_) => StmtKind::Break,
            py::Stmt::Continue(_) => StmtKind::Continue,
            py::Stmt::ClassDef(_) => return self.err(s, "classes are not supported yet"),
            py::Stmt::Import(_) | py::Stmt::ImportFrom(_) => {
                return self.err(s, "imports are not supported: everything is already available (try `help()` to see the functions)");
            }
            py::Stmt::Try(_) | py::Stmt::Raise(_) | py::Stmt::With(_) | py::Stmt::Assert(_) => {
                return self.err(s, "`try`, `raise`, `with` and `assert` are not supported yet");
            }
            _ => return self.err(s, "this kind of statement is not supported yet"),
        };
        Ok(Stmt { kind, line })
    }

    fn params(&self, p: Option<&py::Parameters>, at: impl Ranged + Copy, depth: usize) -> Result<Vec<Param>, SyntaxError> {
        let Some(p) = p else { return Ok(Vec::new()) };
        if p.vararg.is_some() || p.kwarg.is_some() || !p.kwonlyargs.is_empty() {
            return self.err(at, "`*args`, `**kwargs` and keyword-only parameters are not supported yet");
        }
        p.iter_non_variadic_params()
            .map(|a| Ok(Param { name: a.parameter.name.to_string(), default: a.default.as_deref().map(|d| self.expr(d, depth)).transpose()? }))
            .collect()
    }

    fn target(&self, e: &py::Expr) -> Result<Target, SyntaxError> {
        match e {
            py::Expr::Name(n) => Ok(Target::Name(n.id.to_string())),
            py::Expr::Tuple(t) => Ok(Target::Tuple(t.elts.iter().map(|x| self.target(x)).collect::<Result<_, _>>()?)),
            py::Expr::List(t) => Ok(Target::Tuple(t.elts.iter().map(|x| self.target(x)).collect::<Result<_, _>>()?)),
            py::Expr::Subscript(_) | py::Expr::Attribute(_) => {
                self.err(e, "assigning to an element or attribute is not supported: build a new value instead (values are immutable)")
            }
            _ => self.err(e, "cannot assign to this expression"),
        }
    }

    fn binop(&self, op: py::Operator, at: impl Ranged) -> Result<BinOp, SyntaxError> {
        Ok(match op {
            py::Operator::Add => BinOp::Add,
            py::Operator::Sub => BinOp::Sub,
            py::Operator::Mult => BinOp::Mul,
            py::Operator::Div => BinOp::Div,
            py::Operator::FloorDiv => BinOp::FloorDiv,
            py::Operator::Mod => BinOp::Mod,
            py::Operator::Pow => BinOp::Pow,
            py::Operator::MatMult => BinOp::MatMul,
            py::Operator::BitXor => return self.err(at, "`^` is bitwise XOR in Python; use `**` for powers (x**2)"),
            _ => return self.err(at, "bitwise operators are not supported"),
        })
    }

    fn expr(&self, e: &py::Expr, depth: usize) -> Result<Ex, SyntaxError> {
        self.deep(e, depth)?;
        let d = depth + 1;
        let boxed = |x: &py::Expr| -> Result<Box<Ex>, SyntaxError> { Ok(Box::new(self.expr(x, d)?)) };
        let many = |xs: &[py::Expr]| -> Result<Vec<Ex>, SyntaxError> { xs.iter().map(|x| self.expr(x, d)).collect() };
        Ok(match e {
            py::Expr::NumberLiteral(n) => match &n.value {
                py::Number::Int(i) => Ex::Int(i.to_string()),
                py::Number::Float(f) => Ex::Float(*f),
                py::Number::Complex { real, imag } => {
                    let im = if imag.fract() == 0.0 && imag.abs() < 1e15 { Ex::Int(format!("{}", *imag as i64)) } else { Ex::Float(*imag) };
                    let unit = Ex::Bin(Box::new(im), BinOp::Mul, Box::new(Ex::Name("I".to_string())));
                    if *real == 0.0 { unit } else { Ex::Bin(Box::new(Ex::Float(*real)), BinOp::Add, Box::new(unit)) }
                }
            },
            py::Expr::StringLiteral(s) => Ex::Str(s.value.to_str().to_string()),
            py::Expr::BooleanLiteral(b) => Ex::Bool(b.value),
            py::Expr::NoneLiteral(_) => Ex::None,
            py::Expr::Name(n) => Ex::Name(n.id.to_string()),
            py::Expr::BinOp(b) => Ex::Bin(boxed(&b.left)?, self.binop(b.op, e)?, boxed(&b.right)?),
            py::Expr::UnaryOp(u) => Ex::Unary(
                match u.op {
                    py::UnaryOp::USub => UnOp::Neg,
                    py::UnaryOp::UAdd => UnOp::Pos,
                    py::UnaryOp::Not => UnOp::Not,
                    py::UnaryOp::Invert => return self.err(e, "`~` is not supported"),
                },
                boxed(&u.operand)?,
            ),
            py::Expr::BoolOp(b) => match b.op {
                py::BoolOp::And => Ex::And(many(&b.values)?),
                py::BoolOp::Or => Ex::Or(many(&b.values)?),
            },
            py::Expr::Compare(c) => {
                let mut operands = c.operands.iter();
                let Some(first) = operands.next() else { return self.err(e, "empty comparison") };
                let rest = c
                    .ops
                    .iter()
                    .zip(operands)
                    .map(|(op, x)| {
                        let op = match op {
                            py::CmpOp::Eq => CmpOp::Eq,
                            py::CmpOp::NotEq => CmpOp::NotEq,
                            py::CmpOp::Lt => CmpOp::Lt,
                            py::CmpOp::LtE => CmpOp::LtE,
                            py::CmpOp::Gt => CmpOp::Gt,
                            py::CmpOp::GtE => CmpOp::GtE,
                            py::CmpOp::In => CmpOp::In,
                            py::CmpOp::NotIn => CmpOp::NotIn,
                            py::CmpOp::Is => CmpOp::Is,
                            py::CmpOp::IsNot => CmpOp::IsNot,
                        };
                        Ok((op, self.expr(x, d)?))
                    })
                    .collect::<Result<Vec<_>, SyntaxError>>()?;
                Ex::Compare(boxed(first)?, rest)
            }
            py::Expr::Call(c) => {
                let mut kwargs = Vec::new();
                for k in &c.arguments.keywords {
                    match &k.arg {
                        Some(name) => kwargs.push((name.to_string(), self.expr(&k.value, d)?)),
                        None => return self.err(e, "`**kwargs` unpacking is not supported yet"),
                    }
                }
                if c.arguments.args.iter().any(|a| matches!(a, py::Expr::Starred(_))) {
                    return self.err(e, "`*args` unpacking is not supported yet");
                }
                Ex::Call { func: boxed(&c.func)?, args: many(&c.arguments.args)?, kwargs }
            }
            py::Expr::Attribute(a) => Ex::Attr(boxed(&a.value)?, a.attr.to_string()),
            py::Expr::Subscript(s) => Ex::Index(boxed(&s.value)?, boxed(&s.slice)?),
            py::Expr::List(l) => Ex::List(many(&l.elts)?),
            py::Expr::Tuple(t) => Ex::Tuple(many(&t.elts)?),
            py::Expr::Lambda(l) => Ex::Lambda { params: self.params(l.parameters.as_deref(), e, d)?, body: boxed(&l.body)? },
            py::Expr::If(i) => Ex::IfExp { test: boxed(&i.test)?, body: boxed(&i.body)?, orelse: boxed(&i.orelse)? },
            py::Expr::ListComp(c) => {
                let mut gens = Vec::new();
                for g in &c.generators {
                    if g.is_async {
                        return self.err(e, "async comprehensions are not supported");
                    }
                    gens.push(Comp { target: self.target(&g.target)?, iter: self.expr(&g.iter, d)?, ifs: many(&g.ifs)? });
                }
                Ex::ListComp { elt: boxed(&c.elt)?, gens }
            }
            py::Expr::FString(f) => {
                let mut out: Option<Ex> = None;
                let mut push = |part: Ex| {
                    out = Some(match out.take() {
                        None => part,
                        Some(acc) => Ex::Bin(Box::new(acc), BinOp::Add, Box::new(part)),
                    });
                };
                for part in &f.value {
                    let fs = match part {
                        py::FStringPartRef::Literal(l) => {
                            push(Ex::Str(l.value.to_string()));
                            continue;
                        }
                        py::FStringPartRef::FString(fs) => fs,
                    };
                    for el in &fs.elements {
                        match el {
                            py::InterpolatedStringElement::Literal(l) => push(Ex::Str(l.value.to_string())),
                            py::InterpolatedStringElement::Interpolation(i) => {
                                if i.debug_text.is_some() {
                                    return self.err(e, "`{x=}` in f-strings is not supported; write the name yourself");
                                }
                                let mut value = self.expr(&i.expression, d)?;
                                if matches!(i.conversion, py::ConversionFlag::Repr | py::ConversionFlag::Ascii) {
                                    value = Ex::Call { func: Box::new(Ex::Name("repr".into())), args: vec![value], kwargs: vec![] };
                                }
                                let mut spec = Ex::Str(String::new());
                                if let Some(fs) = &i.format_spec {
                                    let mut parts = Vec::new();
                                    for el in &fs.elements {
                                        match el {
                                            py::InterpolatedStringElement::Literal(l) => parts.push(Ex::Str(l.value.to_string())),
                                            py::InterpolatedStringElement::Interpolation(n) => {
                                                let inner = self.expr(&n.expression, d)?;
                                                parts.push(Ex::Call { func: Box::new(Ex::Name("str".into())), args: vec![inner], kwargs: vec![] });
                                            }
                                        }
                                    }
                                    if let Some(first) = parts.into_iter().reduce(|a, b| Ex::Bin(Box::new(a), BinOp::Add, Box::new(b))) {
                                        spec = first;
                                    }
                                }
                                push(Ex::Call { func: Box::new(Ex::Name("format".into())), args: vec![value, spec], kwargs: vec![] });
                            }
                        }
                    }
                }
                out.unwrap_or_else(|| Ex::Str(String::new()))
            }
            py::Expr::Dict(_) | py::Expr::Set(_) | py::Expr::DictComp(_) | py::Expr::SetComp(_) => {
                return self.err(e, "dictionaries and sets are not supported yet; use lists");
            }
            py::Expr::Generator(_) => return self.err(e, "generator expressions are not supported yet; use a list comprehension [...]"),
            py::Expr::Named(_) => return self.err(e, "the `:=` operator is not supported"),
            py::Expr::Starred(_) => return self.err(e, "`*` unpacking is not supported yet"),
            py::Expr::Slice(s) => {
                let part = |x: &Option<Box<py::Expr>>| -> Result<Option<Box<Ex>>, SyntaxError> { x.as_deref().map(boxed).transpose() };
                Ex::Slice(part(&s.lower)?, part(&s.upper)?, part(&s.step)?)
            }
            _ => return self.err(e, "this kind of expression is not supported yet"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(src: &str) -> StmtKind {
        parse(src).unwrap().remove(0).kind
    }

    #[test]
    fn parses_assignment_and_calls() {
        match first("y = diff(sin(x), x)") {
            StmtKind::Assign { targets, value } => {
                assert_eq!(targets, vec![Target::Name("y".into())]);
                assert!(matches!(value, Ex::Call { .. }));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn elif_chain_nests() {
        let StmtKind::If { orelse, .. } = first("if a:\n    pass\nelif b:\n    pass\nelse:\n    pass\n") else { panic!("not an if") };
        assert!(matches!(orelse.first().map(|s| &s.kind), Some(StmtKind::If { .. })));
    }

    #[test]
    fn caret_is_a_power() {
        assert_eq!(caret_to_pow("x^2 + 1"), "x**2 + 1");
        assert_eq!(caret_to_pow("s = 'a^b' # c^d\ny^3"), "s = 'a^b' # c^d\ny**3");
        assert_eq!(caret_to_pow("\"\"\"a^b\"\"\"; x^2"), "\"\"\"a^b\"\"\"; x**2");
        assert_eq!(first("x^2 + 1"), first("x**2 + 1"), "binds tighter than +");
        assert_eq!(first("2^3^2"), first("2**3**2"), "right associative");
        assert_eq!(first("-x^2"), first("-x**2"));
    }

    #[test]
    fn unsupported_things_are_explained_with_line_numbers() {
        let e = parse("x = 1\nclass A: pass\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("classes"));
        assert!(parse("import numpy").unwrap_err().message.contains("imports"));
        assert!(parse("x = {1: 2}").unwrap_err().message.contains("dictionaries"));
    }

    #[test]
    fn slices_and_f_strings_lower() {
        assert!(matches!(first("a[1:2]"), StmtKind::Expr(Ex::Index(_, i)) if matches!(*i, Ex::Slice(Some(_), Some(_), None))));
        assert!(matches!(first("a[::-1]"), StmtKind::Expr(Ex::Index(_, i)) if matches!(*i, Ex::Slice(None, None, Some(_)))));
        assert!(matches!(first("f'a{x}b'"), StmtKind::Expr(Ex::Bin(..))));
    }

    #[test]
    fn syntax_errors_report_the_line() {
        let e = parse("x = 1\ny = (2 +\n").unwrap_err();
        assert!(e.line >= 2, "{e}");
    }

    #[test]
    fn deep_nesting_is_refused() {
        let src = format!("{}1{}", "[".repeat(400), "]".repeat(400));
        assert!(parse(&src).is_err());
    }

    #[test]
    fn huge_integers_survive() {
        let big = "9".repeat(60);
        match first(&big) {
            StmtKind::Expr(Ex::Int(s)) => assert_eq!(s, big),
            other => panic!("unexpected {other:?}"),
        }
    }
}
