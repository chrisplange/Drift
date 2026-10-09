//! Residual equations with analytic gradients.
//!
//! Each equation reads parameter values through a lookup and pushes `(param, ∂r/∂param)` pairs.
//! Formulations are chosen to be well conditioned: lengths are differences of lengths (not of
//! squares), and angles use `sin(θ − φ)` from normalised cross/dot products, so they have no
//! `atan2` discontinuity and treat lines as undirected.

/// A length-like quantity: the distance between two points, or a radius parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// `|b − a|` with params `[ax, ay, bx, by]`.
    Points([usize; 4]),
    /// A radius parameter.
    Param(usize),
}

impl Length {
    fn eval(&self, val: &impl Fn(usize) -> f64, sign: f64, grad: &mut Vec<(usize, f64)>) -> f64 {
        match *self {
            Length::Points([ax, ay, bx, by]) => {
                let dx = val(bx) - val(ax);
                let dy = val(by) - val(ay);
                let len = dx.hypot(dy);
                let (ux, uy) = if len > 0.0 {
                    (dx / len, dy / len)
                } else {
                    (1.0, 0.0)
                };
                grad.extend([
                    (ax, -sign * ux),
                    (ay, -sign * uy),
                    (bx, sign * ux),
                    (by, sign * uy),
                ]);
                len
            }
            Length::Param(p) => {
                grad.push((p, sign));
                val(p)
            }
        }
    }

