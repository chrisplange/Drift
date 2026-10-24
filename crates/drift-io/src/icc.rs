//! CMYK colours as they look: through the ICC profile a PDF gives them (an `ICCBased` colour
//! space, or the file's output intent), or, without one, through an approximation of U.S. Web
//! Coated (SWOP), the profile Illustrator and Acrobat assume by default.
//!
//! Only what CMYK profiles hold in practice is read: a version 2 `mft1` or `mft2` lookup from
//! CMYK to Lab or XYZ. Anything else falls back to the approximation.
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use drift_core::Rgb;

/// A CMYK profile's way to the profile connection space: per-channel input curves, a grid of
/// samples over CMYK, and per-channel output curves (all as 0–1).
#[derive(Clone, Debug)]
pub(crate) struct CmykProfile {
    inputs: Vec<Vec<f64>>,
    /// Points along each side of the grid.
    grid: usize,
    /// Three values per grid point, the first channel varying slowest.
    samples: Vec<f64>,
    outputs: Vec<Vec<f64>>,
    pcs: Pcs,
    /// The lightness (Y) of the profile's black, which black point compensation takes to
    /// the screen's black, as Acrobat and Illustrator show CMYK.
    black: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pcs {
    /// Lab as 8-bit (`mft1`) or legacy 16-bit (`mft2`) numbers.
    Lab8,
    Lab16,
    Xyz,
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl CmykProfile {
    /// Read a profile, if it is a CMYK one Drift can follow.
    pub(crate) fn parse(icc: &[u8]) -> Option<Self> {
        if icc.get(16..20)? != b"CMYK" {
            return None;
        }
        let lab = match icc.get(20..24)? {
            b"Lab " => true,
            b"XYZ " => false,
            _ => return None,
        };
        // Relative colorimetric (as viewers show a page), else perceptual.
        let count = be32(icc, 128)? as usize;
        let tag = |sig: &[u8]| -> Option<&[u8]> {
            (0..count.min(256)).find_map(|i| {
                let at = 132 + i * 12;
                (icc.get(at..at + 4)? == sig).then_some(())?;
                let (off, len) = (be32(icc, at + 4)? as usize, be32(icc, at + 8)? as usize);
                icc.get(off..off.checked_add(len)?)
            })
        };
        let lut = tag(b"A2B1").or_else(|| tag(b"A2B0"))?;
        let (wide, kind) = match lut.get(0..4)? {
            b"mft1" => (false, if lab { Pcs::Lab8 } else { Pcs::Xyz }),
            b"mft2" => (true, if lab { Pcs::Lab16 } else { Pcs::Xyz }),
            _ => return None,
        };
        let (ins, outs, grid) = (
            usize::from(*lut.get(8)?),
            usize::from(*lut.get(9)?),
            usize::from(*lut.get(10)?),
        );
        if ins != 4 || outs != 3 || grid < 2 {
            return None;
        }
        let (n_in, n_out, mut at) = if wide {
            (usize::from(be16(lut, 48)?), usize::from(be16(lut, 50)?), 52)
        } else {
            (256, 256, 48)
        };
        if n_in < 2 || n_out < 2 {
            return None;
        }
        let mut read = |n: usize| -> Option<Vec<f64>> {
            let v = if wide {
                (0..n)
                    .map(|i| be16(lut, at + 2 * i).map(|x| f64::from(x) / 65535.0))
                    .collect::<Option<Vec<f64>>>()?
            } else {
                lut.get(at..at + n)?
                    .iter()
                    .map(|x| f64::from(*x) / 255.0)
                    .collect()
            };
            at += n * if wide { 2 } else { 1 };
            Some(v)
        };
        let inputs = (0..4).map(|_| read(n_in)).collect::<Option<Vec<_>>>()?;
        // (255⁴ × 3 doesn't fit a 32-bit `usize`, as on the web.)
        let samples = read(grid.checked_pow(4)?.checked_mul(3)?)?;
        let outputs = (0..3).map(|_| read(n_out)).collect::<Option<Vec<_>>>()?;
        let mut profile = Self {
            inputs,
            grid,
            samples,
            outputs,
            pcs: kind,
            black: 0.0,
        };
        profile.black = profile.to_xyz([1.0; 4])[1].clamp(0.0, 0.5);
        Some(profile)
    }

    /// A CMYK colour (each 0–1) in sRGB, with black point compensation.
    pub(crate) fn to_rgb(&self, cmyk: [f64; 4]) -> Rgb {
        let b = self.black;
        let xyz = self.to_xyz(cmyk);
        let white = [0.9642, 1.0, 0.8249];
        xyz_d50_to_srgb(std::array::from_fn(|i| (xyz[i] - b * white[i]) / (1.0 - b)))
    }

    /// A CMYK colour (each 0–1) in the profile connection space, XYZ under D50.
    fn to_xyz(&self, cmyk: [f64; 4]) -> [f64; 3] {
        let x: [f64; 4] = std::array::from_fn(|i| curve(&self.inputs[i], cmyk[i]));
        // Interpolated between the sixteen grid points around it.
        let g = self.grid;
        let top = (g - 1) as f64;
        let mut base = [0usize; 4];
        let mut frac = [0.0; 4];
        for i in 0..4 {
            let p = x[i].clamp(0.0, 1.0) * top;
            base[i] = (p.floor() as usize).min(g - 2);
            frac[i] = p - base[i] as f64;
        }
        let mut pcs = [0.0; 3];
        for corner in 0..16usize {
            let mut weight = 1.0;
            let mut index = 0;
            for i in 0..4 {
                let up = (corner >> (3 - i)) & 1;
                weight *= if up == 1 { frac[i] } else { 1.0 - frac[i] };
                index = index * g + base[i] + up;
            }
            if weight == 0.0 {
                continue;
            }
            for (c, v) in pcs.iter_mut().enumerate() {
                *v += weight * self.samples[index * 3 + c];
            }
        }
        let v: [f64; 3] = std::array::from_fn(|i| curve(&self.outputs[i], pcs[i]));
        match self.pcs {
            Pcs::Lab8 => lab_to_xyz([v[0] * 100.0, v[1] * 255.0 - 128.0, v[2] * 255.0 - 128.0]),
            Pcs::Lab16 => {
                let k = 65535.0 / 65280.0;
                lab_to_xyz([
                    v[0] * k * 100.0,
                    v[1] * k * 255.0 - 128.0,
                    v[2] * k * 255.0 - 128.0,
                ])
            }
            Pcs::Xyz => v.map(|c| c * 65535.0 / 32768.0),
        }
    }
}

/// A sampled curve at `x` (0–1), linearly between its samples.
fn curve(table: &[f64], x: f64) -> f64 {
    let top = (table.len() - 1) as f64;
    let p = x.clamp(0.0, 1.0) * top;
    let i = (p.floor() as usize).min(table.len() - 2);
    let t = p - i as f64;
    table[i] * (1.0 - t) + table[i + 1] * t
}

/// CIE Lab (D50) to XYZ.
pub(crate) fn lab_to_xyz([l, a, b]: [f64; 3]) -> [f64; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let f = |t: f64| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    [0.9642 * f(fx), f(fy), 0.8249 * f(fz)]
}

/// XYZ under D50 (the profile connection space) to sRGB, adapted to D65 (Bradford).
pub(crate) fn xyz_d50_to_srgb([x, y, z]: [f64; 3]) -> Rgb {
    let lin = [
        3.133_856_1 * x - 1.616_866_7 * y - 0.490_614_6 * z,
        -0.978_768_4 * x + 1.916_141_5 * y + 0.033_454_0 * z,
        0.071_945_3 * x - 0.228_991_4 * y + 1.405_242_7 * z,
    ];
    Rgb(lin.map(|c| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    }))
}

/// A CMYK colour in sRGB without a profile: as U.S. Web Coated (SWOP) v2 shows it, with
/// black point compensation (within 1 of 255 on average, and 14 at worst, in saturated
/// cyans).
pub(crate) fn cmyk_to_rgb(cmyk: [f64; 4]) -> Rgb {
    let v = cmyk.map(|x| if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) });
    // The terms of a quartic in c, m, y and k: 1, then the products of one to four of them
    // (indices not decreasing), in the order the coefficients are. Each degree's products
    // extend the last's.
    let mut terms = Vec::with_capacity(70);
    terms.push(1.0);
    let mut last = vec![(0, 1.0)];
    for _ in 0..4 {
        let next: Vec<(usize, f64)> = last
            .iter()
            .flat_map(|&(from, p)| {
                v.iter()
                    .enumerate()
                    .skip(from)
                    .map(move |(i, x)| (i, p * x))
            })
            .collect();
        terms.extend(next.iter().map(|t| t.1));
        last = next;
    }
    let lab = SWOP_LAB.map(|coef| coef.iter().zip(&terms).map(|(a, t)| a * t).sum::<f64>());
    xyz_d50_to_srgb(lab_to_xyz(lab))
}

