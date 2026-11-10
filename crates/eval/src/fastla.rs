//! Dense `f64` linear algebra for large matrices, from `faer` (blocked, SIMD, threaded). Every function returns `None`
//! when the feature is off, the matrix is small (the plain loops are as fast there) or the result is not finite, and
//! the caller then uses its own loops, which also produce the friendly "singular matrix" errors.

/// Smallest `rows * inner * cols` for which a matrix product goes to faer.
#[cfg(feature = "fast-linalg")]
const MATMUL_MIN_WORK: usize = 20_000;
/// Smallest dimension for det / inverse / solve.
#[cfg(feature = "fast-linalg")]
const SOLVE_MIN_N: usize = 24;

#[cfg(feature = "fast-linalg")]
mod imp {
    use super::{MATMUL_MIN_WORK, SOLVE_MIN_N};
    use faer::Mat;
    use faer::linalg::solvers::{DenseSolveCore, Solve};

    fn finite(v: &[f64]) -> bool {
        v.iter().all(|x| x.is_finite())
    }

    fn to_mat(rows: &[Vec<f64>]) -> Mat<f64> {
        Mat::from_fn(rows.len(), rows.first().map_or(0, Vec::len), |i, j| rows.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0))
    }

    fn from_mat(m: &Mat<f64>) -> Vec<f64> {
        let mut out = Vec::with_capacity(m.nrows() * m.ncols());
        for i in 0..m.nrows() {
            for j in 0..m.ncols() {
                out.push(m[(i, j)]);
            }
        }
        out
    }

    /// `a` is `lr x lc`, `b` is `lc x rc`, both row-major; the product is row-major.
    pub fn matmul(a: &[f64], b: &[f64], lr: usize, lc: usize, rc: usize) -> Option<Vec<f64>> {
        if lr.saturating_mul(lc).saturating_mul(rc) < MATMUL_MIN_WORK || a.len() != lr * lc || b.len() != lc * rc {
            return None;
        }
        let am = Mat::<f64>::from_fn(lr, lc, |i, j| a.get(i * lc + j).copied().unwrap_or(0.0));
        let bm = Mat::<f64>::from_fn(lc, rc, |i, j| b.get(i * rc + j).copied().unwrap_or(0.0));
        let out = from_mat(&(&am * &bm));
        finite(&out).then_some(out)
    }

    pub fn det(rows: &[Vec<f64>]) -> Option<f64> {
        (rows.len() >= SOLVE_MIN_N).then(|| to_mat(rows).determinant()).filter(|d| d.is_finite())
    }

    pub fn inverse(rows: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
        if rows.len() < SOLVE_MIN_N {
            return None;
        }
        let inv = to_mat(rows).partial_piv_lu().inverse();
        let flat = from_mat(&inv);
        finite(&flat).then(|| flat.chunks(rows.len()).map(<[f64]>::to_vec).collect())
    }

    pub fn solve(a: &[Vec<f64>], b: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
        if a.len() < SOLVE_MIN_N {
            return None;
        }
        let x = to_mat(a).partial_piv_lu().solve(&to_mat(b));
        let flat = from_mat(&x);
        finite(&flat).then(|| flat.chunks(b.first().map_or(1, Vec::len).max(1)).map(<[f64]>::to_vec).collect())
    }
}

#[cfg(not(feature = "fast-linalg"))]
mod imp {
    pub fn matmul(_: &[f64], _: &[f64], _: usize, _: usize, _: usize) -> Option<Vec<f64>> {
        None
    }
    pub fn det(_: &[Vec<f64>]) -> Option<f64> {
        None
    }
    pub fn inverse(_: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
        None
    }
    pub fn solve(_: &[Vec<f64>], _: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
        None
    }
}

pub use imp::{det, inverse, matmul, solve};

#[cfg(all(test, feature = "fast-linalg"))]
mod tests {
    use super::*;

    #[test]
    fn matches_the_plain_loops() {
        let n = 40;
        let a: Vec<f64> = (0..n * n).map(|i| ((i * 7) % 11) as f64 * 0.5 - 2.0).collect();
        let b: Vec<f64> = (0..n * n).map(|i| ((i * 5) % 13) as f64 * 0.25).collect();
        let fast = matmul(&a, &b, n, n, n).unwrap();
        for (i, j) in [(0, 0), (3, 17), (39, 39)] {
            let want: f64 = (0..n).map(|t| a[i * n + t] * b[t * n + j]).sum();
            assert!((fast[i * n + j] - want).abs() < 1e-9);
        }
        assert!(matmul(&a, &b, 2, 2, 2).is_none(), "small products stay on the plain loops");
        let m: Vec<Vec<f64>> = (0..30).map(|i| (0..30).map(|j| if i == j { 5.0 } else { 1.0 / (1.0 + (i + j) as f64) }).collect()).collect();
        let inv = inverse(&m).unwrap();
        let prod: f64 = (0..30).map(|t| m[2][t] * inv[t][2]).sum();
        assert!((prod - 1.0).abs() < 1e-9);
        assert!(det(&m).unwrap() > 0.0);
        let singular: Vec<Vec<f64>> = (0..30).map(|_| vec![1.0; 30]).collect();
        assert!(inverse(&singular).is_none(), "singular matrices fall back to the loops and their error message");
    }
}
