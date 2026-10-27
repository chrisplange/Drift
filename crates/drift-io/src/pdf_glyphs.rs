//! The outlines of a PDF's letters, so its text can be shown as the page draws it (on
//! underlays): from the font program the file embeds (TrueType, OpenType, CFF or Type 1),
//! else from Drift's own font, set to the PDF's widths. Type 3 fonts draw their letters
//! with ordinary painting operators; the page reader runs those (`pdf_import`).
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use drift_core::DVec2;
use lopdf::{Dictionary, Document, Object};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::raw::ps::cff::CffFontRef;
use skrifa::raw::ps::string::Sid;
use skrifa::raw::ps::type1::Type1Font;
use skrifa::raw::tables::cmap::PlatformId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::pdf_text::{Font, glyph_text};

/// A letter's outline: closed contours in ems of the font size (text space), y up.
pub(crate) type Outline = Vec<Vec<DVec2>>;

/// How a font's letters are drawn: the program the file embeds, how its codes find their
/// glyphs, and the outlines worked out so far.
#[derive(Default)]
pub(crate) struct Glyphs {
    program: Option<Program>,
    map: GlyphMap,
    /// The stand-in is Drift's monospaced font (for Courier and the like).
    mono: bool,
    cache: RefCell<HashMap<u32, Option<Rc<Outline>>>>,
}

impl std::fmt::Debug for Glyphs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let program = match &self.program {
            None => "none",
            Some(Program::Sfnt(_)) => "sfnt",
            Some(Program::Cff { .. }) => "cff",
            Some(Program::Type1(..)) => "type1",
        };
        f.debug_struct("Glyphs")
            .field("program", &program)
            .field("mono", &self.mono)
            .finish_non_exhaustive()
    }
}

/// An embedded font program.
enum Program {
    /// TrueType or OpenType.
    Sfnt(Vec<u8>),
    /// A bare CFF font, and its glyphs by name and by what they read as (for simple fonts).
    Cff {
        data: Vec<u8>,
        names: OnceCell<Names>,
    },
    Type1(Box<Type1Font>, OnceCell<Names>),
}

/// A PostScript font's glyphs by name, and by the text their names stand for.
#[derive(Default)]
struct Names {
    by_name: HashMap<String, GlyphId>,
    by_text: HashMap<String, GlyphId>,
}

impl Names {
    fn new<'n>(glyphs: impl Iterator<Item = (GlyphId, &'n str)>) -> Names {
        let mut names = Names::default();
        for (gid, name) in glyphs {
            if let Some(text) = glyph_text(name) {
                names.by_text.entry(text).or_insert(gid);
            }
            names.by_name.entry(name.to_owned()).or_insert(gid);
        }
        names
    }
}

/// How codes find glyphs.
#[derive(Default)]
enum GlyphMap {
    /// A simple font: each code's glyph name from its encoding's `Differences`, and whether
    /// the font says it is symbolic (its codes are its own, not a standard encoding's).
    Simple {
        names: Vec<Option<String>>,
        symbolic: bool,
    },
    /// A composite font: codes are CIDs where its encoding is `Identity-H` or `-V`, and its
    /// `CIDToGIDMap` (none: the same numbers).
    Cid {
        identity: bool,
        to_gid: Option<Vec<u16>>,
    },
    /// Nothing to go by (a Type 3 font, or an unknown kind).
    #[default]
    None,
}

impl Glyphs {
    /// What draws the letters of the font `dict` (a simple font, or a composite one with
    /// its descendant `cid`).
    pub(crate) fn read(doc: &Document, dict: &Dictionary, cid: Option<&Dictionary>) -> Glyphs {
        let deref = |o: &Object| doc.dereference(o).ok().map(|(_, o)| o.clone());
        let base = dict
            .get(b"BaseFont")
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n).to_lowercase())
            .unwrap_or_default();
        let mono = ["courier", "mono", "consol", "typewriter"]
            .iter()
            .any(|m| base.contains(m));
        let descriptor = cid
            .unwrap_or(dict)
            .get(b"FontDescriptor")
            .ok()
            .and_then(deref)
            .and_then(|d| d.as_dict().ok().cloned());
        let flags = descriptor
            .as_ref()
            .and_then(|d| d.get(b"Flags").ok())
            .and_then(|f| f.as_i64().ok())
            .unwrap_or(0);
        let program = descriptor.as_ref().and_then(|d| program(doc, d));
        let map = match cid {
            Some(cid) => {
                let identity = matches!(
                    dict.get(b"Encoding").and_then(Object::as_name),
                    Ok(b"Identity-H" | b"Identity-V")
                );
                let to_gid = match cid.get(b"CIDToGIDMap").ok().and_then(deref) {
                    Some(Object::Stream(s)) => {
                        let data = s
                            .decompressed_content()
                            .unwrap_or_else(|_| s.content.clone());
                        Some(
                            data.as_chunks::<2>()
                                .0
                                .iter()
                                .map(|c| u16::from_be_bytes(*c))
                                .collect(),
                        )
                    }
                    _ => None,
                };
                GlyphMap::Cid { identity, to_gid }
            }
            None => GlyphMap::Simple {
                names: differences(doc, dict),
                symbolic: flags & 4 != 0,
            },
        };
        Glyphs {
            program,
            map,
            mono,
            cache: RefCell::default(),
        }
    }
}

