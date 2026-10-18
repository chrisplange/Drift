//! A vector display list for output: lines, polylines, true arcs and circles, filled areas and
//! text, with a pen (colour, width, line type) and a layer name on every item. Coordinates are
//! millimetres with y up: paper millimetres for a sheet, world millimetres for model space.
//!
//! The SVG, PDF and DXF writers in `drift-io` read this, so every output shows the same thing.

use drift_core::{Bounds, DVec2, LineType, Rgb, SheetColours};

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
    /// An ellipse, or the counter-clockwise part of one from parametric angle `start` over
    /// `sweep` (a whole one sweeps 2π): `center + major·cos t + perp(major̂)·minor·sin t`.
    Ellipse {
        center: DVec2,
        major: DVec2,
        minor: f64,
        start: f64,
        sweep: f64,
    },
    /// A cubic Bézier from `p[0]` to `p[3]` with handles `p[1]` and `p[2]`.
    Bezier {
        p: [DVec2; 4],
    },
    /// A filled area: outer rings and holes, filled even–odd, and the same area as triangles
    /// (for formats without even–odd fills, such as DXF). A `mask` is a paper-coloured patch
    /// behind text (left out of DXF).
    Fill {
        rings: Vec<Vec<DVec2>>,
        triangles: Vec<[DVec2; 3]>,
        mask: bool,
    },
    /// A copy of a block defined in [`Drawing::blocks`]: its items mirrored (x → −x) if
    /// `mirror`, scaled, turned by `angle` degrees and moved to `at`.
    Insert {
        block: String,
        at: DVec2,
        angle: f64,
        scale: f64,
        mirror: bool,
    },
    /// A line of text. `at` is on the baseline, at the left, centre or right per `align`.
    /// `angle` is counter-clockwise in radians; `height` is the cap height in mm. `font` is
    /// sketch text's own font; `None` is the drawing's font (annotations, title blocks).
    Text {
        at: DVec2,
        height: f64,
        angle: f64,
        align: Align,
        text: String,
        style: TextStyle,
        font: Option<drift_core::TextFont>,
    },
}

/// How far italic text leans: the tangent of its slant (about 12°).
pub const ITALIC_SLANT: f64 = 0.21;

/// How a line of text is set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextStyle {
    /// The bold weight.
    pub bold: bool,
    /// Slanted (an oblique of the upright face, [`ITALIC_SLANT`]).
    pub italic: bool,
    /// Extra space after every letter, as a fraction of the font size (0.1 opens small
    /// capitals out nicely).
    pub tracking: f64,
}

impl TextStyle {
    pub const REGULAR: TextStyle = TextStyle {
        bold: false,
        italic: false,
        tracking: 0.0,
    };
    pub const BOLD: TextStyle = TextStyle {
        bold: true,
        italic: false,
        tracking: 0.0,
    };

    pub fn tracked(mut self, tracking: f64) -> Self {
        self.tracking = tracking;
        self
    }
}

/// An ellipse prim as connected points (for outputs without ellipses); other prims give none.
pub fn prim_points(prim: &Prim) -> Vec<DVec2> {
    match *prim {
        Prim::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => drift_core::shape::flatten_ellipse(center, major, minor, start, sweep, 64),
        Prim::Bezier { p } => drift_core::shape::flatten_bezier(&p, 32),
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub layer: String,
    pub pen: Pen,
    pub prim: Prim,
    /// The window the item is seen through, if any (a view's frame on a sheet).
    pub clip: Option<Bounds>,
}

/// Layer properties for formats that keep layers (DXF).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    pub name: String,
    pub color: Rgb,
    pub width: f64,
    pub line_type: LineType,
}

