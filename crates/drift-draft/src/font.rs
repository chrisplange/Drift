//! The drawing's own font: the typeface that dimensions, notes, view titles and title blocks
//! are set in, measured here so the screen, the sheet layout and every output agree.
//!
//! With no drawing font the standard look applies: text is measured as Helvetica, which is
//! what a PDF viewer shows for it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

/// One face of a font: its file, and what is read from it once.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub family: String,
    pub data: Arc<Vec<u8>>,
    pub index: u32,
    /// Font units per em.
    units: f64,
    /// Cap height, ascent and descent (positive down), in ems.
    cap: f64,
    ascent: f64,
    descent: f64,
    /// Its outlines are PostScript (CFF) ones, not TrueType.
    pub cff: bool,
    /// The file is a collection of several faces.
    pub collection: bool,
    /// Its licence flags forbid embedding it in documents.
    pub restricted: bool,
    /// A bold (or heavier) face.
    pub bold: bool,
}

impl Face {
    /// Read a face from a font file. `None` if the bytes aren't a font.
    pub fn new(data: Arc<Vec<u8>>, index: u32) -> Option<Face> {
        let font = FontRef::from_index(&data, index).ok()?;
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let units = f64::from(metrics.units_per_em.max(1));
        let cap = metrics
            .cap_height
            .map(f64::from)
            .filter(|c| *c > 0.0)
            .unwrap_or(0.7 * units);
        let family = font
            .localized_strings(StringId::TYPOGRAPHIC_FAMILY_NAME)
            .english_or_first()
            .or_else(|| {
                font.localized_strings(StringId::FAMILY_NAME)
                    .english_or_first()
            })
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())?;
        // fsType: 2 is "restricted licence"; 0x200 is "bitmaps only". Either rules out
        // carrying the outlines in a drawing or a PDF.
        let restricted = font
            .os2()
            .is_ok_and(|t| t.fs_type() & 0x000f == 0x0002 || t.fs_type() & 0x0200 != 0);
        let face = Face {
            family,
            index,
            units,
            cap: cap / units,
            ascent: f64::from(metrics.ascent) / units,
            descent: -f64::from(metrics.descent) / units,
            cff: font.cff().is_ok() || font.cff2().is_ok(),
            collection: data.starts_with(b"ttcf"),
            restricted,
            bold: font.attributes().weight.value() >= 600.0,
            data: data.clone(),
        };
        Some(face)
    }

    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index).ok()
    }

    /// Cap height per em: text `height` (its capitals' height) over this is the font size.
    pub fn cap_ratio(&self) -> f64 {
        self.cap
    }

    /// Ascent and descent (both positive) per em.
    pub fn extent(&self) -> (f64, f64) {
        (self.ascent, self.descent)
    }

    /// The glyphs of `text` in order: glyph id and advance in ems. A character the font
    /// lacks is glyph 0 (its "missing" box) at that glyph's own width.
    pub fn glyphs(&self, text: &str) -> Vec<(u16, f64)> {
        let Some(font) = self.font() else {
            return Vec::new();
        };
        let charmap = font.charmap();
        let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
        text.chars()
            .map(|c| {
                let gid = charmap.map(c).unwrap_or_default();
                let advance = advances
                    .advance_width(gid)
                    .map_or(0.5, |a| f64::from(a) / self.units);
                (gid.to_u32().min(u32::from(u16::MAX)) as u16, advance)
            })
            .collect()
    }

    /// The width of `text` with capitals `height` tall, plus `tracking` (a fraction of the
    /// font size) between letters.
    pub fn width(&self, text: &str, height: f64, tracking: f64) -> f64 {
        let size = height / self.cap;
        let glyphs = self.glyphs(text);
        let gaps = glyphs.len().saturating_sub(1) as f64;
        (glyphs.iter().map(|g| g.1).sum::<f64>() + gaps * tracking) * size
    }

    /// Whether the face can be carried inside a PDF as a font. (A face from a collection
    /// would need cutting out of it first; text in it is drawn as outlines instead.)
    pub fn embeddable(&self) -> bool {
        !self.restricted && !self.collection
    }
}

