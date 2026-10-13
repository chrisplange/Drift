//! Fonts kept in a drawing, so its text looks the same on a computer that doesn't have them
//! (and in the browser, which can't read installed fonts at all).

use std::collections::BTreeMap;
use std::sync::Arc;

/// A font file's bytes and the face's index in it (font collections hold several faces).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFile {
    pub data: Arc<Vec<u8>>,
    pub index: u32,
}

impl FontFile {
    pub fn new(data: Vec<u8>, index: u32) -> Self {
        Self {
            data: Arc::new(data),
            index,
        }
    }
}

/// A family kept in the drawing: its regular face, and its bold one where there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedFont {
    pub regular: FontFile,
    pub bold: Option<FontFile>,
}

/// The fonts of a drawing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontSet {
    /// Families kept in the drawing, by family name.
    pub embedded: BTreeMap<String, EmbeddedFont>,
    /// The family that dimensions, notes, view titles and title blocks are set in. `None` is
    /// the standard look: Helvetica in print.
    pub drawing: Option<String>,
}

impl FontSet {
    /// Bytes of font data held, for showing what a drawing carries.
    pub fn bytes(&self) -> usize {
        self.embedded
            .values()
            .map(|f| f.regular.data.len() + f.bold.as_ref().map_or(0, |b| b.data.len()))
            .sum()
    }
}
