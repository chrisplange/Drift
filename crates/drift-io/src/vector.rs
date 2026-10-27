//! Vector artwork read from an SVG or a PDF, on its way into a sketch: painted paths (their
//! curves, stroke and fill) and text on named layers, in paper millimetres with y up.
//! [`to_import`] turns it into curves, fills and text at a drawing scale, for
//! [`crate::dxf_import::add_to_document`].
//!
//! Both formats store circles and arcs as Bézier curves. Curves that are pieces of a circle
//! (to the accuracy those approximations have) come back as true arcs and circles, and
//! straight ones as lines, so what comes in is as editable as if it were drawn in Drift.
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use drift_core::{DVec2, LineType, LineWeight, Rgb, Shape, TextAlign};

use crate::dxf_import::{DxfEntity, DxfHatch, DxfImport, DxfLayer, DxfText};

/// How a path is outlined: colour, width and dashes (paper mm).
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Rgb,
    pub width: f64,
    pub dashes: Vec<f64>,
}

/// One piece of a subpath, from where the last one ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(DVec2),
    Cubic(DVec2, DVec2, DVec2),
}

impl Seg {
    pub fn end(&self) -> DVec2 {
        match *self {
            Seg::Line(p) | Seg::Cubic(_, _, p) => p,
        }
    }
}

/// A run of joined curves.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub start: DVec2,
    pub segs: Vec<Seg>,
    pub closed: bool,
}

impl Subpath {
    /// Points along it, from its start to its end.
    fn polygon(&self) -> Vec<DVec2> {
        let mut out = vec![self.start];
        let mut from = self.start;
        for s in &self.segs {
            match *s {
                Seg::Line(p) => out.push(p),
                Seg::Cubic(c1, c2, p) => out.extend(
                    drift_core::shape::flatten_bezier(&[from, c1, c2, p], 8)
                        .into_iter()
                        .skip(1),
                ),
            }
            from = s.end();
        }
        out
    }
}

/// A painted path: its subpaths, and how it is stroked and filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Painted {
    pub layer: String,
    pub subpaths: Vec<Subpath>,
    pub stroke: Option<Stroke>,
    pub fill: Option<Rgb>,
    /// How opaque the fill is, 0–1.
    pub fill_opacity: f64,
}

/// A line of text: its anchor on the baseline (at the start, middle or end, per `align`),
/// font size (the em, paper mm) and angle (degrees, anticlockwise).
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub layer: String,
    pub text: String,
    pub at: DVec2,
    pub size: f64,
    pub angle: f64,
    pub color: Option<Rgb>,
    pub align: TextAlign,
}

/// How a layer starts out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerFlags {
    pub visible: bool,
    pub locked: bool,
    pub printable: bool,
}

impl Default for LayerFlags {
    fn default() -> Self {
        Self {
            visible: true,
            locked: false,
            printable: true,
        }
    }
}

/// What was read from a file: the paper's size (mm), its paths and text, the layers in
/// the order the file has them (and how they start out, where the file says), and what was
/// left out or read approximately.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Artwork {
    pub size: DVec2,
    pub paths: Vec<Painted>,
    pub texts: Vec<Text>,
    /// Pictures (from a PDF page), in paper mm: they come in as an underlay.
    pub images: Vec<drift_core::UnderlayImage>,
    pub layers: Vec<String>,
    pub layer_flags: BTreeMap<String, LayerFlags>,
    /// What clipping (by clip paths, masks and the page) did.
    pub clipping: crate::clip::ClipCount,
    pub skipped: BTreeMap<String, usize>,
    pub notes: Vec<String>,
}

impl Artwork {
    pub(crate) fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_owned()).or_insert(0) += 1;
    }

    pub(crate) fn note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    /// Notes on what clipping did, once the drawing is read.
    pub(crate) fn note_clipping(&mut self) {
        let c = self.clipping;
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut parts = Vec::new();
        if c.trimmed > 0 {
            parts.push(format!(
                "{} path{} trimmed to what shows",
                c.trimmed,
                plural(c.trimmed)
            ));
        }
        if c.hidden > 0 {
            parts.push(format!(
                "{} hidden one{} left out",
                c.hidden,
                plural(c.hidden)
            ));
        }
        if !parts.is_empty() {
            self.note(format!("Clipping is applied: {}", parts.join(", ")));
        }
        if c.whole > 0 {
            self.note(format!(
                "{} filled shape{} cross{} a clipping path too intricate to cut, and come{} in \
                 whole: check around {}",
                c.whole,
                plural(c.whole),
                if c.whole == 1 { "es" } else { "" },
                if c.whole == 1 { "s" } else { "" },
                if c.whole == 1 { "it" } else { "them" }
            ));
        }
    }

    /// How many curves it would be in a sketch, about.
    pub fn curve_count(&self) -> usize {
        self.paths
            .iter()
            .flat_map(|p| &p.subpaths)
            .map(|s| s.segs.len() + usize::from(s.closed))
            .sum()
    }

    /// A layer, added to the list the first time it is used.
    pub(crate) fn layer(&mut self, name: &str) -> String {
        if !self.layers.iter().any(|l| l == name) {
            self.layers.push(name.to_owned());
        }
        name.to_owned()
    }
}

