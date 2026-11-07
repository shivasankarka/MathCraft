//! Numeric `Vector` and `Matrix` values with an explicit element type (`dtype`).
//!
//! * `exact` (the default): entries are exact numbers or symbolic expressions, as everywhere else in MathCraft.
//! * `i8 i16 i32 i64`: fixed-width integers; overflow is an error, never a wrap.
//! * `f32 f64`: machine floats.
//!
//! Types are never mixed implicitly: an `f64` array and an `i32` array cannot be combined; convert one with
//! `astype(a, f64)`. A plain number written in the program (`2 * A`) adopts the array's type if it fits it exactly,
//! like a literal in Rust. Symbols only live in `exact` arrays.

use crate::value::{R, Value, err};
use mathcraft_algebra::linalg;
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind};
use mathcraft_syntax::ast::BinOp;
use std::sync::Arc;

/// Most elements one array may hold.
pub const MAX_ELEMENTS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    Exact,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl DType {
    pub const ALL: [DType; 7] = [DType::Exact, DType::I8, DType::I16, DType::I32, DType::I64, DType::F32, DType::F64];

    pub fn name(self) -> &'static str {
        match self {
            DType::Exact => "exact",
            DType::I8 => "i8",
            DType::I16 => "i16",
            DType::I32 => "i32",
            DType::I64 => "i64",
            DType::F32 => "f32",
            DType::F64 => "f64",
        }
    }

    pub fn parse(name: &str) -> Option<DType> {
        DType::ALL.into_iter().find(|d| d.name() == name)
    }

    fn bits(self) -> Option<u32> {
        match self {
            DType::I8 => Some(8),
            DType::I16 => Some(16),
            DType::I32 => Some(32),
            DType::I64 => Some(64),
            _ => None,
        }
    }

    fn is_float(self) -> bool {
        matches!(self, DType::F32 | DType::F64)
    }

    /// Does the integer `v` fit this integer type?
    fn fits(self, v: i128) -> bool {
        match self.bits() {
            Some(b) => v >= -(1i128 << (b - 1)) && v < (1i128 << (b - 1)),
            None => false,
        }
    }

    /// Round to the precision of this float type.
    fn round(self, v: f64) -> f64 {
        if self == DType::F32 { f64::from(v as f32) } else { v }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Exact(Vec<Expr>),
    Int(Vec<i64>),
    Float(Vec<f64>),
}

/// A vector (`cols == None`, `rows` entries) or a matrix (`rows` by `cols`), stored row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    pub dtype: DType,
    pub rows: usize,
    pub cols: Option<usize>,
    pub data: Data,
}

/// One scalar converted to an array's element type.
enum Scalar {
    Exact(Expr),
    Int(i64),
    Float(f64),
}

fn tick() -> R<()> {
    mathcraft_core::budget::tick()?;
    Ok(())
}

fn too_big() -> crate::value::EvalError {
    crate::value::EvalError::new(format!("arrays are limited to {MAX_ELEMENTS} elements"))
}

