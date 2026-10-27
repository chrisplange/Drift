//! Reading the vector drawing of a PDF page: the stroked and filled paths through nested form
//! XObjects, with their colours, widths, dashes and optional-content layers. As an underlay
//! they're outlines to trace over ([`read_page`]); as artwork, curves to edit
//! ([`read_artwork`]). Raster images come with them as pictures, placed where the page has
//! them.
//! Coordinates are PDF points with the origin at the page's lower-left corner, upright as
//! the page is shown.
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use drift_core::{DVec2, RefPrim, Rgb, UnderlayImage};
use lopdf::{Dictionary, Document, Object};

use crate::clip::{Clip, ClipCount, clip_paint, uncovered};
use crate::icc::CmykProfile;
use crate::pdf_text::Font;
use crate::vector::{Artwork, LayerFlags, Painted, Seg, Stroke, Subpath, Text};

/// The curves of one PDF page.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfPage {
    /// 1-based page number.
    pub number: u32,
    /// How many pages the file has.
    pub pages: u32,
    /// The page size in points, as shown (after its rotation).
    pub size: DVec2,
    pub prims: Vec<RefPrim>,
    /// Its pictures (scans, photos, markup), in points.
    pub images: Vec<UnderlayImage>,
    /// Its text, as the outlines of its letters (closed polylines), in points.
    pub glyphs: Vec<RefPrim>,
    /// Text runs that couldn't be drawn, and images that couldn't be read, that were skipped.
    pub skipped_text: usize,
    pub skipped_images: usize,
}

/// Beyond this many segments a page is cut short (it'd be too heavy to trace over).
const MAX_SEGMENTS: usize = 1_000_000;
/// Form XObjects nest at most this deep.
const MAX_DEPTH: usize = 12;

/// An affine transform `[a b c d e f]`, as in PDF: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, p: DVec2) -> DVec2 {
        let [a, b, c, d, e, f] = self.0;
        DVec2::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// `self` applied first, then `then`.
    fn then(&self, then: &Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = then.0;
        Matrix([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn nums<const N: usize>(ops: &[Object]) -> Option<[f64; N]> {
    if ops.len() < N {
        return None;
    }
    let mut out = [0.0; N];
    for (i, o) in ops[ops.len() - N..].iter().enumerate() {
        out[i] = num(o)?;
    }
    Some(out)
}

/// The number of pages in a PDF file.
pub fn page_count(bytes: &[u8]) -> Result<u32, String> {
    let doc = Document::load_mem(bytes).map_err(|e| format!("not a readable PDF ({e})"))?;
    Ok(doc.get_pages().len() as u32)
}

/// A page as drawn: its painted paths in points, and what was passed over.
struct PageDrawing {
    number: u32,
    pages: u32,
    size: DVec2,
    paths: Vec<PdfPath>,
    texts: Vec<PdfText>,
    /// What its text runs show, letter by letter (when reading it to trace over).
    glyph_runs: Vec<GlyphRun>,
    /// Strokes (and text runs) that opaque fills painted later hide, and how many there were.
    covered: Vec<PdfPath>,
    covered_count: usize,
    /// How each layer starts out (hidden, locked, not printed), by name.
    layers: BTreeMap<String, LayerFlags>,
    /// The layers in the order the page first draws on them: bottom first.
    layer_order: Vec<String>,
    skipped_text: usize,
    /// Its pictures; how many images couldn't be read, by why; and how many pictures are
    /// clipped in the file (they come in whole).
    pictures: Vec<UnderlayImage>,
    unread: BTreeMap<&'static str, usize>,
    clipped_pictures: usize,
    skipped_shadings: usize,
    clipping: ClipCount,
    /// What the drawing has that is left out: see-through colours, blend modes, soft masks.
    transparency: bool,
    blending: bool,
    soft_masks: bool,
    /// It has more than the reader takes, and was cut short.
    cut_short: bool,
}

/// A run of text, in points on the upright page: where its baseline starts and ends, its
/// font size and angle (radians).
#[derive(Clone, Debug)]
struct PdfText {
    text: String,
    start: DVec2,
    end: DVec2,
    size: f64,
    angle: f64,
    color: Rgb,
    layer: Option<String>,
}

/// A text run as shown, to draw its letters: each code with the matrix from its letter's
/// space (ems of the font size) to the upright page, or (Type 3) the letters drawn already.
struct GlyphRun {
    font: Rc<Font>,
    glyphs: Vec<(u32, Matrix)>,
    drawn: Vec<Vec<DVec2>>,
    start: DVec2,
    end: DVec2,
    /// An opaque fill painted later hides it.
    hidden: bool,
}

/// A Type 3 font's glyph procedures by code, and the resources they draw with.
struct Type3<'a> {
    procs: Vec<Option<&'a lopdf::Stream>>,
    resources: Option<&'a Dictionary>,
}

/// Letter outlines past this many points are left out (they'd be too heavy to show).
const MAX_GLYPH_POINTS: usize = 2_000_000;

/// The layer what opaque fills hide comes in on (hidden).
const COVERED: &str = "Covered by fills";

/// Illustrator's stand-in page for a file saved without its PDF copy.
const NO_PDF_CONTENT: &str = "without PDF Content";

/// A painted path, in points on the upright page.
#[derive(Clone)]
struct PdfPath {
    subpaths: Vec<Subpath>,
    stroke: Option<Stroke>,
    fill: Option<Rgb>,
    /// How opaque the fill is, 0–1 (the graphics state's `ca`).
    fill_opacity: f64,
    /// The optional-content group (layer) it is drawn in.
    layer: Option<String>,
    /// Its box (of its points and handles), corner to corner.
    bounds: (DVec2, DVec2),
}

impl PdfPath {
    fn new(
        subpaths: Vec<Subpath>,
        stroke: Option<Stroke>,
        fill: Option<Rgb>,
        fill_opacity: f64,
        layer: Option<String>,
    ) -> Self {
        let bounds = subpaths
            .iter()
            .flat_map(|s| {
                std::iter::once(s.start).chain(s.segs.iter().flat_map(|g| match *g {
                    Seg::Line(p) => [p, p, p],
                    Seg::Cubic(a, b, p) => [a, b, p],
                }))
            })
            .fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(p), hi.max(p)),
            );
        Self {
            subpaths,
            stroke,
            fill,
            fill_opacity,
            layer,
            bounds,
        }
    }
}

/// Read the curves of page `number` (1-based), as outlines to trace over.
pub fn read_page(bytes: &[u8], number: u32) -> Result<PdfPage, String> {
    let page = read_drawing(bytes, number, true)?;
    refuse_stand_in(&page)?;
    let mut prims = Vec::new();
    for path in &page.paths {
        // Filled shapes trace as their outlines, which are closed.
        let close = path.fill.is_some();
        for s in &path.subpaths {
            let mut points = vec![s.start];
            let mut from = s.start;
            for seg in &s.segs {
                match *seg {
                    Seg::Line(p) => points.push(p),
                    Seg::Cubic(c1, c2, p) => flatten_cubic(from, c1, c2, p, &mut points),
                }
                from = seg.end();
            }
            if (close || s.closed) && points.len() > 2 && points.first() != points.last() {
                points.push(s.start);
            }
            add_prim(&mut prims, points);
        }
    }
    let (glyphs, lost) = glyph_outlines(&page.glyph_runs);
    Ok(PdfPage {
        number: page.number,
        pages: page.pages,
        size: page.size,
        prims,
        images: page.pictures,
        glyphs,
        skipped_text: lost,
        skipped_images: page.unread.values().sum(),
    })
}

/// The letters of the runs that show, as closed polylines on the page, and how many runs
/// had letters that couldn't be drawn.
fn glyph_outlines(runs: &[GlyphRun]) -> (Vec<RefPrim>, usize) {
    let mut out = Vec::new();
    let (mut points, mut lost) = (0, 0);
    for run in runs.iter().filter(|r| !r.hidden) {
        let mut missing = false;
        let mut rings: Vec<Vec<DVec2>> = Vec::new();
        for (code, m) in &run.glyphs {
            match run.font.outline(*code) {
                Some(o) => rings.extend(o.iter().map(|r| r.iter().map(|p| m.apply(*p)).collect())),
                None => missing |= run.font.unreadable || !run.font.text(*code).trim().is_empty(),
            }
        }
        rings.extend(run.drawn.iter().cloned());
        lost += usize::from(missing);
        for ring in rings {
            points += ring.len();
            if points > MAX_GLYPH_POINTS {
                return (out, lost + 1);
            }
            if ring.len() >= 2 {
                out.push(RefPrim::Polyline(ring));
            }
        }
    }
    (out, lost)
}

/// An Illustrator file saved without its PDF copy has only a page saying so.
fn refuse_stand_in(page: &PageDrawing) -> Result<(), String> {
    let stand_in = page.texts.iter().any(|t| {
        t.text
            .to_ascii_lowercase()
            .contains(&NO_PDF_CONTENT.to_ascii_lowercase())
    });
    if stand_in {
        Err(
            "the Illustrator file was saved without its PDF copy: open it in Illustrator and \
             save it again with \u{201c}Create PDF Compatible File\u{201d} on"
                .into(),
        )
    } else {
        Ok(())
    }
}

/// Read page `number` (1-based) as artwork to edit: its paths with their strokes and fills
/// (in millimetres on the paper), on the PDF's layers where it has them, otherwise on one
/// named `name`.
pub fn read_artwork(bytes: &[u8], number: u32, name: &str) -> Result<Artwork, String> {
    let page = read_drawing(bytes, number, false)?;
    refuse_stand_in(&page)?;
    let mm = drift_core::underlays::MM_PER_POINT;
    let mut art = Artwork {
        size: page.size * mm,
        ..Default::default()
    };
    // The file's layers in the order it draws them (the last on top, as in Drift), with
    // how they start out.
    for layer in &page.layer_order {
        if page.paths.iter().any(|p| p.layer.as_deref() == Some(layer))
            || page.texts.iter().any(|t| t.layer.as_deref() == Some(layer))
        {
            art.layer(layer);
            let flags = page.layers.get(layer).copied().unwrap_or_default();
            art.layer_flags.insert(layer.clone(), flags);
        }
    }
    let hidden = art.layer_flags.values().filter(|f| !f.visible).count();
    if hidden > 0 {
        art.note(format!(
            "{hidden} layer{} hidden in the file, and come{} in hidden",
            if hidden == 1 { " was" } else { "s were" },
            if hidden == 1 { "s" } else { "" }
        ));
    }
    // What opaque fills hide, kept on a hidden layer of its own.
    if page.covered_count > 0 {
        art.layer(COVERED);
        art.layer_flags.insert(
            COVERED.into(),
            LayerFlags {
                visible: false,
                ..Default::default()
            },
        );
        let n = page.covered_count;
        art.note(format!(
            "{n} line{} or text{} hidden under opaque fills in the file {} on the hidden \
             layer \u{201c}{COVERED}\u{201d}",
            if n == 1 { "" } else { "s" },
            if n == 1 { "" } else { "s" },
            if n == 1 { "is" } else { "are" },
        ));
    }
    for t in &page.texts {
        let layer = art.layer(t.layer.as_deref().unwrap_or(name));
        art.texts.push(Text {
            layer,
            text: t.text.clone(),
            at: t.start * mm,
            size: t.size * mm,
            angle: t.angle.to_degrees(),
            color: Some(t.color),
            align: drift_core::TextAlign::Left,
        });
    }
    for path in page.paths.into_iter().chain(page.covered) {
        let layer = art.layer(path.layer.as_deref().unwrap_or(name));
        let scaled = |p: DVec2| p * mm;
        art.paths.push(Painted {
            layer,
            subpaths: path
                .subpaths
                .into_iter()
                .map(|s| Subpath {
                    start: scaled(s.start),
                    segs: s
                        .segs
                        .into_iter()
                        .map(|seg| match seg {
                            Seg::Line(p) => Seg::Line(scaled(p)),
                            Seg::Cubic(a, b, c) => Seg::Cubic(scaled(a), scaled(b), scaled(c)),
                        })
                        .collect(),
                    closed: s.closed,
                })
                .collect(),
            stroke: path.stroke.map(|s| Stroke {
                color: s.color,
                width: s.width * mm,
                dashes: s.dashes.iter().map(|d| d * mm).collect(),
            }),
            fill: path.fill,
            fill_opacity: path.fill_opacity,
        });
    }
    if page.skipped_text > 0 {
        art.skipped.insert(
            "text in a font whose characters can't be read".into(),
            page.skipped_text,
        );
    }
    for (why, n) in &page.unread {
        art.skipped.insert((*why).into(), *n);
    }
    if !page.pictures.is_empty() {
        let n = page.pictures.len();
        art.note(format!(
            "{n} picture{} (scans, photos or markup) come{} in on an underlay, under the \
             drawing's lines",
            if n == 1 { "" } else { "s" },
            if n == 1 { "s" } else { "" }
        ));
        if page.clipped_pictures > 0 {
            art.note(format!(
                "{} picture{} cropped in the file, and come{} in whole",
                page.clipped_pictures,
                if page.clipped_pictures == 1 {
                    " is"
                } else {
                    "s are"
                },
                if page.clipped_pictures == 1 { "s" } else { "" }
            ));
        }
        art.images = page
            .pictures
            .into_iter()
            .map(|i| UnderlayImage {
                at: i.at * mm,
                across: i.across * mm,
                down: i.down * mm,
                ..i
            })
            .collect();
    }
    if page.skipped_shadings > 0 {
        art.skipped
            .insert("gradient (shading)".into(), page.skipped_shadings);
    }
    art.clipping = page.clipping;
    art.note_clipping();
    if page.transparency {
        art.note("Transparency of lines and text is left out: they come in solid");
    }
    if page.blending {
        art.note("Blend modes (multiply, screen and the like) are left out");
    }
    if page.soft_masks {
        art.note("Soft masks (fades) are left out");
    }
    if page.cut_short {
        art.note(format!(
            "The drawing is very large: only its first {} segments are read",
            MAX_SEGMENTS
        ));
    }
    if page.pages > 1 {
        art.note(format!("Page {} of {}", page.number, page.pages));
    }
    Ok(art)
}