/// The font program a font descriptor embeds, if Drift can read it.
fn program(doc: &Document, descriptor: &Dictionary) -> Option<Program> {
    let stream = |key: &[u8]| {
        let o = descriptor.get(key).ok()?;
        let s = doc.dereference(o).ok()?.1.as_stream().ok()?;
        let data = s
            .decompressed_content()
            .unwrap_or_else(|_| s.content.clone());
        let subtype = s
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .ok()
            .map(<[u8]>::to_vec);
        Some((data, subtype))
    };
    if let Some((data, _)) = stream(b"FontFile2") {
        return FontRef::new(&data).is_ok().then_some(Program::Sfnt(data));
    }
    if let Some((data, subtype)) = stream(b"FontFile3") {
        if subtype.as_deref() == Some(b"OpenType") || FontRef::new(&data).is_ok() {
            return FontRef::new(&data).is_ok().then_some(Program::Sfnt(data));
        }
        return CffFontRef::new(&data, 0, None)
            .is_ok()
            .then_some(Program::Cff {
                data,
                names: OnceCell::new(),
            });
    }
    if let Some((data, _)) = stream(b"FontFile") {
        return Type1Font::new(&data)
            .ok()
            .map(|f| Program::Type1(Box::new(f), OnceCell::new()));
    }
    None
}

/// A simple font's glyph names by code, from its encoding's `Differences`.
pub(crate) fn differences(doc: &Document, dict: &Dictionary) -> Vec<Option<String>> {
    let mut names = vec![None; 256];
    let enc = dict
        .get(b"Encoding")
        .ok()
        .and_then(|o| doc.dereference(o).ok().map(|(_, o)| o.clone()));
    let Some(Object::Dictionary(d)) = enc else {
        return names;
    };
    let Ok(Object::Array(diffs)) = d.get(b"Differences") else {
        return names;
    };
    let mut code = 0usize;
    for o in diffs {
        match o {
            Object::Integer(c) => code = (*c).clamp(0, 255) as usize,
            Object::Name(n) => {
                if let Some(slot) = names.get_mut(code) {
                    *slot = Some(String::from_utf8_lossy(n).into_owned());
                }
                code += 1;
            }
            _ => {}
        }
    }
    names
}

impl Font {
    /// The outline of the letter `code` stands for, in ems: the embedded font's glyph, else
    /// Drift's own font's letters for what it reads as, set to its width. `None` if it can't
    /// be drawn, or reads as blank with no font to draw it.
    pub(crate) fn outline(&self, code: u32) -> Option<Rc<Outline>> {
        let glyphs = self.glyphs.as_ref()?;
        if let Some(found) = glyphs.cache.borrow().get(&code) {
            return found.clone();
        }
        let outline = glyphs
            .embedded(self, code)
            .or_else(|| stand_in(&self.text(code), glyphs.mono, self.width(code) / 1000.0))
            .map(Rc::new);
        glyphs.cache.borrow_mut().insert(code, outline.clone());
        outline
    }
}

