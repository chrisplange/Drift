//! Dimensions and relation glyphs on the canvas: layout (for hit testing) and painting.
//!
//! Dimension labels are stored as an offset in world units from an anchor on the measured
//! geometry (see [`dimension_anchor`]), so they travel with the geometry. Everything else (gaps,
//! arrow sizes, text) is in screen space so annotations stay readable at any zoom.

use std::collections::HashMap;
use std::f32::consts::TAU as TAU32;

use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::overlay::ScreenMap;

/// Default distance of a dimension label from the geometry, in logical points.
const DEFAULT_OFFSET_PT: f64 = 28.0;
const ARROW_LEN: f32 = 9.0;
const ARROW_HALF_WIDTH: f32 = 3.0;
const EXTENSION_GAP: f32 = 3.0;
const EXTENSION_OVERSHOOT: f32 = 5.0;
const GLYPH_SIZE: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationState {
    Normal,
    Hovered,
    Selected,
    Conflict,
    Preview,
}

pub struct AnnotationColors {
    pub normal: Color32,
    pub hovered: Color32,
    pub selected: Color32,
    pub conflict: Color32,
    pub preview: Color32,
    pub text_background: Color32,
    pub glyph_background: Color32,
}

impl AnnotationColors {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                normal: Color32::from_rgb(0x8f, 0xc9, 0xd9),
                hovered: Color32::from_rgb(0xff, 0xd2, 0x7a),
                selected: Color32::from_rgb(0xf5, 0xa6, 0x23),
                conflict: Color32::from_rgb(0xff, 0x5d, 0x5d),
                preview: Color32::from_rgb(0x5e, 0xd3, 0xa3),
                text_background: Color32::from_rgb(0x1b, 0x1d, 0x21),
                glyph_background: Color32::from_rgb(0x2a, 0x2e, 0x35),
            }
        } else {
            Self {
                normal: Color32::from_rgb(0x1f, 0x6f, 0x86),
                hovered: Color32::from_rgb(0xf0, 0xa8, 0x30),
                selected: Color32::from_rgb(0xe0, 0x86, 0x00),
                conflict: Color32::from_rgb(0xd3, 0x2f, 0x2f),
                preview: Color32::from_rgb(0x13, 0x9a, 0x6a),
                text_background: Color32::from_rgb(0xfb, 0xfb, 0xfa),
                glyph_background: Color32::from_rgb(0xe8, 0xea, 0xee),
            }
        }
    }

    fn color(&self, state: AnnotationState) -> Color32 {
        match state {
            AnnotationState::Normal => self.normal,
            AnnotationState::Hovered => self.hovered,
            AnnotationState::Selected => self.selected,
            AnnotationState::Conflict => self.conflict,
            AnnotationState::Preview => self.preview,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dimensions

/// The world point a dimension's label offset is measured from.
pub fn dimension_anchor(doc: &Document, c: &Constraint) -> Option<DVec2> {
    // Points, or lines standing for their midpoints.
    let p = |id| doc.ref_point(id);
    Some(match *c {
        Constraint::Distance { a, b, .. }
        | Constraint::HorizontalDistance { a, b, .. }
        | Constraint::VerticalDistance { a, b, .. } => (p(a)? + p(b)?) * 0.5,
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point)?;
            (q + foot(doc, q, line)?) * 0.5
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            center_of(doc, curve)?
        }
        Constraint::Angle { a, b, .. } => angle_vertex(doc, a, b)?,
        _ => return None,
    })
}