/// A block definition for formats that keep blocks (DXF): items in block coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDef {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    /// Paper size (or, for model space, the extents' size). The page spans (0, 0) to `size`.
    pub size: DVec2,
    pub items: Vec<Item>,
    pub layers: Vec<LayerInfo>,
    /// Line type pattern scale (model space at 1 : n uses n).
    pub line_type_scale: f64,
    /// Blocks that `Insert` items refer to.
    pub blocks: Vec<BlockDef>,
    /// The font its text is set in (`None`: the standard look, Helvetica).
    pub font: Option<std::rc::Rc<crate::font::DrawingFont>>,
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
            clip: None,
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
                style: TextStyle::REGULAR,
                font: None,
            },
        );
    }

    /// A line of text in a given weight and tracking.
    #[allow(clippy::too_many_arguments)]
    pub fn styled_text(
        &mut self,
        layer: &str,
        pen: Pen,
        at: DVec2,
        height: f64,
        align: Align,
        style: TextStyle,
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
                style,
                font: None,
            },
        );
    }

    /// A filled rectangle in the pen's colour.
    pub fn filled_rect(&mut self, layer: &str, pen: Pen, min: DVec2, max: DVec2) {
        let corners = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
        self.push(
            layer,
            pen,
            Prim::Fill {
                rings: vec![corners.to_vec()],
                triangles: vec![
                    [corners[0], corners[1], corners[2]],
                    [corners[0], corners[2], corners[3]],
                ],
                mask: false,
            },
        );
    }

    /// The drawing with every length multiplied by `f` (about the origin): coordinates, radii,
    /// text heights, the clip and the line type scale. Pen widths stay paper millimetres.
    pub fn scaled(&self, f: f64) -> Drawing {
        let mut d = self.clone();
        d.size *= f;
        d.line_type_scale *= f;
        let items = d
            .items
            .iter_mut()
            .chain(d.blocks.iter_mut().flat_map(|b| b.items.iter_mut()));
        for item in items {
            item.clip = item.clip.map(|c| Bounds {
                min: c.min * f,
                max: c.max * f,
            });
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
                Prim::Ellipse {
                    center,
                    major,
                    minor,
                    ..
                } => {
                    *center *= f;
                    *major *= f;
                    *minor *= f;
                }
                Prim::Bezier { p } => p.iter_mut().for_each(|q| *q *= f),
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
                Prim::Insert { at, .. } => *at *= f,
            }
        }
        d
    }

    /// Recolour everything for a sheet's colour choice: strokes and text (lines, hatch lines,
    /// glyphs) as strokes, filled areas as fills, block contents and layers too. Masks are
    /// paper-white in every writer, so they stay white.
    pub fn set_colours(&mut self, colours: SheetColours) {
        if colours == SheetColours::Colour {
            return;
        }
        let items = self
            .items
            .iter_mut()
            .chain(self.blocks.iter_mut().flat_map(|b| b.items.iter_mut()));
        for item in items {
            let fill = matches!(item.prim, Prim::Fill { .. });
            item.pen.color = colours.map(item.pen.color, fill);
        }
        for layer in &mut self.layers {
            layer.color = colours.map(layer.color, false);
        }
    }

    /// The drawing with every `Insert` replaced by its block's items, transformed.
    pub fn exploded(&self) -> Drawing {
        if !self
            .items
            .iter()
            .any(|i| matches!(i.prim, Prim::Insert { .. }))
        {
            return self.clone();
        }
        let mut d = self.clone();
        d.items.clear();
        for item in &self.items {
            match &item.prim {
                Prim::Insert {
                    block,
                    at,
                    angle,
                    scale,
                    mirror,
                } => {
                    let Some(def) = self.blocks.iter().find(|b| &b.name == block) else {
                        continue;
                    };
                    let t = Transform {
                        at: *at,
                        rotor: DVec2::from_angle(angle.to_radians()),
                        angle: angle.to_radians(),
                        scale: *scale,
                        mirror: *mirror,
                    };
                    for inner in &def.items {
                        d.items.push(Item {
                            layer: if inner.layer == "0" {
                                item.layer.clone()
                            } else {
                                inner.layer.clone()
                            },
                            pen: inner.pen,
                            prim: t.prim(&inner.prim),
                            clip: item.clip,
                        });
                    }
                }
                _ => d.items.push(item.clone()),
            }
        }
        d.blocks.clear();
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
                Prim::Ellipse { .. } | Prim::Bezier { .. } => {
                    prim_points(&item.prim).iter().for_each(|p| b.include(*p))
                }
                Prim::Fill { rings, .. } => rings.iter().flatten().for_each(|p| b.include(*p)),
                Prim::Text {
                    at,
                    height,
                    text,
                    style,
                    font,
                    ..
                } => {
                    let w = line_width_in(text, *height, *style, font.as_ref());
                    b.include(*at - DVec2::new(w, *height * 0.3));
                    b.include(*at + DVec2::new(w, *height * 1.2));
                }
                Prim::Insert { .. } => {}
            }
        }
        if self
            .items
            .iter()
            .any(|i| matches!(i.prim, Prim::Insert { .. }))
        {
            let e = self.exploded().bounds();
            if !e.is_empty() {
                b.include(e.min);
                b.include(e.max);
            }
        }
        b
    }
}

/// Mirror, scale, rotate and move (an `Insert`'s placement).
struct Transform {
    at: DVec2,
    rotor: DVec2,
    angle: f64,
    scale: f64,
    mirror: bool,
}

impl Transform {
    fn p(&self, p: DVec2) -> DVec2 {
        let p = if self.mirror {
            DVec2::new(-p.x, p.y)
        } else {
            p
        };
        self.at + self.rotor.rotate(p * self.scale)
    }