/// How artwork comes into a sketch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImportOptions {
    /// Model millimetres per paper millimetre: 50 for a drawing at 1:50.
    pub scale: f64,
    /// Edges that only bound a fill (no stroke of their own) come in as construction, so
    /// they don't print.
    pub construction_edges: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            construction_edges: true,
        }
    }
}

/// The curves, fills and text of `art` at `options.scale`, ready for
/// [`crate::dxf_import::add_to_document`]. Line weights and dashes stay as on paper.
pub fn to_import(art: &Artwork, options: &ImportOptions) -> DxfImport {
    let k = if options.scale > 0.0 && options.scale.is_finite() {
        options.scale
    } else {
        1.0
    };
    let mut out = DxfImport {
        skipped: art.skipped.clone(),
        notes: art.notes.clone(),
        fill_edges_construction: options.construction_edges,
        ..Default::default()
    };
    out.layers = art
        .layers
        .iter()
        .map(|name| {
            let flags = art.layer_flags.get(name).copied().unwrap_or_default();
            DxfLayer {
                name: name.clone(),
                color: None,
                line_type: LineType::Continuous,
                weight: None,
                visible: flags.visible,
                locked: flags.locked,
                printable: flags.printable,
            }
        })
        .collect();
    for p in &art.paths {
        let scaled: Vec<Subpath> = p.subpaths.iter().map(|s| scale_subpath(s, k)).collect();
        if let Some(stroke) = &p.stroke {
            let line_type = line_type_of(&stroke.dashes);
            let weight = Some(weight_of(stroke.width));
            for s in &scaled {
                for shape in shapes_of(s) {
                    out.entities.push(DxfEntity {
                        layer: p.layer.clone(),
                        shape,
                        line_type,
                        color: Some(stroke.color),
                        weight,
                    });
                }
            }
        }
        if let Some(color) = p.fill {
            for loops in fill_islands(&scaled) {
                out.hatches.push(DxfHatch {
                    layer: p.layer.clone(),
                    loops,
                    solid: true,
                    pattern: "SOLID".into(),
                    scale: 1.0,
                    angle: 0.0,
                    definition: Vec::new(),
                    origin: DVec2::ZERO,
                    color: Some(color),
                    opacity: Some(p.fill_opacity),
                    seeds: Vec::new(),
                });
            }
        }
    }
    for t in &art.texts {
        if t.text.trim().is_empty() {
            continue;
        }
        out.texts.push(DxfText {
            layer: t.layer.clone(),
            text: t.text.clone(),
            at: t.at * k,
            height: t.size * drift_draft::page::CAP_HEIGHT * k,
            angle: t.angle,
            color: t.color,
            align: t.align,
            spacing: None,
            wrap: None,
        });
    }
    out
}

/// The artwork's paths as outlines to trace over (paper mm): strokes as drawn, filled shapes
/// closed.
pub fn outlines(art: &Artwork) -> Vec<drift_core::RefPrim> {
    let mut out = Vec::new();
    for p in &art.paths {
        for s in &p.subpaths {
            let mut points = s.polygon();
            if (s.closed || p.fill.is_some()) && points.first() != points.last() {
                points.push(s.start);
            }
            points.dedup_by(|a, b| a.distance(*b) < 1e-9);
            match points.len() {
                0 | 1 => {}
                2 => out.push(drift_core::RefPrim::Line(points[0], points[1])),
                _ => out.push(drift_core::RefPrim::Polyline(points)),
            }
        }
    }
    out
}

/// The artwork's text as letter outlines to trace over (paper mm), in Drift's own font.
pub fn text_outlines(art: &Artwork) -> Vec<drift_core::RefPrim> {
    let texts: Vec<DxfText> = art
        .texts
        .iter()
        .map(|t| DxfText {
            layer: t.layer.clone(),
            text: t.text.clone(),
            at: t.at,
            height: t.size * drift_draft::page::CAP_HEIGHT,
            angle: t.angle,
            color: t.color,
            align: t.align,
            spacing: None,
            wrap: None,
        })
        .collect();
    crate::dxf::text_glyphs(&texts)
}

/// The scale a drawing seems to be at, found from its dimensions or, failing those, from
/// the scales written on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleGuess {
    /// 1 : `scale`.
    pub scale: f64,
    /// How many dimensions (or written scales) agree on it, of how many were read.
    pub agreeing: usize,
    pub measured: usize,
    /// Found from written scales (`SCALE: 1/4" = 1'-0"`) rather than measured dimensions.
    pub written: bool,
    /// How many written scales say the same, when it was found from the dimensions.
    pub confirmed: usize,
    /// The next most agreed-on scale and its count, when there is one: views at other
    /// scales.
    pub runner_up: Option<(f64, usize)>,
}