impl Array {
    pub fn len(&self) -> usize {
        self.rows * self.cols.unwrap_or(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_matrix(&self) -> bool {
        self.cols.is_some()
    }

    pub fn kind_name(&self) -> &'static str {
        if self.is_matrix() { "matrix" } else { "vector" }
    }

    pub fn shape(&self) -> Vec<usize> {
        match self.cols {
            Some(c) => vec![self.rows, c],
            None => vec![self.rows],
        }
    }

    /// `3 by 3 matrix of f64`, for messages and captions.
    pub fn describe(&self) -> String {
        let dims = self.shape().iter().map(usize::to_string).collect::<Vec<_>>().join("×");
        format!("{} {dims} · {}", if self.is_matrix() { "Matrix" } else { "Vector" }, self.dtype.name())
    }

    /// Entry `i` (row-major) as a number or expression. Integers become exact, floats stay floats.
    pub fn elem(&self, i: usize) -> Option<Expr> {
        match &self.data {
            Data::Exact(v) => v.get(i).cloned(),
            Data::Int(v) => v.get(i).map(|x| Expr::int(*x)),
            Data::Float(v) => v.get(i).map(|x| Expr::float(*x)),
        }
    }

    pub fn exprs(&self) -> Vec<Expr> {
        (0..self.len()).filter_map(|i| self.elem(i)).collect()
    }

    fn with(&self, dtype: DType, data: Data) -> Array {
        Array { dtype, rows: self.rows, cols: self.cols, data }
    }

    /// Build an array from expressions, converting each to `dtype` (an explicit request, so ints may become floats).
    pub fn from_exprs(dtype: DType, rows: usize, cols: Option<usize>, items: &[Expr]) -> R<Array> {
        if rows.saturating_mul(cols.unwrap_or(1)) > MAX_ELEMENTS {
            return Err(too_big());
        }
        let data = match dtype {
            DType::Exact => Data::Exact(items.to_vec()),
            d if d.is_float() => Data::Float(items.iter().map(|e| float_of(d, e)).collect::<R<_>>()?),
            d => Data::Int(items.iter().map(|e| int_of(d, e)).collect::<R<_>>()?),
        };
        Ok(Array { dtype, rows, cols, data })
    }

    /// `astype`: an explicit conversion. Integers must stay whole and in range; symbols cannot become floats.
    pub fn cast(&self, to: DType) -> R<Array> {
        if to == self.dtype {
            return Ok(self.clone());
        }
        if let (Data::Float(v), Some(_)) = (&self.data, to.bits()) {
            // Whole-valued floats convert to integers; a fractional part is an error, never a silent truncation.
            let ints: Vec<Expr> = v
                .iter()
                .map(|x| {
                    if x.fract() == 0.0 && x.abs() < 9.0e18 {
                        Ok(Expr::int(*x as i64))
                    } else {
                        err(format!("{x} is not a whole number, so it cannot be {}", to.name()))
                    }
                })
                .collect::<R<_>>()?;
            return Array::from_exprs(to, self.rows, self.cols, &ints);
        }
        Array::from_exprs(to, self.rows, self.cols, &self.exprs())
    }

    // ---- element-wise arithmetic -----------------------------------------------------------------------------

    fn scalar(&self, e: &Expr) -> R<Scalar> {
        Ok(match self.dtype {
            DType::Exact => Scalar::Exact(e.clone()),
            d if d.is_float() => Scalar::Float(float_of(d, e).map_err(|_| weak_error(self.dtype, e))?),
            d => Scalar::Int(int_of(d, e).map_err(|_| weak_error(self.dtype, e))?),
        })
    }

    fn pick(&self, i: usize) -> Scalar {
        match &self.data {
            Data::Exact(v) => Scalar::Exact(v.get(i).cloned().unwrap_or_else(Expr::zero)),
            Data::Int(v) => Scalar::Int(v.get(i).copied().unwrap_or(0)),
            Data::Float(v) => Scalar::Float(v.get(i).copied().unwrap_or(0.0)),
        }
    }

    fn zip(&self, other: &Operand<'_>, op: BinOp, flipped: bool) -> R<Array> {
        let n = self.len();
        let mut exact = Vec::new();
        let mut ints = Vec::new();
        let mut floats = Vec::new();
        let scalar = match other {
            Operand::Scalar(e) => Some(self.scalar(e)?),
            Operand::Array(_) => None,
        };
        for i in 0..n {
            tick()?;
            let tmp;
            let rhs = match (&scalar, other) {
                (Some(s), _) => s,
                (None, Operand::Array(b)) => {
                    tmp = b.pick(i);
                    &tmp
                }
                (None, Operand::Scalar(_)) => return err("internal error: missing scalar"),
            };
            let lhs = self.pick(i);
            let (l, r) = if flipped { (rhs, &lhs) } else { (&lhs, rhs) };
            match (l, r) {
                (Scalar::Exact(x), Scalar::Exact(y)) => exact.push(exact_op(op, x, y)?),
                (Scalar::Int(x), Scalar::Int(y)) => ints.push(int_op(self.dtype, op, *x, *y)?),
                (Scalar::Float(x), Scalar::Float(y)) => floats.push(self.dtype.round(float_op(op, *x, *y))),
                _ => return err("internal error: mismatched element types"),
            }
        }
        let data = match self.dtype {
            DType::Exact => Data::Exact(exact),
            d if d.is_float() => Data::Float(floats),
            _ => Data::Int(ints),
        };
        Ok(self.with(self.dtype, data))
    }

    fn map_exprs(&self, f: &mut dyn FnMut(&Expr) -> R<Expr>) -> R<Array> {
        let items: Vec<Expr> = self.exprs().iter().map(f).collect::<R<_>>()?;
        match self.dtype {
            // The function may produce symbols or exact values; an exact array can hold them.
            DType::Exact => Ok(self.with(DType::Exact, Data::Exact(items))),
            d if d.is_float() => Array::from_exprs(d, self.rows, self.cols, &items),
            _ => err(format!("this function does not keep {} values whole; convert first with astype(a, f64)", self.dtype.name())),
        }
    }

    /// Apply a function to every element (`sin(v)`, `N(v)`, ...). Fixed-width integers must be converted first.
    pub fn map(&self, f: &mut dyn FnMut(&Expr) -> R<Expr>) -> R<Array> {
        self.map_exprs(f)
    }

    pub fn neg(&self) -> R<Array> {
        self.zip(&Operand::Scalar(Expr::int(-1)), BinOp::Mul, false)
    }

    // ---- linear algebra --------------------------------------------------------------------------------------

    pub fn transpose(&self) -> Array {
        let Some(c) = self.cols else { return self.clone() };
        let at = |i: usize, j: usize| j * c + i;
        let order: Vec<usize> = (0..c).flat_map(|i| (0..self.rows).map(move |j| (i, j))).map(|(i, j)| at(i, j)).collect();
        let data = match &self.data {
            Data::Exact(v) => Data::Exact(order.iter().filter_map(|k| v.get(*k).cloned()).collect()),
            Data::Int(v) => Data::Int(order.iter().filter_map(|k| v.get(*k).copied()).collect()),
            Data::Float(v) => Data::Float(order.iter().filter_map(|k| v.get(*k).copied()).collect()),
        };
        Array { dtype: self.dtype, rows: c, cols: Some(self.rows), data }
    }

    fn exact_matrix(&self) -> linalg::Matrix {
        let c = self.cols.unwrap_or(1).max(1);
        self.exprs().chunks(c).map(<[Expr]>::to_vec).collect()
    }

    fn float_rows(&self) -> Vec<Vec<f64>> {
        let c = self.cols.unwrap_or(1).max(1);
        match &self.data {
            Data::Float(v) => v.chunks(c).map(<[f64]>::to_vec).collect(),
            _ => Vec::new(),
        }
    }

    fn from_exact_matrix(m: &linalg::Matrix) -> Array {
        let rows = m.len();
        let cols = m.first().map_or(0, Vec::len);
        Array { dtype: DType::Exact, rows, cols: Some(cols), data: Data::Exact(m.iter().flatten().cloned().collect()) }
    }

    fn from_float_rows(dtype: DType, m: &[Vec<f64>]) -> Array {
        let cols = m.first().map_or(0, Vec::len);
        Array { dtype, rows: m.len(), cols: Some(cols), data: Data::Float(m.iter().flatten().map(|v| dtype.round(*v)).collect()) }
    }

    fn require_square(&self, what: &str) -> R<usize> {
        match self.cols {
            Some(c) if c == self.rows => Ok(c),
            _ => err(format!("{what} needs a square matrix, got a {}", self.describe())),
        }
    }

    fn no_ints(&self, what: &str) -> R<()> {
        if self.dtype.bits().is_some() {
            return err(format!("{what} of an {} matrix is not defined; convert first with astype(a, f64) or astype(a, exact)", self.dtype.name()));
        }
        Ok(())
    }

    pub fn det(&self) -> R<Value> {
        self.require_square("det")?;
        self.no_ints("det")?;
        if self.dtype == DType::Exact {
            return Ok(Value::from_expr(linalg::det(&self.exact_matrix())?));
        }
        let mut m = self.float_rows();
        if let Some(d) = crate::fastla::det(&m) {
            return Ok(Value::Expr(Expr::float(self.dtype.round(d))));
        }
        let n = m.len();
        let mut det = 1.0;
        for k in 0..n {
            let p = (k..n).max_by(|a, b| cell(&m, *a, k).abs().total_cmp(&cell(&m, *b, k).abs())).unwrap_or(k);
            if cell(&m, p, k) == 0.0 {
                return Ok(Value::Expr(Expr::float(0.0)));
            }
            if p != k {
                m.swap(p, k);
                det = -det;
            }
            let pivot = cell(&m, k, k);
            det *= pivot;
            for i in k + 1..n {
                let f = cell(&m, i, k) / pivot;
                for j in k..n {
                    let v = cell(&m, i, j) - f * cell(&m, k, j);
                    put(&mut m, i, j, v);
                }
            }
        }
        Ok(Value::Expr(Expr::float(self.dtype.round(det))))
    }

    pub fn inverse(&self) -> R<Array> {
        let n = self.require_square("inv")?;
        self.no_ints("inv")?;
        if self.dtype == DType::Exact {
            return Ok(Array::from_exact_matrix(&linalg::inverse(&self.exact_matrix())?));
        }
        let mut a = self.float_rows();
        if let Some(inv) = crate::fastla::inverse(&a) {
            return Ok(Array::from_float_rows(self.dtype, &inv));
        }
        let mut b: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| f64::from(u8::from(i == j))).collect()).collect();
        gauss_jordan(&mut a, &mut b)?;
        Ok(Array::from_float_rows(self.dtype, &b))
    }

    /// Solve `self @ x == b` for a float matrix and a float vector (or matrix of columns).
    pub fn solve(&self, rhs: &Array) -> R<Array> {
        let n = self.require_square("solve")?;
        if self.dtype != rhs.dtype || self.dtype != DType::F64 && self.dtype != DType::F32 {
            return err("solve(A, b) with typed arrays needs both of the same float type; for exact systems use solve([eq1, eq2], [x, y])");
        }
        if rhs.rows != n {
            return err(format!("solve: the matrix has {n} rows but the right-hand side has {}", rhs.rows));
        }
        let mut a = self.float_rows();
        let cols = rhs.cols.unwrap_or(1);
        let mut b: Vec<Vec<f64>> = match &rhs.data {
            Data::Float(v) => v.chunks(cols.max(1)).map(<[f64]>::to_vec).collect(),
            _ => Vec::new(),
        };
        if let Some(x) = crate::fastla::solve(&a, &b) {
            b = x;
        } else {
            gauss_jordan(&mut a, &mut b)?;
        }
        let data: Vec<f64> = b.iter().flatten().map(|v| self.dtype.round(*v)).collect();
        Ok(Array { dtype: self.dtype, rows: n, cols: rhs.cols, data: Data::Float(data) })
    }

    /// The exact matrix for eigen-computations (exact dtype, square when `square`).
    fn symbolic(&self, what: &str, square: bool) -> R<linalg::Matrix> {
        if self.dtype != DType::Exact {
            return err(format!("{what} is computed on exact matrices; convert with astype(a, exact)"));
        }
        if square {
            self.require_square(what)?;
        }
        Ok(self.exact_matrix())
    }

    fn vector_value(items: &[Expr]) -> R<Value> {
        Ok(Value::Array(std::sync::Arc::new(Array::from_exprs(DType::Exact, items.len(), None, items)?)))
    }

    /// Eigenvalues with multiplicity, exactly.
    pub fn eigenvals(&self) -> R<Value> {
        let m = self.symbolic("eigenvals", true)?;
        Ok(Value::list(linalg::eigenvalues(&m)?.into_iter().map(Value::from_expr).collect()))
    }

    /// `[(eigenvalue, multiplicity, [eigenvectors])]`, exactly.
    pub fn eigenvects(&self) -> R<Value> {
        let m = self.symbolic("eigenvects", true)?;
        let values = linalg::eigenvalues(&m)?;
        let mut distinct: Vec<(Expr, i64)> = Vec::new();
        for v in values {
            match distinct.iter_mut().find(|(d, _)| *d == v) {
                Some((_, k)) => *k += 1,
                None => distinct.push((v, 1)),
            }
        }
        let mut out = Vec::new();
        for (lambda, k) in distinct {
            let shifted: linalg::Matrix =
                m.iter().enumerate().map(|(i, row)| row.iter().enumerate().map(|(j, v)| if i == j { v.sub(&lambda) } else { v.clone() }).collect()).collect();
            let vectors = linalg::nullspace(&shifted)?.iter().map(|v| Array::vector_value(v)).collect::<R<Vec<_>>>()?;
            out.push(Value::Tuple(std::sync::Arc::new(vec![Value::from_expr(lambda), Value::int(k), Value::list(vectors)])));
        }
        Ok(Value::list(out))
    }

    /// A basis of the null space, as a list of vectors.
    pub fn nullspace(&self) -> R<Value> {
        let m = self.symbolic("nullspace", false)?;
        Ok(Value::list(linalg::nullspace(&m)?.iter().map(|v| Array::vector_value(v)).collect::<R<Vec<_>>>()?))
    }

    /// `det(A - x*I)`.
    pub fn charpoly(&self, x: &Expr) -> R<Value> {
        let m = self.symbolic("charpoly", true)?;
        Ok(Value::from_expr(linalg::charpoly(&m, x)?))
    }

    pub fn rank(&self) -> R<usize> {
        if self.dtype != DType::Exact {
            return err("rank is computed on exact matrices; convert with astype(a, exact)");
        }
        Ok(linalg::rank(&self.exact_matrix())?)
    }

    pub fn matmul(&self, other: &Array) -> R<Value> {
        if self.dtype != other.dtype {
            return Err(mix_error("@", self.dtype, other.dtype));
        }
        let (n, k) = (self.rows, self.cols);
        // Treat a vector on the left as a 1 x n row and a vector on the right as an n x 1 column.
        let (lr, lc) = match k {
            Some(c) => (n, c),
            None => (1, n),
        };
        let (rr, rc) = match other.cols {
            Some(c) => (other.rows, c),
            None => (other.rows, 1),
        };
        if lc != rr {
            return err(format!("cannot multiply a {} by a {}: {lc} columns against {rr} rows", self.describe(), other.describe()));
        }
        if lr.saturating_mul(rc) > MAX_ELEMENTS {
            return Err(too_big());
        }
        let data = match (&self.data, &other.data) {
            (Data::Exact(_), Data::Exact(_)) => {
                let a: linalg::Matrix = self.exprs().chunks(lc.max(1)).map(<[Expr]>::to_vec).collect();
                let b: linalg::Matrix = other.exprs().chunks(rc.max(1)).map(<[Expr]>::to_vec).collect();
                Data::Exact(linalg::matmul(&a, &b)?.into_iter().flatten().collect())
            }
            (Data::Int(a), Data::Int(b)) => {
                let mut out = Vec::with_capacity(lr * rc);
                for i in 0..lr {
                    for j in 0..rc {
                        tick()?;
                        let mut acc: i128 = 0;
                        for t in 0..lc {
                            let prod = i128::from(a.get(i * lc + t).copied().unwrap_or(0)) * i128::from(b.get(t * rc + j).copied().unwrap_or(0));
                            acc = acc.checked_add(prod).ok_or_else(|| overflow(self.dtype, "@"))?;
                        }
                        if !self.dtype.fits(acc) {
                            return Err(overflow(self.dtype, "@"));
                        }
                        out.push(i64::try_from(acc).map_err(|_| overflow(self.dtype, "@"))?);
                    }
                }
                Data::Int(out)
            }
            (Data::Float(a), Data::Float(b)) if let Some(fast) = crate::fastla::matmul(a, b, lr, lc, rc) => {
                tick()?;
                Data::Float(fast.into_iter().map(|v| self.dtype.round(v)).collect())
            }
            (Data::Float(a), Data::Float(b)) => {
                let mut out = Vec::with_capacity(lr * rc);
                for i in 0..lr {
                    tick()?;
                    for j in 0..rc {
                        let mut acc = 0.0;
                        for t in 0..lc {
                            acc += a.get(i * lc + t).copied().unwrap_or(0.0) * b.get(t * rc + j).copied().unwrap_or(0.0);
                        }
                        out.push(self.dtype.round(acc));
                    }
                }
                Data::Float(out)
            }
            _ => return err("internal error: mismatched element types"),
        };
        let shaped = match (k, other.cols) {
            (None, None) => {
                // vector @ vector: a single number.
                let first = Array { dtype: self.dtype, rows: 1, cols: None, data };
                return Ok(Value::Expr(first.elem(0).unwrap_or_else(Expr::zero)));
            }
            (Some(_), None) => Array { dtype: self.dtype, rows: lr, cols: None, data },
            (None, Some(_)) => Array { dtype: self.dtype, rows: rc, cols: None, data },
            (Some(_), Some(_)) => Array { dtype: self.dtype, rows: lr, cols: Some(rc), data },
        };
        Ok(Value::Array(Arc::new(shaped)))
    }

    /// Row `i` of a matrix as a vector, or entry `i` of a vector.
    pub fn index(&self, i: usize) -> Option<Value> {
        match self.cols {
            None => self.elem(i).map(Value::from_expr),
            Some(c) => {
                if i >= self.rows {
                    return None;
                }
                let range = i * c..(i + 1) * c;
                let data = match &self.data {
                    Data::Exact(v) => Data::Exact(v.get(range)?.to_vec()),
                    Data::Int(v) => Data::Int(v.get(range)?.to_vec()),
                    Data::Float(v) => Data::Float(v.get(range)?.to_vec()),
                };
                Some(Value::Array(Arc::new(Array { dtype: self.dtype, rows: c, cols: None, data })))
            }
        }
    }

    pub fn index2(&self, i: usize, j: usize) -> Option<Expr> {
        let c = self.cols?;
        if i >= self.rows || j >= c { None } else { self.elem(i * c + j) }
    }

    /// Rows of expressions, for typesetting and `tolist`.
    pub fn rows_of_exprs(&self) -> Vec<Vec<Expr>> {
        match self.cols {
            Some(c) => self.exprs().chunks(c.max(1)).map(<[Expr]>::to_vec).collect(),
            None => self.exprs().into_iter().map(|e| vec![e]).collect(),
        }
    }

    pub fn to_list(&self) -> Value {
        match self.cols {
            None => Value::list(self.exprs().into_iter().map(Value::from_expr).collect()),
            Some(_) => Value::list(self.rows_of_exprs().into_iter().map(|r| Value::list(r.into_iter().map(Value::from_expr).collect())).collect()),
        }
    }
}

