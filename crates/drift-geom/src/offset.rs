//! Offsetting curves that have no exact offset of their own kind (ellipses, Béziers): the
//! parallel curve is fitted with cubic Béziers, subdivided until they follow it closely.

use drift_core::DVec2;

use crate::primitives::nearest_on_bezier;

/// Why a curve can't be offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetError {
    /// The distance is more than the curve's tightest bend on that side: the offset would
    /// fold back on itself.
    TooTight,
    /// The curve has no direction somewhere (a zero-length stretch).
    Degenerate,
}

/// The most halvings of a piece while fitting (so at most 2^8 curves per starting piece).
const MAX_DEPTH: u32 = 8;

/// Cubic Béziers following the curve `at(u)`, `u` in `0..=span`, moved `d` to its left (as
/// travelled with rising `u`; negative `d` is to the right), within `tol` of the true offset.
/// The pieces join end to end with matching directions. `at` must also give sensible points
/// a little outside the span (it is differenced for directions). Starts from `pieces` equal
/// stretches.
pub fn offset_curve(
    at: &dyn Fn(f64) -> DVec2,
    span: f64,
    d: f64,
    tol: f64,
    pieces: usize,
) -> Result<Vec<[DVec2; 4]>, OffsetError> {
    let h = 1e-5 * span;
    let direction = |u: f64| (at(u + h) - at(u - h)) / (2.0 * h);
    // A point of the offset curve.
    let offset = |u: f64| -> Result<DVec2, OffsetError> {
        let t = direction(u);
        let n = t.perp().try_normalize().ok_or(OffsetError::Degenerate)?;
        Ok(at(u) + n * d)
    };
    // Its derivative; where it turns against the curve's direction, the offset has folded.
    let k = 1e-4 * span;
    let velocity = |u: f64| -> Result<DVec2, OffsetError> {
        let v = (offset(u + k)? - offset(u - k)?) / (2.0 * k);
        if v.dot(direction(u)) <= 0.0 {
            return Err(OffsetError::TooTight);
        }
        Ok(v)
    };
    let checks = 64 * pieces.max(1);
    for i in 0..=checks {
        velocity(span * i as f64 / checks as f64)?;
    }
    let mut out = Vec::new();
    let n = pieces.max(1);
    for i in 0..n {
        let (u0, u1) = (span * i as f64 / n as f64, span * (i + 1) as f64 / n as f64);
        fit(&offset, &velocity, u0, u1, tol, 0, &mut out)?;
    }
    Ok(out)
}