impl ScaleGuess {
    /// How sure it is: the share of what was read that agrees.
    pub fn confidence(&self) -> f64 {
        if self.measured == 0 {
            0.0
        } else {
            self.agreeing as f64 / self.measured as f64
        }
    }
}

/// A scale as a drawing writes it: `1/4" = 1'-0"`, `3/4" : 1'`, `SCALE: 1:50`, `1 : 100`.
/// Returns 1 : n.
pub fn written_scale(s: &str) -> Option<f64> {
    let s = s.trim();
    let upper = s.to_ascii_uppercase();
    let s = if upper.starts_with("SCALE") {
        s[5..].trim_start().trim_start_matches(':').trim()
    } else {
        s
    };
    let imperial = s.contains(['\'', '"', '\u{2032}', '\u{2033}', '\u{2019}', '\u{201d}']);
    let (paper, real) = match s.split_once('=') {
        Some(parts) => parts,
        None => s.split_once(':')?,
    };
    let n = if imperial {
        written_length(real, drift_core::units::MM_PER_INCH)?
            / written_length(paper, drift_core::units::MM_PER_INCH)?
    } else {
        let number = |t: &str| t.trim().replace(',', "").parse::<f64>().ok();
        number(real)? / number(paper)?
    };
    // (Inches and feet in millimetres don't divide exactly.)
    let n = (n * 1e6).round() / 1e6;
    (n > 0.0 && n.is_finite()).then_some(n)
}

/// A length as a drawing writes it, in mm: `3'-6"`, `3' 6 1/2"`, `42"`, `10'`, `1200 mm`,
/// `3.6 m`, or a bare number (`3600`, `3,600`) in `unit_mm`. `None` for anything else
/// (room names, radii and diameters, which aren't measured along a line).
pub fn written_length(s: &str, unit_mm: f64) -> Option<f64> {
    let s = s
        .trim()
        .replace(['\u{2032}', '\u{2019}'], "'")
        .replace(['\u{2033}', '\u{201d}'], "\"")
        .replace('\u{2013}', "-");
    if s.is_empty() || s.starts_with(['R', 'r', 'Ø', '⌀']) {
        return None;
    }
    // A whole number with an optional fraction: `6`, `6 1/2`, `6-1/2`, `1/2`, `6.5`.
    let mixed = |t: &str| -> Option<f64> {
        let t = t.trim().trim_start_matches('-').trim();
        if t.is_empty() {
            return Some(0.0);
        }
        let mut total = 0.0;
        for part in t.split([' ', '-']).filter(|p| !p.is_empty()) {
            total += match part.split_once('/') {
                Some((a, b)) => {
                    let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
                    if b == 0.0 {
                        return None;
                    }
                    a / b
                }
                None => part.parse::<f64>().ok()?,
            };
        }
        Some(total)
    };
    let mm = if s.contains(['\'', '"']) {
        let (feet, inches) = match s.split_once('\'') {
            Some((f, rest)) => (f.trim().parse::<f64>().ok()?, rest),
            None => (0.0, s.as_str()),
        };
        let inches = inches.trim().trim_end_matches('"');
        if inches.contains(['\'', '"']) {
            return None;
        }
        feet * 304.8 + mixed(inches)? * 25.4
    } else {
        let lower = s.to_ascii_lowercase();
        let (number, unit) = if let Some(n) = lower.strip_suffix("mm") {
            (n, 1.0)
        } else if let Some(n) = lower.strip_suffix("cm") {
            (n, 10.0)
        } else if let Some(n) = lower.strip_suffix('m') {
            (n, 1000.0)
        } else {
            (lower.as_str(), unit_mm)
        };
        let number = number.trim();
        // A comma before three digits groups thousands; otherwise it's a decimal point.
        let grouped = number
            .rsplit_once(',')
            .is_some_and(|(_, tail)| tail.len() == 3 && tail.chars().all(|c| c.is_ascii_digit()));
        let number = if grouped {
            number.replace(',', "")
        } else {
            number.replace(',', ".")
        };
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return None;
        }
        number.parse::<f64>().ok()? * unit
    };
    (mm > 0.0 && mm.is_finite()).then_some(mm)
}