impl Glyphs {
    /// The embedded font's outline for a code.
    fn embedded(&self, font: &Font, code: u32) -> Option<Outline> {
        match self.program.as_ref()? {
            Program::Sfnt(data) => {
                let f = FontRef::new(data).ok()?;
                let gid = self.sfnt_glyph(&f, font, code)?;
                sfnt_outline(&f, gid)
            }
            Program::Cff { data, names } => {
                let f = CffFontRef::new(data, 0, None).ok()?;
                let gid = match &self.map {
                    GlyphMap::Cid { identity, .. } => {
                        if !identity {
                            return None;
                        }
                        let cid = u16::try_from(code).ok()?;
                        if f.is_cid() {
                            f.charset()?.glyph_id(Sid::new(cid)).ok()?
                        } else {
                            GlyphId::new(u32::from(cid))
                        }
                    }
                    GlyphMap::Simple { .. } => {
                        let names = names.get_or_init(|| cff_names(&f));
                        let builtin = || f.encoding()?.map(u8::try_from(code).ok()?);
                        self.simple_glyph(names, font, code, builtin)?
                    }
                    GlyphMap::None => return None,
                };
                let sub = f.subfont(f.subfont_index(gid)?, &[]).ok()?;
                let mut pen = Pen::new(f64::from(f.upem().max(1)));
                f.draw(&sub, gid, &[], None, &mut pen).ok()?;
                Some(pen.finish())
            }
            Program::Type1(f, names) => {
                let names = names.get_or_init(|| Names::new(f.glyph_names()));
                let builtin = || f.encoding()?.map(u8::try_from(code).ok()?);
                let gid = self.simple_glyph(names, font, code, builtin)?;
                let mut pen = Pen::new(f64::from(f.upem().max(1)));
                f.draw(gid, None, &mut pen).ok()?;
                Some(pen.finish())
            }
        }
    }

    /// A simple PostScript font's glyph for a code: by the name its `Differences` give it,
    /// by what the code reads as, or by the font's own encoding (first, for a symbolic font
    /// that names none).
    fn simple_glyph(
        &self,
        names: &Names,
        font: &Font,
        code: u32,
        builtin: impl Fn() -> Option<GlyphId>,
    ) -> Option<GlyphId> {
        let GlyphMap::Simple {
            names: differences,
            symbolic,
        } = &self.map
        else {
            return None;
        };
        let named = differences.get(code as usize).cloned().flatten();
        if let Some(gid) = named.as_ref().and_then(|n| names.by_name.get(n)) {
            return Some(*gid);
        }
        if *symbolic
            && named.is_none()
            && let Some(gid) = builtin()
        {
            return Some(gid);
        }
        let text = font.text(code);
        names
            .by_text
            .get(&text)
            .copied()
            .or_else(builtin)
            .filter(|g| g.to_u32() != 0)
    }

    /// A TrueType or OpenType font's glyph for a code.
    fn sfnt_glyph(&self, f: &FontRef<'_>, font: &Font, code: u32) -> Option<GlyphId> {
        match &self.map {
            GlyphMap::Cid { identity, to_gid } => {
                if !identity {
                    return None;
                }
                let gid = match to_gid {
                    Some(map) => u32::from(*map.get(code as usize)?),
                    None => code,
                };
                Some(GlyphId::new(gid))
            }
            GlyphMap::Simple { names, symbolic } => {
                let Ok(cmap) = f.cmap() else {
                    // A subset with no character map: its codes are its glyphs.
                    return Some(GlyphId::new(code));
                };
                let table = |platform: PlatformId, encoding: u16| {
                    cmap.encoding_records()
                        .iter()
                        .find(|r| r.platform_id() == platform && r.encoding_id() == encoding)
                        .and_then(|r| r.subtable(cmap.offset_data()).ok())
                };
                let found = |g: Option<GlyphId>| g.filter(|g| g.to_u32() != 0);
                let text = names
                    .get(code as usize)
                    .cloned()
                    .flatten()
                    .and_then(|n| glyph_text(&n))
                    .unwrap_or_else(|| font.text(code));
                let mut chars = text.chars();
                let single = chars.next().filter(|_| chars.next().is_none());
                let unicode = || {
                    let c = single?;
                    found(table(PlatformId::Windows, 1)?.map_codepoint(c))
                        .or_else(|| found(table(PlatformId::Unicode, 3)?.map_codepoint(c)))
                };
                let mac = || found(table(PlatformId::Macintosh, 0)?.map_codepoint(code));
                let symbol = || {
                    let t = table(PlatformId::Windows, 0)?;
                    found(t.map_codepoint(0xf000 | code)).or_else(|| found(t.map_codepoint(code)))
                };
                if *symbolic {
                    symbol().or_else(mac).or_else(unicode)
                } else {
                    unicode().or_else(mac).or_else(symbol)
                }
            }
            GlyphMap::None => None,
        }
    }
}

