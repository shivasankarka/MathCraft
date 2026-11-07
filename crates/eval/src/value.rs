//! Runtime values.

use crate::array::{Array, DType};
use mathcraft_core::{Expr, Kind, MathError};
use mathcraft_syntax::ast::{Ex, Param, Stmt};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// A runtime error with the source line it happened on, when known.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalError {
    pub line: Option<u32>,
    pub message: String,
}

impl EvalError {
    pub fn new(message: impl Into<String>) -> Self {
        EvalError { line: None, message: message.into() }
    }
    pub fn at(mut self, line: u32) -> Self {
        self.line.get_or_insert(line);
        self
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "line {l}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl From<MathError> for EvalError {
    fn from(e: MathError) -> Self {
        EvalError::new(e.to_string())
    }
}

pub type R<T> = Result<T, EvalError>;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(EvalError::new(msg))
}

#[derive(Debug)]
pub enum FuncBody {
    Block(Vec<Stmt>),
    Lambda(Ex),
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub defaults: Vec<Option<Value>>,
    pub body: FuncBody,
    /// Local variables of the enclosing function at definition time (closures).
    pub captured: HashMap<String, Value>,
}

/// Curves ready to draw: each series is a list of polyline segments (a gap splits a segment).
#[derive(Debug, Clone, PartialEq)]
pub struct PlotSpec {
    pub series: Vec<PlotSeries>,
    pub x_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlotSeries {
    pub label: String,
    pub segments: Vec<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Expr(Expr),
    List(Arc<Vec<Value>>),
    Tuple(Arc<Vec<Value>>),
    Str(Arc<str>),
    Bool(bool),
    None,
    Range {
        start: i64,
        stop: i64,
        step: i64,
    },
    Func(Arc<Function>),
    Builtin(&'static str),
    Plot(Arc<PlotSpec>),
    /// A numeric vector or matrix with an explicit element type.
    Array(Arc<Array>),
    DType(DType),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Expr(a), Value::Expr(b)) => a == b,
            (Value::List(a), Value::List(b)) | (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::None, Value::None) => true,
            (Value::Range { start: a, stop: b, step: c }, Value::Range { start: d, stop: e, step: f }) => (a, b, c) == (d, e, f),
            (Value::Builtin(a), Value::Builtin(b)) => a == b,
            (Value::Func(a), Value::Func(b)) => Arc::ptr_eq(a, b),
            (Value::Plot(a), Value::Plot(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::DType(a), Value::DType(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn int(v: i64) -> Value {
        Value::Expr(Expr::int(v))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Arc::new(items))
    }

    /// Convert an expression into a value; expression lists become real lists so they can be indexed and looped over.
    pub fn from_expr(e: Expr) -> Value {
        match e.kind() {
            Kind::List(items) => Value::list(items.iter().cloned().map(Value::from_expr).collect()),
            _ => Value::Expr(e),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Expr(e) => match e.kind() {
                Kind::Num(n) if n.is_integer() && n.is_exact() => "integer",
                Kind::Num(n) if n.is_exact() => "rational number",
                Kind::Num(_) => "float",
                Kind::Eq(..) => "equation",
                _ => "expression",
            },
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Str(_) => "string",
            Value::Bool(_) => "boolean",
            Value::None => "None",
            Value::Range { .. } => "range",
            Value::Func(_) | Value::Builtin(_) => "function",
            Value::Plot(_) => "plot",
            Value::Array(a) => a.kind_name(),
            Value::DType(_) => "type",
        }
    }

    /// Convert to a symbolic expression (lists become expression lists, e.g. for matrices and systems).
    pub fn to_expr(&self) -> R<Expr> {
        match self {
            Value::Expr(e) => Ok(e.clone()),
            Value::Bool(b) => Ok(Expr::int(i64::from(*b))),
            Value::List(v) | Value::Tuple(v) => Ok(Expr::list(v.iter().map(Value::to_expr).collect::<R<_>>()?)),
            Value::Array(a) => Ok(match a.cols {
                None => Expr::list(a.exprs()),
                Some(_) => Expr::list(a.rows_of_exprs().into_iter().map(Expr::list).collect()),
            }),
            other => err(format!("expected a number or formula, got a {}", other.type_name())),
        }
    }

    /// The text `print` shows (strings are not quoted).
    pub fn to_text(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            other => other.to_string(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seq = |f: &mut fmt::Formatter<'_>, open: &str, items: &[Value], close: &str| -> fmt::Result {
            f.write_str(open)?;
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{v}")?;
            }
            f.write_str(close)
        };
        match self {
            Value::Expr(e) => write!(f, "{e}"),
            Value::List(v) => seq(f, "[", v, "]"),
            Value::Tuple(v) if v.len() == 1 => seq(f, "(", v, ",)"),
            Value::Tuple(v) => seq(f, "(", v, ")"),
            Value::Str(s) => write!(f, "'{s}'"),
            Value::Bool(true) => f.write_str("True"),
            Value::Bool(false) => f.write_str("False"),
            Value::None => f.write_str("None"),
            Value::Range { start, stop, step } if *step == 1 => write!(f, "range({start}, {stop})"),
            Value::Range { start, stop, step } => write!(f, "range({start}, {stop}, {step})"),
            Value::Func(func) => write!(f, "<function {}>", func.name),
            Value::Builtin(n) => write!(f, "<built-in function {n}>"),
            Value::DType(d) => f.write_str(d.name()),
            Value::Array(a) => {
                let items = |f: &mut fmt::Formatter<'_>, v: &[Expr]| -> fmt::Result {
                    f.write_str("[")?;
                    for (i, e) in v.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{e}")?;
                    }
                    f.write_str("]")
                };
                f.write_str(if a.is_matrix() { "matrix(" } else { "vec(" })?;
                match a.cols {
                    None => items(f, &a.exprs())?,
                    Some(_) => {
                        f.write_str("[")?;
                        for (i, r) in a.rows_of_exprs().iter().enumerate() {
                            if i > 0 {
                                f.write_str(", ")?;
                            }
                            items(f, r)?;
                        }
                        f.write_str("]")?;
                    }
                }
                if a.dtype != DType::Exact {
                    write!(f, ", dtype={}", a.dtype.name())?;
                }
                f.write_str(")")
            }
            Value::Plot(p) => write!(f, "<plot of {} curve{}>", p.series.len(), if p.series.len() == 1 { "" } else { "s" }),
        }
    }
}