/// The scale a drawing is at, from its dimensions: for each figure that reads as a length,
/// the line it is written along (a dimension line, perhaps broken for the figure) gives a
/// ratio, and the standard scale most of them agree on wins; failing that, the scale most
/// often written on it. Bare numbers are in `unit_mm`. Dimensions decide when at least two
/// agree, and they are at least a third of those measured (a sheet may have views at
/// several scales).
pub fn guess_scale(art: &Artwork, unit_mm: f64) -> Option<ScaleGuess> {
    // Dimension lines are open strokes (not, say, the masks behind their figures).
    let lines: Vec<(DVec2, DVec2, &str)> = art
        .paths
        .iter()
        .filter(|p| p.stroke.is_some() && p.fill.is_none())
        .flat_map(|p| {
            p.subpaths
                .iter()
                .filter(|s| !s.closed)
                .flat_map(shapes_of)
                .map(move |s| (s, p.layer.as_str()))
        })
        .filter_map(|(s, layer)| match s {
            Shape::Line { a, b } if a.distance(b) > 1e-6 => Some((a, b, layer)),
            _ => None,
        })
        .collect();
    let scales: Vec<f64> = drift_core::sheets::PREFERRED_SCALES
        .iter()
        .copied()
        .chain(
            drift_core::sheets::ARCHITECTURAL_SCALES
                .iter()
                .map(|(n, _)| *n),
        )
        .chain(
            drift_core::sheets::ENGINEERING_SCALES
                .iter()
                .map(|(n, _)| *n),
        )
        .collect();
    let snap = |ratio: f64| {
        scales
            .iter()
            .copied()
            .filter(|n| (ratio / n - 1.0).abs() < 0.03)
            .min_by(|a, b| (ratio / a - 1.0).abs().total_cmp(&(ratio / b - 1.0).abs()))
    };
    let mut votes: BTreeMap<u64, usize> = BTreeMap::new();
    let mut measured = 0;
    for t in &art.texts {
        let Some(real) = written_length(&t.text, unit_mm) else {
            continue;
        };
        let dir = DVec2::from_angle(t.angle.to_radians());
        let across = dir.perp();
        let size = t.size.max(1e-6);
        // Where the figure's middle is, roughly (Helvetica averages about half an em).
        let width = 0.5 * size * t.text.chars().count() as f64;
        let middle = match t.align {
            TextAlign::Left => t.at + dir * (width / 2.0),
            TextAlign::Center => t.at,
            TextAlign::Right => t.at - dir * (width / 2.0),
        };
        // The lengths of the lines along the figure, near it, that run under it: grouped by
        // how far off they are, and joined where they're broken for the figure.
        let spans = |same_layer: bool| -> Vec<f64> {
            let mut along: Vec<(f64, f64, f64)> = lines
                .iter()
                .filter(|(_, _, layer)| !same_layer || *layer == t.layer)
                .filter(|(a, b, _)| ((*b - *a).normalize().perp_dot(dir)).abs() < 0.035)
                .filter_map(|(a, b, _)| {
                    let off = (*a - middle).dot(across);
                    (off.abs() < 2.5 * size).then(|| {
                        let (u, v) = ((*a - middle).dot(dir), (*b - middle).dot(dir));
                        (off, u.min(v), u.max(v))
                    })
                })
                .collect();
            along.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
            let mut out = Vec::new();
            let mut k = 0;
            while k < along.len() {
                let off = along[k].0;
                let mut group: Vec<(f64, f64)> = Vec::new();
                while k < along.len() && (along[k].0 - off).abs() < 0.05 * size {
                    group.push((along[k].1, along[k].2));
                    k += 1;
                }
                group.sort_by(|x, y| x.0.total_cmp(&y.0));
                // Pieces join across a gap only where the figure sits in it (a line broken
                // for its figure); a chain of dimensions has small gaps at its extension
                // lines, which part one dimension from the next.
                let mut joined: Vec<(f64, f64)> = Vec::new();
                for (lo, hi) in group {
                    match joined.last_mut() {
                        Some(last)
                            if lo <= last.1 + 1e-6
                                || (lo - last.1 < width * 2.0 + size * 2.0
                                    && last.1 < 0.0
                                    && lo > 0.0) =>
                        {
                            last.1 = last.1.max(hi);
                        }
                        _ => joined.push((lo, hi)),
                    }
                }
                out.extend(
                    joined
                        .into_iter()
                        .filter(|(lo, hi)| *lo - width / 2.0 <= 0.0 && *hi + width / 2.0 >= 0.0)
                        .map(|(lo, hi)| hi - lo)
                        .filter(|len| *len > width * 0.8),
                );
            }
            out
        };
        // The figure's own layer's lines first (dimension tools keep both on one).
        let mut lengths = spans(true);
        if lengths.is_empty() {
            lengths = spans(false);
        }
        if lengths.is_empty() {
            continue;
        }
        measured += 1;
        // A vote for each scale a line near it gives: real dimension lines agree from one
        // figure to the next, where other lines nearby scatter.
        let mut said: Vec<u64> = lengths
            .iter()
            .filter_map(|len| snap(real / len))
            .map(f64::to_bits)
            .collect();
        said.sort_unstable();
        said.dedup();
        for bits in said {
            *votes.entry(bits).or_insert(0) += 1;
        }
    }
    // The scales written on it.
    let mut stated: BTreeMap<u64, usize> = BTreeMap::new();
    let mut statements = 0;
    for t in &art.texts {
        if let Some(ratio) = written_scale(&t.text)
            && let Some(n) = scales
                .iter()
                .copied()
                .find(|n| (ratio / n - 1.0).abs() < 0.01)
        {
            statements += 1;
            *stated.entry(n.to_bits()).or_insert(0) += 1;
        }
    }
    // The most agreed-on, and the next.
    let ranked = |m: BTreeMap<u64, usize>| {
        let mut v: Vec<(u64, usize)> = m.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        v
    };
    let runner_up = |v: &[(u64, usize)]| v.get(1).map(|(b, n)| (f64::from_bits(*b), *n));
    let measured_votes = ranked(votes);
    if let Some(&(bits, agreeing)) = measured_votes.first()
        && agreeing >= 2
        && agreeing * 3 >= measured
    {
        return Some(ScaleGuess {
            scale: f64::from_bits(bits),
            agreeing,
            measured,
            written: false,
            confirmed: stated.get(&bits).copied().unwrap_or(0),
            runner_up: runner_up(&measured_votes).filter(|(_, n)| *n >= 2),
        });
    }
    let stated = ranked(stated);
    let &(bits, agreeing) = stated.first()?;
    Some(ScaleGuess {
        scale: f64::from_bits(bits),
        agreeing,
        measured: statements,
        written: true,
        confirmed: agreeing,
        runner_up: runner_up(&stated),
    })
}