fn cell(m: &[Vec<f64>], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
}

fn put(m: &mut [Vec<f64>], i: usize, j: usize, v: f64) {
    if let Some(slot) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
        *slot = v;
    }
}

/// Reduce `[a | b]` so that `a` becomes the identity; `b` then holds `a^-1 b`.
fn gauss_jordan(a: &mut [Vec<f64>], b: &mut [Vec<f64>]) -> R<()> {
    let n = a.len();
    for k in 0..n {
        tick()?;
        let p = (k..n).max_by(|x, y| cell(a, *x, k).abs().total_cmp(&cell(a, *y, k).abs())).unwrap_or(k);
        if cell(a, p, k).abs() < 1e-300 {
            return err("the matrix is singular (it has no inverse)");
        }
        a.swap(p, k);
        b.swap(p, k);
        let pivot = cell(a, k, k);
        for row in [&mut *a, &mut *b] {
            if let Some(r) = row.get_mut(k) {
                r.iter_mut().for_each(|v| *v /= pivot);
            }
        }
        for i in 0..n {
            if i == k {
                continue;
            }
            let f = cell(a, i, k);
            if f == 0.0 {
                continue;
            }
            for m in [&mut *a, &mut *b] {
                let pivot_row: Vec<f64> = m.get(k).cloned().unwrap_or_default();
                if let Some(r) = m.get_mut(i) {
                    for (v, pv) in r.iter_mut().zip(pivot_row) {
                        *v -= f * pv;
                    }
                }
            }
        }
    }
    Ok(())
}

