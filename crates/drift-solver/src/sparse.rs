//! Sparse symmetric positive-definite solves for the normal equations `(JᵀJ + D) Δ = r`.
//!
//! Sketches are mostly chains and loops of entities, so after a Reverse Cuthill–McKee
//! reordering the matrix has a narrow envelope. An envelope (skyline) Cholesky factorisation
//! then costs O(n · b²) for bandwidth b, instead of O(n³) for a dense factorisation, and never
//! fills in outside the envelope.
//!
//! "Hub" unknowns shared by very many equations (e.g. one y coordinate that a long run of
//! horizontal constraints has merged) would widen the envelope of everything ordered after
//! them, so they are ordered last: each adds one long row, nothing more.

// Factorisations index several parallel arrays by row/column; explicit indices read clearer.
#![allow(clippy::needless_range_loop)]

use std::collections::VecDeque;

/// A number held to about 32 digits as the sum of two doubles, for the normal matrix and
/// its right-hand sides. The minimum-norm step regularises the relations' null space with a
/// weight of 1e-10 of their own, so a solve amplifies round-off in those directions 1e10
/// times: the step would land where the relations don't care to a part in 1e-6, which is
/// where the rounding of the moment happened to put it (the same sketch shifted across the
/// plane rounds differently; #94). Accumulated exactly, the right-hand sides and matrix let
/// one step of iterative refinement take that out ([`Factor::solve_refined`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dd {
    pub hi: f64,
    pub lo: f64,
}

/// `a + b` as a sum and its rounding error (Knuth's TwoSum).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let v = s - a;
    (s, (a - (s - v)) + (b - v))
}

impl Dd {
    /// Add a double exactly.
    pub fn add(self, b: f64) -> Self {
        let (hi, e) = two_sum(self.hi, b);
        let (hi, lo) = two_sum(hi, self.lo + e);
        Self { hi, lo }
    }

    /// Add the product `a·b` exactly.
    pub fn add_product(self, a: f64, b: f64) -> Self {
        let p = a * b;
        self.add(p).add(a.mul_add(b, -p))
    }

    pub fn sub(self, other: Self) -> Self {
        self.add(-other.hi).add(-other.lo)
    }
}

/// Lower-triangular envelope storage of a symmetric matrix, in a bandwidth-reducing order.
#[derive(Clone, Debug)]
pub struct Skyline {
    n: usize,
    /// Original index → permuted index.
    to_new: Vec<usize>,
    /// For permuted row i: first stored column.
    first: Vec<usize>,
    /// For permuted row i: offset of `(i, first[i])` in `values`.
    offset: Vec<usize>,
    values: Vec<f64>,
    /// The part of each value below double precision (see [`Dd`]).
    low: Vec<f64>,
}

impl Skyline {
    /// Build the structure for an `n × n` matrix whose off-diagonal non-zeros are given as
    /// adjacency lists (`neighbors[i]` contains `j` if `A[i][j] ≠ 0`). Values start at zero.
    pub fn new(n: usize, neighbors: &[Vec<usize>]) -> Self {
        let order = ordering(n, neighbors);
        let mut to_new = vec![0; n];
        for (new, &old) in order.iter().enumerate() {
            to_new[old] = new;
        }
        let mut first: Vec<usize> = (0..n).collect();
        for (old, ns) in neighbors.iter().enumerate() {
            let i = to_new[old];
            for &nb in ns {
                let j = to_new[nb];
                if j < i {
                    first[i] = first[i].min(j);
                }
            }
        }
        let mut offset = Vec::with_capacity(n);
        let mut len = 0;
        for i in 0..n {
            offset.push(len);
            len += i - first[i] + 1;
        }
        Self {
            n,
            to_new,
            first,
            offset,
            values: vec![0.0; len],
            low: vec![0.0; len],
        }
    }

    pub fn clear(&mut self) {
        self.values.iter_mut().for_each(|v| *v = 0.0);
        self.low.iter_mut().for_each(|v| *v = 0.0);
    }