fn scale_subpath(s: &Subpath, k: f64) -> Subpath {
    Subpath {
        start: s.start * k,
        segs: s
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg::Line(p) => Seg::Line(p * k),
                Seg::Cubic(a, b, c) => Seg::Cubic(a * k, b * k, c * k),
            })
            .collect(),
        closed: s.closed,
    }
}

/// A stroke's width as a line weight: the nearest AutoCAD weight, as from DXF and DWG.
fn weight_of(width: f64) -> LineWeight {
    LineWeight::nearest_cad(width)
}

/// The line type a dash array (on, off, on, off, ... in mm) looks most like, by the same
/// rule as DXF line types (none for a solid line).
fn line_type_of(dashes: &[f64]) -> Option<LineType> {
    let dashes: Vec<f64> = dashes.iter().copied().filter(|d| *d >= 0.0).collect();
    // All gaps, or nothing on: solid.
    if dashes.iter().all(|d| *d <= 0.0) {
        return None;
    }
    // An odd list repeats to make it even; the marks are its even entries.
    let dashes = if dashes.len() % 2 == 1 {
        [&dashes[..], &dashes[..]].concat()
    } else {
        dashes
    };
    let marks: Vec<f64> = dashes.iter().step_by(2).copied().collect();
    Some(LineType::of_marks(&marks)).filter(|t| *t != LineType::Continuous)
}

/// The fill of a path as islands: each loop that isn't inside another (or is inside an even
/// number), with the loops directly inside it as its holes. A fill closes open subpaths.
fn fill_islands(subpaths: &[Subpath]) -> Vec<Vec<Vec<Shape>>> {
    let loops: Vec<(Vec<Shape>, Vec<DVec2>)> = subpaths
        .iter()
        .filter(|s| !s.segs.is_empty())
        .map(|s| {
            let mut shapes = shapes_of(s);
            let end = s.segs.last().map_or(s.start, Seg::end);
            if !s.closed && end.distance(s.start) > 1e-9 {
                shapes.push(Shape::Line { a: end, b: s.start });
            }
            (shapes, s.polygon())
        })
        .filter(|(shapes, _)| !shapes.is_empty())
        .collect();
    // How many other loops each one is inside.
    let inside = |p: DVec2, poly: &[DVec2]| drift_core::polygon::contains(poly, p);
    let probe = |poly: &[DVec2]| {
        // A point of the loop just off its first corner, along the way it goes.
        poly.first().copied().unwrap_or_default()
    };
    let parents: Vec<Vec<usize>> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|j| *j != i && inside(probe(&loops[i].1), &loops[*j].1))
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..loops.len() {
        if parents[i].len() % 2 == 1 {
            continue;
        }
        let mut island = vec![loops[i].0.clone()];
        for j in 0..loops.len() {
            if parents[j].len() == parents[i].len() + 1 && parents[j].contains(&i) {
                island.push(loops[j].0.clone());
            }
        }
        out.push(island);
    }
    out
}