/// Where a dimension's label goes when the user hasn't placed it.
pub fn default_offset(doc: &Document, c: &Constraint, scale: f64) -> DVec2 {
    let d = DEFAULT_OFFSET_PT / scale;
    // Points, or lines standing for their midpoints.
    let p = |id| doc.ref_point(id);
    let diagonal = DVec2::new(1.0, 1.0).normalize();
    match *c {
        Constraint::Distance { a, b, .. } => match (p(a), p(b)) {
            (Some(pa), Some(pb)) if pa != pb => (pb - pa).normalize().perp() * d,
            _ => DVec2::new(0.0, d),
        },
        Constraint::HorizontalDistance { .. } => DVec2::new(0.0, d),
        Constraint::VerticalDistance { .. } => DVec2::new(d, 0.0),
        Constraint::PointLineDistance { point, line, .. } => {
            match p(point).and_then(|q| foot(doc, q, line).map(|f| (q, f))) {
                Some((q, f)) if q != f => (q - f).normalize().perp() * d,
                _ => DVec2::new(0.0, d),
            }
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            let r = drift_solver::radius_of(doc, curve).unwrap_or(0.0);
            diagonal * (r + d)
        }
        Constraint::Angle { a, b, .. } => {
            let bisector = match (line_dir_from_vertex(doc, a), line_dir_from_vertex(doc, b)) {
                (Some(u1), Some(u2)) if (u1 + u2).length() > 1e-9 => (u1 + u2).normalize(),
                _ => diagonal,
            };
            bisector * d * 2.0
        }
        _ => DVec2::ZERO,
    }
}

fn center_of(doc: &Document, curve: EntityId) -> Option<DVec2> {
    match doc.get(curve)?.kind {
        EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. } => {
            doc.point(center).ok()
        }
        _ => None,
    }
}

fn line_points(doc: &Document, line: EntityId) -> Option<(DVec2, DVec2)> {
    match doc.get(line)?.kind {
        EntityKind::Line { a, b } => Some((doc.point(a).ok()?, doc.point(b).ok()?)),
        _ => None,
    }
}

/// Foot of the perpendicular from `q` onto the (infinite) line.
fn foot(doc: &Document, q: DVec2, line: EntityId) -> Option<DVec2> {
    let (a, b) = line_points(doc, line)?;
    let d = b - a;
    let len2 = d.length_squared();
    (len2 > 0.0).then(|| a + d * ((q - a).dot(d) / len2))
}

/// Intersection of two lines' supporting lines, or the midpoint between them if parallel.
fn angle_vertex(doc: &Document, l1: EntityId, l2: EntityId) -> Option<DVec2> {
    let (a1, b1) = line_points(doc, l1)?;
    let (a2, b2) = line_points(doc, l2)?;
    let (d1, d2) = (b1 - a1, b2 - a2);
    let denom = d1.perp_dot(d2);
    if denom.abs() < 1e-12 * d1.length() * d2.length() {
        return Some((a1 + b1 + a2 + b2) * 0.25);
    }
    let t = (a2 - a1).perp_dot(d2) / denom;
    Some(a1 + d1 * t)
}

/// Unit direction of a line pointing away from its intersection with the other line, toward
/// its farther end point.
fn line_dir_from_vertex(doc: &Document, line: EntityId) -> Option<DVec2> {
    let (a, b) = line_points(doc, line)?;
    let d = b - a;
    (d.length() > 0.0).then(|| d.normalize())
}

#[derive(Clone, Debug)]
pub enum DimShape {
    /// Extension lines from the measured points `p` to the dimension line ends `d`.
    Linear { p: [Pos2; 2], d: [Pos2; 2] },
    /// A leader from `from` to the arrow tip `tip` on the curve.
    Radius { center: Pos2, tip: Pos2, from: Pos2 },
    /// A line across the circle through its centre, arrows at both ends.
    Diameter { ends: [Pos2; 2] },
    /// An arc around `center` from screen angle `start` sweeping `sweep` (radians, y-down).
    Angular {
        center: Pos2,
        radius: f32,
        start: f32,
        sweep: f32,
    },
}

#[derive(Clone, Debug)]
pub struct DimLayout {
    /// `None` for a dimension that doesn't exist yet (a tool preview).
    pub id: Option<ConstraintId>,
    pub constraint: Constraint,
    pub anchor: DVec2,
    pub text: String,
    pub text_pos: Pos2,
    pub text_rect: Rect,
    pub shape: DimShape,
}