// ---- scalar conversions and per-type operations -------------------------------------------------------------------

fn weak_error(dtype: DType, e: &Expr) -> crate::value::EvalError {
    crate::value::EvalError::new(format!(
        "the value {e} does not fit an {} array. Types are not mixed implicitly: convert the array first (astype(a, f64) or astype(a, exact))",
        dtype.name()
    ))
}

fn mix_error(op: &str, a: DType, b: DType) -> crate::value::EvalError {
    crate::value::EvalError::new(format!(
        "cannot use `{op}` on {} and {} arrays: types are never mixed implicitly. Convert one explicitly, e.g. astype(a, {})",
        a.name(),
        b.name(),
        b.name()
    ))
}

fn overflow(dtype: DType, what: &str) -> crate::value::EvalError {
    crate::value::EvalError::new(format!("integer overflow: the result of `{what}` does not fit {}. Use a wider type (astype) or exact numbers", dtype.name()))
}

fn float_of(dtype: DType, e: &Expr) -> R<f64> {
    let v = match e.kind() {
        Kind::Num(n) => n.to_f64(),
        _ if e.free_symbols().is_empty() => eval_f64(e, &|_| None).map_err(|m| crate::value::EvalError::new(m.to_string()))?,
        _ => return err(format!("`{e}` contains symbols, so it cannot be a {} number; symbols live in exact arrays", dtype.name())),
    };
    Ok(dtype.round(v))
}

