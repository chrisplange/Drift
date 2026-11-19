//! Small dense linear algebra: just what the solver needs, in pure Rust (WASM-friendly).

/// Row-major dense matrix.
#[derive(Clone, Debug)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Matrix {
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            data: vec![0.0; rows * cols],
        }
    }

    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }

    #[inline]
    pub fn at_mut(&mut self, r: usize, c: usize) -> &mut f64 {
        &mut self.data[r * self.cols + c]
    }
}

#[cfg(test)]
/// Solve `A x = b` for symmetric positive-definite `A` (n × n, row-major) by Cholesky
/// decomposition. Overwrites `a` with the factor and `b` with the solution. Returns false if
/// `A` is not numerically positive definite.
pub fn cholesky_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= a[j * n + k] * a[j * n + k];
        }
        if d.is_nan() || d <= 0.0 || d.is_infinite() {
            return false;
        }
        let d = d.sqrt();
        a[j * n + j] = d;
        for i in j + 1..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    // Forward substitution: L y = b.
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    // Back substitution: Lᵀ x = y.
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in i + 1..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    true
}

/// Reduce `m` to reduced row echelon form in place with partial pivoting. Entries with
/// magnitude at or below `tol` are treated as zero. Returns the pivot column of each
/// non-zero row, in row order; its length is the numerical rank.
pub fn rref(m: &mut Matrix, tol: f64) -> Vec<usize> {
    let (rows, cols) = (m.rows, m.cols);
    let mut pivots = Vec::new();
    let mut row = 0;
    for col in 0..cols {
        if row == rows {
            break;
        }
        // Pick the largest entry in this column at or below `row`.
        let (best, best_abs) = (row..rows)
            .map(|r| (r, m.at(r, col).abs()))
            .fold((row, -1.0), |acc, x| if x.1 > acc.1 { x } else { acc });
        if best_abs <= tol {
            for r in row..rows {
                *m.at_mut(r, col) = 0.0;
            }
            continue;
        }
        if best != row {
            for c in 0..cols {
                m.data.swap(best * cols + c, row * cols + c);
            }
        }
        let p = m.at(row, col);
        for c in col..cols {
            *m.at_mut(row, c) /= p;
        }
        for r in 0..rows {
            if r != row {
                let f = m.at(r, col);
                if f != 0.0 {
                    for c in col..cols {
                        let v = m.at(row, c);
                        *m.at_mut(r, c) -= f * v;
                    }
                }
            }
        }
        pivots.push(col);
        row += 1;
    }
    pivots
}

/// The eigenvalues and eigenvectors of a small symmetric matrix (`a`, `n × n`, row-major,
/// overwritten), by Jacobi's rotations: the vectors as the columns of the row-major matrix
/// returned, `a = V·diag(values)·Vᵀ`.
pub fn symmetric_eigen(a: &mut [f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _ in 0..64 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i * n + j] * a[i * n + j])
            .sum();
        let diag: f64 = (0..n).map(|i| a[i * n + i] * a[i * n + i]).sum();
        if off <= 1e-30 * diag || off == 0.0 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cholesky_solves_spd_system() {
        // A = [[4, 2], [2, 3]], b = [2, 1] → x = [0.5, 0]
        let mut a = vec![4.0, 2.0, 2.0, 3.0];
        let mut b = vec![2.0, 1.0];
        assert!(cholesky_solve(&mut a, 2, &mut b));
        assert!((b[0] - 0.5).abs() < 1e-15 && b[1].abs() < 1e-15);
        let mut singular = vec![1.0, 1.0, 1.0, 1.0];
        assert!(!cholesky_solve(&mut singular, 2, &mut [1.0, 1.0]));
    }

    #[test]
    fn symmetric_eigen_rebuilds_its_matrix() {
        let a = [4.0, 1.0, -2.0, 1.0, 3.0, 0.5, -2.0, 0.5, -1.0];
        let mut work = a;
        let (values, v) = symmetric_eigen(&mut work, 3);
        for i in 0..3 {
            for j in 0..3 {
                let rebuilt: f64 = (0..3).map(|k| v[i * 3 + k] * values[k] * v[j * 3 + k]).sum();
                assert!((rebuilt - a[i * 3 + j]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn rref_finds_rank_and_pivots() {
        let mut m = Matrix {
            rows: 3,
            cols: 3,
            data: vec![1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 0.0, 1.0, 1.0],
        };
        let pivots = rref(&mut m, 1e-12);
        assert_eq!(pivots, vec![0, 1]);
        assert!((m.at(0, 2) - 1.0).abs() < 1e-12); // x0 + x2 = 0 after reduction
        assert!((m.at(1, 2) - 1.0).abs() < 1e-12);
    }
}