/// A bare CFF font's glyphs by name.
fn cff_names(f: &CffFontRef<'_>) -> Names {
    let Some(charset) = f.charset() else {
        return Names::default();
    };
    let named: Vec<(GlyphId, String)> = charset
        .iter()
        .filter_map(|(gid, sid)| {
            let name = std::str::from_utf8(f.string(sid)?).ok()?;
            Some((gid, name.to_owned()))
        })
        .collect();
    Names::new(named.iter().map(|(g, n)| (*g, n.as_str())))
}

/// A glyph of a TrueType or OpenType font, in ems.
fn sfnt_outline(f: &FontRef<'_>, gid: GlyphId) -> Option<Outline> {
    let units = f64::from(f.head().ok()?.units_per_em().max(1));
    let glyph = f.outline_glyphs().get(gid)?;
    let mut pen = Pen::new(units);
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .ok()?;
    Some(pen.finish())
}

/// What `text` looks like in Drift's own font (the sans, or the monospaced one), in ems,
/// squeezed or stretched to `width` ems (within reason) so the line keeps the PDF's length.
fn stand_in(text: &str, mono: bool, width: f64) -> Option<Outline> {
    if text.trim().is_empty() {
        return None;
    }
    let data = drift_draft::outline::builtin_font(mono);
    let f = FontRef::new(&data).ok()?;
    let units = f64::from(f.head().ok()?.units_per_em().max(1));
    let charmap = f.charmap();
    let advances = f.glyph_metrics(Size::unscaled(), LocationRef::default());
    let mut out: Outline = Vec::new();
    let mut x = 0.0;
    for c in text.chars() {
        let gid = charmap.map(c)?;
        if let Some(contours) = sfnt_outline(&f, gid) {
            out.extend(
                contours
                    .into_iter()
                    .map(|ring| ring.into_iter().map(|p| p + DVec2::new(x, 0.0)).collect()),
            );
        }
        x += advances
            .advance_width(gid)
            .map_or(0.5, |a| f64::from(a) / units);
    }
    if width > 0.0 && x > 0.0 {
        let squeeze = (width / x).clamp(0.6, 1.6);
        for ring in &mut out {
            for p in ring.iter_mut() {
                p.x *= squeeze;
            }
        }
    }
    Some(out)
}

/// Collects an outline as flattened contours, scaled from font units to ems.
pub(crate) struct Pen {
    scale: f64,
    contours: Outline,
    current: Vec<DVec2>,
}

impl Pen {
    pub(crate) fn new(units_per_em: f64) -> Pen {
        Pen {
            scale: 1.0 / units_per_em,
            contours: Vec::new(),
            current: Vec::new(),
        }
    }

    fn at(&self, x: f32, y: f32) -> DVec2 {
        DVec2::new(f64::from(x), f64::from(y)) * self.scale
    }

    fn last(&self) -> DVec2 {
        self.current.last().copied().unwrap_or(DVec2::ZERO)
    }

    /// Close the contour being drawn (a closed polyline ends where it starts).
    fn end(&mut self) {
        let mut ring = std::mem::take(&mut self.current);
        ring.dedup();
        if ring.len() > 2 {
            if ring.first() != ring.last() {
                ring.push(ring[0]);
            }
            self.contours.push(ring);
        }
    }

    pub(crate) fn finish(mut self) -> Outline {
        self.end();
        self.contours
    }
}