fn int_of(dtype: DType, e: &Expr) -> R<i64> {
    let Some(n) = e.as_num() else { return err(format!("`{e}` is not a whole number, so it cannot be {}", dtype.name())) };
    if !(n.is_exact() && n.is_integer()) {
        return err(format!("`{e}` is not a whole number, so it cannot be {}", dtype.name()));
    }
    match n.as_i64() {
        Some(v) if dtype.fits(i128::from(v)) => Ok(v),
        _ => err(format!("{e} does not fit {}", dtype.name())),
    }
}

fn exact_op(op: BinOp, a: &Expr, b: &Expr) -> R<Expr> {
    Ok(match op {
        BinOp::Add => Expr::add(vec![a.clone(), b.clone()]),
        BinOp::Sub => a.sub(b),
        BinOp::Mul => Expr::mul(vec![a.clone(), b.clone()]),
        BinOp::Div => {
            if b.is_zero() {
                return err("division by zero");
            }
            a.div(b)
        }
        BinOp::Pow => {
            if a.is_zero() && b.as_num().is_some_and(mathcraft_core::Num::is_negative) {
                return err("zero cannot be raised to a negative power");
            }
            Expr::pow(a, b)
        }
        _ => return err("this operator is not defined element-wise on exact arrays"),
    })
}