/// The curves of a subpath: lines, arcs and circles where the curves are those, Béziers
/// otherwise. Arcs one after another on the same circle are joined.
pub fn shapes_of(s: &Subpath) -> Vec<Shape> {
    let mut out: Vec<Shape> = Vec::new();
    // The arc being gathered: centre, radius, start angle and sweep (signed).
    let mut arc: Option<(DVec2, f64, f64, f64)> = None;
    let flush = |arc: &mut Option<(DVec2, f64, f64, f64)>, out: &mut Vec<Shape>| {
        if let Some((center, radius, start, sweep)) = arc.take() {
            out.push(if sweep.abs() >= TAU - 1e-6 {
                Shape::Circle { center, radius }
            } else if sweep >= 0.0 {
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                }
            } else {
                Shape::Arc {
                    center,
                    radius,
                    start: start + sweep,
                    sweep: -sweep,
                }
            });
        }
    };
    let mut from = s.start;
    for seg in &s.segs {
        let to = seg.end();
        match *seg {
            Seg::Line(p) => {
                flush(&mut arc, &mut out);
                if p.distance(from) > 1e-9 {
                    out.push(Shape::Line { a: from, b: p });
                }
            }
            Seg::Cubic(c1, c2, p) => {
                if let Some((center, radius, start, sweep)) = circular(from, c1, c2, p) {
                    // (Files round their coordinates, so the pieces of one circle agree
                    // only to about a ten-thousandth of its radius.)
                    let joins = arc.is_some_and(|(c, r, _, sw)| {
                        let tol = r.max(radius) * 1e-4 + 1e-9;
                        c.distance(center) <= tol
                            && (r - radius).abs() <= tol
                            && sw.signum() == sweep.signum()
                    });
                    if joins && let Some(a) = &mut arc {
                        a.3 += sweep;
                    } else {
                        flush(&mut arc, &mut out);
                        arc = Some((center, radius, start, sweep));
                    }
                } else if straight(from, c1, c2, p) {
                    flush(&mut arc, &mut out);
                    if p.distance(from) > 1e-9 {
                        out.push(Shape::Line { a: from, b: p });
                    }
                } else {
                    flush(&mut arc, &mut out);
                    out.push(Shape::Bezier {
                        p: [from, c1, c2, p],
                    });
                }
            }
        }
        from = to;
    }
    flush(&mut arc, &mut out);
    // A closed subpath runs back to its start.
    if s.closed && from.distance(s.start) > 1e-9 {
        out.push(Shape::Line {
            a: from,
            b: s.start,
        });
    }
    out
}

fn cubic_at(p: [DVec2; 4], t: f64) -> DVec2 {
    let u = 1.0 - t;
    p[0] * (u * u * u) + p[1] * (3.0 * u * u * t) + p[2] * (3.0 * u * t * t) + p[3] * (t * t * t)
}

