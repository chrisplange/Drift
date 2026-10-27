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

/// The least a length (or a squared length) is taken to be where an equation divides by it:
/// its square, or the product of two, is still a normal number (1e-300). With 1e-300 itself
/// the square underflowed to 0 and a zero-length line gave 0/0.
const TINY: f64 = 1e-150;

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

/// What an [`Equation::OffsetFrom`] measures from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Offset {
    /// The line through `[ax, ay, bx, by]`.
    Line([usize; 4]),
    /// The circle round `center` of `radius`.
    Circle { center: [usize; 2], radius: Length },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Equation {
    /// The signed distance of a point from a line or a circle is `value`: to the left of the
    /// line `a → b` (as [`PointLine`](Equation::PointLine) with sign 1), or outside the circle
    /// (`|p − c| − r`). How a relation on a curve offset from another (a wall's face, from its
    /// centreline) is held on that other curve, on the side it is on.
    OffsetFrom {
        point: [usize; 2],
        from: Offset,
        value: f64,
    },
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
    /// A point on a cubic Bézier: its offset from the nearest point of the curve, measured
    /// along the curve's normal there, is zero. `ctrl` are the four control points (an
    /// anchor, its handle, the other handle, the other anchor). Beyond an end the curve
    /// carries on along its tangent.
    OnBezier {
        point: [usize; 2],
        ctrl: [[usize; 2]; 4],
    },
    /// A direction tangent to an ellipse at its point `at` (or, with `normal`, square to it):
    /// the sine of the angle between `dir` (`[ax, ay, bx, by]`) and the ellipse's tangent
    /// (or normal) there. With `M = major − center`, `P = M⊥`, `d = at − center` and
    /// `b = |minor − center|`, the outward normal is `b²(d·M)M + |M|²(d·P)P`.
    EllipseTangent {
        at: [usize; 2],
        center: [usize; 2],
        major: [usize; 2],
        minor: [usize; 2],
        dir: [usize; 4],
        normal: bool,
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
        matches!(
            self,
            Equation::Angle { .. } | Equation::EllipseTangent { .. }
        )
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
            Equation::OffsetFrom { point, from, .. } => {
                let mut v = point.to_vec();
                match from {
                    Offset::Line(line) => v.extend(line),
                    Offset::Circle { center, radius } => {
                        v.extend(center);
                        v.extend(radius.params());
                    }
                }
                v
            }
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
            Equation::OnBezier { point, ctrl } => {
                point.iter().chain(ctrl.iter().flatten()).copied().collect()
            }
            Equation::EllipseTangent {
                at,
                center,
                major,
                minor,
                dir,
                ..
            } => at
                .iter()
                .chain(center)
                .chain(major)
                .chain(minor)
                .chain(dir)
                .copied()
                .collect(),
        }
    }

    /// The residual, appending its gradient to `grad` (a parameter may appear more than once;
    /// callers must sum duplicates).
    pub fn eval(&self, val: &impl Fn(usize) -> f64, grad: &mut Vec<(usize, f64)>) -> f64 {
        match *self {
            Equation::OnBezier { point, ctrl } => {
                use drift_core::DVec2;
                use drift_core::shape::{bezier_point, bezier_second, bezier_tangent};
                let at = |p: [usize; 2]| DVec2::new(val(p[0]), val(p[1]));
                let p = at(point);
                let c = ctrl.map(at);
                // The nearest point of the curve: the best of a coarse scan, refined by
                // Newton's method on (B(t) − P)·B′(t) = 0.
                let mut t = (0..=16)
                    .map(|k| f64::from(k) / 16.0)
                    .min_by(|a, b| {
                        bezier_point(&c, *a)
                            .distance_squared(p)
                            .total_cmp(&bezier_point(&c, *b).distance_squared(p))
                    })
                    .unwrap_or(0.5);
                for _ in 0..24 {
                    let (d, d1, d2) = (
                        bezier_point(&c, t) - p,
                        bezier_tangent(&c, t),
                        bezier_second(&c, t),
                    );
                    let slope = d1.dot(d1) + d.dot(d2);
                    if slope.abs() < 1e-18 {
                        break;
                    }
                    let next = (t - d.dot(d1) / slope).clamp(0.0, 1.0);
                    if (next - t).abs() < 1e-14 {
                        t = next;
                        break;
                    }
                    t = next;
                }
                let on = bezier_point(&c, t);
                let tangent = bezier_tangent(&c, t);
                let n = if tangent.length_squared() > 1e-24 {
                    tangent.perp().normalize()
                } else {
                    (p - on).try_normalize().unwrap_or(DVec2::Y)
                };
                // At the nearest point the offset is along the normal, so to first order
                // only the points' own movement along it changes the residual.
                let u = 1.0 - t;
                let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                grad.extend([(point[0], n.x), (point[1], n.y)]);
                for (k, w) in weights.into_iter().enumerate() {
                    grad.extend([(ctrl[k][0], -w * n.x), (ctrl[k][1], -w * n.y)]);
                }
                (p - on).dot(n)
            }
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
            Equation::EllipseTangent {
                at,
                center,
                major,
                minor,
                dir,
                normal,
            } => {
                use drift_core::DVec2;
                let pt = |p: [usize; 2]| DVec2::new(val(p[0]), val(p[1]));
                let c = pt(center);
                let (d, m, f) = (pt(at) - c, pt(major) - c, pt(minor) - c);
                let p = m.perp();
                let (a2, b2) = (m.length_squared(), f.length_squared());
                let (s, k) = (d.dot(m), d.dot(p));
                let n = m * (b2 * s) + p * (a2 * k);
                let v = DVec2::new(val(dir[2]) - val(dir[0]), val(dir[3]) - val(dir[1]));
                // As `Angle` with line 1 = v and line 2 = n: sin of the angle from the
                // normal (cross) or from the tangent (dot).
                let (cs, sn) = if normal { (1.0, 0.0) } else { (0.0, -1.0) };
                let len1sq = v.length_squared().max(TINY);
                let len2sq = n.length_squared().max(TINY);
                let norm = (len1sq * len2sq).sqrt();
                let r = (v.perp_dot(n) * cs - v.dot(n) * sn) / norm;
                let gv =
                    DVec2::new(cs * n.y - sn * n.x, -cs * n.x - sn * n.y) / norm - v * (r / len1sq);
                let gn =
                    DVec2::new(-cs * v.y - sn * v.x, cs * v.x - sn * v.y) / norm - n * (r / len2sq);
                // Through n = b²·s·M + |M|²·k·P to d, M and the minor point.
                let (mg, pg) = (m.dot(gn), p.dot(gn));
                let gd = m * (b2 * mg) + p * (a2 * pg);
                let gm = d * (b2 * mg)
                    + gn * (b2 * s)
                    + m * (2.0 * k * pg)
                    + DVec2::new(d.y, -d.x) * (a2 * pg)
                    + DVec2::new(gn.y, -gn.x) * (a2 * k);
                let gf = f * (2.0 * s * mg);
                let gc = -(gd + gm + gf);
                grad.extend([
                    (at[0], gd.x),
                    (at[1], gd.y),
                    (major[0], gm.x),
                    (major[1], gm.y),
                    (minor[0], gf.x),
                    (minor[1], gf.y),
                    (center[0], gc.x),
                    (center[1], gc.y),
                    (dir[0], -gv.x),
                    (dir[1], -gv.y),
                    (dir[2], gv.x),
                    (dir[3], gv.y),
                ]);
                r
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
                let len = d.0.hypot(d.1).max(TINY);
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
                let len = d.0.hypot(d.1).max(TINY);
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
            Equation::OffsetFrom { point, from, value } => match from {
                Offset::Line(line) => point_line(val, point, line, 1.0, grad) - value,
                Offset::Circle { center, radius } => {
                    let out = Length::Points([center[0], center[1], point[0], point[1]]);
                    out.eval(val, 1.0, grad) - radius.eval(val, -1.0, grad) - value
                }
            },
            Equation::Angle {
                l1,
                l2,
                sin_phi: s,
                cos_phi: c,
            } => {
                let d1 = (val(l1[2]) - val(l1[0]), val(l1[3]) - val(l1[1]));
                let d2 = (val(l2[2]) - val(l2[0]), val(l2[3]) - val(l2[1]));
                let len1sq = (d1.0 * d1.0 + d1.1 * d1.1).max(TINY);
                let len2sq = (d2.0 * d2.0 + d2.1 * d2.1).max(TINY);
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
    let len = d.0.hypot(d.1).max(TINY);
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
            Equation::OffsetFrom {
                point: [6, 7],
                from: Offset::Line(line1),
                value: -0.5,
            },
            Equation::OffsetFrom {
                point: [6, 7],
                from: Offset::Circle {
                    center: [0, 1],
                    radius: Length::Param(8),
                },
                value: 0.25,
            },
            Equation::OffsetFrom {
                point: [6, 7],
                from: Offset::Circle {
                    center: [0, 1],
                    radius: Length::Points([0, 1, 2, 3]),
                },
                value: -1.0,
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
            Equation::EllipseTangent {
                at: [0, 1],
                center: [2, 3],
                major: [4, 5],
                minor: [6, 7],
                dir: [0, 1, 8, 2],
                normal: false,
            },
            Equation::EllipseTangent {
                at: [8, 2],
                center: [0, 1],
                major: [6, 3],
                minor: [4, 7],
                dir: [5, 6, 2, 3],
                normal: true,
            },
        ];
        for eq in eqs {
            check_gradient(eq, &X);
        }
    }

    #[test]
    fn point_on_a_bezier() {
        // An arch from (0, 0) to (4, 0), and points on, above and beside it.
        let curve = [[2, 3], [4, 5], [6, 7], [8, 9]];
        let eq = Equation::OnBezier {
            point: [0, 1],
            ctrl: curve,
        };
        let arch = [0.0, 0.0, 0.3, 2.1, 3.6, 2.4, 4.0, 0.0];
        let with = |px: f64, py: f64| {
            let mut x = vec![px, py];
            x.extend(arch);
            x
        };
        for (px, py) in [(1.0, 2.5), (2.2, 0.6), (3.4, 1.9), (0.9, 0.4)] {
            check_gradient(eq, &with(px, py));
        }
        // A point of the curve itself has no offset.
        let c = [
            drift_core::DVec2::new(0.0, 0.0),
            drift_core::DVec2::new(0.3, 2.1),
            drift_core::DVec2::new(3.6, 2.4),
            drift_core::DVec2::new(4.0, 0.0),
        ];
        for t in [0.0, 0.2, 0.55, 1.0] {
            let p = drift_core::shape::bezier_point(&c, t);
            let x = with(p.x, p.y);
            assert!(eq.eval(&|i| x[i], &mut Vec::new()).abs() < 1e-12);
        }
    }

    #[test]
    fn ellipse_tangent_residual() {
        // Ellipse centred at (1, 2), semi-axes 4 along 30° and 2; a point at t = 0.8 with
        // its tangent and normal, and a direction 0.1 rad off the tangent.
        let (c, u) = (
            drift_core::DVec2::new(1.0, 2.0),
            drift_core::DVec2::from_angle(30f64.to_radians()),
        );
        let t = 0.8f64;
        let p = c + u * 4.0 * t.cos() + u.perp() * 2.0 * t.sin();
        let tangent = -u * 4.0 * t.sin() + u.perp() * 2.0 * t.cos();
        let (major, minor) = (c + u * 4.0, c - u.perp() * 2.0);
        let eval = |dir: drift_core::DVec2, normal: bool| {
            let q = p + dir;
            let x = [
                p.x, p.y, c.x, c.y, major.x, major.y, minor.x, minor.y, q.x, q.y,
            ];
            let eq = Equation::EllipseTangent {
                at: [0, 1],
                center: [2, 3],
                major: [4, 5],
                minor: [6, 7],
                dir: [0, 1, 8, 9],
                normal,
            };
            check_gradient(eq, &x);
            eq.eval(&|i| x[i], &mut Vec::new())
        };
        assert!(eval(tangent, false).abs() < 1e-14);
        assert!(eval(-tangent * 3.0, false).abs() < 1e-14);
        assert!(eval(tangent.perp(), true).abs() < 1e-14);
        let off = drift_core::DVec2::from_angle(0.1).rotate(tangent);
        assert!((eval(off, false).abs() - 0.1f64.sin()).abs() < 1e-12);
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

    /// A zero-length line: the guards keep every residual and gradient a number (#215).
    #[test]
    fn zero_length_line_stays_finite() {
        // Line 1 from (0, 0) to (0, 0); line 2 and a point elsewhere.
        let x = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 4.0, 3.0, 2.0, 5.0];
        let val = |i: usize| x[i];
        let equations = [
            Equation::angle([0, 1, 2, 3], [4, 5, 6, 7], 0.5),
            Equation::angle([4, 5, 6, 7], [0, 1, 2, 3], 0.5),
            Equation::PointLine {
                point: [8, 9],
                line: [0, 1, 2, 3],
                sign: 1.0,
                value: 1.0,
            },
            Equation::SymmetricPerpendicular {
                a: [4, 5],
                b: [8, 9],
                axis: [0, 1, 2, 3],
            },
            Equation::SymmetricMidpoint {
                a: [4, 5],
                b: [8, 9],
                axis: [0, 1, 2, 3],
            },
        ];
        for (i, e) in equations.iter().enumerate() {
            let mut grad = Vec::new();
            let r = e.eval(&val, &mut grad);
            assert!(r.is_finite(), "equation {i}: residual {r}");
            assert!(
                grad.iter().all(|(_, g)| g.is_finite()),
                "equation {i}: {grad:?}"
            );
        }
    }
}
