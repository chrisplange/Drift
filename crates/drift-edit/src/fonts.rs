//! The fonts a drawing uses: keeping them inside it, its own font for dimensions, notes and
//! sheets, and making a font's file known for outlining text. The app makes the drawing's
//! font known to the screen's text renderer (drift-app's `fonts`).

use std::rc::Rc;
use std::sync::Arc;

use drift_core::{EmbeddedFont, FontFile, TextFont};
use drift_draft::font::{DrawingFont, Face};

use crate::editor::Editor;

/// Larger font files than this aren't kept in a drawing (a full CJK font is tens of
/// megabytes, written into the file as text).
pub const MAX_EMBEDDED_BYTES: usize = 12 * 1024 * 1024;

/// Why a font couldn't be kept in the drawing (it can still be used on this computer).
fn refusal(face: &Face) -> Option<String> {
    if face.restricted {
        Some(format!(
            "{}'s licence doesn't allow it to be kept in a drawing: it is used from this \
             computer, and other computers will show their own font",
            face.family
        ))
    } else if face.data.len() > MAX_EMBEDDED_BYTES {
        Some(format!(
            "{} is too large to keep in a drawing ({} MB): it is used from this computer",
            face.family,
            face.data.len() / (1024 * 1024)
        ))
    } else {
        None
    }
}

fn existing_is_bold(font: &EmbeddedFont) -> bool {
    Face::new(font.regular.data.clone(), font.regular.index).is_some_and(|f| f.bold)
}

impl Editor {
    /// Make a font's file known for outlining text: the drawing's own copy first, else the
    /// one installed on this computer (found once; a missing one is remembered as missing).
    pub fn ensure_font(&self, font: &TextFont) {
        let TextFont::System(family) = font else {
            return;
        };
        let known = drift_draft::outline::font_known(family);
        if known == Some(true) {
            return;
        }
        if let Some(own) = self.doc.fonts().embedded.get(family) {
            let file = &own.regular;
            drift_draft::outline::register_font(family, Some((file.data.to_vec(), file.index)));
        } else if known.is_none() {
            drift_draft::outline::register_font(family, drift_platform::fonts::load(family));
        }
    }

    /// Keep an installed family in the drawing, so it travels with the file (one undo
    /// step). `Ok(false)` if it was already there. An error says why it can't be kept; the
    /// font is still usable on this computer.
    pub fn embed_font(&mut self, family: &str) -> Result<bool, String> {
        if self.doc.fonts().embedded.contains_key(family) {
            return Ok(false);
        }
        let (data, index) = drift_platform::fonts::load(family)
            .ok_or_else(|| format!("{family} isn't installed on this computer"))?;
        let regular = Face::new(Arc::new(data), index)
            .ok_or_else(|| format!("{family}'s font file couldn't be read"))?;
        if let Some(why) = refusal(&regular) {
            return Err(why);
        }
        // Its bold face too, where the family has one that may be kept.
        let bold = drift_platform::fonts::load_bold(family)
            .and_then(|(data, index)| Face::new(Arc::new(data), index))
            .filter(|face| refusal(face).is_none());
        let font = EmbeddedFont {
            regular: FontFile {
                data: regular.data.clone(),
                index: regular.index,
            },
            bold: bold.map(|b| FontFile {
                data: b.data.clone(),
                index: b.index,
            }),
        };
        let mut fonts = self.doc.fonts().clone();
        fonts.embedded.insert(family.to_owned(), font);
        self.doc.begin();
        self.doc.set_fonts(fonts);
        self.commit("Keep font in drawing");
        Ok(true)
    }

    /// Add a font file (`.ttf`, `.otf`) to the drawing under its own family name, which is
    /// returned. A second file of the same family that is bold becomes its bold face.
    pub fn add_font_file(&mut self, bytes: Vec<u8>) -> Result<String, String> {
        let face = Face::new(Arc::new(bytes), 0).ok_or("that file isn't a font Drift can read")?;
        if let Some(why) = refusal(&face) {
            return Err(why);
        }
        let family = face.family.clone();
        let file = FontFile {
            data: face.data.clone(),
            index: 0,
        };
        let mut fonts = self.doc.fonts().clone();
        match fonts.embedded.get_mut(&family) {
            Some(existing) if face.bold && !existing_is_bold(existing) => {
                existing.bold = Some(file)
            }
            _ => {
                fonts.embedded.insert(
                    family.clone(),
                    EmbeddedFont {
                        regular: file,
                        bold: None,
                    },
                );
            }
        }
        self.doc.begin();
        self.doc.set_fonts(fonts);
        self.commit("Add font");
        // Text already set in it is redrawn in the real thing.
        drift_draft::outline::register_font(&family, Some((face.data.to_vec(), 0)));
        self.text_cache.borrow_mut().clear();
        Ok(family)
    }

    /// Set the family that dimensions, notes, view titles and title blocks are set in
    /// (`None`: the standard look). An installed family is kept in the drawing where it can
    /// be; if it can't, the returned note says why.
    pub fn set_drawing_font(&mut self, family: Option<String>) -> Option<String> {
        let note = family
            .as_deref()
            .and_then(|f| self.embed_font(f).err())
            .filter(|_| {
                family
                    .as_deref()
                    .is_some_and(|f| drift_platform::fonts::load(f).is_some())
            });
        let mut fonts = self.doc.fonts().clone();
        fonts.drawing = family;
        self.doc.begin();
        self.doc.set_fonts(fonts);
        self.commit("Drawing font");
        note
    }

    /// The drawing's font, ready to measure and draw with: from the drawing's own copy, else
    /// from this computer. `None` for the standard look, or when the font can't be found
    /// here (the name is kept for where it can).
    pub fn drawing_font(&self) -> Option<Rc<DrawingFont>> {
        let family = self.doc.fonts().drawing.clone()?;
        let own = self.doc.fonts().embedded.get(&family);
        // The same family and the same file as last time: the same font.
        let key = (
            family.clone(),
            own.map_or(0, |f| Arc::as_ptr(&f.regular.data) as usize),
        );
        if let Some((k, font)) = &*self.font_cache.borrow()
            && *k == key
        {
            return font.clone();
        }
        let face = |file: &FontFile| Face::new(file.data.clone(), file.index);
        let font = match own {
            Some(own) => face(&own.regular).map(|regular| DrawingFont {
                regular,
                bold: own.bold.as_ref().and_then(face),
            }),
            None => drift_platform::fonts::load(&family)
                .and_then(|(data, index)| Face::new(Arc::new(data), index))
                .map(|regular| DrawingFont {
                    regular,
                    bold: drift_platform::fonts::load_bold(&family)
                        .and_then(|(data, index)| Face::new(Arc::new(data), index)),
                }),
        }
        .map(Rc::new);
        *self.font_cache.borrow_mut() = Some((key, font.clone()));
        font
    }

    /// Whether the drawing names a font for its annotations that can't be found: neither in
    /// the drawing nor on this computer.
    pub fn drawing_font_missing(&self) -> Option<String> {
        let family = self.doc.fonts().drawing.clone()?;
        self.drawing_font().is_none().then_some(family)
    }
}