/// Whether a cubic is a straight line: its handles on the chord between its ends.
fn straight(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> bool {
    let chord = p3 - p0;
    let len = chord.length();
    if len < 1e-12 {
        return c1.distance(p0) < 1e-9 && c2.distance(p0) < 1e-9;
    }
    let d = chord / len;
    let off = |q: DVec2| (q - p0).perp_dot(d).abs();
    let along = |q: DVec2| (q - p0).dot(d);
    let tol = len * 1e-6 + 1e-9;
    off(c1) <= tol
        && off(c2) <= tol
        && (-tol..=len + tol).contains(&along(c1))
        && (-tol..=len + tol).contains(&along(c2))
}

/// The circular arc a cubic draws, if it is one (as the usual Bézier approximation of an arc
/// of up to a half turn is): centre, radius, start angle and signed sweep.
fn circular(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> Option<(DVec2, f64, f64, f64)> {
    let (t0, t1) = (c1 - p0, p3 - c2);
    if t0.length() < 1e-12 || t1.length() < 1e-12 || p0.distance(p3) < 1e-12 {
        return None;
    }
    // The centre is on the normals at both ends.
    let (n0, n1) = (t0.perp(), t1.perp());
    let cross = n0.perp_dot(n1);
    if cross.abs() < 1e-12 * n0.length() * n1.length() {
        return None;
    }
    let s = (p3 - p0).perp_dot(n1) / cross;
    let center = p0 + n0 * s;
    let radius = center.distance(p0);
    if radius.is_nan() || radius <= 1e-9 || !radius.is_finite() {
        return None;
    }
    // The approximation strays from the circle by under 0.03% of its radius.
    let tol = radius * 5e-4;
    let on = |q: DVec2| (center.distance(q) - radius).abs() <= tol;
    let p = [p0, c1, c2, p3];
    if !on(p3) || ![0.25, 0.5, 0.75].iter().all(|t| on(cubic_at(p, *t))) {
        return None;
    }
    let ccw = (p0 - center).perp_dot(t0) > 0.0;
    let angle = |q: DVec2| (q.y - center.y).atan2(q.x - center.x);
    let (a0, a3) = (angle(p0), angle(p3));
    let sweep = if ccw {
        (a3 - a0).rem_euclid(TAU)
    } else {
        -(a0 - a3).rem_euclid(TAU)
    };
    if sweep.abs() < 1e-9 || sweep.abs() > PI * 1.0001 {
        return None;
    }
    // Its handles are as long as an arc's are.
    let handle = 4.0 / 3.0 * (sweep.abs() / 4.0).tan() * radius;
    let close = |len: f64| (len - handle).abs() <= radius * 2e-3;
    (close(t0.length()) && close(t1.length())).then_some((center, radius, a0, sweep))
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: f64 = 0.552_284_749_830_793_4;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A circle of radius r about c as four cubics, as PDF and SVG writers make it.
    fn circle(c: DVec2, r: f64) -> Subpath {
        let q = |a: f64, b: f64| c + v(a, b) * r;
        Subpath {
            start: q(1.0, 0.0),
            segs: vec![
                Seg::Cubic(q(1.0, K), q(K, 1.0), q(0.0, 1.0)),
                Seg::Cubic(q(-K, 1.0), q(-1.0, K), q(-1.0, 0.0)),
                Seg::Cubic(q(-1.0, -K), q(-K, -1.0), q(0.0, -1.0)),
                Seg::Cubic(q(K, -1.0), q(1.0, -K), q(1.0, 0.0)),
            ],
            closed: true,
        }
    }

    #[test]
    fn approximated_circles_and_arcs_come_back_true() {
        let shapes = shapes_of(&circle(v(5.0, 7.0), 3.0));
        assert_eq!(shapes.len(), 1);
        match shapes[0] {
            Shape::Circle { center, radius } => {
                assert!(center.distance(v(5.0, 7.0)) < 1e-9 && (radius - 3.0).abs() < 1e-9)
            }
            ref s => panic!("{s:?}"),
        }
        // A half circle drawn clockwise, then a line: an arc and a line.
        let reversed = Subpath {
            start: v(-2.0, 0.0),
            segs: vec![
                Seg::Cubic(v(-2.0, K * 2.0), v(-K * 2.0, 2.0), v(0.0, 2.0)),
                Seg::Cubic(v(K * 2.0, 2.0), v(2.0, K * 2.0), v(2.0, 0.0)),
                Seg::Line(v(-2.0, 0.0)),
            ],
            closed: true,
        };
        let shapes = shapes_of(&reversed);
        assert_eq!(shapes.len(), 2, "{shapes:?}");
        match shapes[0] {
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                assert!(center.length() < 1e-9 && (radius - 2.0).abs() < 1e-9);
                assert!(
                    start.abs() < 1e-9 && (sweep - PI).abs() < 1e-9,
                    "{start} {sweep}"
                );
            }
            ref s => panic!("{s:?}"),
        }
        // A real curve stays a Bézier; a straight one becomes a line.
        let wavy = Subpath {
            start: v(0.0, 0.0),
            segs: vec![
                Seg::Cubic(v(1.0, 3.0), v(3.0, -3.0), v(4.0, 0.0)),
                Seg::Cubic(v(5.0, 0.0), v(6.0, 0.0), v(8.0, 0.0)),
            ],
            closed: false,
        };
        let shapes = shapes_of(&wavy);
        assert!(matches!(shapes[0], Shape::Bezier { .. }));
        assert!(matches!(shapes[1], Shape::Line { .. }));
    }

    #[test]
    fn a_painted_path_becomes_curves_a_fill_and_its_hole() {
        let square = |lo: f64, hi: f64| Subpath {
            start: v(lo, lo),
            segs: vec![
                Seg::Line(v(hi, lo)),
                Seg::Line(v(hi, hi)),
                Seg::Line(v(lo, hi)),
            ],
            closed: true,
        };
        let mut art = Artwork {
            size: v(100.0, 100.0),
            ..Default::default()
        };
        let layer = art.layer("Walls");
        art.paths.push(Painted {
            layer: layer.clone(),
            subpaths: vec![square(0.0, 10.0), square(2.0, 8.0), square(20.0, 30.0)],
            stroke: Some(Stroke {
                color: Rgb([255, 0, 0]),
                width: 0.35,
                dashes: vec![2.0, 1.0],
            }),
            fill: Some(Rgb([0, 0, 255])),
            fill_opacity: 1.0,
        });
        let import = to_import(
            &art,
            &ImportOptions {
                scale: 50.0,
                construction_edges: true,
            },
        );
        // Each square outlined, closing edge and all.
        assert_eq!(import.entities.len(), 12);
        assert!(
            import
                .entities
                .iter()
                .all(|e| e.weight == Some(LineWeight(35))
                    && e.line_type == Some(LineType::Hidden)
                    && e.color == Some(Rgb([255, 0, 0])))
        );
        // At 1:50.
        let far = import
            .entities
            .iter()
            .flat_map(|e| match e.shape {
                Shape::Line { a, b } => vec![a, b],
                _ => vec![],
            })
            .fold(0.0f64, |m, p| m.max(p.x));
        assert!((far - 1500.0).abs() < 1e-9);
        // Two islands: the framed square (with its hole), and the other one.
        assert_eq!(import.hatches.len(), 2);
        let mut loops: Vec<usize> = import.hatches.iter().map(|h| h.loops.len()).collect();
        loops.sort_unstable();
        assert_eq!(loops, [1, 2]);
        assert!(import.fill_edges_construction);
        assert_eq!(import.layers.len(), 1);
    }
    #[test]
    fn written_lengths_read_in_feet_inches_and_metric() {
        let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(near(written_length("3'-6\"", 1.0), 3.5 * 304.8));
        assert!(near(
            written_length("3' 6 1/2\"", 1.0),
            3.0 * 304.8 + 6.5 * 25.4
        ));
        assert!(near(
            written_length("12'-0 3/4\"", 1.0),
            12.0 * 304.8 + 0.75 * 25.4
        ));
        assert!(near(written_length("42\"", 1.0), 42.0 * 25.4));
        assert!(near(written_length("10\u{2032}", 1.0), 3048.0));
        assert!(near(written_length("3,600", 1.0), 3600.0));
        assert!(near(written_length("3.6 m", 1.0), 3600.0));
        assert!(near(written_length("1200mm", 1.0), 1200.0));
        assert!(near(written_length("2400", 25.4), 2400.0 * 25.4));
        assert_eq!(written_length("ROOM 101", 1.0), None);
        assert_eq!(written_length("R600", 1.0), None);
        assert_eq!(written_length("Ø50", 1.0), None);
    }

    #[test]
    fn the_scale_is_found_from_the_dimensions() {
        // A plan at 1/4" = 1'-0": dimension lines with their figures over them, one broken
        // for its figure, and a room name that measures nothing.
        let mut art = Artwork::default();
        let layer = art.layer("Dims");
        let inch = 25.4;
        let mut dim = |from: DVec2, feet: f64, text: &str, broken: bool| {
            let len = feet * 12.0 / 48.0 * inch;
            let to = from + DVec2::new(len, 0.0);
            let piece = |a: DVec2, b: DVec2| Subpath {
                start: a,
                segs: vec![Seg::Line(b)],
                closed: false,
            };
            let subpaths = if broken {
                let mid = from.x + len / 2.0;
                vec![
                    piece(from, DVec2::new(mid - 6.0, from.y)),
                    piece(DVec2::new(mid + 6.0, from.y), to),
                ]
            } else {
                vec![piece(from, to)]
            };
            art.paths.push(Painted {
                layer: layer.clone(),
                subpaths,
                stroke: Some(Stroke {
                    color: Rgb([0, 0, 0]),
                    width: 0.18,
                    dashes: Vec::new(),
                }),
                fill: None,
                fill_opacity: 1.0,
            });
            art.texts.push(Text {
                layer: layer.clone(),
                text: text.into(),
                at: DVec2::new(from.x + len / 2.0, from.y + 1.0),
                size: 2.5,
                angle: 0.0,
                color: None,
                align: TextAlign::Center,
            });
        };
        dim(DVec2::new(10.0, 10.0), 12.0, "12'-0\"", false);
        dim(DVec2::new(10.0, 40.0), 14.5, "14'-6\"", true);
        dim(DVec2::new(10.0, 70.0), 9.0, "9'-0\"", false);
        art.texts.push(Text {
            layer: "Dims".into(),
            text: "KITCHEN".into(),
            at: DVec2::new(30.0, 100.0),
            size: 3.0,
            angle: 0.0,
            color: None,
            align: TextAlign::Left,
        });
        let guess = guess_scale(&art, 25.4).expect("a scale");
        assert_eq!(guess.scale, 48.0);
        assert_eq!((guess.agreeing, guess.measured), (3, 3));
        assert!(!guess.written);
        // Nothing to go on: no guess.
        art.texts.retain(|t| t.text == "KITCHEN");
        assert_eq!(guess_scale(&art, 25.4), None);
        // A written scale, failing dimensions.
        art.texts[0].text = "SCALE: 3/4\" : 1\u{2019}".into();
        let guess = guess_scale(&art, 25.4).expect("a written scale");
        assert_eq!(guess.scale, 16.0);
        assert!(guess.written);
    }

    #[test]
    fn written_scales_read() {
        assert_eq!(written_scale("1/4\" = 1'-0\""), Some(48.0));
        assert_eq!(written_scale("SCALE: 3/4\u{201d} : 1\u{2019}"), Some(16.0));
        assert_eq!(written_scale("3/4\" : 1'"), Some(16.0));
        assert_eq!(written_scale("1\" = 20'"), Some(240.0));
        assert_eq!(written_scale("SCALE: 1:50"), Some(50.0));
        assert_eq!(written_scale("1 : 100"), Some(100.0));
        assert_eq!(written_scale("NOTES: SEE A-101"), None);
        assert_eq!(written_scale("DATE: 5/17/2024"), None);
    }
}
