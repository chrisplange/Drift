//! Reading the vector drawing of a PDF page: the stroked and filled paths through nested form
//! XObjects, with their colours, widths, dashes and optional-content layers. As an underlay
//! they're outlines to trace over ([`read_page`]); as artwork, curves to edit
//! ([`read_artwork`]). Text and images are skipped.
//! Coordinates are PDF points with the origin at the page's lower-left corner, upright as
//! the page is shown.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use drift_core::{DVec2, RefPrim, Rgb};
use lopdf::{Dictionary, Document, Object};

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
    /// Text runs and images that were skipped.
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
    /// How each layer starts out (hidden, locked, not printed), by name.
    layers: BTreeMap<String, LayerFlags>,
    skipped_text: usize,
    skipped_images: usize,
    skipped_shadings: usize,
    clipped: bool,
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

/// Illustrator's stand-in page for a file saved without its PDF copy.
const NO_PDF_CONTENT: &str = "without PDF Content";

/// A painted path, in points on the upright page.
struct PdfPath {
    subpaths: Vec<Subpath>,
    stroke: Option<Stroke>,
    fill: Option<Rgb>,
    /// The optional-content group (layer) it is drawn in.
    layer: Option<String>,
}

/// Read the curves of page `number` (1-based), as outlines to trace over.
pub fn read_page(bytes: &[u8], number: u32) -> Result<PdfPage, String> {
    let page = read_drawing(bytes, number)?;
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
    Ok(PdfPage {
        number: page.number,
        pages: page.pages,
        size: page.size,
        prims,
        skipped_text: page.skipped_text + page.texts.len(),
        skipped_images: page.skipped_images,
    })
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
    let page = read_drawing(bytes, number)?;
    refuse_stand_in(&page)?;
    let mm = drift_core::underlays::MM_PER_POINT;
    let mut art = Artwork {
        size: page.size * mm,
        ..Default::default()
    };
    // The file's layers in its order, with how they start out.
    for (layer, flags) in &page.layers {
        if page.paths.iter().any(|p| p.layer.as_deref() == Some(layer))
            || page.texts.iter().any(|t| t.layer.as_deref() == Some(layer))
        {
            art.layer(layer);
            art.layer_flags.insert(layer.clone(), *flags);
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
    for path in page.paths {
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
        });
    }
    if page.skipped_text > 0 {
        art.skipped.insert(
            "text in a font whose characters can't be read".into(),
            page.skipped_text,
        );
    }
    if page.skipped_images > 0 {
        art.skipped.insert("image".into(), page.skipped_images);
    }
    if page.skipped_shadings > 0 {
        art.skipped
            .insert("gradient (shading)".into(), page.skipped_shadings);
    }
    if page.clipped {
        art.note("Clipping was left out: everything it hid came in too");
    }
    if page.pages > 1 {
        art.note(format!("Page {} of {}", page.number, page.pages));
    }
    Ok(art)
}

fn read_drawing(bytes: &[u8], number: u32) -> Result<PageDrawing, String> {
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
        paths: Vec::new(),
        texts: Vec::new(),
        fonts: HashMap::new(),
        segments: 0,
        skipped_text: 0,
        skipped_images: 0,
        skipped_shadings: 0,
        clipped: false,
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
        text: TextState::default(),
    };
    reader.run(&content, state, &res, &mut Vec::new(), 0)?;
    Ok(PageDrawing {
        number,
        pages: count,
        size,
        paths: reader.paths,
        texts: merge_runs(reader.texts),
        layers: layer_states(&doc),
        skipped_text: reader.skipped_text,
        skipped_images: reader.skipped_images,
        skipped_shadings: reader.skipped_shadings,
        clipped: reader.clipped,
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
    text: TextState,
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

/// A colour from operands: one (grey), three (RGB) or four (CMYK) numbers.
fn color_from(ops: &[Object]) -> Option<Rgb> {
    let v: Vec<f64> = ops.iter().filter_map(num).collect();
    let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(match v.as_slice() {
        [g] => Rgb([byte(*g); 3]),
        [r, g, b] => Rgb([byte(*r), byte(*g), byte(*b)]),
        [c, m, y, k] => Rgb([
            byte((1.0 - c) * (1.0 - k)),
            byte((1.0 - m) * (1.0 - k)),
            byte((1.0 - y) * (1.0 - k)),
        ]),
        _ => return None,
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
    paths: Vec<PdfPath>,
    texts: Vec<PdfText>,
    /// Fonts read so far, by where their dictionaries are.
    fonts: HashMap<usize, Rc<Font>>,
    segments: usize,
    skipped_text: usize,
    skipped_images: usize,
    skipped_shadings: usize,
    clipped: bool,
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
        Some(font)
    }

    /// Show strings (with `TJ`'s spacing between them), moving the text matrix past them,
    /// and keep what they read as one run.
    fn show(&mut self, gs: &State, tm: &mut Matrix, parts: &[&Object], layer: Option<String>) {
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
        let mut text = String::new();
        let mut unreadable = false;
        for part in parts {
            match part {
                Object::String(bytes, _) => {
                    for code in font.codes(bytes) {
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
        if unreadable && font.unreadable {
            self.skipped_text += 1;
            return;
        }
        if t.mode == 3 || text.trim().is_empty() {
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
                "RG" | "G" | "K" | "SC" | "SCN" => {
                    if let Some(c) = color_from(o) {
                        gs.stroke = c;
                    }
                }
                "rg" | "g" | "k" | "sc" | "scn" => {
                    if let Some(c) = color_from(o) {
                        gs.fill = c;
                    }
                }
                "CS" => gs.stroke = Rgb([0, 0, 0]),
                "cs" => gs.fill = Rgb([0, 0, 0]),
                "gs" => {
                    let ext = o
                        .first()
                        .and_then(|n| n.as_name().ok())
                        .and_then(|n| self.resource(resources, b"ExtGState", n))
                        .and_then(|e| e.as_dict().ok());
                    if let Some(ext) = ext {
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
                    if std::mem::take(&mut clip_next) && operator == "n" {
                        // A clipping path that isn't drawn.
                        self.clipped = true;
                        continue;
                    }
                    let strokes = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
                    let fills = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                    if paths.is_empty() || !(strokes || fills) {
                        continue;
                    }
                    self.segments += paths.iter().map(|s| s.segs.len()).sum::<usize>();
                    let scale = gs.scale();
                    self.paths.push(PdfPath {
                        subpaths: paths,
                        stroke: strokes.then(|| Stroke {
                            color: gs.stroke,
                            width: gs.width * scale,
                            dashes: gs.dashes.iter().map(|d| d * scale).collect(),
                        }),
                        fill: fills.then_some(gs.fill),
                        layer: layers.iter().rev().find_map(Clone::clone),
                    });
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
                    self.show(&gs, &mut tm, &parts, layer);
                }
                "BI" => self.skipped_images += 1,
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
                        self.skipped_images += 1;
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
}