fn int_op(dtype: DType, op: BinOp, a: i64, b: i64) -> R<i64> {
    let (x, y) = (i128::from(a), i128::from(b));
    let r = match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::FloorDiv | BinOp::Mod => {
            if y == 0 {
                return err("division by zero");
            }
            let (mut d, mut m) = (x.div_euclid(y), x.rem_euclid(y));
            if y < 0 && m != 0 {
                d += 1;
                m += y;
            }
            if op == BinOp::FloorDiv { d } else { m }
        }
        BinOp::Div => {
            return err(format!("`/` is not defined for {} arrays (the result is not a whole number). Use `//`, or convert with astype(a, f64)", dtype.name()));
        }
        BinOp::Pow => {
            if y < 0 {
                return err(format!("a negative power of an {} integer is not an integer; convert with astype(a, f64)", dtype.name()));
            }
            match x {
                0 => i128::from(y == 0),
                1 => 1,
                -1 => {
                    if y % 2 == 0 {
                        1
                    } else {
                        -1
                    }
                }
                // |x| >= 2: 64 multiplications exceed every supported width.
                _ => {
                    if y > 64 {
                        return Err(overflow(dtype, "**"));
                    }
                    let mut acc: i128 = 1;
                    for _ in 0..y {
                        acc = acc.saturating_mul(x);
                        if !dtype.fits(acc) {
                            return Err(overflow(dtype, "**"));
                        }
                    }
                    acc
                }
            }
        }
        BinOp::MatMul => return err("`@` is for matrices"),
    };
    if !dtype.fits(r) {
        return Err(overflow(dtype, &format!("{a} and {b}")));
    }
    i64::try_from(r).map_err(|_| overflow(dtype, "the operation"))
}