    pub fn params(&self) -> Vec<usize> {
        match *self {
            Length::Points(p) => p.to_vec(),
            Length::Param(p) => vec![p],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Equation {
    /// `length − value = 0`
    LengthValue { length: Length, value: f64 },
    /// `a − b = 0`
    EqualLength { a: Length, b: Length },
    /// `sign·(b − a) − value = 0` for two scalar parameters (horizontal / vertical distances).
    /// `sign` (±1) records which side `b` was on, so the solution doesn't flip over.
    Difference {
        a: usize,
        b: usize,
        sign: f64,
        value: f64,
    },
    /// Signed perpendicular distance from point `[px, py]` to the line through
    /// `[ax, ay, bx, by]`: `sign·cross(b − a, p − a)/|b − a| − value = 0`.
    PointLine {
        point: [usize; 2],
        line: [usize; 4],
        sign: f64,
        value: f64,
    },
    /// Line tangent to a circle: `sign·cross(b − a, c − a)/|b − a| − radius = 0`, i.e. the
    /// centre `c` lies one radius from the line, on the side recorded by `sign`.
    TangentLine {
        center: [usize; 2],
        line: [usize; 4],
        sign: f64,
        radius: Length,
    },
    /// `Σ kᵢ·lengthᵢ = 0` (e.g. circle–circle tangency `|c₁c₂| − r₁ − r₂ = 0`).
    LengthSum { terms: [(Length, f64); 3] },
    /// Symmetry, part 1: `(b − a)·d/|d| = 0` — the points' join is perpendicular to the axis.
    SymmetricPerpendicular {
        a: [usize; 2],
        b: [usize; 2],
        axis: [usize; 4],
    },
    /// Symmetry, part 2: the points' midpoint lies on the axis.
    SymmetricMidpoint {
        a: [usize; 2],
        b: [usize; 2],
        axis: [usize; 4],
    },
    /// `p − (a + b)/2 = 0` for one coordinate: a point at the midpoint of two others.
    Midpoint { p: usize, a: usize, b: usize },
    /// A point on an ellipse. With `M = major − center`, `d = point − center` and
    /// `b = |minor − center|`, stretching across the major axis by `a/b` makes the ellipse a
    /// circle of radius `a = |M|`: `√((d·M/a)² + (M×d/b)²) − a = 0`.
    OnEllipse {
        point: [usize; 2],
        center: [usize; 2],
        major: [usize; 2],
        minor: [usize; 2],
    },
    /// `sin(θ − φ) = 0`, where θ is the angle from line 1 to line 2. Params are
    /// `[ax, ay, bx, by]` for each line.
    Angle {
        l1: [usize; 4],
        l2: [usize; 4],
        sin_phi: f64,
        cos_phi: f64,
    },
}

impl Equation {
    pub fn angle(l1: [usize; 4], l2: [usize; 4], phi_radians: f64) -> Self {
        Equation::Angle {
            l1,
            l2,
            sin_phi: phi_radians.sin(),
            cos_phi: phi_radians.cos(),
        }
    }

    /// Whether the residual is an angle (unitless) rather than a length (mm).
    pub fn is_angular(&self) -> bool {
        matches!(self, Equation::Angle { .. })
    }

    pub fn params(&self) -> Vec<usize> {
        match self {
            Equation::LengthValue { length, .. } => length.params(),
            Equation::EqualLength { a, b } => {
                let mut p = a.params();
                p.extend(b.params());
                p
            }
            Equation::Angle { l1, l2, .. } => l1.iter().chain(l2).copied().collect(),
            Equation::Difference { a, b, .. } => vec![*a, *b],
            Equation::Midpoint { p, a, b } => vec![*p, *a, *b],
            Equation::TangentLine {
                center,
                line,
                radius,
                ..
            } => {
                let mut v: Vec<usize> = center.iter().chain(line).copied().collect();
                v.extend(radius.params());
                v
            }
            Equation::LengthSum { terms } => terms.iter().flat_map(|(l, _)| l.params()).collect(),
            Equation::SymmetricPerpendicular { a, b, axis }
            | Equation::SymmetricMidpoint { a, b, axis } => {
                a.iter().chain(b).chain(axis).copied().collect()
            }
            Equation::PointLine { point, line, .. } => point.iter().chain(line).copied().collect(),
            Equation::OnEllipse {
                point,
                center,
                major,
                minor,
            } => point
                .iter()
                .chain(center)
                .chain(major)
                .chain(minor)
                .copied()
                .collect(),
        }
    }

    /// The residual, appending its gradient to `grad` (a parameter may appear more than once;
    /// callers must sum duplicates).
    pub fn eval(&self, val: &impl Fn(usize) -> f64, grad: &mut Vec<(usize, f64)>) -> f64 {
        match *self {
            Equation::OnEllipse {
                point,
                center,
                major,
                minor,
            } => {
                let at = |p: [usize; 2]| (val(p[0]), val(p[1]));
                let (cx, cy) = at(center);
                let (px, py) = at(point);
                let (mx, my) = at(major);
                let (nx, ny) = at(minor);
                let (dx, dy) = (px - cx, py - cy);
                let (ex, ey) = (mx - cx, my - cy);
                let (fx, fy) = (nx - cx, ny - cy);
                let a = ex.hypot(ey).max(1e-12);
                let b = fx.hypot(fy).max(1e-12);
                let dot = dx * ex + dy * ey;
                let cross = ex * dy - ey * dx;
                let u = dot / a;
                let w = cross / b;
                let r = u.hypot(w).max(1e-12);
                // ∂/∂d, ∂/∂M and ∂/∂b of √(u² + w²) − a.
                let gd = [(u * ex / a - w * ey / b) / r, (u * ey / a + w * ex / b) / r];
                let gm = [
                    (u * (dx / a - dot * ex / (a * a * a)) + w * dy / b) / r - ex / a,
                    (u * (dy / a - dot * ey / (a * a * a)) - w * dx / b) / r - ey / a,
                ];
                let gb = -(w * w / b) / r;
                let gn = [gb * fx / b, gb * fy / b];
                grad.extend([
                    (point[0], gd[0]),
                    (point[1], gd[1]),
                    (major[0], gm[0]),
                    (major[1], gm[1]),
                    (minor[0], gn[0]),
                    (minor[1], gn[1]),
                    (center[0], -(gd[0] + gm[0] + gn[0])),
                    (center[1], -(gd[1] + gm[1] + gn[1])),
                ]);
                r - a
            }
            Equation::LengthValue { length, value } => length.eval(val, 1.0, grad) - value,
            Equation::EqualLength { a, b } => a.eval(val, 1.0, grad) - b.eval(val, -1.0, grad),
            Equation::Midpoint { p, a, b } => {
                grad.extend([(p, 1.0), (a, -0.5), (b, -0.5)]);
                val(p) - 0.5 * (val(a) + val(b))
            }
            Equation::TangentLine {
                center,
                line,
                sign,
                radius,
            } => point_line(val, center, line, sign, grad) - radius.eval(val, -1.0, grad),
            Equation::LengthSum { terms } => terms
                .iter()
                .map(|(length, k)| k * length.eval(val, *k, grad))
                .sum(),
            Equation::SymmetricPerpendicular {
                a: [ax, ay],
                b: [bx, by],
                axis: [px, py, qx, qy],
            } => {
                let w = (val(bx) - val(ax), val(by) - val(ay));
                let d = (val(qx) - val(px), val(qy) - val(py));
                let len = d.0.hypot(d.1).max(1e-300);
                let r = (w.0 * d.0 + w.1 * d.1) / len;
                let gd = (
                    w.0 / len - r * d.0 / (len * len),
                    w.1 / len - r * d.1 / (len * len),
                );
                grad.extend([
                    (ax, -d.0 / len),
                    (ay, -d.1 / len),
                    (bx, d.0 / len),
                    (by, d.1 / len),
                    (px, -gd.0),
                    (py, -gd.1),
                    (qx, gd.0),
                    (qy, gd.1),
                ]);
                r
            }
            Equation::SymmetricMidpoint {
                a: [ax, ay],
                b: [bx, by],
                axis: [px, py, qx, qy],
            } => {
                let d = (val(qx) - val(px), val(qy) - val(py));
                let q = (
                    0.5 * (val(ax) + val(bx)) - val(px),
                    0.5 * (val(ay) + val(by)) - val(py),
                );
                let len = d.0.hypot(d.1).max(1e-300);
                let r = (d.0 * q.1 - d.1 * q.0) / len;
                let gq = (-d.1 / len, d.0 / len);
                let gd = (
                    q.1 / len - r * d.0 / (len * len),
                    -q.0 / len - r * d.1 / (len * len),
                );
                grad.extend([
                    (ax, 0.5 * gq.0),
                    (ay, 0.5 * gq.1),
                    (bx, 0.5 * gq.0),
                    (by, 0.5 * gq.1),
                    (px, -gq.0 - gd.0),
                    (py, -gq.1 - gd.1),
                    (qx, gd.0),
                    (qy, gd.1),
                ]);
                r
            }
            Equation::Difference { a, b, sign, value } => {
                grad.extend([(a, -sign), (b, sign)]);
                sign * (val(b) - val(a)) - value
            }
            Equation::PointLine {
                point,
                line,
                sign,
                value,
            } => point_line(val, point, line, sign, grad) - value,
            Equation::Angle {
                l1,
                l2,
                sin_phi: s,
                cos_phi: c,
            } => {
                let d1 = (val(l1[2]) - val(l1[0]), val(l1[3]) - val(l1[1]));
                let d2 = (val(l2[2]) - val(l2[0]), val(l2[3]) - val(l2[1]));
                let len1sq = (d1.0 * d1.0 + d1.1 * d1.1).max(1e-300);
                let len2sq = (d2.0 * d2.0 + d2.1 * d2.1).max(1e-300);
                let norm = (len1sq * len2sq).sqrt();
                let cross = d1.0 * d2.1 - d1.1 * d2.0;
                let dot = d1.0 * d2.0 + d1.1 * d2.1;
                // r = (cross·cosφ − dot·sinφ) / (|d1||d2|) = sin(θ − φ)
                let r = (cross * c - dot * s) / norm;
                let g1 = (
                    (c * d2.1 - s * d2.0) / norm - r * d1.0 / len1sq,
                    (-c * d2.0 - s * d2.1) / norm - r * d1.1 / len1sq,
                );
                let g2 = (
                    (-c * d1.1 - s * d1.0) / norm - r * d2.0 / len2sq,
                    (c * d1.0 - s * d1.1) / norm - r * d2.1 / len2sq,
                );
                grad.extend([
                    (l1[0], -g1.0),
                    (l1[1], -g1.1),
                    (l1[2], g1.0),
                    (l1[3], g1.1),
                    (l2[0], -g2.0),
                    (l2[1], -g2.1),
                    (l2[2], g2.0),
                    (l2[3], g2.1),
                ]);
                r
            }
        }
    }
}

/// Signed distance `sign·cross(b − a, p − a)/|b − a|` from a point to a line, with gradient.
fn point_line(
    val: &impl Fn(usize) -> f64,
    [px, py]: [usize; 2],
    [ax, ay, bx, by]: [usize; 4],
    sign: f64,
    grad: &mut Vec<(usize, f64)>,
) -> f64 {
    let d = (val(bx) - val(ax), val(by) - val(ay));
    let w = (val(px) - val(ax), val(py) - val(ay));
    let len = d.0.hypot(d.1).max(1e-300);
    let cross = d.0 * w.1 - d.1 * w.0;
    let f = cross / len;
    // ∂f/∂d and ∂f/∂w; then a = −(d + w) terms, b = d terms, p = w terms.
    let gd = (
        w.1 / len - f * d.0 / (len * len),
        -w.0 / len - f * d.1 / (len * len),
    );
    let gw = (-d.1 / len, d.0 / len);
    grad.extend([
        (px, sign * gw.0),
        (py, sign * gw.1),
        (ax, -sign * (gd.0 + gw.0)),
        (ay, -sign * (gd.1 + gw.1)),
        (bx, sign * gd.0),
        (by, sign * gd.1),
    ]);
    sign * f
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;

    /// Compare the analytic gradient with central finite differences at `x`.
    fn check_gradient(eq: Equation, x: &[f64]) {
        let mut grad = Vec::new();
        eq.eval(&|i| x[i], &mut grad);
        let mut analytic = vec![0.0; x.len()];
        for (i, g) in grad {
            analytic[i] += g;
        }
        for i in 0..x.len() {
            let h = 1e-6 * x[i].abs().max(1.0);
            let mut xp = x.to_vec();
            let mut xm = x.to_vec();
            xp[i] += h;
            xm[i] -= h;
            let fp = eq.eval(&|j| xp[j], &mut Vec::new());
            let fm = eq.eval(&|j| xm[j], &mut Vec::new());
            let numeric = (fp - fm) / (2.0 * h);
            let err = (numeric - analytic[i]).abs();
            assert!(
                err <= 1e-6 * numeric.abs().max(1.0),
                "{eq:?} ∂/∂x{i}: analytic {} vs numeric {numeric}",
                analytic[i]
            );
        }
    }

    const X: [f64; 9] = [1.3, -0.7, 4.1, 2.2, -2.0, 3.5, 0.4, -1.9, 2.75];

    #[test]
    fn gradients_match_finite_differences() {
        let line1 = [0, 1, 2, 3];
        let line2 = [4, 5, 6, 7];
        let eqs = [
            Equation::LengthValue {
                length: Length::Points(line1),
                value: 3.0,
            },
            Equation::LengthValue {
                length: Length::Param(8),
                value: 1.0,
            },
            Equation::EqualLength {
                a: Length::Points(line1),
                b: Length::Points(line2),
            },
            Equation::EqualLength {
                a: Length::Points([0, 1, 4, 5]),
                b: Length::Param(8),
            },
            Equation::angle(line1, line2, 0.0),
            Equation::angle(line1, line2, FRAC_PI_2),
            Equation::angle(line1, line2, 0.7),
            Equation::angle(line1, line2, -2.3),
            // Lines sharing a point.
            Equation::angle([0, 1, 2, 3], [2, 3, 4, 5], 1.1),
            Equation::Difference {
                a: 0,
                b: 2,
                sign: -1.0,
                value: 2.5,
            },
            Equation::Midpoint { p: 4, a: 0, b: 2 },
            Equation::TangentLine {
                center: [4, 5],
                line: line1,
                sign: -1.0,
                radius: Length::Param(8),
            },
            Equation::LengthSum {
                terms: [
                    (Length::Points([0, 1, 4, 5]), 1.0),
                    (Length::Param(8), -1.0),
                    (Length::Points([4, 5, 6, 7]), 1.0),
                ],
            },
            Equation::SymmetricPerpendicular {
                a: [0, 1],
                b: [2, 3],
                axis: line2,
            },
            Equation::SymmetricMidpoint {
                a: [0, 1],
                b: [2, 3],
                axis: line2,
            },
            Equation::PointLine {
                point: [4, 5],
                line: line1,
                sign: 1.0,
                value: 1.0,
            },
            Equation::PointLine {
                point: [6, 7],
                line: line1,
                sign: -1.0,
                value: 3.0,
            },
            Equation::OnEllipse {
                point: [0, 1],
                center: [2, 3],
                major: [4, 5],
                minor: [6, 7],
            },
            Equation::OnEllipse {
                point: [8, 2],
                center: [0, 1],
                major: [6, 3],
                minor: [4, 7],
            },
        ];
        for eq in eqs {
            check_gradient(eq, &X);
        }
    }

    #[test]
    fn angle_residual_is_sine_of_error() {
        // Line 1 along +x, line 2 at 30°.
        let x = [0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3f64.sqrt(), 1.0];
        let at = |phi: f64| {
            Equation::angle([0, 1, 2, 3], [4, 5, 6, 7], phi).eval(&|i| x[i], &mut Vec::new())
        };
        assert!(at(30f64.to_radians()).abs() < 1e-15);
        // Undirected: 210° is the same pair of lines.
        assert!(at(210f64.to_radians()).abs() < 1e-14);
        assert!((at(0.0) - 0.5).abs() < 1e-15);
    }
}