    fn prim(&self, prim: &Prim) -> Prim {
        match prim {
            Prim::Line(a, b) => Prim::Line(self.p(*a), self.p(*b)),
            Prim::Polyline { points, closed } => Prim::Polyline {
                points: points.iter().map(|q| self.p(*q)).collect(),
                closed: *closed,
            },
            Prim::Circle { center, radius } => Prim::Circle {
                center: self.p(*center),
                radius: radius * self.scale,
            },
            Prim::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let start = if self.mirror {
                    std::f64::consts::PI - (start + sweep)
                } else {
                    *start
                };
                Prim::Arc {
                    center: self.p(*center),
                    radius: radius * self.scale,
                    start: start + self.angle,
                    sweep: *sweep,
                }
            }
            Prim::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                // Mirroring reverses the direction: parametric angle t becomes −t.
                let start = if self.mirror {
                    -(start + sweep)
                } else {
                    *start
                };
                let c = self.p(*center);
                Prim::Ellipse {
                    center: c,
                    major: self.p(*center + *major) - c,
                    minor: minor * self.scale,
                    start,
                    sweep: *sweep,
                }
            }
            Prim::Bezier { p } => Prim::Bezier {
                p: p.map(|q| self.p(q)),
            },
            Prim::Fill {
                rings,
                triangles,
                mask,
            } => Prim::Fill {
                rings: rings
                    .iter()
                    .map(|r| r.iter().map(|q| self.p(*q)).collect())
                    .collect(),
                triangles: triangles.iter().map(|t| t.map(|q| self.p(q))).collect(),
                mask: *mask,
            },
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
                style,
                font,
            } => Prim::Text {
                at: self.p(*at),
                height: height * self.scale,
                angle: angle + self.angle,
                align: *align,
                text: text.clone(),
                style: *style,
                font: font.clone(),
            },
            Prim::Insert { .. } => prim.clone(),
        }
    }
}

/// How wide a line of text is: in its own font (sketch text), else as the drawing's font
/// sets it (see [`styled_width`]).
pub fn line_width_in(
    text: &str,
    height: f64,
    style: TextStyle,
    font: Option<&drift_core::TextFont>,
) -> f64 {
    match font {
        Some(f) => crate::outline::line_width(text, f, height),
        None => styled_width(text, height, style),
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

/// Helvetica Bold advance widths (per 1000 em) for ASCII 32–126.
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Helvetica's cap height per em: text `height` (cap height) → font size.
pub const CAP_HEIGHT: f64 = 0.718;

/// The width of `text` in Helvetica at cap height `height` (mm).
pub fn text_width(text: &str, height: f64) -> f64 {
    styled_width(text, height, TextStyle::REGULAR)
}

/// The width of `text` at cap height `height` (mm) in a weight and tracking (the tracking
/// after the last letter isn't counted).
pub fn styled_width(text: &str, height: f64, style: TextStyle) -> f64 {
    // In the drawing's own font, where it has one.
    if let Some(font) = crate::font::drawing_font() {
        return font.face(style.bold).width(text, height, style.tracking);
    }
    let size = height / CAP_HEIGHT;
    let table = if style.bold {
        &HELVETICA_BOLD
    } else {
        &HELVETICA
    };
    let units: u32 = text
        .chars()
        .map(|c| match c as u32 {
            32..=126 => u32::from(table[(c as u32 - 32) as usize]),
            0xD8 | 0xF8 => 778, // Ø ø
            0xB1 | 0xD7 => 584, // ± ×
            0xB0 => 400,        // °
            0xB2 | 0xB3 => 333, // ² ³
            _ => 556,
        })
        .sum();
    let gaps = text.chars().count().saturating_sub(1) as f64;
    (f64::from(units) / 1000.0 + gaps * style.tracking) * size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_recolour_strokes_fills_blocks_and_layers() {
        let red = Rgb([255, 0, 0]);
        let blue = Rgb([0, 0, 255]);
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        d.layer(LayerInfo {
            name: "Walls".into(),
            color: red,
            width: 0.35,
            line_type: LineType::Continuous,
        });
        d.line(
            "Walls",
            Pen::new(0.35).with_color(red),
            DVec2::ZERO,
            DVec2::ONE,
        );
        d.filled_rect(
            "Walls",
            Pen::new(0.0).with_color(blue),
            DVec2::ZERO,
            DVec2::ONE,
        );
        d.text(
            "Walls",
            Pen::new(0.25).with_color(blue),
            DVec2::ZERO,
            3.5,
            Align::Left,
            "A",
        );
        d.blocks.push(BlockDef {
            name: "B".into(),
            items: vec![Item {
                layer: "0".into(),
                pen: Pen::new(0.25).with_color(red),
                prim: Prim::Circle {
                    center: DVec2::ZERO,
                    radius: 1.0,
                },
                clip: None,
            }],
        });
        let colours = |d: &Drawing| -> Vec<Rgb> {
            d.items
                .iter()
                .chain(&d.blocks[0].items)
                .map(|i| i.pen.color)
                .chain(d.layers.iter().map(|l| l.color))
                .collect()
        };
        let mut same = d.clone();
        same.set_colours(SheetColours::Colour);
        assert_eq!(same, d);
        let mut grey = d.clone();
        grey.set_colours(SheetColours::Greyscale);
        let (r, b) = (Rgb([76, 76, 76]), Rgb([29, 29, 29]));
        assert_eq!(colours(&grey), [r, b, b, r, r]);
        let mut mono = d.clone();
        mono.set_colours(SheetColours::BlackWhite);
        let k = Pen::BLACK;
        // The fill keeps its grey; the line, the text, the block's circle and the layer
        // are black.
        assert_eq!(colours(&mono), [k, b, k, k, k]);
    }

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