fn float_op(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Pow => a.powf(b),
        BinOp::FloorDiv => (a / b).floor(),
        BinOp::Mod => a - (a / b).floor() * b,
        BinOp::MatMul => f64::NAN,
    }
}

// ---- operators ----------------------------------------------------------------------------------------------------

enum Operand<'a> {
    Array(&'a Array),
    Scalar(Expr),
}

/// Apply a binary operator when at least one side is an array. `scalar` converts the other side if it is a number.
pub fn binop(a: &Value, op: BinOp, b: &Value, number: &dyn Fn(&Value) -> Option<Expr>) -> R<Value> {
    let wrap = |arr: Array| Ok(Value::Array(Arc::new(arr)));
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            if op == BinOp::MatMul {
                return x.matmul(y);
            }
            if x.dtype != y.dtype {
                return Err(mix_error(op_symbol(op), x.dtype, y.dtype));
            }
            if x.shape() != y.shape() {
                return err(format!("shapes do not match: {} and {} (no implicit broadcasting between arrays)", x.describe(), y.describe()));
            }
            wrap(x.zip(&Operand::Array(y), op, false)?)
        }
        (Value::Array(x), other) | (other, Value::Array(x)) => {
            let flipped = matches!(b, Value::Array(_));
            if op == BinOp::MatMul {
                return err("`@` multiplies two vectors or matrices");
            }
            let Some(n) = number(other) else {
                return err(format!("cannot combine a {} with a {}; convert lists with vec([...]) or matrix([[...]])", x.kind_name(), other.type_name()));
            };
            wrap(x.zip(&Operand::Scalar(n), op, flipped)?)
        }
        _ => err("internal error: array operator without an array"),
    }
}

fn op_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::FloorDiv => "//",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::MatMul => "@",
    }
}

/// `vec(...)`/`matrix(...)` from a nested list of values.
pub fn build(dtype: DType, v: &Value, matrix: bool) -> R<Array> {
    let items = match v {
        Value::List(l) | Value::Tuple(l) => l.as_ref().clone(),
        Value::Array(a) => return a.cast(dtype),
        other => return err(format!("expected a list, got a {}", other.type_name())),
    };
    if !matrix {
        let exprs: Vec<Expr> = items.iter().map(Value::to_expr).collect::<R<_>>()?;
        if exprs.iter().any(|e| matches!(e.kind(), Kind::List(_))) {
            return err("vec() takes a flat list; use matrix([[...], [...]]) for rows");
        }
        return Array::from_exprs(dtype, exprs.len(), None, &exprs);
    }
    let mut rows: Vec<Vec<Expr>> = Vec::new();
    for item in &items {
        match item {
            Value::List(r) | Value::Tuple(r) => rows.push(r.iter().map(Value::to_expr).collect::<R<_>>()?),
            Value::Array(a) if !a.is_matrix() => rows.push(a.exprs()),
            _ => return err("matrix() takes a list of rows, such as matrix([[1, 2], [3, 4]])"),
        }
    }
    let width = rows.first().map_or(0, Vec::len);
    if rows.iter().any(|r| r.len() != width) {
        return err("all rows of a matrix must have the same length");
    }
    if rows.iter().flatten().any(|e| matches!(e.kind(), Kind::List(_))) {
        return err("a matrix is two-dimensional: its entries must be numbers or formulas");
    }
    let flat: Vec<Expr> = rows.into_iter().flatten().collect();
    let nrows = flat.len().checked_div(width).unwrap_or(items.len());
    Array::from_exprs(dtype, nrows, Some(width), &flat)
}