/// The drawing's font: a regular face, and a bold one where the family has one (otherwise
/// bold is set in the regular face).
#[derive(Clone, Debug, PartialEq)]
pub struct DrawingFont {
    pub regular: Face,
    pub bold: Option<Face>,
}

impl DrawingFont {
    /// The face for a weight. Without a bold face, bold text is set in the regular one:
    /// bold isn't faked by stroking the outlines (a variable font, such as Bahnschrift, has
    /// its weights inside one face, and a stroke reads as a smudge).
    pub fn face(&self, bold: bool) -> &Face {
        match (&self.bold, bold) {
            (Some(b), true) => b,
            _ => &self.regular,
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<DrawingFont>>> = const { RefCell::new(None) };
}

/// Set the font that annotation and sheet text is measured and drawn in on this thread
/// (`None` for the standard look). The app sets it from the document before laying anything
/// out.
pub fn set_drawing_font(font: Option<Rc<DrawingFont>>) {
    CURRENT.with(|c| *c.borrow_mut() = font);
}

pub fn drawing_font() -> Option<Rc<DrawingFont>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Cap height per em of the font text is set in: the drawing's, else Helvetica's.
pub fn cap_ratio() -> f64 {
    drawing_font().map_or(crate::page::CAP_HEIGHT, |f| f.regular.cap_ratio())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sans() -> Face {
        Face::new(Arc::new(epaint_default_fonts::UBUNTU_LIGHT.to_vec()), 0).expect("a font")
    }

    #[test]
    fn a_face_knows_its_name_metrics_and_glyphs() {
        let face = sans();
        assert_eq!(face.family, "Ubuntu");
        assert!(!face.cff && !face.collection);
        assert!(face.cap_ratio() > 0.6 && face.cap_ratio() < 0.8);
        let glyphs = face.glyphs("AV!");
        assert_eq!(glyphs.len(), 3);
        assert!(
            glyphs
                .iter()
                .all(|(gid, advance)| *gid != 0 && *advance > 0.1)
        );
        // A character it lacks is the missing glyph, not a gap of nothing.
        assert_eq!(face.glyphs("\u{4e2d}")[0].0, 0);
        // Twice the height, twice the width; tracking adds between letters only.
        let w = face.width("Drift", 3.0, 0.0);
        assert!((face.width("Drift", 6.0, 0.0) - 2.0 * w).abs() < 1e-9);
        let size = 3.0 / face.cap_ratio();
        assert!((face.width("Drift", 3.0, 0.1) - (w + 4.0 * 0.1 * size)).abs() < 1e-9);
        assert!(Face::new(Arc::new(b"not a font".to_vec()), 0).is_none());
    }

    #[test]
    fn text_is_measured_in_the_drawing_font_when_there_is_one() {
        use crate::page::{TextStyle, styled_width};
        let helvetica = styled_width("Mounting plate", 3.0, TextStyle::REGULAR);
        let font = Rc::new(DrawingFont {
            regular: sans(),
            bold: None,
        });
        set_drawing_font(Some(font.clone()));
        let own = styled_width("Mounting plate", 3.0, TextStyle::REGULAR);
        assert!((own - font.regular.width("Mounting plate", 3.0, 0.0)).abs() < 1e-9);
        assert!(
            (own - helvetica).abs() > 0.1,
            "a different font, a different width"
        );
        assert_eq!(cap_ratio(), font.regular.cap_ratio());
        // Without a bold face, bold is the regular face, as it is.
        assert!(std::ptr::eq(font.face(true), &font.regular));
        assert_eq!(
            styled_width("Rev", 3.0, TextStyle::BOLD),
            font.regular.width("Rev", 3.0, 0.0)
        );
        set_drawing_font(None);
        assert_eq!(
            styled_width("Mounting plate", 3.0, TextStyle::REGULAR),
            helvetica
        );
        assert_eq!(cap_ratio(), crate::page::CAP_HEIGHT);
    }
}