/// Lab (D50) as quartics in CMYK, fitted to U.S. Web Coated (SWOP) v2 (relative
/// colorimetric, black point compensated) over an 11-step grid; one row each for L, a, b.
#[rustfmt::skip]
const SWOP_LAB: [[f64; 70]; 3] = [
    [
        100.0, -49.37266, -59.41573, -5.68503, -83.89972, 16.514, 26.5126, 1.61033, 54.22799,
        17.19505, 5.711944, 64.7346, -0.6101562, 0.5180062, 20.19117, -9.235366, -5.028283,
        -2.043099, -14.94327, -11.90575, -0.3536443, -27.99155, -0.3684189, 1.158182, -40.70289,
        -5.58178, -0.5856353, -27.08041, -1.625327, -2.04532, -48.62649, 1.962164, 1.791548,
        -0.5662279, 11.60869, 4.72453, -0.9794898, -0.1755919, 0.2241433, 3.685803, -0.8931415,
        3.341862, 1.014368, 2.10299, 7.352499, -0.01501773, -1.322513, 9.481657, 0.3840336,
        0.4851517, 5.450601, 0.03832637, -1.686778, 0.3885863, 22.66267, 0.2768646, 0.7522852,
        4.296026, 0.7483102, 0.107165, 14.50576, -0.5284672, 0.2476657, 0.8413652, 26.32613,
        -0.2206097, -1.141019, -0.7965386, 2.82927, -31.07232,
    ],
    [
        0.0, -51.76584, 82.91565, -14.60898, -0.3758342, 25.32626, -23.19264, -10.24113,
        72.04214, -17.48713, 10.9063, -109.3778, 10.75612, 17.39054, -6.983052, -24.15257,
        2.630689, -11.55914, -45.17312, 7.658576, 2.848913, 30.68996, 5.786505, -8.841224,
        -69.3267, 13.87818, -2.425772, 59.03327, -7.633801, -14.78489, 101.0016, -7.513456,
        -3.581687, -20.61378, 15.31438, 6.68174, 0.4490037, -6.249037, 14.67575, -5.623249,
        0.2445798, 2.542348, 2.033358, 19.90361, 24.21896, 5.23296, 3.565573, -14.47059,
        -1.559199, -4.557962, -5.821159, 0.5527346, -3.962171, 6.742099, 32.23545, 0.183457,
        -3.089298, -14.32875, 1.212768, 0.6572609, -43.29951, 1.965294, 6.315002, 4.62826,
        -45.91977, 3.634936, -1.862993, 2.741036, 8.955877, -6.788837,
    ],
    [
        0.0, -64.39936, -19.79836, 102.0068, -6.146417, 18.83541, 28.19184, -8.142557, 81.75535,
        18.84396, -41.53492, 31.73156, -45.86643, -101.1057, -1.42561, -10.70898, -6.713126,
        -1.543841, -18.90881, -22.422, 4.462789, -35.60755, -7.236591, 4.557216, -81.53172,
        -4.839144, 5.009933, -29.92092, -0.2879153, 50.98474, -31.90747, 76.17123, 19.51046,
        85.94229, 19.19887, 5.78334, 0.4259766, 1.788898, -2.070477, 5.293462, -2.744217,
        3.559254, 1.337293, -1.669981, 16.13611, -0.7551778, 2.290158, 16.98731, 5.412014,
        -6.820429, 11.63081, -0.6830356, 2.106933, 11.60314, 39.54559, -0.948008, 0.594759,
        6.431913, -2.140339, -5.060063, 12.6025, 0.9050943, -5.910216, -12.41468, 16.90117,
        -38.24374, -3.389016, -18.36639, -55.31729, -11.84103,
    ],
];

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny `mft2` CMYK → Lab profile: a 2-point grid where each corner's lightness falls
    /// with the ink on it and a, b stay neutral.
    pub(crate) fn tiny_profile() -> Vec<u8> {
        let mut lut = Vec::new();
        lut.extend_from_slice(b"mft2\0\0\0\0");
        lut.extend_from_slice(&[4, 3, 2, 0]);
        // The identity matrix (unused for CMYK).
        for i in 0..9 {
            let one: i32 = if i % 4 == 0 { 0x10000 } else { 0 };
            lut.extend_from_slice(&one.to_be_bytes());
        }
        lut.extend_from_slice(&2u16.to_be_bytes());
        lut.extend_from_slice(&2u16.to_be_bytes());
        let ramp = |lut: &mut Vec<u8>| {
            lut.extend_from_slice(&0u16.to_be_bytes());
            lut.extend_from_slice(&65535u16.to_be_bytes());
        };
        for _ in 0..4 {
            ramp(&mut lut);
        }
        let neutral = (128.0 * 65280.0 / 255.0) as u16;
        for corner in 0..16u32 {
            let inks = corner.count_ones();
            let l = 100.0 - 20.0 * f64::from(inks);
            lut.extend_from_slice(&((l / 100.0 * 65280.0) as u16).to_be_bytes());
            lut.extend_from_slice(&neutral.to_be_bytes());
            lut.extend_from_slice(&neutral.to_be_bytes());
        }
        for _ in 0..3 {
            ramp(&mut lut);
        }
        let mut icc = vec![0u8; 128];
        icc[12..16].copy_from_slice(b"prtr");
        icc[16..20].copy_from_slice(b"CMYK");
        icc[20..24].copy_from_slice(b"Lab ");
        icc.extend_from_slice(&1u32.to_be_bytes());
        icc.extend_from_slice(b"A2B0");
        icc.extend_from_slice(&(144u32).to_be_bytes());
        icc.extend_from_slice(&(lut.len() as u32).to_be_bytes());
        icc.extend_from_slice(&lut);
        let len = icc.len() as u32;
        icc[0..4].copy_from_slice(&len.to_be_bytes());
        icc
    }

    #[test]
    fn a_cmyk_profile_is_followed() {
        let p = CmykProfile::parse(&tiny_profile()).expect("a profile");
        // No ink is white; all four, the profile's black, are black on screen.
        assert_eq!(p.to_rgb([0.0; 4]), Rgb([255, 255, 255]));
        assert_eq!(p.to_rgb([1.0; 4]), Rgb([0, 0, 0]));
        // One ink is a neutral grey, and half of it is between that and white.
        let Rgb([r, g, b]) = p.to_rgb([1.0, 0.0, 0.0, 0.0]);
        assert!(r == g && g == b && (150..230).contains(&r), "{r} {g} {b}");
        let Rgb([half, _, _]) = p.to_rgb([0.5, 0.0, 0.0, 0.0]);
        assert!(r < half && half < 255, "{half}");
        assert!(CmykProfile::parse(b"not a profile").is_none());
    }

    #[test]
    fn a_broken_profile_is_refused_or_followed_never_a_panic() {
        let good = tiny_profile();
        let colours = [
            [0.0; 4],
            [1.0; 4],
            [0.3, 0.6, f64::NAN, 2.0],
            [-1.0, 0.5, 0.5, 0.5],
        ];
        let try_one = |icc: &[u8]| {
            if let Some(p) = CmykProfile::parse(icc) {
                for c in colours {
                    p.to_rgb(c);
                }
            }
        };
        for end in 0..good.len() {
            try_one(&good[..end]);
        }
        for at in 0..good.len() {
            for value in [0x00, 0x01, 0x02, 0x7f, 0x80, 0xff] {
                let mut icc = good.clone();
                icc[at] = value;
                try_one(&icc);
            }
        }
        // The biggest grid there can be, and no samples for it.
        let mut huge = good.clone();
        huge[144 + 10] = 255;
        assert!(CmykProfile::parse(&huge).is_none());
    }

    #[test]
    fn cmyk_without_a_profile_looks_as_swop_shows_it() {
        // Paper, black and the primaries, as the profile has them.
        let near = |c: [f64; 4], want: [u8; 3]| {
            let Rgb(got) = cmyk_to_rgb(c);
            assert!(
                got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 8),
                "{c:?}: {got:?}, not {want:?}"
            );
        };
        assert_eq!(cmyk_to_rgb([0.0; 4]), Rgb([255, 255, 255]));
        near([0.0, 0.0, 0.0, 1.0], [44, 41, 41]);
        near([1.0, 0.0, 0.0, 0.0], [0, 175, 239]);
        near([0.0, 1.0, 0.0, 0.0], [236, 20, 141]);
        near([0.0, 0.0, 1.0, 0.0], [255, 242, 0]);
        // A light grey that the naive formula turns cyan (#E8EEED).
        near([0.089, 0.065, 0.069, 0.0], [229, 229, 229]);
    }
}
