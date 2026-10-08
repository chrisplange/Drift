//! A vector display list for output: lines, polylines, true arcs and circles, filled areas and
//! text, with a pen (colour, width, line type) and a layer name on every item. Coordinates are
//! millimetres with y up: paper millimetres for a sheet, world millimetres for model space.
//!
//! The SVG, PDF and DXF writers in `drift-io` read this, so every output shows the same thing.

use drift_core::{Bounds, DVec2, LineType, Rgb};

/// How an item is stroked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub color: Rgb,
    /// Line width in mm (on paper).
    pub width: f64,
    pub line_type: LineType,
}

impl Pen {
    pub const BLACK: Rgb = Rgb([0, 0, 0]);

    pub fn new(width: f64) -> Self {
        Self {
            color: Self::BLACK,
            width,
            line_type: LineType::Continuous,
        }
    }

    pub fn with_type(mut self, line_type: LineType) -> Self {
        self.line_type = line_type;
        self
    }

    pub fn with_color(mut self, color: Rgb) -> Self {
        self.color = color;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Line(DVec2, DVec2),
    Polyline {
        points: Vec<DVec2>,
        closed: bool,
    },
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Counter-clockwise from `start` by `sweep` radians (sweep > 0).
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// A filled area: outer rings and holes, filled even–odd, and the same area as triangles
    /// (for formats without even–odd fills, such as DXF). A `mask` is a paper-coloured patch
    /// behind text (left out of DXF).
    Fill {
        rings: Vec<Vec<DVec2>>,
        triangles: Vec<[DVec2; 3]>,
        mask: bool,
    },
    /// A line of text. `at` is on the baseline, at the left, centre or right per `align`.
    /// `angle` is counter-clockwise in radians; `height` is the cap height in mm.
    Text {
        at: DVec2,
        height: f64,
        angle: f64,
        align: Align,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub layer: String,
    pub pen: Pen,
    pub prim: Prim,
    /// Clipped to the drawing's viewport (geometry seen through a sheet's drawing area).
    pub clipped: bool,
}

/// Layer properties for formats that keep layers (DXF).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    pub name: String,
    pub color: Rgb,
    pub width: f64,
    pub line_type: LineType,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    /// Paper size (or, for model space, the extents' size). The page spans (0, 0) to `size`.
    pub size: DVec2,
    pub items: Vec<Item>,
    /// The viewport `clipped` items are cut to.
    pub clip: Option<Bounds>,
    pub layers: Vec<LayerInfo>,
    /// Line type pattern scale (model space at 1 : n uses n).
    pub line_type_scale: f64,
}

impl Drawing {
    pub fn new(size: DVec2) -> Self {
        Self {
            size,
            line_type_scale: 1.0,
            ..Self::default()
        }
    }

    pub fn push(&mut self, layer: &str, pen: Pen, prim: Prim) {
        self.items.push(Item {
            layer: layer.to_owned(),
            pen,
            prim,
            clipped: false,
        });
    }

    pub fn line(&mut self, layer: &str, pen: Pen, a: DVec2, b: DVec2) {
        self.push(layer, pen, Prim::Line(a, b));
    }

    pub fn rect(&mut self, layer: &str, pen: Pen, min: DVec2, max: DVec2) {
        self.push(
            layer,
            pen,
            Prim::Polyline {
                points: vec![min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)],
                closed: true,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        layer: &str,
        pen: Pen,
        at: DVec2,
        height: f64,
        align: Align,
        text: impl Into<String>,
    ) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        self.push(
            layer,
            pen,
            Prim::Text {
                at,
                height,
                angle: 0.0,
                align,
                text,
            },
        );
    }