/// Segments a curve is cut into: by its control polygon's length in ems, so small curves
/// stay light and big letters stay round.
fn pieces(length: f64) -> usize {
    ((length * 24.0).ceil() as usize).clamp(1, 16)
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end();
        let p = self.at(x, y);
        self.current.push(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.current.push(p);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (p0, c, p) = (self.last(), self.at(cx0, cy0), self.at(x, y));
        let n = pieces(p0.distance(c) + c.distance(p));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.current.push(p0 * u * u + c * 2.0 * u * t + p * t * t);
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (p0, c0, c1, p) = (
            self.last(),
            self.at(cx0, cy0),
            self.at(cx1, cy1),
            self.at(x, y),
        );
        let n = pieces(p0.distance(c0) + c0.distance(c1) + c1.distance(p));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.current.push(
                p0 * (u * u * u)
                    + c0 * (3.0 * u * u * t)
                    + c1 * (3.0 * u * t * t)
                    + p * (t * t * t),
            );
        }
    }

    fn close(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font with no program, reading codes as WinAnsi letters, at 500 thousandths wide.
    fn plain_font() -> Font {
        Font {
            encoding: Some(
                (0..256u32)
                    .map(|c| char::from_u32(c).map(String::from))
                    .collect(),
            ),
            default_width: 500.0,
            glyphs: Some(Rc::new(Glyphs::default())),
            ..Default::default()
        }
    }

    fn bounds(o: &Outline) -> (DVec2, DVec2) {
        o.iter().flatten().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    #[test]
    fn letters_without_an_embedded_font_come_from_drifts_own() {
        let font = plain_font();
        let h = font.outline(u32::from(b'H')).expect("an H");
        let (lo, hi) = bounds(&h);
        // A capital stands on the baseline, about 0.7 em tall, within its 0.5 em width.
        assert!(lo.y.abs() < 0.02, "{lo}");
        assert!((0.6..0.8).contains(&hi.y), "{hi}");
        assert!(lo.x >= -0.01 && hi.x <= 0.55, "{lo} {hi}");
        // Every contour is closed.
        assert!(h.iter().all(|r| r.first() == r.last() && r.len() > 3));
        // A space has nothing to draw, and a code that reads as nothing can't be drawn.
        assert!(font.outline(u32::from(b' ')).is_none());
        assert!(font.outline(1).is_none());
    }

    #[test]
    fn the_stand_in_keeps_the_pdfs_widths() {
        let narrow = stand_in("W", false, 0.5).unwrap();
        let wide = stand_in("W", false, 1.0).unwrap();
        let (a, b) = (bounds(&narrow), bounds(&wide));
        assert!(b.1.x - b.0.x > 1.5 * (a.1.x - a.0.x) * 0.9);
        // Heights don't change.
        assert!((a.1.y - b.1.y).abs() < 1e-9);
    }

    #[test]
    fn embedded_truetype_glyphs_are_found_by_character() {
        // Drift's own sans, embedded as a simple TrueType font.
        let data = drift_draft::outline::builtin_font(false).to_vec();
        let font = Font {
            encoding: Some(
                (0..256u32)
                    .map(|c| char::from_u32(c).map(String::from))
                    .collect(),
            ),
            default_width: 500.0,
            glyphs: Some(Rc::new(Glyphs {
                program: Some(Program::Sfnt(data.clone())),
                map: GlyphMap::Simple {
                    names: vec![None; 256],
                    symbolic: false,
                },
                ..Default::default()
            })),
            ..Default::default()
        };
        let o = font.outline(u32::from(b'O')).unwrap();
        // An O is two rings (outside and in), drawn from the font itself (not squeezed).
        assert_eq!(o.len(), 2);
        let f = FontRef::new(&data).unwrap();
        let gid = f.charmap().map('O').unwrap();
        assert_eq!(*o, sfnt_outline(&f, gid).unwrap());
    }

    #[test]
    fn identity_cids_are_glyph_numbers() {
        let data = drift_draft::outline::builtin_font(false).to_vec();
        let f = FontRef::new(&data).unwrap();
        let gid = f.charmap().map('A').unwrap().to_u32();
        let font = Font {
            two_byte: true,
            default_width: 1000.0,
            unreadable: true,
            glyphs: Some(Rc::new(Glyphs {
                program: Some(Program::Sfnt(data.clone())),
                map: GlyphMap::Cid {
                    identity: true,
                    to_gid: None,
                },
                ..Default::default()
            })),
            ..Default::default()
        };
        // Readable or not as text, its letters draw.
        assert!(font.text(gid).is_empty());
        let o = font.outline(gid).unwrap();
        assert_eq!(*o, sfnt_outline(&f, GlyphId::new(gid)).unwrap());
    }

    #[test]
    fn curves_flatten_into_closed_rings() {
        let mut pen = Pen::new(1000.0);
        pen.move_to(0.0, 0.0);
        pen.curve_to(0.0, 500.0, 500.0, 500.0, 500.0, 0.0);
        pen.close();
        let rings = pen.finish();
        assert_eq!(rings.len(), 1);
        let r = &rings[0];
        assert!(r.len() > 8, "{}", r.len());
        assert_eq!(r.first(), r.last());
        // Through the middle of the curve: 0.375 em up at x = 0.25.
        let top = r.iter().map(|p| p.y).fold(0.0, f64::max);
        assert!((top - 0.375).abs() < 0.01, "{top}");
    }
}