    #[inline]
    fn index(&self, i: usize, j: usize) -> usize {
        debug_assert!(j <= i && j >= self.first[i]);
        self.offset[i] + (j - self.first[i])
    }

    /// Add `v` to `A[i][j]` (original indices). Only one of `(i, j)` / `(j, i)` needs adding
    /// for off-diagonal entries; the matrix is symmetric.
    #[cfg(test)]
    pub fn add(&mut self, i: usize, j: usize, v: f64) {
        let k = self.slot(i, j);
        let sum = Dd {
            hi: self.values[k],
            lo: self.low[k],
        }
        .add(v);
        (self.values[k], self.low[k]) = (sum.hi, sum.lo);
    }

    /// Add the product `a·b` to `A[i][j]` (original indices), exactly.
    pub fn add_product(&mut self, i: usize, j: usize, a: f64, b: f64) {
        let k = self.slot(i, j);
        let sum = Dd {
            hi: self.values[k],
            lo: self.low[k],
        }
        .add_product(a, b);
        (self.values[k], self.low[k]) = (sum.hi, sum.lo);
    }

    fn slot(&self, i: usize, j: usize) -> usize {
        let (a, b) = (self.to_new[i], self.to_new[j]);
        let (r, c) = if a >= b { (a, b) } else { (b, a) };
        self.index(r, c)
    }

    /// `(A + diag(shift)) x`, to double-double precision (original ordering).
    fn multiply(&self, shift: &[f64], x: &[f64]) -> Vec<Dd> {
        let n = self.n;
        let mut xp = vec![0.0; n];
        for (old, &new) in self.to_new.iter().enumerate() {
            xp[new] = x[old];
        }
        let mut y = vec![Dd::default(); n];
        for i in 0..n {
            for j in self.first[i]..=i {
                let k = self.index(i, j);
                let (hi, lo) = (self.values[k], self.low[k]);
                y[i] = y[i].add_product(hi, xp[j]).add(lo * xp[j]);
                if j != i {
                    y[j] = y[j].add_product(hi, xp[i]).add(lo * xp[i]);
                }
            }
        }
        let mut out = vec![Dd::default(); n];
        for (old, &new) in self.to_new.iter().enumerate() {
            out[old] = y[new].add_product(shift[old], x[old]);
        }
        out
    }

    pub fn diagonal(&self, i: usize) -> f64 {
        let r = self.to_new[i];
        self.values[self.index(r, r)]
    }

    /// Factor a copy of `A + diag(shift)` and solve `(A + diag(shift)) x = b` in place
    /// (original ordering). Returns false if the matrix is not numerically positive definite.
    #[cfg(test)]
    pub fn solve(&self, shift: &[f64], b: &mut [f64]) -> bool {
        match self.factor(shift) {
            Some(f) => {
                f.solve(b);
                true
            }
            None => false,
        }
    }

    /// The Cholesky factor of `A + diag(shift)` (original ordering for `shift`), for any
    /// number of solves against it; `None` if the matrix is not numerically positive
    /// definite.
    pub fn factor(&self, shift: &[f64]) -> Option<Factor<'_>> {
        let n = self.n;
        let mut l = self.values.clone();
        for (old, s) in shift.iter().enumerate() {
            let r = self.to_new[old];
            let k = self.index(r, r);
            l[k] += s;
        }
        for i in 0..n {
            let fi = self.first[i];
            for j in fi..=i {
                let fj = self.first[j];
                let start = fi.max(fj);
                let mut s = l[self.index(i, j)];
                let (oi, oj) = (self.offset[i] - fi, self.offset[j] - fj);
                for k in start..j {
                    s -= l[oi + k] * l[oj + k];
                }
                if i == j {
                    if s.is_nan() || s <= 0.0 || s.is_infinite() {
                        return None;
                    }
                    l[self.index(i, i)] = s.sqrt();
                } else {
                    l[self.index(i, j)] = s / l[self.index(j, j)];
                }
            }
        }
        Some(Factor {
            matrix: self,
            shift: shift.to_vec(),
            l,
        })
    }
}