/// The page as drawn; with `outline_text`, with its text runs' letters too.
fn read_drawing(bytes: &[u8], number: u32, outline_text: bool) -> Result<PageDrawing, String> {
    let doc = Document::load_mem(bytes).map_err(|e| format!("not a readable PDF ({e})"))?;
    if doc.is_encrypted() {
        return Err("the PDF is encrypted".into());
    }
    let pages = doc.get_pages();
    let count = pages.len() as u32;
    let &page_id = pages
        .get(&number)
        .ok_or_else(|| format!("there is no page {number} (the file has {count})"))?;
    // The visible area and the page's rotation (both may be inherited from the page tree).
    let inherited = |key: &[u8]| -> Option<&Object> {
        let mut id = page_id;
        let mut seen = HashSet::new();
        loop {
            let dict = doc.get_dictionary(id).ok()?;
            if let Ok(o) = dict.get(key) {
                return doc.dereference(o).ok().map(|(_, o)| o);
            }
            id = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
            if !seen.insert(id) {
                return None;
            }
        }
    };
    let rect = |o: Option<&Object>| -> Option<[f64; 4]> {
        let a = o?.as_array().ok()?;
        let v: Vec<f64> = a
            .iter()
            .filter_map(|o| num(doc.dereference(o).ok()?.1))
            .collect();
        (v.len() == 4).then(|| {
            [
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ]
        })
    };
    let [x0, y0, x1, y1] = rect(inherited(b"CropBox"))
        .or_else(|| rect(inherited(b"MediaBox")))
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let rotate = inherited(b"Rotate")
        .and_then(num)
        .map_or(0, |r| (r as i64).rem_euclid(360));
    let (w, h) = (x1 - x0, y1 - y0);
    // From PDF user space to the upright page with its lower-left corner at the origin.
    let shift = Matrix([1.0, 0.0, 0.0, 1.0, -x0, -y0]);
    let (turn, size) = match rotate {
        90 => (Matrix([0.0, -1.0, 1.0, 0.0, 0.0, w]), DVec2::new(h, w)),
        180 => (Matrix([-1.0, 0.0, 0.0, -1.0, w, h]), DVec2::new(w, h)),
        270 => (Matrix([0.0, 1.0, -1.0, 0.0, h, 0.0]), DVec2::new(h, w)),
        _ => (Matrix::IDENTITY, DVec2::new(w, h)),
    };
    let base = shift.then(&turn);

    let mut reader = Reader {
        doc: &doc,
        cmyk: output_intent(&doc),
        profiles: HashMap::new(),
        paths: Vec::new(),
        texts: Vec::new(),
        glyph_runs: Vec::new(),
        outline_text,
        in_glyph: false,
        type3: HashMap::new(),
        covered: Vec::new(),
        covered_count: 0,
        fonts: HashMap::new(),
        segments: 0,
        layer_order: Vec::new(),
        skipped_text: 0,
        pictures: Vec::new(),
        unread: BTreeMap::new(),
        clipped_pictures: 0,
        skipped_shadings: 0,
        clipping: ClipCount::default(),
        transparency: false,
        blending: false,
        soft_masks: false,
        cut_short: false,
    };
    let content = doc.get_page_content(page_id);
    let (resources, inherited_ids) = doc
        .get_page_resources(page_id)
        .unwrap_or((None, Vec::new()));
    let mut res: Vec<&Dictionary> = resources.into_iter().collect();
    res.extend(
        inherited_ids
            .iter()
            .filter_map(|id| doc.get_dictionary(*id).ok()),
    );
    let state = State {
        ctm: base,
        width: 1.0,
        dashes: Vec::new(),
        stroke: Rgb([0, 0, 0]),
        fill: Rgb([0, 0, 0]),
        stroke_space: Space::Gray,
        fill_space: Space::Gray,
        stroke_alpha: 1.0,
        fill_alpha: 1.0,
        normal_blend: true,
        soft_mask: false,
        text: TextState::default(),
        // Nothing shows beyond the page (an artboard, for Illustrator).
        clips: vec![Rc::new(Clip::rect(DVec2::ZERO, size))],
    };
    reader.run(&content, state, &res, &mut Vec::new(), 0)?;
    Ok(PageDrawing {
        number,
        pages: count,
        size,
        paths: reader.paths,
        texts: merge_runs(reader.texts),
        glyph_runs: reader.glyph_runs,
        covered: reader.covered,
        covered_count: reader.covered_count,
        layers: layer_states(&doc),
        layer_order: reader.layer_order,
        skipped_text: reader.skipped_text,
        pictures: reader.pictures,
        unread: reader.unread,
        clipped_pictures: reader.clipped_pictures,
        skipped_shadings: reader.skipped_shadings,
        clipping: reader.clipping,
        transparency: reader.transparency,
        blending: reader.blending,
        soft_masks: reader.soft_masks,
        cut_short: reader.cut_short,
    })
}

/// The graphics state that painting uses.
#[derive(Clone, Debug)]
struct State {
    ctm: Matrix,
    /// Line width and dashes, in the user space they were set in.
    width: f64,
    dashes: Vec<f64>,
    stroke: Rgb,
    fill: Rgb,
    /// The colour spaces `SC`/`sc` colours are in.
    stroke_space: Space,
    fill_space: Space,
    /// Constant opacity (`CA`, `ca`), 0–1.
    stroke_alpha: f64,
    fill_alpha: f64,
    /// Painting replaces what is beneath (no blend mode, no soft mask), so an opaque fill
    /// hides it.
    normal_blend: bool,
    soft_mask: bool,
    text: TextState,
    /// The clipping paths painting is seen through, outermost first.
    clips: Vec<Rc<Clip>>,
}

/// The text state: font and size, spacing, scaling, leading, rise and rendering mode.
#[derive(Clone, Debug)]
struct TextState {
    font: Option<Rc<Font>>,
    size: f64,
    char_space: f64,
    word_space: f64,
    scale: f64,
    leading: f64,
    rise: f64,
    /// 3 is invisible (as scanned pages' searchable text is).
    mode: i64,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            size: 12.0,
            char_space: 0.0,
            word_space: 0.0,
            scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            mode: 0,
        }
    }
}

impl State {
    /// How much the current transform scales lengths.
    fn scale(&self) -> f64 {
        let [a, b, c, d, _, _] = self.ctm.0;
        (a * d - b * c).abs().sqrt()
    }
}

/// A colour space, as far as Drift follows it to a colour on screen.
#[derive(Clone, Debug)]
pub(crate) enum Space {
    Gray,
    Rgb,
    /// Through its ICC profile, or else as U.S. Web Coated (SWOP) shows it.
    Cmyk(Option<Rc<CmykProfile>>),
    /// CIE Lab (taken as D50).
    Lab,
    /// Colours by number from a table in another space.
    Indexed {
        base: Rc<Space>,
        high: usize,
        table: Rc<Vec<u8>>,
    },
    /// Tints of named inks (a spot colour; `DeviceN`), shown in another space.
    Tint {
        inputs: usize,
        alternate: Rc<Space>,
        function: TintFunction,
    },
    /// Patterns (gradients and tiles), which aren't read: what is painted keeps the colour
    /// before.
    Pattern,
}

/// How tints map into a separation's alternate space.
#[derive(Clone, Debug)]
pub(crate) enum TintFunction {
    /// Type 2: `C0 + t^N (C1 − C0)`.
    Exponential { c0: Vec<f64>, c1: Vec<f64>, n: f64 },
    /// Others (sampled, PostScript): shown as grey, darker with more ink.
    Unknown,
}

impl Space {
    pub(crate) fn components(&self) -> usize {
        match self {
            Space::Gray | Space::Indexed { .. } | Space::Pattern => 1,
            Space::Rgb | Space::Lab => 3,
            Space::Cmyk(_) => 4,
            Space::Tint { inputs, .. } => *inputs,
        }
    }

    /// The colour `cs`/`CS` starts out with: black, or full ink.
    fn initial(&self) -> Option<Rgb> {
        match self {
            Space::Cmyk(_) => self.color(&[0.0, 0.0, 0.0, 1.0]),
            Space::Tint { inputs, .. } => self.color(&vec![1.0; *inputs]),
            Space::Pattern => None,
            _ => self.color(&vec![0.0; self.components()]),
        }
    }

    /// The colour of operands in this space (`None` for a pattern, or too few numbers).
    /// How an image's samples map to colour values when it doesn't say (`Decode`).
    pub(crate) fn default_decode(&self, bits: usize) -> Vec<(f64, f64)> {
        match self {
            Space::Indexed { .. } => vec![(0.0, f64::from((1u32 << bits) - 1))],
            Space::Lab => vec![(0.0, 100.0), (-100.0, 100.0), (-100.0, 100.0)],
            _ => vec![(0.0, 1.0); self.components()],
        }
    }

    pub(crate) fn color(&self, v: &[f64]) -> Option<Rgb> {
        let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        if v.len() < self.components() {
            return None;
        }
        Some(match self {
            Space::Gray => Rgb([byte(v[0]); 3]),
            Space::Rgb => Rgb([byte(v[0]), byte(v[1]), byte(v[2])]),
            Space::Cmyk(profile) => {
                let c = [v[0], v[1], v[2], v[3]];
                match profile {
                    Some(p) => p.to_rgb(c),
                    None => crate::icc::cmyk_to_rgb(c),
                }
            }
            Space::Lab => crate::icc::xyz_d50_to_srgb(crate::icc::lab_to_xyz([v[0], v[1], v[2]])),
            Space::Indexed { base, high, table } => {
                let n = base.components();
                let i = (v[0].round().max(0.0) as usize).min(*high);
                let entry = table.get(i * n..i * n + n)?;
                let values: Vec<f64> = entry.iter().map(|b| f64::from(*b) / 255.0).collect();
                // (A Lab table's entries span its ranges; taken as the usual ones.)
                let values = if matches!(**base, Space::Lab) {
                    vec![
                        values[0] * 100.0,
                        values[1] * 255.0 - 128.0,
                        values[2] * 255.0 - 128.0,
                    ]
                } else {
                    values
                };
                base.color(&values)?
            }
            Space::Tint {
                inputs,
                alternate,
                function,
            } => match function {
                TintFunction::Exponential { c0, c1, n } if *inputs == 1 => {
                    let t = v[0].clamp(0.0, 1.0).powf(*n);
                    let out: Vec<f64> = c0.iter().zip(c1).map(|(a, b)| a + t * (b - a)).collect();
                    alternate.color(&out)?
                }
                _ => {
                    let ink = v[..*inputs].iter().sum::<f64>().clamp(0.0, 1.0);
                    Rgb([byte(1.0 - ink); 3])
                }
            },
            Space::Pattern => return None,
        })
    }
}

