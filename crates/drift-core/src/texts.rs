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

/// The typefaces text can use: two built in (so files open the same everywhere), or one
/// installed on the computer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextFont {
    /// A clean sans-serif (Ubuntu Light).
    #[default]
    Sans,
    /// A monospaced font (Hack).
    Mono,
    /// A font installed on the computer, by its family name. Where it isn't installed the
    /// text is shown in the built-in sans, and keeps the name.
    System(String),
}

impl TextFont {
    pub const BUILT_IN: [TextFont; 2] = [TextFont::Sans, TextFont::Mono];
    /// What a system font's name is written with in a file: `system:Helvetica Neue`.
    const SYSTEM_PREFIX: &'static str = "system:";

    /// The name in a file.
    pub fn name(&self) -> String {
        match self {
            TextFont::Sans => "sans".into(),
            TextFont::Mono => "mono".into(),
            TextFont::System(family) => format!("{}{family}", Self::SYSTEM_PREFIX),
        }
    }

    pub fn label(&self) -> String {
        match self {
            TextFont::Sans => "Sans".into(),
            TextFont::Mono => "Monospace".into(),
            TextFont::System(family) => family.clone(),
        }
    }

    pub fn from_name(s: &str) -> Option<TextFont> {
        match s {
            "sans" => Some(TextFont::Sans),
            "mono" => Some(TextFont::Mono),
            _ => s
                .strip_prefix(Self::SYSTEM_PREFIX)
                .map(str::trim)
                .filter(|family| !family.is_empty())
                .map(|family| TextFont::System(family.to_owned())),
        }
    }
}

/// How the lines of a text sit against its anchor point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

    pub fn name(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TextAlign::Left => "Left",
            TextAlign::Center => "Centre",
            TextAlign::Right => "Right",
        }
    }

    pub fn from_name(s: &str) -> Option<TextAlign> {
        Self::ALL.into_iter().find(|a| a.name() == s)
    }

    /// How much of a line's width lies before the anchor: none, half or all of it.
    pub fn factor(self) -> f64 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
        }
    }
}

/// Text in the sketch: one line or several (separated by `\n`).
#[derive(Clone, Debug, PartialEq)]
pub struct SketchText {
    pub text: String,
    /// The anchor: on the first line's baseline, at its start, middle or end as `align`
    /// says. Further lines hang below it.
    pub at: DVec2,
    /// Height of capital letters, in sketch units (mm).
    pub height: f64,
    /// Counter-clockwise, in degrees.
    pub angle: f64,
    pub font: TextFont,
    pub align: TextAlign,
    /// From one baseline to the next, in cap heights.
    pub spacing: f64,
    /// Lines longer than this (mm) are broken at spaces. `None`: only where the text says.
    pub wrap: Option<f64>,
    pub layer: LayerId,
    pub appearance: Appearance,
}

impl SketchText {
    /// The usual distance from one baseline to the next, in cap heights.
    pub const SPACING: f64 = 1.6;

    pub fn new(text: impl Into<String>, at: DVec2, height: f64) -> Self {
        Self {
            text: text.into(),
            at,
            height,
            angle: 0.0,
            font: TextFont::Sans,
            align: TextAlign::Left,
            spacing: Self::SPACING,
            wrap: None,
            layer: LayerId::DEFAULT,
            appearance: Appearance::default(),
        }
    }
}