/// A Cholesky factor `L Lᵀ` of a [`Skyline`] matrix (plus a diagonal shift), from
/// [`Skyline::factor`].
pub struct Factor<'a> {
    matrix: &'a Skyline,
    shift: Vec<f64>,
    l: Vec<f64>,
}

impl Factor<'_> {
    /// Solve `(A + diag(shift)) x = b` for a right-hand side held to double-double
    /// precision, with one step of iterative refinement: the residual of the first solution
    /// is taken against the matrix's exact values, so the refinement corrects the round-off
    /// the solve amplified (see [`Dd`]).
    pub fn solve_refined(&self, b: &[Dd]) -> Vec<f64> {
        let mut x: Vec<f64> = b.iter().map(|v| v.hi).collect();
        self.solve(&mut x);
        let product = self.matrix.multiply(&self.shift, &x);
        let mut correction: Vec<f64> = b
            .iter()
            .zip(&product)
            .map(|(b, ax)| b.sub(*ax).hi)
            .collect();
        self.solve(&mut correction);
        for (x, c) in x.iter_mut().zip(&correction) {
            *x += c;
        }
        x
    }

    /// Solve `L Lᵀ x = b` in place (original ordering).
    pub fn solve(&self, b: &mut [f64]) {
        let m = self.matrix;
        let l = &self.l;
        let n = m.n;
        // Permute b into the new order, solve L Lᵀ x = b, permute back.
        let mut x = vec![0.0; n];
        for (old, &new) in m.to_new.iter().enumerate() {
            x[new] = b[old];
        }
        for i in 0..n {
            let fi = m.first[i];
            let oi = m.offset[i] - fi;
            let mut s = x[i];
            for k in fi..i {
                s -= l[oi + k] * x[k];
            }
            x[i] = s / l[m.index(i, i)];
        }
        for i in (0..n).rev() {
            x[i] /= l[m.index(i, i)];
            let xi = x[i];
            let fi = m.first[i];
            let oi = m.offset[i] - fi;
            for k in fi..i {
                x[k] -= l[oi + k] * xi;
            }
        }
        for (old, &new) in m.to_new.iter().enumerate() {
            b[old] = x[new];
        }
    }
}

