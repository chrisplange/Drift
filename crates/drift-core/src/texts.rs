//! Sketch text: a single line of text drawn as its glyph outlines at a real size (for
//! engraving, signage, labels cut from material), positioned by its baseline start and turned
//! by an angle. It can be converted into curves. (Notes, by contrast, are drafting text sized
//! on paper.)

use glam::DVec2;

use crate::layers::{Appearance, LayerId};

/// Stable identifier of a sketch text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextId(pub u32);

impl std::fmt::Display for TextId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{}", self.0)
    }
}

/// The typefaces text can use (built in, so files open the same everywhere).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextFont {
    /// A clean sans-serif (Ubuntu Light).
    #[default]
    Sans,
    /// A monospaced font (Hack).
    Mono,
}

impl TextFont {
    pub const ALL: [TextFont; 2] = [TextFont::Sans, TextFont::Mono];

    pub fn name(self) -> &'static str {
        match self {
            TextFont::Sans => "sans",
            TextFont::Mono => "mono",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TextFont::Sans => "Sans",
            TextFont::Mono => "Monospace",
        }
    }

    pub fn from_name(s: &str) -> Option<TextFont> {
        Self::ALL.into_iter().find(|f| f.name() == s)
    }
}

/// A line of text in the sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchText {
    pub text: String,
    /// The start of the baseline.
    pub at: DVec2,
    /// Height of capital letters, in sketch units (mm).
    pub height: f64,
    /// Counter-clockwise, in degrees.
    pub angle: f64,
    pub font: TextFont,
    pub layer: LayerId,
    pub appearance: Appearance,
}

impl SketchText {
    pub fn new(text: impl Into<String>, at: DVec2, height: f64) -> Self {
        Self {
            text: text.into(),
            at,
            height,
            angle: 0.0,
            font: TextFont::Sans,
            layer: LayerId::DEFAULT,
            appearance: Appearance::default(),
        }
    }
}