/// The CMYK profile of the file's output intent (what a PDF/X file is printed with), which
/// plain CMYK colours are shown through.
fn output_intent(doc: &Document) -> Option<Rc<CmykProfile>> {
    let intents = doc.catalog().ok()?.get_deref(b"OutputIntents", doc).ok()?;
    intents.as_array().ok()?.iter().find_map(|i| {
        let intent = doc.dereference(i).ok()?.1.as_dict().ok()?;
        let stream = intent
            .get_deref(b"DestOutputProfile", doc)
            .ok()?
            .as_stream()
            .ok()?;
        let data = stream.decompressed_content().ok()?;
        CmykProfile::parse(&data).map(Rc::new)
    })
}

/// A PDF text string as text (UTF-16 with its byte-order mark, or one byte a character).
fn text_string(o: &Object) -> Option<String> {
    let Object::String(bytes, _) = o else {
        return None;
    };
    Some(if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|b| char::from(*b)).collect()
    })
}

struct Reader<'a> {
    doc: &'a Document,
    /// What plain CMYK is shown through: the output intent's profile, if any.
    cmyk: Option<Rc<CmykProfile>>,
    /// ICC profiles read so far, by where their streams are.
    profiles: HashMap<usize, Option<Rc<CmykProfile>>>,
    paths: Vec<PdfPath>,
    texts: Vec<PdfText>,
    glyph_runs: Vec<GlyphRun>,
    /// Keep text runs' letters (`glyph_runs`), to draw them.
    outline_text: bool,
    /// A Type 3 letter is being drawn (its fills hide nothing).
    in_glyph: bool,
    /// Type 3 fonts' glyph procedures, by where the fonts are (`Rc` pointers).
    type3: HashMap<usize, Type3<'a>>,
    covered: Vec<PdfPath>,
    covered_count: usize,
    /// Fonts read so far, by where their dictionaries are.
    fonts: HashMap<usize, Rc<Font>>,
    segments: usize,
    /// The layers in the order the page first draws on them.
    layer_order: Vec<String>,
    skipped_text: usize,
    pictures: Vec<UnderlayImage>,
    unread: BTreeMap<&'static str, usize>,
    clipped_pictures: usize,
    skipped_shadings: usize,
    clipping: ClipCount,
    transparency: bool,
    blending: bool,
    soft_masks: bool,
    cut_short: bool,
}

impl<'a> Reader<'a> {
    /// A named resource of a kind (`XObject`, `ExtGState`, `Properties`) from the resource
    /// dictionaries.
    fn resource(
        &self,
        resources: &[&'a Dictionary],
        kind: &[u8],
        name: &[u8],
    ) -> Option<&'a Object> {
        for res in resources {
            let Ok(dict) = res.get_deref(kind, self.doc).and_then(Object::as_dict) else {
                continue;
            };
            if let Ok(o) = dict.get(name) {
                return self.doc.dereference(o).ok().map(|(_, o)| o);
            }
        }
        None
    }