/// The text a dimension shows.
pub fn dimension_text(c: &Constraint) -> String {
    let v = c.value().unwrap_or(0.0);
    let n = format_number(v);
    match c {
        Constraint::Radius { .. } => format!("R{n}"),
        Constraint::Diameter { .. } => format!("Ø{n}"),
        Constraint::Angle { .. } => format!("{n}°"),
        _ => n,
    }
}

/// Up to 3 decimals, without trailing zeros.
pub fn format_number(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn dir(a: Pos2, b: Pos2) -> Vec2 {
    let d = b - a;
    if d.length() > 1e-6 {
        d.normalized()
    } else {
        vec2(1.0, 0.0)
    }
}

/// Lay out a dimension. `placement` is the label offset from the anchor (world units).
pub fn layout_dimension(
    painter: &egui::Painter,
    doc: &Document,
    id: Option<ConstraintId>,
    c: &Constraint,
    placement: Option<DVec2>,
    map: &ScreenMap,
) -> Option<DimLayout> {
    let anchor = dimension_anchor(doc, c)?;
    let offset = placement.unwrap_or_else(|| default_offset(doc, c, map.camera.scale));
    let label = map.to_screen(anchor + offset);
    let s = |p: DVec2| map.to_screen(p);
    // Points, or lines standing for their midpoints.
    let p = |id| doc.ref_point(id);

    let linear = |p1: Pos2, p2: Pos2| {
        let u = dir(p1, p2);
        let n = u.rot90();
        let d = (label - p1).dot(n);
        let d1 = p1 + n * d;
        let d2 = p2 + n * d;
        let along = (label - d1).dot(u);
        (
            DimShape::Linear {
                p: [p1, p2],
                d: [d1, d2],
            },
            d1 + u * along,
        )
    };

    let (shape, text_pos) = match *c {
        Constraint::Distance { a, b, .. } => linear(s(p(a)?), s(p(b)?)),
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point)?;
            linear(s(q), s(foot(doc, q, line)?))
        }
        Constraint::HorizontalDistance { a, b, .. } => {
            let (p1, p2) = (s(p(a)?), s(p(b)?));
            let d = [pos2(p1.x, label.y), pos2(p2.x, label.y)];
            (DimShape::Linear { p: [p1, p2], d }, label)
        }
        Constraint::VerticalDistance { a, b, .. } => {
            let (p1, p2) = (s(p(a)?), s(p(b)?));
            let d = [pos2(label.x, p1.y), pos2(label.x, p2.y)];
            (DimShape::Linear { p: [p1, p2], d }, label)
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            let center = s(center_of(doc, curve)?);
            let r = (drift_solver::radius_of(doc, curve)? * map.camera.scale) as f32;
            let v = dir(center, label);
            let shape = if matches!(c, Constraint::Radius { .. }) {
                DimShape::Radius {
                    center,
                    tip: center + v * r,
                    from: if (label - center).length() > r {
                        label
                    } else {
                        center
                    },
                }
            } else {
                DimShape::Diameter {
                    ends: [center - v * r, center + v * r],
                }
            };
            (shape, label)
        }
        Constraint::Angle { a, b, degrees } => {
            let center = s(anchor);
            let u1 = line_dir_from_vertex(doc, a)?;
            line_dir_from_vertex(doc, b)?;
            // Screen angles are y-down, so world counter-clockwise is negative.
            let start = (-u1.y).atan2(u1.x) as f32;
            let sweep = -(degrees.to_radians() as f32);
            let radius = (label - center).length().max(12.0);
            (
                DimShape::Angular {
                    center,
                    radius,
                    start,
                    sweep,
                },
                label,
            )
        }
        _ => return None,
    };
    let text = dimension_text(c);
    let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(13.0), Color32::WHITE);
    let text_rect = Rect::from_center_size(text_pos, galley.size()).expand2(vec2(3.0, 1.0));
    Some(DimLayout {
        id,
        constraint: *c,
        anchor,
        text,
        text_pos,
        text_rect,
        shape,
    })
}