/// Rank and free unknowns of a sparse matrix `J` (rows of `(column, value)` over `n` columns,
/// with columns scaled to unit length), by row-wise Givens QR in the envelope of `JᵀJ`.
///
/// QR works on `J` directly, so rounding errors scale with cond(J) rather than cond(J)², which
/// matters for rank decisions on nearly-parallel constraints. Returns the numerical rank and,
/// per column, whether the unknown can move (has a component in the null space of `J`).
pub fn qr_rank(rows: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for row in rows {
        for &(c1, _) in row {
            for &(c2, _) in row {
                if c1 != c2 {
                    neighbors[c1].push(c2);
                }
            }
        }
    }
    for ns in &mut neighbors {
        ns.sort_unstable();
        ns.dedup();
    }
    let sky = Skyline::new(n, &neighbors);
    // R row k spans columns k..=last[k], where last[k] is the furthest column whose envelope
    // reaches back to k. Rotations never create entries outside this profile.
    let mut last = vec![0; n];
    let mut reach = 0;
    let mut by_first: Vec<(usize, usize)> = (0..n).map(|j| (sky.first[j], j)).collect();
    by_first.sort_unstable();
    let mut idx = 0;
    for (k, slot) in last.iter_mut().enumerate() {
        while idx < n && by_first[idx].0 <= k {
            reach = reach.max(by_first[idx].1);
            idx += 1;
        }
        *slot = reach.max(k);
    }
    let mut row_start = Vec::with_capacity(n);
    let mut len = 0;
    for k in 0..n {
        row_start.push(len);
        len += last[k] - k + 1;
    }
    let mut r = vec![0.0; len];
    let mut filled = vec![false; n];
    let at = |k: usize, j: usize| row_start[k] + (j - k);

    let mut w = vec![0.0; n];
    for row in rows {
        let mut lo = n;
        let mut hi = 0;
        let mut scale = 0.0f64;
        for &(c, g) in row {
            let c = sky.to_new[c];
            w[c] += g;
            lo = lo.min(c);
            hi = hi.max(c);
            scale = scale.max(g.abs());
        }
        let noise = 1e-13 * scale;
        let mut k = lo;
        while k <= hi && k < n {
            let wk = w[k];
            if wk.abs() <= noise {
                w[k] = 0.0;
                k += 1;
                continue;
            }
            if !filled[k] {
                for j in k..=hi.max(k) {
                    r[at(k, j)] = w[j];
                    w[j] = 0.0;
                }
                filled[k] = true;
                hi = 0;
                break;
            }
            let rkk = r[at(k, k)];
            let h = rkk.hypot(wk);
            let (c, s) = (rkk / h, wk / h);
            for j in k..=last[k] {
                let (a, b) = (r[at(k, j)], w[j]);
                r[at(k, j)] = c * a + s * b;
                w[j] = -s * a + c * b;
            }
            w[k] = 0.0;
            hi = hi.max(last[k]);
            k += 1;
        }
        for v in w.iter_mut().take(hi + 1) {
            *v = 0.0;
        }
    }

    // Columns are unit length, so a dependent column leaves a diagonal at round-off level.
    let free: Vec<bool> = (0..n)
        .map(|k| !filled[k] || r[at(k, k)].abs() <= 1e-9)
        .collect();
    let rank = free.iter().filter(|f| !**f).count();

    // One null vector per free column f: v_f = 1, other free entries 0, and the independent
    // entries from back substitution through R.
    let mut movable_new = vec![false; n];
    let mut v = vec![0.0; n];
    for f in (0..n).filter(|&k| free[k]) {
        v.iter_mut().for_each(|x| *x = 0.0);
        v[f] = 1.0;
        for k in (0..f).rev() {
            if free[k] {
                continue;
            }
            let mut s = 0.0;
            for j in k + 1..=last[k] {
                s += r[at(k, j)] * v[j];
            }
            v[k] = -s / r[at(k, k)];
        }
        let scale = v.iter().fold(1.0f64, |m, x| m.max(x.abs()));
        for (c, x) in v.iter().enumerate() {
            if x.abs() > 1e-7 * scale {
                movable_new[c] = true;
            }
        }
    }
    let movable = (0..n).map(|old| movable_new[sky.to_new[old]]).collect();
    (rank, movable)
}

/// Nodes with more neighbours than this (and well above average) are ordered last.
const HUB_DEGREE: usize = 24;

/// Reverse Cuthill–McKee over ordinary nodes, then hubs. Returns new → old.
fn ordering(n: usize, neighbors: &[Vec<usize>]) -> Vec<usize> {
    let average = neighbors.iter().map(Vec::len).sum::<usize>() as f64 / n.max(1) as f64;
    let is_hub: Vec<bool> = neighbors
        .iter()
        .map(|ns| ns.len() > HUB_DEGREE && ns.len() as f64 > 4.0 * average)
        .collect();
    let pruned: Vec<Vec<usize>> = neighbors
        .iter()
        .map(|ns| ns.iter().copied().filter(|&j| !is_hub[j]).collect())
        .collect();
    let mut order: Vec<usize> = reverse_cuthill_mckee(n, &pruned, &is_hub);
    order.extend((0..n).filter(|&i| is_hub[i]));
    order
}