/// Fit `u0..=u1` with one cubic, halving the stretch while it strays more than `tol` from
/// the offset. The cubic leaves each end in the offset's direction there (so pieces join
/// smoothly); how far its handles reach is chosen to hug the offset between.
fn fit(
    offset: &dyn Fn(f64) -> Result<DVec2, OffsetError>,
    velocity: &dyn Fn(f64) -> Result<DVec2, OffsetError>,
    u0: f64,
    u1: f64,
    tol: f64,
    depth: u32,
    out: &mut Vec<[DVec2; 4]>,
) -> Result<(), OffsetError> {
    const SAMPLES: usize = 9;
    let (p0, p3) = (offset(u0)?, offset(u1)?);
    let (v0, v1) = (velocity(u0)?, velocity(u1)?);
    let truth: Vec<DVec2> = (1..SAMPLES)
        .map(|i| offset(u0 + (u1 - u0) * i as f64 / SAMPLES as f64))
        .collect::<Result<_, _>>()?;
    let error = |cubic: &[DVec2; 4]| {
        truth
            .iter()
            .map(|q| nearest_on_bezier(cubic, *q).0.distance(*q))
            .fold(0.0f64, f64::max)
    };
    // Matching the offset's derivative at both ends is the starting point...
    let third = (u1 - u0) / 3.0;
    let mut cubic = [p0, p0 + v0 * third, p3 - v1 * third, p3];
    let mut worst = error(&cubic);
    // ...then the handle lengths are refitted by least squares, re-pairing each sample with
    // its nearest point of the curve between rounds.
    let (t0, t1) = (v0.normalize_or_zero(), v1.normalize_or_zero());
    let mut at: Vec<f64> = (1..SAMPLES).map(|i| i as f64 / SAMPLES as f64).collect();
    for _ in 0..4 {
        // B(s) = base(s) + α·t0·b1(s) − β·t1·b2(s); solve for α and β.
        let (mut m00, mut m01, mut m11, mut g0, mut g1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (q, s) in truth.iter().zip(&at) {
            let r = 1.0 - s;
            let (b1, b2) = (3.0 * r * r * s, 3.0 * r * s * s);
            let base = p0 * (r * r * r + b1) + p3 * (b2 + s * s * s);
            let (x, y) = (t0 * b1, -t1 * b2);
            let d = *q - base;
            m00 += x.dot(x);
            m01 += x.dot(y);
            m11 += y.dot(y);
            g0 += x.dot(d);
            g1 += y.dot(d);
        }
        let det = m00 * m11 - m01 * m01;
        if det.abs() < 1e-12 * (m00 * m11).max(f64::MIN_POSITIVE) {
            break;
        }
        let (alpha, beta) = ((m11 * g0 - m01 * g1) / det, (m00 * g1 - m01 * g0) / det);
        // Handles must point along the curve, and not overshoot wildly.
        let chord = p0.distance(p3);
        if !(alpha > 0.0 && beta > 0.0 && alpha < 3.0 * chord && beta < 3.0 * chord) {
            break;
        }
        let candidate = [p0, p0 + t0 * alpha, p3 - t1 * beta, p3];
        let e = error(&candidate);
        if e >= worst {
            break;
        }
        (cubic, worst) = (candidate, e);
        for (q, s) in truth.iter().zip(at.iter_mut()) {
            *s = nearest_on_bezier(&cubic, *q).1;
        }
    }
    if worst <= tol || depth >= MAX_DEPTH {
        out.push(cubic);
        return Ok(());
    }
    let mid = (u0 + u1) * 0.5;
    fit(offset, velocity, u0, mid, tol, depth + 1, out)?;
    fit(offset, velocity, mid, u1, tol, depth + 1, out)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use drift_core::shape::{bezier_point, ellipse_point};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn a_circle_offsets_to_a_circle() {
        // Anticlockwise, so its left is inside: 3 to the left of radius 10 is radius 7.
        let at = |u: f64| v(5.0, -2.0) + 10.0 * DVec2::from_angle(u);
        let curves = offset_curve(&at, TAU, 3.0, 1e-4, 4).unwrap();
        assert!(curves.len() >= 4);
        for c in &curves {
            for k in 0..=10 {
                let p = bezier_point(c, f64::from(k) / 10.0);
                assert!((p.distance(v(5.0, -2.0)) - 7.0).abs() < 1e-4);
            }
        }
        // They join end to end, and the last meets the first.
        for w in curves.windows(2) {
            assert!(w[0][3].distance(w[1][0]) < 1e-12);
        }
        assert!(curves[curves.len() - 1][3].distance(curves[0][0]) < 1e-9);
        // To the right it grows.
        let out = offset_curve(&at, TAU, -3.0, 1e-4, 4).unwrap();
        assert!((out[0][0].distance(v(5.0, -2.0)) - 13.0).abs() < 1e-9);
    }

    #[test]
    fn an_ellipse_offsets_until_it_would_fold() {
        // Semi-axes 20 and 10: the tightest bend (at the long ends) has radius 10²/20 = 5.
        let at = |u: f64| ellipse_point(v(0.0, 0.0), v(20.0, 0.0), 10.0, u);
        let inside = offset_curve(&at, TAU, 4.0, 1e-3, 4).unwrap();
        // Every fitted point is 4 from the ellipse (measured against a dense sampling).
        let dense: Vec<DVec2> = (0..40_000)
            .map(|k| at(TAU * f64::from(k) / 40_000.0))
            .collect();
        for c in &inside {
            for k in 0..=6 {
                let p = bezier_point(c, f64::from(k) / 6.0);
                let gap = dense
                    .iter()
                    .map(|q| q.distance(p))
                    .fold(f64::INFINITY, f64::min);
                assert!((gap - 4.0).abs() < 2e-3, "{gap}");
            }
        }
        assert_eq!(
            offset_curve(&at, TAU, 6.0, 1e-3, 4),
            Err(OffsetError::TooTight)
        );
        // Outside there's no limit.
        assert!(offset_curve(&at, TAU, -50.0, 1e-3, 4).is_ok());
    }

    #[test]
    fn a_bezier_offsets_to_curves_at_the_distance() {
        let arch = [v(0.0, 0.0), v(0.0, 20.0), v(40.0, 20.0), v(40.0, 0.0)];
        let at = |u: f64| bezier_point(&arch, u);
        for d in [3.0, -5.0] {
            let curves = offset_curve(&at, 1.0, d, 1e-3, 1).unwrap();
            for c in &curves {
                for k in 0..=6 {
                    let p = bezier_point(c, f64::from(k) / 6.0);
                    let gap = nearest_on_bezier(&arch, p).0.distance(p);
                    assert!((gap - d.abs()).abs() < 2e-3, "{gap}");
                }
            }
            // The ends sit square to the curve's ends.
            assert!(
                (curves[0][0] - v(-d, 0.0)).length() < 1e-6,
                "{:?}",
                curves[0][0]
            );
        }
    }
}