fn arrow(painter: &egui::Painter, tip: Pos2, direction: Vec2, color: Color32) {
    let d = if direction.length() > 1e-6 {
        direction.normalized()
    } else {
        vec2(1.0, 0.0)
    };
    let base = tip - d * ARROW_LEN;
    let n = d.rot90() * ARROW_HALF_WIDTH;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + n, base - n],
        color,
        Stroke::NONE,
    ));
}

/// Draw a dimension laid out by [`layout_dimension`].
pub fn paint_dimension(
    painter: &egui::Painter,
    dim: &DimLayout,
    state: AnnotationState,
    colors: &AnnotationColors,
) {
    let color = colors.color(state);
    let stroke = Stroke::new(1.0, color);
    match &dim.shape {
        DimShape::Linear { p, d } => {
            for i in 0..2 {
                let v = d[i] - p[i];
                if v.length() > EXTENSION_GAP {
                    let u = v.normalized();
                    painter.line_segment(
                        [p[i] + u * EXTENSION_GAP, d[i] + u * EXTENSION_OVERSHOOT],
                        stroke,
                    );
                }
            }
            painter.line_segment([d[0], d[1]], stroke);
            let u = dir(d[0], d[1]);
            let len = (d[1] - d[0]).length();
            let along = (dim.text_pos - d[0]).dot(u);
            // Extend the dimension line to a label placed outside it.
            if along < 0.0 {
                painter.line_segment([d[0], dim.text_pos], stroke);
            } else if along > len {
                painter.line_segment([d[1], dim.text_pos], stroke);
            }
            if len > 2.5 * ARROW_LEN {
                arrow(painter, d[0], -u, color);
                arrow(painter, d[1], u, color);
            } else {
                // Too tight: arrows point inward from outside.
                arrow(painter, d[0], u, color);
                arrow(painter, d[1], -u, color);
            }
        }
        DimShape::Radius { center, tip, from } => {
            painter.line_segment([*from, *tip], stroke);
            if from == center {
                painter.line_segment([*center, *tip], stroke);
            }
            // The arrow points along the leader: inward from an outside label, outward from
            // the centre when the label is inside.
            arrow(painter, *tip, *tip - *from, color);
        }
        DimShape::Diameter { ends } => {
            painter.line_segment([ends[0], ends[1]], stroke);
            let u = dir(ends[0], ends[1]);
            let along = (dim.text_pos - ends[0]).dot(u);
            let len = (ends[1] - ends[0]).length();
            if along < 0.0 {
                painter.line_segment([ends[0], dim.text_pos], stroke);
            } else if along > len {
                painter.line_segment([ends[1], dim.text_pos], stroke);
            }
            arrow(painter, ends[0], -u, color);
            arrow(painter, ends[1], u, color);
        }
        DimShape::Angular {
            center,
            radius,
            start,
            sweep,
        } => {
            let steps = ((sweep.abs() / TAU32) * 64.0).ceil().max(2.0) as usize;
            let points: Vec<Pos2> = (0..=steps)
                .map(|k| {
                    let a = start + sweep * k as f32 / steps as f32;
                    *center + vec2(a.cos(), a.sin()) * *radius
                })
                .collect();
            painter.add(egui::Shape::line(points.clone(), stroke));
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                let tangent = |a: f32| vec2(-a.sin(), a.cos()) * sweep.signum();
                arrow(painter, *first, -tangent(*start), color);
                arrow(painter, *last, tangent(start + sweep), color);
            }
        }
    }
    painter.rect_filled(dim.text_rect, 2.0, colors.text_background);
    if state != AnnotationState::Normal {
        painter.rect_stroke(
            dim.text_rect,
            2.0,
            Stroke::new(1.0, color),
            StrokeKind::Inside,
        );
    }
    painter.text(
        dim.text_pos,
        egui::Align2::CENTER_CENTER,
        &dim.text,
        FontId::proportional(13.0),
        color,
    );
}

// ---------------------------------------------------------------------------------------------
// Relation glyphs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphKind {
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    Fix,
    Coincident,
    Midpoint,
}

#[derive(Clone, Debug)]
pub struct GlyphLayout {
    pub id: ConstraintId,
    pub kind: GlyphKind,
    pub rect: Rect,
}