/// Reverse Cuthill–McKee ordering of the non-excluded nodes (handles disconnected graphs).
fn reverse_cuthill_mckee(n: usize, neighbors: &[Vec<usize>], excluded: &[bool]) -> Vec<usize> {
    let degree = |i: usize| neighbors[i].len();
    let mut visited = excluded.to_vec();
    let mut order = Vec::with_capacity(n);
    let mut by_degree: Vec<usize> = (0..n).collect();
    by_degree.sort_by_key(|&i| degree(i));
    for &start in &by_degree {
        if visited[start] {
            continue;
        }
        // Start from a pseudo-peripheral node: the last node reached by a BFS from `start`.
        let root = farthest(start, neighbors, &visited).unwrap_or(start);
        let mut queue = VecDeque::from([root]);
        visited[root] = true;
        while let Some(i) = queue.pop_front() {
            order.push(i);
            let mut next: Vec<usize> = neighbors[i]
                .iter()
                .copied()
                .filter(|&j| !visited[j])
                .collect();
            next.sort_by_key(|&j| degree(j));
            next.dedup();
            for j in next {
                if !visited[j] {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
    }
    order.reverse();
    order
}

/// The last node of a BFS from `start` among unvisited nodes (a cheap peripheral estimate).
fn farthest(start: usize, neighbors: &[Vec<usize>], visited: &[bool]) -> Option<usize> {
    let mut seen = std::collections::HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut last = None;
    while let Some(i) = queue.pop_front() {
        last = Some(i);
        for &j in &neighbors[i] {
            if !visited[j] && seen.insert(j) {
                queue.push_back(j);
            }
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linalg::cholesky_solve;

    /// A random-ish sparse SPD matrix: a scrambled tridiagonal "chain" plus extra couplings.
    #[test]
    fn matches_dense_cholesky() {
        let n = 40;
        let perm: Vec<usize> = (0..n).map(|i| (i * 17 + 5) % n).collect();
        let mut dense = vec![0.0; n * n];
        let mut neighbors = vec![Vec::new(); n];
        let mut link = |a: usize, b: usize, v: f64, dense: &mut Vec<f64>| {
            dense[a * n + b] += v;
            dense[b * n + a] += v;
            neighbors[a].push(b);
            neighbors[b].push(a);
        };
        for k in 0..n - 1 {
            link(perm[k], perm[k + 1], -1.0, &mut dense);
        }
        link(perm[0], perm[n - 1], -0.5, &mut dense); // closes a loop
        link(perm[3], perm[20], 0.25, &mut dense);
        for i in 0..n {
            dense[i * n + i] += 4.0 + (i % 3) as f64;
        }

        let mut sky = Skyline::new(n, &neighbors);
        for i in 0..n {
            for j in 0..=i {
                let v = dense[i * n + j];
                if v != 0.0 {
                    sky.add(i, j, v);
                }
            }
        }
        let b: Vec<f64> = (0..n).map(|i| (i as f64 * 0.37).sin()).collect();
        let mut x_sparse = b.clone();
        assert!(sky.solve(&vec![0.0; n], &mut x_sparse));
        let mut x_dense = b.clone();
        assert!(cholesky_solve(&mut dense.clone(), n, &mut x_dense));
        for (s, d) in x_sparse.iter().zip(&x_dense) {
            assert!((s - d).abs() < 1e-12, "{s} vs {d}");
        }
    }

    #[test]
    fn chain_has_narrow_envelope() {
        // A chain given in scrambled order: RCM should recover bandwidth 1.
        let n = 100;
        let perm: Vec<usize> = (0..n).map(|i| (i * 37 + 11) % n).collect();
        let mut neighbors = vec![Vec::new(); n];
        for k in 0..n - 1 {
            neighbors[perm[k]].push(perm[k + 1]);
            neighbors[perm[k + 1]].push(perm[k]);
        }
        let sky = Skyline::new(n, &neighbors);
        assert!(
            sky.values.len() <= 2 * n,
            "envelope size {}",
            sky.values.len()
        );
    }

    #[test]
    fn hubs_do_not_widen_the_envelope() {
        // A chain plus one node connected to everything.
        let n = 200;
        let hub = n - 1;
        let mut neighbors = vec![Vec::new(); n];
        for k in 0..hub - 1 {
            neighbors[k].push(k + 1);
            neighbors[k + 1].push(k);
        }
        for k in 0..hub {
            neighbors[k].push(hub);
            neighbors[hub].push(k);
        }
        let sky = Skyline::new(n, &neighbors);
        assert!(
            sky.values.len() <= 3 * n + 10,
            "envelope size {}",
            sky.values.len()
        );
    }
}
