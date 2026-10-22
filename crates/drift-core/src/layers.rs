//! Layers and line appearance: colour, ISO 128 line weights and line types.

/// Stable identifier of a layer. Layer `l0` ("0") always exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub u32);

impl LayerId {
    /// The default layer, which can't be deleted.
    pub const DEFAULT: LayerId = LayerId(0);
}

impl std::fmt::Display for LayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "l{}", self.0)
    }
}

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb([byte(0)?, byte(2)?, byte(4)?]))
    }
}

/// A line weight in hundredths of a millimetre on paper; 0 is the thinnest line the output
/// can draw (a hairline).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineWeight(pub u16);

impl LineWeight {
    pub const HAIRLINE: LineWeight = LineWeight(0);
    /// The ISO 128 series, plus the hairline.
    pub const ISO: [LineWeight; 10] = [
        LineWeight(0),
        LineWeight(13),
        LineWeight(18),
        LineWeight(25),
        LineWeight(35),
        LineWeight(50),
        LineWeight(70),
        LineWeight(100),
        LineWeight(140),
        LineWeight(200),
    ];

    pub fn mm(self) -> f64 {
        f64::from(self.0) / 100.0
    }

    pub fn label(self) -> String {
        if self.0 == 0 {
            "Hairline".into()
        } else {
            format!("{:.2} mm", self.mm())
        }
    }

    /// The nearest ISO weight to a width in mm.
    pub fn nearest(mm: f64) -> LineWeight {
        Self::ISO
            .into_iter()
            .min_by(|a, b| (a.mm() - mm).abs().total_cmp(&(b.mm() - mm).abs()))
            .unwrap_or_default()
    }
}

/// A line's dash pattern (ISO 128 types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineType {
    #[default]
    Continuous,
    Dashed,
    /// Short dashes (hidden edges).
    Hidden,
    /// Long dash, dot (centre lines).
    Center,
    /// Long dash, two dots.
    Phantom,
    /// No stroke: the curve isn't drawn or printed (it is shown dashed only while picked
    /// or hovered), but it still bounds areas, snaps and holds relations, as a shape with
    /// a fill and no stroke in a drawing program. A curve's own setting, never a layer's.
    None,
}

impl LineType {
    pub const ALL: [LineType; 5] = [
        LineType::Continuous,
        LineType::Dashed,
        LineType::Hidden,
        LineType::Center,
        LineType::Phantom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LineType::Continuous => "Continuous",
            LineType::Dashed => "Dashed",
            LineType::Hidden => "Hidden",
            LineType::Center => "Centre",
            LineType::Phantom => "Phantom",
            LineType::None => "None",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LineType::Continuous => "continuous",
            LineType::Dashed => "dashed",
            LineType::Hidden => "hidden",
            LineType::Center => "center",
            LineType::Phantom => "phantom",
            LineType::None => "none",
        }
    }

    pub fn from_name(s: &str) -> Option<LineType> {
        Self::ALL
            .into_iter()
            .chain([LineType::None])
            .find(|t| t.name() == s)
    }

    /// Whether a curve of this type is drawn and printed (see [`LineType::None`]).
    pub fn is_stroked(self) -> bool {
        self != LineType::None
    }
}

/// A layer: default appearance for its entities, and visibility / locking.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Stroke colour. `None` is the theme's foreground colour (black on light, white on
    /// dark).
    pub color: Option<Rgb>,
    /// Colour of fills and hatches; `None` uses the stroke colour.
    pub fill_color: Option<Rgb>,
    pub weight: LineWeight,
    pub line_type: LineType,
    pub visible: bool,
    /// Locked layers can be seen and snapped to, but not edited; the solver keeps them fixed.
    pub locked: bool,
    /// Included in printed / exported output.
    pub printable: bool,
    /// Its geometry can be snapped to (off: seen, and editable unless locked, but the pointer
    /// doesn't snap to it).
    pub snappable: bool,
    /// Draw order: higher draws on top (ties by id). See [`Document::reorder_layer`].
    ///
    /// [`Document::reorder_layer`]: crate::Document::reorder_layer
    pub order: i32,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: None,
            fill_color: None,
            weight: LineWeight(25),
            line_type: LineType::Continuous,
            visible: true,
            locked: false,
            printable: true,
            snappable: true,
            order: 0,
        }
    }

    /// The colour of fills and hatches on this layer: its fill colour, else its stroke colour.
    pub fn fill_colour(&self) -> Option<Rgb> {
        self.fill_color.or(self.color)
    }
}

/// Per-entity appearance overrides; `None` means "by layer".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Appearance {
    pub color: Option<Rgb>,
    pub weight: Option<LineWeight>,
    pub line_type: Option<LineType>,
}

impl Appearance {
    pub fn is_by_layer(&self) -> bool {
        *self == Appearance::default()
    }
}