/// `zeros`, `ones` and `eye`.
pub fn filled(dtype: DType, rows: usize, cols: Option<usize>, f: &dyn Fn(usize, usize) -> i64) -> R<Array> {
    let c = cols.unwrap_or(1);
    if rows.saturating_mul(c) > MAX_ELEMENTS {
        return Err(too_big());
    }
    let items: Vec<Expr> = (0..rows * c).map(|k| Expr::int(f(k / c.max(1), k % c.max(1)))).collect();
    Array::from_exprs(dtype, rows, cols, &items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: i64) -> Option<Expr> {
        Some(Expr::int(v))
    }
    fn number(v: &Value) -> Option<Expr> {
        match v {
            Value::Expr(e) => Some(e.clone()),
            _ => None,
        }
    }
    fn vec_of(dt: DType, v: &[i64]) -> Value {
        let items: Vec<Expr> = v.iter().map(|x| Expr::int(*x)).collect();
        Value::Array(Arc::new(Array::from_exprs(dt, v.len(), None, &items).unwrap()))
    }
    fn show(v: &Value) -> String {
        match v {
            Value::Array(a) => a.exprs().iter().map(ToString::to_string).collect::<Vec<_>>().join(" "),
            other => other.to_string(),
        }
    }

    #[test]
    fn exact_elementwise() {
        let a = vec_of(DType::Exact, &[1, 2, 3]);
        let out = binop(&a, BinOp::Mul, &Value::Expr(Expr::sym("x")), &number).unwrap();
        assert_eq!(show(&out), "x 2*x 3*x");
        let out = binop(&a, BinOp::Div, &Value::int(2), &number).unwrap();
        assert_eq!(show(&out), "1/2 1 3/2");
        assert_eq!(num(1), Some(Expr::int(1)));
    }

    #[test]
    fn integers_overflow_loudly_and_never_mix() {
        let a = vec_of(DType::I8, &[100, 1]);
        assert!(binop(&a, BinOp::Add, &a, &number).is_err_and(|e| e.message.contains("overflow")));
        let f = vec_of(DType::F64, &[1, 2]);
        let err = binop(&a, BinOp::Add, &f, &number).unwrap_err();
        assert!(err.message.contains("never mixed"), "{}", err.message);
        // A literal that fits is fine; one that does not is an error, not a silent conversion.
        assert_eq!(show(&binop(&a, BinOp::Add, &Value::int(5), &number).unwrap()), "105 6");
        let half = Value::Expr(Expr::rational(1, 2).unwrap());
        assert!(binop(&a, BinOp::Mul, &half, &number).is_err());
        assert!(binop(&a, BinOp::Div, &Value::int(2), &number).is_err());
    }

    #[test]
    fn floats_and_casts() {
        let a = vec_of(DType::F32, &[1, 3]);
        let third = binop(&a, BinOp::Div, &Value::int(3), &number).unwrap();
        assert_eq!(show(&third), "0.3333333432674408 1.0");
        let Value::Array(arr) = vec_of(DType::Exact, &[1, 2]) else { return };
        assert_eq!(arr.cast(DType::F64).unwrap().dtype, DType::F64);
        let half = Array::from_exprs(DType::Exact, 1, None, &[Expr::rational(1, 2).unwrap()]).unwrap();
        assert!(half.cast(DType::I32).is_err());
        let sym = Array::from_exprs(DType::Exact, 1, None, &[Expr::sym("x")]).unwrap();
        assert!(sym.cast(DType::F64).is_err());
    }

    #[test]
    fn matrices() {
        let m = build(DType::Exact, &Value::list(vec![Value::list(vec![Value::int(1), Value::int(2)]), Value::list(vec![Value::int(3), Value::int(4)])]), true)
            .unwrap();
        assert_eq!(m.det().unwrap().to_string(), "-2");
        assert_eq!(show(&Value::Array(Arc::new(m.inverse().unwrap()))), "-2 1 3/2 -1/2");
        let sq = m.matmul(&m).unwrap();
        assert_eq!(show(&sq), "7 10 15 22");
        let f = m.cast(DType::F64).unwrap();
        assert_eq!(f.det().unwrap().to_string(), "-2.0");
        let inv = f.inverse().unwrap();
        assert!((inv.elem(0).and_then(|e| e.as_num().map(mathcraft_core::Num::to_f64)).unwrap_or(0.0) + 2.0).abs() < 1e-12);
        let b = Array::from_exprs(DType::F64, 2, None, &[Expr::int(5), Expr::int(6)]).unwrap();
        let x = f.solve(&b).unwrap();
        assert!((x.elem(0).and_then(|e| e.as_num().map(mathcraft_core::Num::to_f64)).unwrap_or(0.0) + 4.0).abs() < 1e-12);
        let t = m.transpose();
        assert_eq!(show(&Value::Array(Arc::new(t))), "1 3 2 4");
        assert!(Array::from_exprs(DType::I32, 2, Some(2), &vec![Expr::int(1); 4]).unwrap().det().is_err());
        assert!(f.matmul(&m).is_err());
    }
}