    /// The drawing with every length multiplied by `f` (about the origin): coordinates, radii,
    /// text heights, the clip and the line type scale. Pen widths stay paper millimetres.
    pub fn scaled(&self, f: f64) -> Drawing {
        let mut d = self.clone();
        d.size *= f;
        d.line_type_scale *= f;
        d.clip = d.clip.map(|c| Bounds {
            min: c.min * f,
            max: c.max * f,
        });
        for item in &mut d.items {
            match &mut item.prim {
                Prim::Line(a, b) => {
                    *a *= f;
                    *b *= f;
                }
                Prim::Polyline { points, .. } => points.iter_mut().for_each(|p| *p *= f),
                Prim::Circle { center, radius } | Prim::Arc { center, radius, .. } => {
                    *center *= f;
                    *radius *= f;
                }
                Prim::Fill {
                    rings, triangles, ..
                } => {
                    rings.iter_mut().flatten().for_each(|p| *p *= f);
                    triangles.iter_mut().flatten().for_each(|p| *p *= f);
                }
                Prim::Text { at, height, .. } => {
                    *at *= f;
                    *height *= f;
                }
            }
        }
        d
    }

    /// Ensure a layer is listed (first definition wins).
    pub fn layer(&mut self, info: LayerInfo) {
        if !self.layers.iter().any(|l| l.name == info.name) {
            self.layers.push(info);
        }
    }

    /// Bounds of everything drawn (text approximated by its box).
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        for item in &self.items {
            match &item.prim {
                Prim::Line(p, q) => {
                    b.include(*p);
                    b.include(*q);
                }
                Prim::Polyline { points, .. } => points.iter().for_each(|p| b.include(*p)),
                Prim::Circle { center, radius } | Prim::Arc { center, radius, .. } => {
                    b.include(*center - DVec2::splat(*radius));
                    b.include(*center + DVec2::splat(*radius));
                }
                Prim::Fill { rings, .. } => rings.iter().flatten().for_each(|p| b.include(*p)),
                Prim::Text {
                    at, height, text, ..
                } => {
                    let w = text_width(text, *height);
                    b.include(*at - DVec2::new(w, *height * 0.3));
                    b.include(*at + DVec2::new(w, *height * 1.2));
                }
            }
        }
        b
    }
}

/// Dash pattern of a line type, in mm at line type scale 1: dash, gap, dash, gap…
pub fn dash_pattern(line_type: LineType) -> &'static [f64] {
    match line_type {
        LineType::Continuous => &[],
        LineType::Dashed => &[4.0, 1.5],
        LineType::Hidden => &[2.0, 1.0],
        LineType::Center => &[12.0, 1.5, 1.5, 1.5],
        LineType::Phantom => &[12.0, 1.5, 1.5, 1.5, 1.5, 1.5],
    }
}

/// Helvetica advance widths (per 1000 em) for ASCII 32–126.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Helvetica's cap height per em: text `height` (cap height) → font size.
pub const CAP_HEIGHT: f64 = 0.718;

/// The width of `text` in Helvetica at cap height `height` (mm).
pub fn text_width(text: &str, height: f64) -> f64 {
    let size = height / CAP_HEIGHT;
    let units: u32 = text
        .chars()
        .map(|c| match c as u32 {
            32..=126 => u32::from(HELVETICA[(c as u32 - 32) as usize]),
            0xD8 | 0xF8 => 778, // Ø ø
            0xB1 | 0xD7 => 584, // ± ×
            0xB0 => 400,        // °
            0xB2 | 0xB3 => 333, // ² ³
            _ => 556,
        })
        .sum();
    f64::from(units) / 1000.0 * size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_bounds() {
        // "Ø6" at 3.5 mm cap height.
        let w = text_width("Ø6", 3.5);
        assert!((w - (778.0 + 556.0) / 1000.0 * 3.5 / CAP_HEIGHT).abs() < 1e-12);
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.rect(
            "frame",
            Pen::new(0.5),
            DVec2::splat(10.0),
            DVec2::new(287.0, 200.0),
        );
        d.push(
            "geometry",
            Pen::new(0.35),
            Prim::Circle {
                center: DVec2::new(5.0, 5.0),
                radius: 2.0,
            },
        );
        let b = d.bounds();
        assert_eq!(b.min, DVec2::splat(3.0));
        assert_eq!(b.max, DVec2::new(287.0, 200.0));
        assert_eq!(dash_pattern(LineType::Center).len(), 4);
    }
}