/// Lay out a glyph for every relation (dimensions are drawn separately). Relations between
/// two curves get a glyph on each. Glyphs on the same entity are stacked.
pub fn layout_glyphs(doc: &Document, map: &ScreenMap) -> Vec<GlyphLayout> {
    let mut out = Vec::new();
    let mut stack: HashMap<EntityId, usize> = HashMap::new();
    let p = |id| doc.point(id).ok().map(|w| map.to_screen(w));
    let mut place = |entity: EntityId, base: Option<(Pos2, Vec2)>, id, kind, out: &mut Vec<_>| {
        let Some((at, along)) = base else { return };
        let k = stack.entry(entity).or_insert(0);
        let center = at + along * (*k as f32) * (GLYPH_SIZE + 2.0);
        *k += 1;
        out.push(GlyphLayout {
            id,
            kind,
            rect: Rect::from_center_size(center, Vec2::splat(GLYPH_SIZE)),
        });
    };
    // Glyphs for a line sit beside its midpoint and stack along it.
    let line_base = |line: EntityId| -> Option<(Pos2, Vec2)> {
        let EntityKind::Line { a, b } = doc.get(line)?.kind else {
            return None;
        };
        let (pa, pb) = (p(a)?, p(b)?);
        let u = dir(pa, pb);
        let mut n = u.rot90();
        if n.y > 0.0 {
            n = -n; // prefer above
        }
        Some((pa + (pb - pa) * 0.5 + n * 13.0, u))
    };
    let point_base = |point: EntityId| p(point).map(|q| (q + vec2(12.0, -12.0), vec2(1.0, 0.0)));

    for (id, c) in doc.constraints() {
        use GlyphKind::*;
        match *c {
            Constraint::Horizontal { line } => {
                place(line, line_base(line), id, Horizontal, &mut out)
            }
            Constraint::Vertical { line } => place(line, line_base(line), id, Vertical, &mut out),
            Constraint::HorizontalPoints { a, b } | Constraint::VerticalPoints { a, b } => {
                let base = p(a)
                    .zip(p(b))
                    .map(|(pa, pb)| (pa + (pb - pa) * 0.5 + vec2(0.0, -12.0), vec2(1.0, 0.0)));
                let kind = if matches!(c, Constraint::HorizontalPoints { .. }) {
                    Horizontal
                } else {
                    Vertical
                };
                place(a, base, id, kind, &mut out);
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Equal { a, b } => {
                let kind = match c {
                    Constraint::Parallel { .. } => Parallel,
                    Constraint::Perpendicular { .. } => Perpendicular,
                    _ => Equal,
                };
                for e in [a, b] {
                    let base = match doc.get(e).map(|x| x.kind) {
                        Some(EntityKind::Line { .. }) => line_base(e),
                        _ => center_of(doc, e).map(|w| {
                            let r =
                                drift_solver::radius_of(doc, e).unwrap_or(0.0) * map.camera.scale;
                            (
                                map.to_screen(w)
                                    + vec2(r as f32 * 0.7 + 12.0, -(r as f32) * 0.7 - 12.0),
                                vec2(1.0, 0.0),
                            )
                        }),
                    };
                    place(e, base, id, kind, &mut out);
                }
            }
            Constraint::Fix { point, .. } => place(point, point_base(point), id, Fix, &mut out),
            Constraint::FixRadius { curve, .. } => {
                let base = center_of(doc, curve)
                    .map(|w| (map.to_screen(w) + vec2(12.0, 12.0), vec2(1.0, 0.0)));
                place(curve, base, id, Fix, &mut out);
            }
            Constraint::Coincident { a, .. } => place(a, point_base(a), id, Coincident, &mut out),
            Constraint::Midpoint { point, .. } => {
                place(point, point_base(point), id, Midpoint, &mut out)
            }
            _ => {}
        }
    }
    out
}

/// Draw a relation glyph: a small badge with a vector icon.
pub fn paint_glyph(
    painter: &egui::Painter,
    rect: Rect,
    kind: GlyphKind,
    state: AnnotationState,
    colors: &AnnotationColors,
) {
    let color = colors.color(state);
    painter.rect_filled(rect, 3.0, colors.glyph_background);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, color.gamma_multiply(0.7)),
        StrokeKind::Inside,
    );
    let c = rect.center();
    let s = Stroke::new(1.4, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([c + vec2(a.0, a.1), c + vec2(b.0, b.1)], s);
    };
    match kind {
        GlyphKind::Horizontal => line((-4.5, 0.0), (4.5, 0.0)),
        GlyphKind::Vertical => line((0.0, -4.5), (0.0, 4.5)),
        GlyphKind::Parallel => {
            line((-4.0, 3.5), (-0.5, -3.5));
            line((0.5, 3.5), (4.0, -3.5));
        }
        GlyphKind::Perpendicular => {
            line((0.0, -4.5), (0.0, 3.5));
            line((-4.5, 3.5), (4.5, 3.5));
        }
        GlyphKind::Equal => {
            line((-4.0, -2.0), (4.0, -2.0));
            line((-4.0, 2.0), (4.0, 2.0));
        }
        GlyphKind::Fix => {
            line((0.0, -4.5), (0.0, 0.0));
            line((-4.5, 0.0), (4.5, 0.0));
            line((-3.0, 2.2), (3.0, 2.2));
            line((-1.5, 4.4), (1.5, 4.4));
        }
        GlyphKind::Coincident => {
            painter.circle_stroke(c, 3.5, s);
            painter.circle_filled(c, 1.2, color);
        }
        GlyphKind::Midpoint => {
            line((-5.0, 3.0), (5.0, 3.0));
            painter.add(egui::Shape::convex_polygon(
                vec![c + vec2(0.0, -3.5), c + vec2(3.2, 2.0), c + vec2(-3.2, 2.0)],
                color,
                Stroke::NONE,
            ));
        }
    }
}