    /// A font by its resource name, read once.
    fn font(&mut self, resources: &[&'a Dictionary], name: &[u8]) -> Option<Rc<Font>> {
        let dict = self.resource(resources, b"Font", name)?.as_dict().ok()?;
        let key = std::ptr::from_ref(dict) as usize;
        if let Some(f) = self.fonts.get(&key) {
            return Some(f.clone());
        }
        let font = Rc::new(Font::read(self.doc, dict));
        self.fonts.insert(key, font.clone());
        if font.type3.is_some() {
            let doc = self.doc;
            let procs = dict
                .get_deref(b"CharProcs", doc)
                .and_then(Object::as_dict)
                .ok();
            let procs = crate::pdf_glyphs::differences(doc, dict)
                .into_iter()
                .map(|name| {
                    let o = procs?.get(name?.as_bytes()).ok()?;
                    doc.dereference(o).ok()?.1.as_stream().ok()
                })
                .collect();
            let resources = dict
                .get_deref(b"Resources", doc)
                .and_then(Object::as_dict)
                .ok();
            self.type3
                .insert(Rc::as_ptr(&font) as usize, Type3 { procs, resources });
        }
        Some(font)
    }

    /// An image painted with the graphics state `gs`: its pixels, placed where it fills the
    /// unit square of its user space (its first row at the top).
    fn picture(&mut self, stream: &lopdf::Stream, resources: &[&'a Dictionary], gs: &State) {
        let space = match stream.dict.get(b"ColorSpace") {
            Ok(cs) => self.space(resources, cs, 0),
            Err(_) => Space::Gray,
        };
        let m = gs.ctm;
        let at = m.apply(DVec2::new(0.0, 1.0));
        let across = m.apply(DVec2::new(1.0, 1.0)) - at;
        let down = m.apply(DVec2::ZERO) - at;
        // Off the page (or clipped away by the page's edge): nothing to show.
        let corners = [at, at + across, at + down, at + across + down];
        let (lo, hi) = corners.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        if let Some(page) = gs.clips.first() {
            let (plo, phi) = page.bounds();
            if hi.x < plo.x || lo.x > phi.x || hi.y < plo.y || lo.y > phi.y {
                return;
            }
        }
        if across.perp_dot(down).abs() < 1e-9 {
            return;
        }
        match crate::pdf_image::decode(self.doc, stream, &space, gs.fill, gs.fill_alpha) {
            Ok(px) => {
                if gs.clips.len() > 1 {
                    self.clipped_pictures += 1;
                }
                self.pictures.push(UnderlayImage {
                    width: px.width,
                    height: px.height,
                    rgba: std::sync::Arc::new(px.rgba),
                    at,
                    across,
                    down,
                });
            }
            Err(why) => *self.unread.entry(why).or_insert(0) += 1,
        }
    }

    /// Strokes and text painted before an opaque fill over `region`, under it: put on the
    /// covered layer, strokes cut where the fill's edge crosses them. A stroke along the
    /// fill's edge still shows (half of it does).
    fn cover(&mut self, region: &[Subpath]) {
        let Some(cover) = Clip::from_subpaths(region) else {
            return;
        };
        let (lo, hi) = cover.bounds();
        let overlaps =
            |(a, b): (DVec2, DVec2)| a.x <= hi.x && b.x >= lo.x && a.y <= hi.y && b.y >= lo.y;
        let mut i = 0;
        while i < self.paths.len() {
            let path = &self.paths[i];
            let Some(stroke) = path.stroke.clone().filter(|_| overlaps(path.bounds)) else {
                i += 1;
                continue;
            };
            let margin = (stroke.width / 2.0).max(0.01);
            let (mut shown, mut covered) = (Vec::new(), Vec::new());
            for s in &path.subpaths {
                let (a, b) = uncovered(s, &cover, margin);
                shown.extend(a);
                covered.extend(b);
            }
            if covered.is_empty() {
                i += 1;
                continue;
            }
            self.covered_count += 1;
            self.covered.push(PdfPath::new(
                covered,
                Some(stroke.clone()),
                None,
                1.0,
                Some(COVERED.into()),
            ));
            // A filled path keeps its fill as it was (fills keep their order); its outline
            // is what still shows.
            let mut replace = Vec::new();
            if path.fill.is_some() {
                replace.push(PdfPath::new(
                    path.subpaths.clone(),
                    None,
                    path.fill,
                    path.fill_opacity,
                    path.layer.clone(),
                ));
            }
            if !shown.is_empty() {
                replace.push(PdfPath::new(
                    shown,
                    Some(stroke),
                    None,
                    1.0,
                    path.layer.clone(),
                ));
            }
            let n = replace.len();
            self.paths.splice(i..=i, replace);
            i += n;
        }
        for r in &mut self.glyph_runs {
            if [r.start, r.end].iter().all(|p| cover.covers(*p, 0.01)) {
                r.hidden = true;
            }
        }
        for t in &mut self.texts {
            let hidden = [t.start, t.end].iter().all(|p| cover.covers(*p, 0.01));
            if hidden && t.layer.as_deref() != Some(COVERED) {
                t.layer = Some(COVERED.into());
                self.covered_count += 1;
            }
        }
    }

    /// The colour space an operand of `cs`/`CS` names (or, nested, is).
    fn space(&mut self, resources: &[&'a Dictionary], o: &Object, depth: usize) -> Space {
        let doc = self.doc;
        let o = doc.dereference(o).map_or(o, |(_, o)| o);
        if depth > 4 {
            return Space::Gray;
        }
        let family = match o {
            Object::Name(n) => n.as_slice(),
            Object::Array(a) => a.first().and_then(|n| n.as_name().ok()).unwrap_or_default(),
            _ => return Space::Gray,
        };
        let arg = |i: usize| match o {
            Object::Array(a) => a
                .get(i)
                .and_then(|x| doc.dereference(x).ok().map(|(_, x)| x)),
            _ => None,
        };
        match family {
            b"DeviceGray" | b"G" | b"CalGray" => Space::Gray,
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Space::Rgb,
            b"DeviceCMYK" | b"CMYK" => Space::Cmyk(self.cmyk.clone()),
            b"Lab" => Space::Lab,
            b"Pattern" => Space::Pattern,
            b"ICCBased" => {
                let Some(stream) = arg(1).and_then(|s| s.as_stream().ok()) else {
                    return Space::Gray;
                };
                let n = stream.dict.get(b"N").ok().and_then(num).unwrap_or(0.0) as usize;
                match n {
                    1 => Space::Gray,
                    3 => Space::Rgb,
                    4 => {
                        let key = std::ptr::from_ref(stream) as usize;
                        let profile = self
                            .profiles
                            .entry(key)
                            .or_insert_with(|| {
                                let data = stream.decompressed_content().ok()?;
                                CmykProfile::parse(&data).map(Rc::new)
                            })
                            .clone();
                        Space::Cmyk(profile.or_else(|| self.cmyk.clone()))
                    }
                    _ => match stream.dict.get(b"Alternate") {
                        Ok(alt) => self.space(resources, alt, depth + 1),
                        Err(_) => Space::Gray,
                    },
                }
            }
            b"Indexed" | b"I" => {
                let base = match arg(1) {
                    Some(b) => self.space(resources, b, depth + 1),
                    None => return Space::Gray,
                };
                let high = arg(2).and_then(num).unwrap_or(0.0).clamp(0.0, 255.0) as usize;
                let table = match arg(3) {
                    Some(Object::String(bytes, _)) => bytes.clone(),
                    Some(Object::Stream(st)) => st
                        .decompressed_content()
                        .unwrap_or_else(|_| st.content.clone()),
                    _ => return Space::Gray,
                };
                Space::Indexed {
                    base: Rc::new(base),
                    high,
                    table: Rc::new(table),
                }
            }
            b"Separation" | b"DeviceN" => {
                let inputs = if family == b"Separation" {
                    1
                } else {
                    arg(1)
                        .and_then(|a| a.as_array().ok())
                        .map_or(1, |a| a.len().max(1))
                };
                let alternate = match arg(2) {
                    Some(a) => self.space(resources, a, depth + 1),
                    None => Space::Gray,
                };
                let function = arg(3)
                    .and_then(|f| match f {
                        Object::Dictionary(d) => Some(d),
                        Object::Stream(s) => Some(&s.dict),
                        _ => None,
                    })
                    .filter(|d| d.get(b"FunctionType").ok().and_then(num) == Some(2.0))
                    .map_or(TintFunction::Unknown, |d| {
                        let list = |k: &[u8], default: f64| -> Vec<f64> {
                            d.get(k)
                                .ok()
                                .and_then(|a| a.as_array().ok())
                                .map(|a| a.iter().filter_map(num).collect())
                                .unwrap_or_else(|| vec![default])
                        };
                        TintFunction::Exponential {
                            c0: list(b"C0", 0.0),
                            c1: list(b"C1", 1.0),
                            n: d.get(b"N").ok().and_then(num).unwrap_or(1.0),
                        }
                    });
                Space::Tint {
                    inputs,
                    alternate: Rc::new(alternate),
                    function,
                }
            }
            name => match self.resource(resources, b"ColorSpace", name) {
                Some(found) if !matches!(found, Object::Name(n) if n == name) => {
                    self.space(resources, found, depth + 1)
                }
                _ => Space::Gray,
            },
        }
    }

    /// Show strings (with `TJ`'s spacing between them), moving the text matrix past them,
    /// and keep what they read as one run.
    fn show(
        &mut self,
        gs: &State,
        tm: &mut Matrix,
        parts: &[&Object],
        layer: Option<String>,
        resources: &[&'a Dictionary],
        depth: usize,
    ) {
        let t = &gs.text;
        let Some(font) = t.font.clone() else {
            return;
        };
        let to_page = |tm: &Matrix| {
            Matrix([1.0, 0.0, 0.0, 1.0, 0.0, t.rise])
                .then(tm)
                .then(&gs.ctm)
        };
        let start = to_page(tm).apply(DVec2::ZERO);
        // Its letters, to draw (modes 3 and 7 paint nothing; a clip may hide the run).
        let outline = self.outline_text
            && !self.in_glyph
            && !matches!(t.mode, 3 | 7)
            && (gs.fill_alpha > 0.0 || matches!(t.mode, 1 | 2 | 5 | 6))
            && gs.clips.iter().all(|c| c.contains(start));
        // From a letter's space (ems of the font size) to text space.
        let em = Matrix([t.size * t.scale, 0.0, 0.0, t.size, 0.0, t.rise]);
        let mut glyphs = Vec::new();
        let mut drawn = Vec::new();
        let mut text = String::new();
        let mut unreadable = false;
        for part in parts {
            match part {
                Object::String(bytes, _) => {
                    for code in font.codes(bytes) {
                        if outline {
                            let m = em.then(tm).then(&gs.ctm);
                            match font.type3 {
                                Some(fm) => drawn.extend(self.type3_glyph(
                                    &font,
                                    code,
                                    Matrix(fm).then(&m),
                                    gs,
                                    resources,
                                    depth,
                                )),
                                None => glyphs.push((code, m)),
                            }
                        }
                        let s = font.text(code);
                        if s.is_empty() && code != 0 {
                            unreadable = true;
                        }
                        text.push_str(&s);
                        let word = if !font.two_byte && code == 32 {
                            t.word_space
                        } else {
                            0.0
                        };
                        let advance =
                            (font.width(code) / 1000.0 * t.size + t.char_space + word) * t.scale;
                        *tm = Matrix([1.0, 0.0, 0.0, 1.0, advance, 0.0]).then(tm);
                    }
                }
                o => {
                    if let Some(n) = num(o) {
                        // Thousandths of the font size, back the way the text runs; a big
                        // step is a space.
                        let back = n / 1000.0 * t.size * t.scale;
                        if n < -250.0 && !text.ends_with(' ') && !text.is_empty() {
                            text.push(' ');
                        }
                        *tm = Matrix([1.0, 0.0, 0.0, 1.0, -back, 0.0]).then(tm);
                    }
                }
            }
        }
        if !glyphs.is_empty() || !drawn.is_empty() {
            self.glyph_runs.push(GlyphRun {
                font: font.clone(),
                glyphs,
                drawn,
                start,
                end: to_page(tm).apply(DVec2::ZERO),
                hidden: false,
            });
        }
        if unreadable && font.unreadable {
            self.skipped_text += 1;
            return;
        }
        if t.mode == 3 || text.trim().is_empty() || gs.fill_alpha <= 0.0 {
            return;
        }
        if gs.fill_alpha < 1.0 {
            self.transparency = true;
        }
        // Text that a clip hides (or that's off the page).
        if !gs.clips.iter().all(|c| c.contains(start)) {
            return;
        }
        let page = tm.then(&gs.ctm);
        let [a, b, c, d, _, _] = page.0;
        let size = t.size * (c * c + d * d).sqrt();
        self.texts.push(PdfText {
            text,
            start,
            end: to_page(tm).apply(DVec2::ZERO),
            size: size.abs(),
            angle: b.atan2(a),
            color: gs.fill,
            layer,
        });
    }

    /// Draw a Type 3 letter (its procedure, painted with `to_page` from its glyph space) and
    /// hand back what it painted, as polylines on the page: closed for fills, open for
    /// strokes. Its paint doesn't stay on the page as curves of its own.
    fn type3_glyph(
        &mut self,
        font: &Rc<Font>,
        code: u32,
        to_page: Matrix,
        gs: &State,
        resources: &[&'a Dictionary],
        depth: usize,
    ) -> Vec<Vec<DVec2>> {
        let Some(t3) = self.type3.get(&(Rc::as_ptr(font) as usize)) else {
            return Vec::new();
        };
        let Some(stream) = t3.procs.get(code as usize).copied().flatten() else {
            return Vec::new();
        };
        if depth >= MAX_DEPTH {
            return Vec::new();
        }
        let mut res: Vec<&'a Dictionary> = t3.resources.into_iter().collect();
        res.extend_from_slice(resources);
        let data = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let mut inner = gs.clone();
        inner.ctm = to_page;
        inner.text = TextState::default();
        let before = self.paths.len();
        self.in_glyph = true;
        // A letter that can't be drawn is left out.
        let _ = self.run(&data, inner, &res, &mut Vec::new(), depth + 1);
        self.in_glyph = false;
        let mut out = Vec::new();
        for path in self.paths.drain(before.min(self.paths.len())..) {
            let close = path.fill.is_some();
            for s in &path.subpaths {
                let mut points = vec![s.start];
                let mut from = s.start;
                for seg in &s.segs {
                    match *seg {
                        Seg::Line(p) => points.push(p),
                        Seg::Cubic(c1, c2, p) => flatten_cubic(from, c1, c2, p, &mut points),
                    }
                    from = seg.end();
                }
                if (close || s.closed) && points.len() > 2 && points.first() != points.last() {
                    points.push(s.start);
                }
                out.push(points);
            }
        }
        out
    }

    /// Note a layer the first time the page draws on it.
    fn enter_layer(&mut self, layer: &Option<String>) {
        if let Some(name) = layer
            && !self.layer_order.contains(name)
        {
            self.layer_order.push(name.clone());
        }
    }

    /// The name of the layer an optional-content tag stands for.
    fn layer_name(&self, resources: &[&'a Dictionary], props: &Object) -> Option<String> {
        let dict = match props {
            Object::Name(n) => self.resource(resources, b"Properties", n)?.as_dict().ok()?,
            Object::Dictionary(d) => d,
            _ => return None,
        };
        if let Ok(name) = dict.get(b"Name") {
            return text_string(self.doc.dereference(name).ok()?.1);
        }
        // A membership dictionary: its first group's name.
        let groups = self.doc.dereference(dict.get(b"OCGs").ok()?).ok()?.1;
        let first = match groups {
            Object::Array(a) => self.doc.dereference(a.first()?).ok()?.1,
            o => o,
        };
        let name = first.as_dict().ok()?.get(b"Name").ok()?;
        text_string(self.doc.dereference(name).ok()?.1)
    }

    fn run(
        &mut self,
        content: &[u8],
        state: State,
        resources: &[&'a Dictionary],
        layers: &mut Vec<Option<String>>,
        depth: usize,
    ) -> Result<(), String> {
        let content = lopdf::content::Content::decode(content)
            .map_err(|e| format!("the page's drawing couldn't be read ({e})"))?;
        let mut gs = state;
        let mut stack: Vec<State> = Vec::new();
        // The path being built, in page space.
        let mut subpaths: Vec<Subpath> = Vec::new();
        let mut current: Option<Subpath> = None;
        let mut at = DVec2::ZERO;
        let mut clip_next = false;
        // The text matrix and the start of its line.
        let mut tm = Matrix::IDENTITY;
        let mut tlm = Matrix::IDENTITY;
        for op in &content.operations {
            if self.segments >= MAX_SEGMENTS {
                self.cut_short = true;
                break;
            }
            let o = &op.operands;
            let point = |gs: &State, x: f64, y: f64| gs.ctm.apply(DVec2::new(x, y));
            match op.operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => gs = stack.pop().unwrap_or(gs),
                "cm" => {
                    if let Some(m) = nums::<6>(o) {
                        gs.ctm = Matrix(m).then(&gs.ctm);
                    }
                }
                "w" => {
                    if let Some([w]) = nums::<1>(o) {
                        gs.width = w.max(0.0);
                    }
                }
                "d" => {
                    gs.dashes = o
                        .first()
                        .and_then(|a| a.as_array().ok())
                        .map(|a| a.iter().filter_map(num).collect())
                        .unwrap_or_default();
                }
                "G" | "RG" | "K" | "g" | "rg" | "k" => {
                    let space = match op.operator.to_ascii_lowercase().as_str() {
                        "g" => Space::Gray,
                        "rg" => Space::Rgb,
                        _ => Space::Cmyk(self.cmyk.clone()),
                    };
                    let v: Vec<f64> = o.iter().filter_map(num).collect();
                    if let Some(c) = space.color(&v) {
                        if op.operator.starts_with(|c: char| c.is_ascii_uppercase()) {
                            (gs.stroke, gs.stroke_space) = (c, space);
                        } else {
                            (gs.fill, gs.fill_space) = (c, space);
                        }
                    }
                }
                "SC" | "SCN" | "sc" | "scn" => {
                    let v: Vec<f64> = o.iter().filter_map(num).collect();
                    if op.operator.starts_with('S') {
                        if let Some(c) = gs.stroke_space.color(&v) {
                            gs.stroke = c;
                        }
                    } else if let Some(c) = gs.fill_space.color(&v) {
                        gs.fill = c;
                    }
                }
                "CS" | "cs" => {
                    let Some(name) = o.first() else {
                        continue;
                    };
                    let space = self.space(resources, name, 0);
                    let initial = space.initial();
                    if op.operator == "CS" {
                        gs.stroke_space = space;
                        gs.stroke = initial.unwrap_or(gs.stroke);
                    } else {
                        gs.fill_space = space;
                        gs.fill = initial.unwrap_or(gs.fill);
                    }
                }
                "gs" => {
                    let ext = o
                        .first()
                        .and_then(|n| n.as_name().ok())
                        .and_then(|n| self.resource(resources, b"ExtGState", n))
                        .and_then(|e| e.as_dict().ok());
                    if let Some(ext) = ext {
                        let alpha = |k: &[u8]| ext.get(k).ok().and_then(num);
                        if let Some(a) = alpha(b"CA") {
                            gs.stroke_alpha = a.clamp(0.0, 1.0);
                        }
                        if let Some(a) = alpha(b"ca") {
                            gs.fill_alpha = a.clamp(0.0, 1.0);
                        }
                        if let Ok(bm) = ext.get(b"BM") {
                            gs.normal_blend = bm
                                .as_name()
                                .ok()
                                .is_none_or(|b| b == b"Normal" || b == b"Compatible");
                            self.blending |= !gs.normal_blend;
                        }
                        if let Ok(mask) = ext.get(b"SMask") {
                            gs.soft_mask = mask.as_name().ok() != Some(b"None".as_slice());
                            self.soft_masks |= gs.soft_mask;
                        }
                        if let Some(w) = ext.get(b"LW").ok().and_then(num) {
                            gs.width = w.max(0.0);
                        }
                        if let Some(d) = ext.get(b"D").ok().and_then(|d| d.as_array().ok()) {
                            gs.dashes = d
                                .first()
                                .and_then(|a| a.as_array().ok())
                                .map(|a| a.iter().filter_map(num).collect())
                                .unwrap_or_default();
                        }
                    }
                }
                "BDC" | "BMC" => {
                    let layer = (op.operator == "BDC"
                        && o.first().and_then(|t| t.as_name().ok()) == Some(b"OC".as_slice()))
                    .then(|| o.get(1).and_then(|p| self.layer_name(resources, p)))
                    .flatten();
                    self.enter_layer(&layer);
                    layers.push(layer);
                }
                "EMC" => {
                    layers.pop();
                }
                "m" => {
                    if let Some([x, y]) = nums::<2>(o) {
                        if let Some(s) = current.take()
                            && !s.segs.is_empty()
                        {
                            subpaths.push(s);
                        }
                        at = point(&gs, x, y);
                        current = Some(Subpath {
                            start: at,
                            segs: Vec::new(),
                            closed: false,
                        });
                    }
                }
                "l" => {
                    if let Some([x, y]) = nums::<2>(o) {
                        let s = current.get_or_insert_with(|| Subpath {
                            start: at,
                            segs: Vec::new(),
                            closed: false,
                        });
                        at = point(&gs, x, y);
                        s.segs.push(Seg::Line(at));
                    }
                }
                "c" | "v" | "y" => {
                    let Some(s) = &mut current else {
                        continue;
                    };
                    let pts: Option<[DVec2; 3]> = match op.operator.as_str() {
                        "c" => nums::<6>(o).map(|[a, b, c, d, e, f]| {
                            [point(&gs, a, b), point(&gs, c, d), point(&gs, e, f)]
                        }),
                        "v" => nums::<4>(o)
                            .map(|[c, d, e, f]| [at, point(&gs, c, d), point(&gs, e, f)]),
                        _ => nums::<4>(o).map(|[a, b, e, f]| {
                            let p3 = point(&gs, e, f);
                            [point(&gs, a, b), p3, p3]
                        }),
                    };
                    if let Some([p1, p2, p3]) = pts {
                        s.segs.push(Seg::Cubic(p1, p2, p3));
                        at = p3;
                    }
                }
                "re" => {
                    if let Some([x, y, w, h]) = nums::<4>(o) {
                        if let Some(s) = current.take()
                            && !s.segs.is_empty()
                        {
                            subpaths.push(s);
                        }
                        let start = point(&gs, x, y);
                        subpaths.push(Subpath {
                            start,
                            segs: vec![
                                Seg::Line(point(&gs, x + w, y)),
                                Seg::Line(point(&gs, x + w, y + h)),
                                Seg::Line(point(&gs, x, y + h)),
                            ],
                            closed: true,
                        });
                        at = start;
                    }
                }
                "h" => {
                    if let Some(mut s) = current.take() {
                        s.closed = true;
                        at = s.start;
                        if !s.segs.is_empty() {
                            subpaths.push(s);
                        }
                    }
                }
                "W" | "W*" => clip_next = true,
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    let operator = op.operator.as_str();
                    if let Some(mut s) = current.take() {
                        if matches!(operator, "s" | "b" | "b*") {
                            s.closed = true;
                        }
                        if !s.segs.is_empty() {
                            subpaths.push(s);
                        }
                    }
                    let paths = std::mem::take(&mut subpaths);
                    // A path ending in W clips what is painted after it (it is painted
                    // through the clips before it first, if at all).
                    let clips_after = std::mem::take(&mut clip_next);
                    // (What is painted wholly see-through isn't painted at all.)
                    let strokes = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*")
                        && gs.stroke_alpha > 0.0;
                    let fills = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*")
                        && gs.fill_alpha > 0.0;
                    if strokes && gs.stroke_alpha < 1.0 {
                        self.transparency = true;
                    }
                    if !paths.is_empty() && (strokes || fills) {
                        self.segments += paths.iter().map(|s| s.segs.len()).sum::<usize>();
                        let (stroke_parts, fill_parts, clipped) = clip_paint(&paths, &gs.clips);
                        self.clipping.add(clipped);
                        let scale = gs.scale();
                        let stroke = strokes.then(|| Stroke {
                            color: gs.stroke,
                            width: gs.width * scale,
                            dashes: gs.dashes.iter().map(|d| d * scale).collect(),
                        });
                        let fill = fills.then_some(gs.fill);
                        // An opaque fill hides what was painted before it, under it.
                        if fills
                            && !self.in_glyph
                            && gs.fill_alpha >= 1.0
                            && gs.normal_blend
                            && !gs.soft_mask
                            && !fill_parts.is_empty()
                        {
                            self.cover(&fill_parts);
                        }
                        let layer = layers.iter().rev().find_map(Clone::clone);
                        if stroke_parts == fill_parts {
                            if !stroke_parts.is_empty() {
                                self.paths.push(PdfPath::new(
                                    stroke_parts,
                                    stroke,
                                    fill,
                                    gs.fill_alpha,
                                    layer,
                                ));
                            }
                        } else {
                            // Clipped differently: the outline and the fill apart.
                            if stroke.is_some() && !stroke_parts.is_empty() {
                                self.paths.push(PdfPath::new(
                                    stroke_parts,
                                    stroke,
                                    None,
                                    1.0,
                                    layer.clone(),
                                ));
                            }
                            if fill.is_some() && !fill_parts.is_empty() {
                                self.paths.push(PdfPath::new(
                                    fill_parts,
                                    None,
                                    fill,
                                    gs.fill_alpha,
                                    layer,
                                ));
                            }
                        }
                    }
                    if clips_after && let Some(clip) = Clip::from_subpaths(&paths) {
                        gs.clips.push(Rc::new(clip));
                    }
                }
                "BT" => {
                    tm = Matrix::IDENTITY;
                    tlm = Matrix::IDENTITY;
                }
                "Tf" => {
                    if let (Some(name), Some(size)) = (
                        o.first().and_then(|n| n.as_name().ok()),
                        o.get(1).and_then(num),
                    ) {
                        gs.text.size = size;
                        gs.text.font = self.font(resources, name);
                    }
                }
                "Tc" => gs.text.char_space = o.first().and_then(num).unwrap_or(0.0),
                "Tw" => gs.text.word_space = o.first().and_then(num).unwrap_or(0.0),
                "Tz" => gs.text.scale = o.first().and_then(num).unwrap_or(100.0) / 100.0,
                "TL" => gs.text.leading = o.first().and_then(num).unwrap_or(0.0),
                "Ts" => gs.text.rise = o.first().and_then(num).unwrap_or(0.0),
                "Tr" => {
                    gs.text.mode = o.first().and_then(num).map_or(0, |m| m as i64);
                }
                "Td" | "TD" => {
                    if let Some([x, y]) = nums::<2>(o) {
                        if op.operator == "TD" {
                            gs.text.leading = -y;
                        }
                        tlm = Matrix([1.0, 0.0, 0.0, 1.0, x, y]).then(&tlm);
                        tm = tlm;
                    }
                }
                "Tm" => {
                    if let Some(m) = nums::<6>(o) {
                        tlm = Matrix(m);
                        tm = tlm;
                    }
                }
                "T*" => {
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -gs.text.leading]).then(&tlm);
                    tm = tlm;
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    let operator = op.operator.as_str();
                    if operator == "\"" {
                        gs.text.word_space = o.first().and_then(num).unwrap_or(0.0);
                        gs.text.char_space = o.get(1).and_then(num).unwrap_or(0.0);
                    }
                    if operator == "'" || operator == "\"" {
                        tlm = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -gs.text.leading]).then(&tlm);
                        tm = tlm;
                    }
                    let parts: Vec<&Object> = match o.last() {
                        Some(Object::Array(a)) if operator == "TJ" => a.iter().collect(),
                        Some(s) => vec![s],
                        None => continue,
                    };
                    let layer = layers.iter().rev().find_map(Clone::clone);
                    self.show(&gs, &mut tm, &parts, layer, resources, depth);
                }
                "BI" => match o.first() {
                    Some(Object::Stream(inline)) => {
                        let inline = crate::pdf_image::expand_inline(inline);
                        self.picture(&inline, resources, &gs);
                    }
                    _ => *self.unread.entry(crate::pdf_image::UNREADABLE).or_insert(0) += 1,
                },
                "sh" => self.skipped_shadings += 1,
                "Do" => {
                    let Some(name) = o.first().and_then(|n| n.as_name().ok()) else {
                        continue;
                    };
                    let Some(obj) = self.resource(resources, b"XObject", name) else {
                        continue;
                    };
                    let Ok(stream) = obj.as_stream() else {
                        continue;
                    };
                    let subtype = stream.dict.get(b"Subtype").and_then(Object::as_name).ok();
                    if subtype == Some(b"Image".as_slice()) {
                        self.picture(stream, resources, &gs);
                        continue;
                    }
                    if subtype != Some(b"Form".as_slice()) || depth >= MAX_DEPTH {
                        continue;
                    }
                    let matrix = stream
                        .dict
                        .get(b"Matrix")
                        .and_then(Object::as_array)
                        .ok()
                        .and_then(|a| {
                            let v: Vec<f64> = a.iter().filter_map(num).collect();
                            (v.len() == 6).then(|| Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]))
                        })
                        .unwrap_or(Matrix::IDENTITY);
                    let own = stream
                        .dict
                        .get_deref(b"Resources", self.doc)
                        .and_then(Object::as_dict)
                        .ok();
                    let mut res: Vec<&'a Dictionary> = own.into_iter().collect();
                    res.extend_from_slice(resources);
                    let data = stream
                        .decompressed_content()
                        .unwrap_or_else(|_| stream.content.clone());
                    // A form on a layer of its own.
                    let layer = stream
                        .dict
                        .get(b"OC")
                        .ok()
                        .and_then(|oc| self.doc.dereference(oc).ok())
                        .and_then(|(_, oc)| self.layer_name(resources, oc));
                    let mut inner = gs.clone();
                    inner.ctm = matrix.then(&gs.ctm);
                    self.enter_layer(&layer);
                    layers.push(layer);
                    let result = self.run(&data, inner, &res, layers, depth + 1);
                    layers.pop();
                    result?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Runs of text that continue one another as single lines: the same size and angle, each
/// starting where the last ended (as PDF writers often place words or letters one by one),
/// or touching it in a smaller or raised size (the stacked fractions and superscripts that
/// CADtools and Illustrator draw dimensions with: `28 1/2"` comes as `28 `, a raised `1`,
/// `/`, `2` and `"`). A line keeps its first run's size.
fn merge_runs(runs: Vec<PdfText>) -> Vec<PdfText> {
    let mut out: Vec<PdfText> = Vec::new();
    for run in runs {
        if let Some(last) = out.last_mut() {
            let dir = DVec2::from_angle(last.angle);
            let gap = run.start - last.end;
            let (along, across) = (gap.dot(dir), gap.perp_dot(dir).abs());
            let same_size = (run.size / last.size - 1.0).abs() < 0.05;
            let touching = along.abs() < 0.05 * last.size
                && across < 0.5 * last.size.max(run.size)
                && (0.4..2.5).contains(&(run.size / last.size));
            let continues = last.layer == run.layer
                && (run.angle - last.angle).abs() < 0.02
                && (touching
                    || same_size
                        && across < 0.2 * last.size
                        && (-0.3 * last.size..1.2 * last.size).contains(&along));
            if continues {
                if along > 0.2 * last.size
                    && !last.text.ends_with(' ')
                    && !run.text.starts_with(' ')
                {
                    last.text.push(' ');
                }
                last.text.push_str(&run.text);
                last.end = run.end;
                continue;
            }
        }
        out.push(run);
    }
    for t in &mut out {
        t.text = t.text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    out.retain(|t| !t.text.is_empty());
    out
}

/// How each optional-content group (layer) starts out: hidden (the default view's `OFF`),
/// locked, or not printed (its print usage).
fn layer_states(doc: &Document) -> BTreeMap<String, LayerFlags> {
    let mut out = BTreeMap::new();
    let Some(props) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get_deref(b"OCProperties", doc).ok())
        .and_then(|o| o.as_dict().ok())
    else {
        return out;
    };
    fn deref<'d>(doc: &'d Document, o: &'d Object) -> Option<&'d Object> {
        doc.dereference(o).ok().map(|(_, o)| o)
    }
    let name_of = |o: &Object| -> Option<String> {
        let dict = deref(doc, o)?.as_dict().ok()?;
        text_string(deref(doc, dict.get(b"Name").ok()?)?)
    };
    let refs = |o: Option<&Object>| -> Vec<lopdf::ObjectId> {
        o.and_then(|x| deref(doc, x))
            .and_then(|a| a.as_array().ok())
            .map(|a| a.iter().filter_map(|r| r.as_reference().ok()).collect())
            .unwrap_or_default()
    };
    let config = props
        .get(b"D")
        .ok()
        .and_then(|x| deref(doc, x))
        .and_then(|d| d.as_dict().ok());
    let off = refs(config.and_then(|d| d.get(b"OFF").ok()));
    let locked = refs(config.and_then(|d| d.get(b"Locked").ok()));
    let base_off = config
        .and_then(|d| d.get(b"BaseState").ok())
        .and_then(|b| b.as_name().ok())
        == Some(b"OFF".as_slice());
    let on = refs(config.and_then(|d| d.get(b"ON").ok()));
    for id in refs(props.get(b"OCGs").ok()) {
        let Some(name) = name_of(&Object::Reference(id)) else {
            continue;
        };
        let printable = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get_deref(b"Usage", doc).ok())
            .and_then(|u| u.as_dict().ok())
            .and_then(|u| u.get_deref(b"Print", doc).ok())
            .and_then(|p| p.as_dict().ok())
            .and_then(|p| p.get(b"PrintState").ok())
            .and_then(|s| s.as_name().ok())
            != Some(b"OFF".as_slice());
        out.insert(
            name,
            LayerFlags {
                visible: if base_off {
                    on.contains(&id)
                } else {
                    !off.contains(&id)
                },
                locked: locked.contains(&id),
                printable,
            },
        );
    }
    out
}

/// Keep a run of points as an outline: drop repeated points; two points make a line.
fn add_prim(prims: &mut Vec<RefPrim>, path: Vec<DVec2>) {
    let mut points: Vec<DVec2> = Vec::with_capacity(path.len());
    for p in path {
        if !p.is_finite() {
            return;
        }
        // A ten-thousandth of a point is far below anything drawn; it keeps files small.
        let p = (p * 1e4).round() / 1e4;
        if points.last().is_none_or(|q| q.distance(p) > 1e-6) {
            points.push(p);
        }
    }
    match points.len() {
        0 | 1 => {}
        2 => prims.push(RefPrim::Line(points[0], points[1])),
        _ => prims.push(RefPrim::Polyline(points)),
    }
}

/// Append a cubic Bézier from `p0` (already in `out`) as line segments, finely enough that
/// it stays within about a hundredth of a point of the curve.
fn flatten_cubic(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, out: &mut Vec<DVec2>) {
    let span = p0.distance(p1) + p1.distance(p2) + p2.distance(p3);
    let n = (span.sqrt() * 2.0).ceil().clamp(2.0, 64.0) as usize;
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push(
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::underlays::MM_PER_POINT;
    use drift_draft::page::{Drawing, Pen, Prim};

    fn page_with_curves() -> Vec<u8> {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.push(
            "0",
            Pen::new(0.35),
            Prim::Line(DVec2::new(10.0, 10.0), DVec2::new(110.0, 10.0)),
        );
        d.push(
            "0",
            Pen::new(0.35),
            Prim::Circle {
                center: DVec2::new(150.0, 100.0),
                radius: 20.0,
            },
        );
        crate::pdf::write(&[d.clone(), d], "Test")
    }

    #[test]
    fn reads_lines_and_curves_at_paper_size() {
        let bytes = page_with_curves();
        assert_eq!(page_count(&bytes).unwrap(), 2);
        let page = read_page(&bytes, 1).unwrap();
        assert_eq!(page.pages, 2);
        // A4 landscape in points.
        assert!(
            (page.size.x * MM_PER_POINT - 297.0).abs() < 0.5,
            "{:?}",
            page.size
        );
        let mm = |p: DVec2| p * MM_PER_POINT;
        // The line, at its place on the paper.
        let line = page.prims.iter().find_map(|p| match p {
            RefPrim::Line(a, b) => Some((mm(*a), mm(*b))),
            RefPrim::Polyline(pts) if pts.len() == 2 => Some((mm(pts[0]), mm(pts[1]))),
            _ => None,
        });
        let (a, b) = line.expect("the line");
        assert!(a.distance(DVec2::new(10.0, 10.0)) < 0.01, "{a:?}");
        assert!(b.distance(DVec2::new(110.0, 10.0)) < 0.01, "{b:?}");
        // The circle, flattened: every point is on it.
        let circle = page
            .prims
            .iter()
            .find_map(|p| match p {
                RefPrim::Polyline(pts) if pts.len() > 8 => Some(pts.clone()),
                _ => None,
            })
            .expect("the circle");
        for p in circle {
            let r = mm(p).distance(DVec2::new(150.0, 100.0));
            assert!((r - 20.0).abs() < 0.05, "{r}");
        }
        assert!(read_page(&bytes, 3).is_err());
        assert!(read_page(b"not a pdf", 1).is_err());
    }

    #[test]
    fn a_page_reads_back_as_artwork_to_edit() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        let mut red = Pen::new(0.5);
        red.color = Rgb([255, 0, 0]);
        red.line_type = drift_core::LineType::Dashed;
        d.push(
            "0",
            red,
            Prim::Line(DVec2::new(10.0, 10.0), DVec2::new(110.0, 10.0)),
        );
        d.push(
            "0",
            Pen::new(0.25),
            Prim::Circle {
                center: DVec2::new(150.0, 100.0),
                radius: 20.0,
            },
        );
        let square = vec![
            DVec2::new(200.0, 50.0),
            DVec2::new(240.0, 50.0),
            DVec2::new(240.0, 90.0),
            DVec2::new(200.0, 90.0),
        ];
        let mut blue = Pen::new(0.0);
        blue.color = Rgb([0, 0, 255]);
        d.push(
            "0",
            blue,
            Prim::Fill {
                rings: vec![square.clone()],
                triangles: vec![
                    [square[0], square[1], square[2]],
                    [square[0], square[2], square[3]],
                ],
                mask: false,
            },
        );
        let bytes = crate::pdf::write(&[d], "Test");
        let art = read_artwork(&bytes, 1, "plan").unwrap();
        assert!((art.size.x - 297.0).abs() < 0.5, "{:?}", art.size);
        assert_eq!(art.layers, ["plan"]);
        // The red dashed line: its colour, width and dashes kept.
        let line = art
            .paths
            .iter()
            .find(|p| {
                p.stroke
                    .as_ref()
                    .is_some_and(|s| s.color == Rgb([255, 0, 0]))
            })
            .expect("the line");
        let stroke = line.stroke.as_ref().unwrap();
        assert!((stroke.width - 0.5).abs() < 1e-3, "{}", stroke.width);
        assert!(!stroke.dashes.is_empty());
        // The circle comes back a circle (the PDF has it as four curves).
        let circle = art
            .paths
            .iter()
            .find(|p| {
                p.subpaths
                    .iter()
                    .any(|s| s.segs.len() >= 4 && matches!(s.segs[0], Seg::Cubic(..)))
            })
            .expect("the circle");
        let shapes = crate::vector::shapes_of(&circle.subpaths[0]);
        assert!(
            matches!(shapes[..], [drift_core::Shape::Circle { center, radius }]
                if center.distance(DVec2::new(150.0, 100.0)) < 0.01 && (radius - 20.0).abs() < 0.01),
            "{shapes:?}"
        );
        // The blue square, filled.
        assert!(art.paths.iter().any(|p| p.fill == Some(Rgb([0, 0, 255]))));
    }

    /// A page as Illustrator writes one: text in a composite font with a character map,
    /// placed a letter at a time, on a layer that starts hidden, locked and not printed. Or,
    /// with `stand_in`, the page of a file saved without its PDF copy.
    fn illustrator_page(stand_in: bool) -> Vec<u8> {
        use lopdf::{Stream, dictionary};
        let mut doc = Document::with_version("1.6");
        let pages = doc.new_object_id();
        let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
            1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
            2 beginbfchar <0001> <0041> <0002> <0042> endbfchar\n\
            endcmap end end";
        let to_unicode = doc.add_object(Stream::new(dictionary! {}, cmap.to_vec()));
        let cid = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "MyriadPro", "DW" => 600,
        });
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "MyriadPro",
            "Encoding" => "Identity-H", "DescendantFonts" => vec![cid.into()],
            "ToUnicode" => to_unicode,
        });
        let helvetica = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let ocg = doc.add_object(dictionary! {
            "Type" => "OCG", "Name" => Object::string_literal("Notes"),
            "Usage" => dictionary! { "Print" => dictionary! { "PrintState" => "OFF" } },
        });
        let content: &[u8] = if stand_in {
            b"BT /F2 12 Tf 72 700 Td (This is an Adobe\xae Illustrator\xae File that was saved \
              without PDF Content.) Tj ET"
        } else {
            b"/OC /L1 BDC 0 0 1 rg BT /F1 10 Tf 1 0 0 1 100 700 Tm <0001> Tj 6 0 Td <0002> Tj \
              ET 1 w 10 10 m 50 10 l S EMC"
        };
        let contents = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => contents,
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font, "F2" => helvetica },
                "Properties" => dictionary! { "L1" => ocg },
            },
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages,
            "OCProperties" => dictionary! {
                "OCGs" => vec![ocg.into()],
                "D" => dictionary! { "OFF" => vec![ocg.into()], "Locked" => vec![ocg.into()] },
            },
        });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn text_and_layer_states_come_in_from_an_illustrator_page() {
        let art = read_artwork(&illustrator_page(false), 1, "plan").unwrap();
        let mm = drift_core::underlays::MM_PER_POINT;
        // Two letters placed one after the other are one line.
        assert_eq!(art.texts.len(), 1, "{:?}", art.texts);
        let t = &art.texts[0];
        assert_eq!(t.text, "AB");
        assert_eq!(t.layer, "Notes");
        assert!(
            t.at.distance(DVec2::new(100.0, 700.0) * mm) < 1e-9,
            "{}",
            t.at
        );
        assert!((t.size - 10.0 * mm).abs() < 1e-9);
        assert_eq!(t.color, Some(Rgb([0, 0, 255])));
        // The layer starts hidden, locked and not printed, as the file has it.
        assert_eq!(art.layers, ["Notes"]);
        assert_eq!(
            art.layer_flags["Notes"],
            LayerFlags {
                visible: false,
                locked: true,
                printable: false
            }
        );
        assert!(art.notes.iter().any(|n| n.contains("hidden")));
        // Into a sketch: the layer keeps them.
        let import = crate::vector::to_import(&art, &Default::default());
        assert!(
            !import.layers[0].visible && import.layers[0].locked && !import.layers[0].printable
        );
        // Saved without its PDF copy: said so.
        let err = read_artwork(&illustrator_page(true), 1, "plan").unwrap_err();
        assert!(err.contains("Create PDF Compatible File"), "{err}");
    }

    /// Layers come in the order the page draws them (the last on top), not by name; a
    /// ligature its encoding names `f_i` reads as "fi".
    #[test]
    fn layers_keep_the_files_order_and_ligatures_read_as_letters() {
        use lopdf::{Stream, dictionary};
        let mut doc = Document::with_version("1.6");
        let pages = doc.new_object_id();
        let encoding = doc.add_object(dictionary! {
            "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![31.into(), Object::Name(b"f_i".to_vec())],
        });
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ABCDEF+MyriadPro-Regular",
            "Encoding" => encoding,
        });
        let ocg = |doc: &mut Document, name: &str| {
            doc.add_object(dictionary! {
                "Type" => "OCG", "Name" => Object::string_literal(name),
            })
        };
        let (walls, notes, marks) = (
            ocg(&mut doc, "Walls"),
            ocg(&mut doc, "Notes"),
            ocg(&mut doc, "Annotations"),
        );
        let content = b"/OC /W BDC 1 w 10 10 m 50 10 l S EMC \
            /OC /N BDC BT /F1 10 Tf 100 700 Td (\x1frst floor) Tj ET EMC \
            /OC /A BDC 1 w 10 20 m 50 20 l S EMC";
        let contents = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => contents,
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "Properties" => dictionary! { "W" => walls, "N" => notes, "A" => marks },
            },
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages,
            "OCProperties" => dictionary! {
                "OCGs" => vec![marks.into(), notes.into(), walls.into()],
                "D" => dictionary! { "Order" => vec![marks.into(), notes.into(), walls.into()] },
            },
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let art = read_artwork(&bytes, 1, "plan").unwrap();
        assert_eq!(art.layers, ["Walls", "Notes", "Annotations"]);
        assert_eq!(art.texts.len(), 1, "{:?}", art.texts);
        assert_eq!(art.texts[0].text, "first floor");
        assert_eq!(art.texts[0].layer, "Notes");
    }

    #[test]
    fn drift_text_reads_back_from_its_pdf() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.text(
            "0",
            Pen::new(0.25),
            DVec2::new(20.0, 30.0),
            3.5,
            drift_draft::page::Align::Left,
            "3'-6\" BEDROOM",
        );
        let art = read_artwork(&crate::pdf::write(&[d], "Test"), 1, "plan").unwrap();
        let t = art.texts.first().expect("the text");
        assert_eq!(t.text, "3'-6\" BEDROOM");
        assert!(t.at.distance(DVec2::new(20.0, 30.0)) < 0.05, "{}", t.at);
        // A 3.5 mm cap height.
        let cap = t.size * drift_draft::page::CAP_HEIGHT;
        assert!((cap - 3.5).abs() < 0.05, "{cap}");
    }

    #[test]
    fn a_pdf_page_is_seen_through_its_clips_and_its_edges() {
        use lopdf::{Stream, dictionary};
        let mut doc = Document::with_version("1.6");
        let pages = doc.new_object_id();
        // A clip to a 100-point square, a line across it; then, unclipped, a line off the
        // page's right edge and one wholly beyond it.
        let content = b"q 0 0 100 100 re W n 1 w 50 50 m 300 50 l S Q \
                        500 300 m 700 300 l S 800 300 m 900 300 l S"
            .to_vec();
        let contents = doc.add_object(Stream::new(dictionary! {}, content));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => contents,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let art = read_artwork(&bytes, 1, "plan").unwrap();
        let mm = drift_core::underlays::MM_PER_POINT;
        let ends: Vec<(DVec2, DVec2)> = art
            .paths
            .iter()
            .map(|p| {
                let s = &p.subpaths[0];
                (s.start / mm, s.segs.last().unwrap().end() / mm)
            })
            .collect();
        assert_eq!(ends.len(), 2, "{ends:?}");
        // Inside the clip: 50 to 100.
        assert!(ends[0].0.distance(DVec2::new(50.0, 50.0)) < 1e-6);
        assert!(
            ends[0].1.distance(DVec2::new(100.0, 50.0)) < 1e-6,
            "{:?}",
            ends[0]
        );
        // To the page's edge.
        assert!((ends[1].1.x - 612.0).abs() < 1e-6, "{:?}", ends[1]);
        assert_eq!(art.clipping.trimmed, 2);
        assert_eq!(art.clipping.hidden, 1);
    }

    /// A one-page PDF (612 × 792 points) drawing `content` with `resources`.
    fn one_page(content: &[u8], resources: lopdf::Dictionary) -> Vec<u8> {
        one_page_with(content, resources, |_, _| {})
    }

    /// The same, letting `extra` add objects (it gets the document and the resources).
    fn one_page_with(
        content: &[u8],
        mut resources: lopdf::Dictionary,
        extra: impl FnOnce(&mut Document, &mut lopdf::Dictionary),
    ) -> Vec<u8> {
        use lopdf::{Stream, dictionary};
        let mut doc = Document::with_version("1.6");
        let pages = doc.new_object_id();
        extra(&mut doc, &mut resources);
        let contents = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => contents,
            "Resources" => resources,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    /// The fill colours of a page's paths, in the order they are painted.
    fn fills(bytes: &[u8]) -> Vec<Rgb> {
        let art = read_artwork(bytes, 1, "plan").unwrap();
        art.paths.iter().filter_map(|p| p.fill).collect()
    }

    #[test]
    fn cmyk_fills_come_in_as_they_look() {
        use lopdf::{Stream, dictionary};
        // A light grey in plain CMYK: as SWOP shows it, a neutral grey (the naive formula
        // made it a pale cyan, #E8EEED); paper white stays white, and black is near black.
        let plain = one_page(
            b"0.089 0.065 0.069 0 k 0 0 10 10 re f \
              0 0 0 0 k 20 0 10 10 re f 0 0 0 1 k 40 0 10 10 re f",
            dictionary! {},
        );
        let [grey, white, black] = fills(&plain)[..] else {
            panic!("{:?}", fills(&plain));
        };
        let Rgb([r, g, b]) = grey;
        assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "{grey:?}");
        assert!((222..236).contains(&r), "{grey:?}");
        assert_eq!(white, Rgb([255, 255, 255]));
        assert!(black.0.iter().all(|c| *c < 60), "{black:?}");

        // An ICC-based CMYK space is shown through its profile (here a toy one in which
        // one ink is a mid grey); `cs` starts it at full black ink (here grey too, the
        // profile's black being all four), and `scn` sets its colour.
        let icc = one_page_with(
            b"/CS0 cs 0 0 10 10 re f 1 0 0 0 scn 20 0 10 10 re f",
            dictionary! {},
            |doc, res| {
                let profile = doc.add_object(Stream::new(
                    dictionary! { "N" => 4 },
                    crate::icc::tests::tiny_profile(),
                ));
                res.set(
                    "ColorSpace",
                    dictionary! { "CS0" => vec![Object::Name(b"ICCBased".to_vec()), profile.into()] },
                );
            },
        );
        let [initial, one_ink] = fills(&icc)[..] else {
            panic!("{:?}", fills(&icc));
        };
        let profile = crate::icc::CmykProfile::parse(&crate::icc::tests::tiny_profile()).unwrap();
        assert_eq!(initial, profile.to_rgb([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(one_ink, profile.to_rgb([1.0, 0.0, 0.0, 0.0]));
        assert_eq!(one_ink, Rgb([one_ink.0[0]; 3]), "neutral");

        // A spot colour (a separation with an exponential tint into RGB), an indexed colour,
        // and a pattern (not read: the colour before stays).
        let spot = one_page(
            b"/Spot cs 0.5 scn 0 0 10 10 re f /Idx cs 1 scn 20 0 10 10 re f \
              /Pattern cs /P0 scn 40 0 10 10 re f",
            dictionary! {
                "ColorSpace" => dictionary! {
                    "Spot" => vec![
                        Object::Name(b"Separation".to_vec()),
                        Object::Name(b"Brick".to_vec()),
                        Object::Name(b"DeviceRGB".to_vec()),
                        Object::Dictionary(dictionary! {
                            "FunctionType" => 2,
                            "Domain" => vec![0.into(), 1.into()],
                            "C0" => vec![1.into(), 1.into(), 1.into()],
                            "C1" => vec![0.8.into(), 0.2.into(), 0.0.into()],
                            "N" => 1,
                        }),
                    ],
                    "Idx" => vec![
                        Object::Name(b"Indexed".to_vec()),
                        Object::Name(b"DeviceRGB".to_vec()),
                        1.into(),
                        Object::String(vec![0, 0, 0, 0, 128, 255], lopdf::StringFormat::Hexadecimal),
                    ],
                },
            },
        );
        let [half_spot, indexed, pattern] = fills(&spot)[..] else {
            panic!("{:?}", fills(&spot));
        };
        assert_eq!(half_spot, Rgb([230, 153, 128]));
        assert_eq!(indexed, Rgb([0, 128, 255]));
        assert_eq!(pattern, indexed);
    }

    #[test]
    fn imported_fills_keep_the_files_opacity() {
        use lopdf::dictionary;
        // An opaque fill (no graphics state says otherwise), a half see-through one, and a
        // see-through stroke.
        let bytes = one_page(
            b"1 0 0 rg 0 0 10 10 re f /Half gs 0 0 1 rg 20 0 10 10 re f \
              /Faint gs 0 0 m 50 50 l S",
            dictionary! {
                "ExtGState" => dictionary! {
                    "Half" => dictionary! { "Type" => "ExtGState", "ca" => 0.5 },
                    "Faint" => dictionary! { "Type" => "ExtGState", "CA" => 0.25 },
                },
            },
        );
        let art = read_artwork(&bytes, 1, "plan").unwrap();
        let opacities: Vec<f64> = art
            .paths
            .iter()
            .filter(|p| p.fill.is_some())
            .map(|p| p.fill_opacity)
            .collect();
        assert_eq!(opacities, [1.0, 0.5]);
        // Only the stroke's transparency is lost, and said so.
        assert!(
            art.notes.iter().any(|n| n.contains("lines and text")),
            "{:?}",
            art.notes
        );
        // Into a sketch: the fills keep it (not Drift's default for new fills).
        let import = crate::vector::to_import(&art, &Default::default());
        let mut doc = drift_core::Document::new();
        doc.begin();
        crate::dxf::add_to_document(&mut doc, &import);
        doc.commit("Import");
        let mut got: Vec<(Option<Rgb>, f64)> =
            doc.fills().map(|(_, f)| (f.color, f.opacity)).collect();
        got.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(
            got,
            [(Some(Rgb([0, 0, 255])), 0.5), (Some(Rgb([255, 0, 0])), 1.0)]
        );
    }

    #[test]
    fn images_come_in_as_pictures_where_the_page_has_them() {
        use lopdf::{Stream, dictionary};
        let zlib = |b: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(b, 6);
        // An RGB image over the page's lower-left quarter; a 1-bit grey one (compressed)
        // with a soft mask; an indexed one; a stencil painted half see-through in red; an
        // inline one; and a JPEG 2000 one, which can't be read.
        let bytes = one_page_with(
            b"q 306 0 0 396 0 0 cm /Rgb Do Q \
              q 80 0 0 10 400 700 cm /Bits Do Q \
              q 20 0 0 10 400 600 cm /Idx Do Q \
              q 1 0 0 rg /Half gs 10 0 0 10 400 500 cm /Stencil Do Q \
              q 5 0 0 5 400 400 cm BI /W 1 /H 1 /CS /RGB /BPC 8 ID \x10\x20\x30 EI Q \
              q 10 0 0 10 400 300 cm /Jpx Do Q \
              1 w 10 10 m 100 10 l S",
            dictionary! {
                "ExtGState" => dictionary! {
                    "Half" => dictionary! { "Type" => "ExtGState", "ca" => 0.5 },
                },
            },
            |doc, res| {
                let image = |doc: &mut Document, dict: lopdf::Dictionary, data: Vec<u8>| {
                    let mut d = dictionary! { "Type" => "XObject", "Subtype" => "Image" };
                    d.extend(&dict);
                    doc.add_object(Stream::new(d, data))
                };
                let rgb = image(
                    doc,
                    dictionary! { "Width" => 2, "Height" => 2, "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8 },
                    vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
                );
                let mask = image(
                    doc,
                    dictionary! { "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceGray",
                    "BitsPerComponent" => 8 },
                    vec![0, 255],
                );
                let bits = image(
                    doc,
                    dictionary! { "Width" => 8, "Height" => 1, "ColorSpace" => "DeviceGray",
                    "BitsPerComponent" => 1, "Filter" => "FlateDecode", "SMask" => mask },
                    zlib(&[0b1111_0000]),
                );
                let idx = image(
                    doc,
                    dictionary! { "Width" => 2, "Height" => 1, "BitsPerComponent" => 8,
                    "ColorSpace" => vec![
                        Object::Name(b"Indexed".to_vec()),
                        Object::Name(b"DeviceRGB".to_vec()),
                        1.into(),
                        Object::String(vec![10, 20, 30, 200, 210, 220],
                            lopdf::StringFormat::Hexadecimal),
                    ] },
                    vec![1, 0],
                );
                let stencil = image(
                    doc,
                    dictionary! { "Width" => 2, "Height" => 1, "ImageMask" => true,
                    "BitsPerComponent" => 1 },
                    vec![0b0100_0000],
                );
                let jpx = image(
                    doc,
                    dictionary! { "Width" => 2, "Height" => 2, "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8, "Filter" => "JPXDecode" },
                    vec![0; 16],
                );
                res.set(
                    "XObject",
                    dictionary! { "Rgb" => rgb, "Bits" => bits, "Idx" => idx,
                    "Stencil" => stencil, "Jpx" => jpx },
                );
            },
        );
        let art = read_artwork(&bytes, 1, "plan").unwrap();
        let mm = MM_PER_POINT;
        assert_eq!(art.paths.len(), 1);
        assert_eq!(art.images.len(), 5, "{:?}", art.skipped);
        // Placed by its transform: its top-left corner, its top edge and its left edge.
        let rgb = &art.images[0];
        assert_eq!((rgb.width, rgb.height), (2, 2));
        assert!(rgb.at.distance(DVec2::new(0.0, 396.0) * mm) < 1e-9);
        assert!(rgb.across.distance(DVec2::new(306.0, 0.0) * mm) < 1e-9);
        assert!(rgb.down.distance(DVec2::new(0.0, -396.0) * mm) < 1e-9);
        assert_eq!(
            *rgb.rgba,
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
        // Four white pixels then four black, the left half masked out.
        let px = |i: &drift_core::UnderlayImage, k: usize| i.rgba[k * 4..k * 4 + 4].to_vec();
        let bits = &art.images[1];
        assert_eq!((bits.width, bits.height), (8, 1));
        assert_eq!(px(bits, 0), [255, 255, 255, 0]);
        assert_eq!(px(bits, 3), [255, 255, 255, 0]);
        assert_eq!(px(bits, 4), [0, 0, 0, 255]);
        assert_eq!(px(bits, 7), [0, 0, 0, 255]);
        // Colours by number.
        assert_eq!(px(&art.images[2], 0), [200, 210, 220, 255]);
        assert_eq!(px(&art.images[2], 1), [10, 20, 30, 255]);
        // A stencil: painted (in the fill colour, half see-through) where its samples are 0.
        assert_eq!(px(&art.images[3], 0), [255, 0, 0, 128]);
        assert_eq!(px(&art.images[3], 1)[3], 0);
        // Inline.
        assert_eq!(px(&art.images[4], 0), [16, 32, 48, 255]);
        // What can't be read is said so; what can comes in as an underlay.
        assert_eq!(art.skipped.get(crate::pdf_image::UNREADABLE), Some(&1));
        assert!(
            art.notes.iter().any(|n| n.contains("5 pictures")),
            "{:?}",
            art.notes
        );
        let page = read_page(&bytes, 1).unwrap();
        assert_eq!((page.images.len(), page.skipped_images), (5, 1));
        assert!(page.images[0].across.distance(DVec2::new(306.0, 0.0)) < 1e-9);
    }

    #[test]
    fn jpeg_images_are_read() {
        use lopdf::{Stream, dictionary};
        // A 16 × 8 JPEG, red on its left half and blue on its right (made with Windows'
        // own encoder).
        let jpeg = include_bytes!("testdata/red_blue.jpg").to_vec();
        let bytes = one_page_with(
            b"q 160 0 0 80 100 100 cm /Im0 Do Q",
            dictionary! {},
            |doc, res| {
                let image = doc.add_object(Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Image", "Width" => 16, "Height" => 8,
                        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                        "Filter" => "DCTDecode",
                    },
                    jpeg,
                ));
                res.set("XObject", dictionary! { "Im0" => image });
            },
        );
        let page = read_page(&bytes, 1).unwrap();
        assert_eq!(page.skipped_images, 0);
        let image = &page.images[0];
        assert_eq!((image.width, image.height), (16, 8));
        let px = |x: usize, y: usize| {
            let at = (y * 16 + x) * 4;
            Rgb([image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]])
        };
        let near = |a: Rgb, b: [u8; 3]| a.0.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 24);
        assert!(near(px(2, 4), [255, 0, 0]), "{:?}", px(2, 4));
        assert!(near(px(13, 4), [0, 0, 255]), "{:?}", px(13, 4));
        assert_eq!(image.rgba[3], 255);
    }

    #[test]
    fn opaque_fills_hide_what_was_drawn_before_them() {
        use lopdf::dictionary;
        // A line across where a white square is painted next, another along its bottom
        // edge, a note under it; then the square, a line drawn after it over it, and a
        // see-through square over the first line's far end.
        let bytes = one_page(
            b"1 w 0 50 m 100 50 l S 30 30 m 70 30 l S \
              BT /F1 10 Tf 40 45 Td (H) Tj ET \
              1 1 1 rg 30 30 40 40 re f \
              0 60 m 100 60 l S \
              /Half gs 0 0 1 rg 85 40 10 20 re f",
            dictionary! {
                "Font" => dictionary! {
                    "F1" => dictionary! {
                        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                        "Encoding" => "WinAnsiEncoding",
                    },
                },
                "ExtGState" => dictionary! {
                    "Half" => dictionary! { "Type" => "ExtGState", "ca" => 0.5 },
                },
            },
        );
        let art = read_artwork(&bytes, 1, "plan").unwrap();
        let mm = drift_core::underlays::MM_PER_POINT;
        let spans = |layer: &str, y: f64| -> Vec<(f64, f64)> {
            let mut v: Vec<(f64, f64)> = art
                .paths
                .iter()
                .filter(|p| p.layer == layer && p.stroke.is_some())
                .flat_map(|p| &p.subpaths)
                .filter(|s| (s.start.y / mm - y).abs() < 1e-6)
                .map(|s| {
                    let (a, b) = (s.start.x / mm, s.segs.last().unwrap().end().x / mm);
                    (
                        (a.min(b) * 1e6).round() / 1e6,
                        (a.max(b) * 1e6).round() / 1e6,
                    )
                })
                .collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        };
        // The line under the square shows on either side of it; its middle is on the
        // hidden layer.
        assert_eq!(spans("plan", 50.0), [(0.0, 30.0), (70.0, 100.0)]);
        assert_eq!(spans(COVERED, 50.0), [(30.0, 70.0)]);
        // Along the square's edge, and drawn after it: untouched.
        assert_eq!(spans("plan", 30.0), [(30.0, 70.0)]);
        assert_eq!(spans("plan", 60.0), [(0.0, 100.0)]);
        // The see-through square hides nothing.
        assert!(spans(COVERED, 50.0).iter().all(|s| s.1 <= 70.0));
        // The note under the square goes with what it hides.
        let note = art.texts.iter().find(|t| t.text == "H").expect("the note");
        assert_eq!(note.layer, COVERED);
        // That layer starts hidden, and the import says so.
        assert!(!art.layer_flags[COVERED].visible);
        assert!(
            art.notes.iter().any(|n| n.contains(COVERED)),
            "{:?}",
            art.notes
        );
        // An underlay traces only what shows.
        let page = read_page(&bytes, 1).unwrap();
        let lines_at_50 = page
            .prims
            .iter()
            .filter(|p| matches!(p, RefPrim::Line(a, b) if (a.y - 50.0).abs() < 1e-6 && (b.y - 50.0).abs() < 1e-6))
            .count();
        assert_eq!(lines_at_50, 2);
    }

    #[test]
    fn text_traces_as_the_outlines_of_its_letters() {
        use lopdf::{Stream, dictionary};
        // Upright and mirrored runs in Helvetica (not embedded: Drift's own letters stand
        // in), an invisible run, one an opaque fill paints over, and a Type 3 letter: a
        // square.
        let content = b"BT /F1 10 Tf 1 0 0 1 100 700 Tm (AB) Tj ET \
            BT /F1 10 Tf -1 0 0 1 300 700 Tm (AB) Tj ET \
            BT /F1 10 Tf 3 Tr 1 0 0 1 100 600 Tm (HIDDEN) Tj ET \
            BT /F1 10 Tf 0 Tr 1 0 0 1 100 500 Tm (UNDER) Tj ET \
            1 1 1 rg 90 490 100 30 re f \
            BT /T3 10 Tf 1 0 0 1 100 400 Tm (a) Tj ET";
        let bytes = one_page_with(content, dictionary! {}, |doc, res| {
            let square = doc.add_object(Stream::new(
                dictionary! {},
                b"1000 0 0 0 1000 1000 d1 0 0 1000 1000 re f".to_vec(),
            ));
            res.set(
                "Font",
                dictionary! {
                    "F1" => dictionary! {
                        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    },
                    "T3" => dictionary! {
                        "Type" => "Font", "Subtype" => "Type3",
                        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
                        "FontMatrix" => vec![
                            0.001.into(), 0.into(), 0.into(), 0.001.into(), 0.into(), 0.into(),
                        ],
                        "Encoding" => dictionary! {
                            "Differences" => vec![97.into(), Object::Name(b"sq".to_vec())],
                        },
                        "CharProcs" => dictionary! { "sq" => square },
                        "FirstChar" => 97, "LastChar" => 97, "Widths" => vec![1000.into()],
                    },
                },
            );
        });
        let page = read_page(&bytes, 1).unwrap();
        assert_eq!(page.skipped_text, 0);
        let rings: Vec<&Vec<DVec2>> = page
            .glyphs
            .iter()
            .map(|g| match g {
                RefPrim::Polyline(r) => r,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(rings.iter().all(|r| r.first() == r.last()), "closed rings");
        let bounds = |pick: &dyn Fn(DVec2) -> bool| {
            rings
                .iter()
                .filter(|r| pick(r[0]))
                .flat_map(|r| r.iter())
                .fold(
                    (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                    |(lo, hi), p| (lo.min(*p), hi.max(*p)),
                )
        };
        // Upright: from its start rightwards, capitals about 7 pt tall on the baseline.
        let (lo, hi) = bounds(&|p| p.y > 650.0 && p.x < 200.0);
        assert!((lo.x - 100.0).abs() < 1.0 && hi.x < 111.0, "{lo} {hi}");
        assert!(
            (lo.y - 700.0).abs() < 0.5 && (706.0..708.0).contains(&hi.y),
            "{lo} {hi}"
        );
        // Mirrored: from its start leftwards.
        let (lo, hi) = bounds(&|p| p.y > 650.0 && p.x > 200.0);
        assert!((hi.x - 300.0).abs() < 1.0 && lo.x > 289.0, "{lo} {hi}");
        // Nothing for the invisible run, or for the one a fill hides.
        assert!(rings.iter().all(|r| !(480.0..620.0).contains(&r[0].y)));
        // The Type 3 square, 10 pt.
        let (lo, hi) = bounds(&|p| p.y < 450.0);
        assert!(lo.distance(DVec2::new(100.0, 400.0)) < 1e-6, "{lo}");
        assert!(hi.distance(DVec2::new(110.0, 410.0)) < 1e-6, "{hi}");
        // Reading the page to edit leaves its letters as text (the square isn't a path).
        let art = read_artwork(&bytes, 1, "plan").unwrap();
        assert_eq!(art.paths.len(), 1, "only the white fill");
        assert!(art.texts.iter().any(|t| t.text == "AB"));
    }
}
