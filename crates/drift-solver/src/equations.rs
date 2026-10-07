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
        }
    }

    /// The residual, appending its gradient to `grad` (a parameter may appear more than once;
    /// callers must sum duplicates).
    pub fn eval(&self, val: &impl Fn(usize) -> f64, grad: &mut Vec<(usize, f64)>) -> f64 {
        match *self {
            Equation::LengthValue { length, value } => length.eval(val, 1.0, grad) - value,
            Equation::EqualLength { a, b } => a.eval(val, 1.0, grad) - b.eval(val, -1.0, grad),
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