/// A line's midpoint marker: a small triangle, filled when picked.
pub fn paint_midpoint_marker(painter: &egui::Painter, at: Pos2, color: Color32, filled: bool) {
    let pts = vec![
        at + vec2(0.0, -6.0),
        at + vec2(5.5, 4.0),
        at + vec2(-5.5, 4.0),
    ];
    if filled {
        painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
    } else {
        painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, color)));
    }
}

/// A small badge naming an automatic relation next to the cursor while drawing.
pub fn paint_auto_relation(
    painter: &egui::Painter,
    at: Pos2,
    kind: GlyphKind,
    colors: &AnnotationColors,
) {
    let rect = Rect::from_center_size(at, Vec2::splat(GLYPH_SIZE));
    paint_glyph(painter, rect, kind, AnnotationState::Preview, colors);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_trimmed() {
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(12.34567), "12.346");
    }

    #[test]
    fn dimension_texts() {
        let e = EntityId(1);
        assert_eq!(
            dimension_text(&Constraint::Diameter {
                curve: e,
                value: 25.0
            }),
            "Ø25"
        );
        assert_eq!(
            dimension_text(&Constraint::Radius {
                curve: e,
                value: 2.5
            }),
            "R2.5"
        );
        assert_eq!(
            dimension_text(&Constraint::Angle {
                a: e,
                b: e,
                degrees: 30.0
            }),
            "30°"
        );
    }

    #[test]
    fn anchors_follow_geometry() {
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::new(0.0, 0.0));
        let b = doc.add_point(DVec2::new(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let q = doc.add_point(DVec2::new(4.0, 6.0));
        let dist = Constraint::Distance { a, b, value: 10.0 };
        assert_eq!(dimension_anchor(&doc, &dist), Some(DVec2::new(5.0, 0.0)));
        let pl = Constraint::PointLineDistance {
            point: q,
            line,
            value: 6.0,
        };
        assert_eq!(dimension_anchor(&doc, &pl), Some(DVec2::new(4.0, 3.0)));
        // Default offset for a horizontal line's length is straight up (left normal).
        let off = default_offset(&doc, &dist, 2.0);
        assert!(off.x.abs() < 1e-12 && off.y > 0.0);
    }
}
