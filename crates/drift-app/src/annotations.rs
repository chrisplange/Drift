//! Dimensions and relation glyphs on the canvas: layout (for hit testing) and painting.
//!
//! Dimension labels are stored as an offset in world units from an anchor on the measured
//! geometry (see [`dimension_anchor`]), so they travel with the geometry. Everything else (gaps,
//! arrow sizes, text) is in screen space so annotations stay readable at any zoom.

use std::collections::HashMap;
use std::f32::consts::TAU as TAU32;

use drift_core::{
    ArrowStyle, Constraint, ConstraintId, DVec2, Document, DraftStyle, EntityId, EntityKind,
    Tolerance, Units,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::overlay::ScreenMap;

/// Default distance of a dimension label from the geometry, in logical points.
const DEFAULT_OFFSET_PT: f64 = 28.0;
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
    /// Driven (reference) dimensions.
    pub driven: Color32,
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
                driven: Color32::from_rgb(0x8a, 0x90, 0x99),
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
                driven: Color32::from_rgb(0x7a, 0x80, 0x88),
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

/// Logical points per millimetre on paper (96 dpi), for drafting sizes on screen.
pub const PT_PER_MM: f32 = 96.0 / 25.4;

/// Dimension drawing sizes on screen, from the document's drafting style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimStyle {
    pub text: f32,
    pub arrow: ArrowStyle,
    pub arrow_len: f32,
    pub gap: f32,
    pub overshoot: f32,
    /// ISO: linear dimension text sits above the line, parallel to it.
    pub aligned: bool,
}

impl DimStyle {
    pub fn of(style: &DraftStyle) -> Self {
        let pt = |mm: f64| (mm as f32 * PT_PER_MM).clamp(0.0, 60.0);
        Self {
            text: pt(style.text_height).clamp(8.0, 40.0),
            arrow: style.arrow,
            arrow_len: pt(style.arrow_size).max(3.0),
            gap: pt(style.extension_gap),
            overshoot: pt(style.extension_overshoot),
            aligned: style.aligned_text,
        }
    }
}

impl Default for DimStyle {
    fn default() -> Self {
        Self::of(&DraftStyle::default())
    }
}

/// A dimension's text: the main part, an optional stacked pair (deviations or limits) and a
/// trailing part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimText {
    pub before: String,
    pub stack: Option<(String, String)>,
    pub after: String,
}

impl DimText {
    /// The text on one line (stacked parts as `upper/lower`).
    #[cfg(test)]
    pub fn plain(&self) -> String {
        match &self.stack {
            Some((u, l)) => format!("{} {u}/{l}{}", self.before, self.after)
                .trim()
                .to_owned(),
            None => format!("{}{}", self.before, self.after),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DimLayout {
    /// `None` for a dimension that doesn't exist yet (a tool preview).
    pub id: Option<ConstraintId>,
    pub constraint: Constraint,
    pub anchor: DVec2,
    pub label: DimText,
    /// The label's centre.
    pub text_pos: Pos2,
    /// The label's bounds on screen (of the rotated text, for aligned labels).
    pub text_rect: Rect,
    /// The label's rotation (radians, screen y-down): 0 for horizontal text.
    pub angle: f32,
    pub shape: DimShape,
    /// A driven (reference) dimension: drawn grey, its value in parentheses.
    pub driven: bool,
    pub style: DimStyle,
}

/// The text a dimension shows, in the document's units. A value the display rounds gets a
/// `~` prefix, so a rounded label is never mistaken for the exact value.
#[cfg(test)]
pub fn dimension_text(c: &Constraint, units: &Units) -> String {
    dimension_text_styled(c, units, &DraftStyle::default())
}

fn dimension_text_styled(c: &Constraint, units: &Units, style: &DraftStyle) -> String {
    let v = c.value().unwrap_or(0.0);
    let (n, exact) = match c {
        Constraint::Angle { .. } => {
            let scale = 10f64.powi(i32::from(units.angle_decimals));
            (units.format_angle(v), (v * scale).round() / scale == v)
        }
        _ => (style.length(units, v), units.is_exact(v)),
    };
    let n = if exact { n } else { format!("~{n}") };
    match c {
        Constraint::Radius { .. } => format!("R{n}"),
        Constraint::Diameter { .. } => format!("Ø{n}"),
        _ => n,
    }
}

/// The full label of a dimension: value (measured, in parentheses, if driven; marked Σ if
/// linked to an expression), prefix, suffix, tolerance and alternate units.
pub fn dimension_label(doc: &Document, id: Option<ConstraintId>, c: &Constraint) -> DimText {
    let units = doc.units();
    let style = doc.draft_style();
    let driven = id.is_some_and(|id| doc.is_driven(id));
    let shown = if driven {
        drift_solver::measure(doc, c)
            .map(|v| c.with_value(v))
            .unwrap_or(*c)
    } else {
        *c
    };
    let value = shown.value().unwrap_or(0.0);
    let angle = matches!(c, Constraint::Angle { .. });
    let fmt = |v: f64| {
        if angle {
            units.format_angle(v)
        } else {
            style.length(&units, v)
        }
    };
    let signed = |v: f64| {
        if v == 0.0 {
            "0".to_owned()
        } else if v > 0.0 {
            format!("+{}", fmt(v))
        } else {
            format!("−{}", fmt(-v))
        }
    };
    let main = dimension_text_styled(&shown, &units, &style);
    let note = id
        .and_then(|id| doc.dim_note(id))
        .cloned()
        .unwrap_or_default();
    let prefix = if note.prefix.is_empty() {
        String::new()
    } else {
        format!("{} ", note.prefix)
    };
    let suffix = if note.suffix.is_empty() {
        String::new()
    } else {
        format!(" {}", note.suffix)
    };
    let alt = if angle {
        None
    } else {
        style.alternate(&units, value).map(|a| format!(" {a}"))
    };
    let alt = alt.unwrap_or_default();
    let mark = if !driven && id.is_some_and(|id| doc.expression(id).is_some()) {
        // Linked to an expression (as SolidWorks marks equation-driven dimensions).
        "Σ"
    } else {
        ""
    };
    let mut text = match &note.tolerance {
        Tolerance::None => DimText {
            before: format!("{mark}{prefix}{main}{suffix}{alt}"),
            ..Default::default()
        },
        Tolerance::Symmetric(t) => DimText {
            before: format!("{mark}{prefix}{main} ±{}{suffix}{alt}", fmt(t.abs())),
            ..Default::default()
        },
        Tolerance::Fit(f) => DimText {
            before: format!("{mark}{prefix}{main} {f}{suffix}{alt}"),
            ..Default::default()
        },
        Tolerance::Deviation { upper, lower } => DimText {
            before: format!("{mark}{prefix}{main}"),
            stack: Some((signed(*upper), signed(-*lower))),
            after: format!("{suffix}{alt}"),
        },
        Tolerance::Limits { upper, lower } => {
            let sign = match c {
                Constraint::Radius { .. } => "R",
                Constraint::Diameter { .. } => "Ø",
                _ => "",
            };
            DimText {
                before: format!("{mark}{prefix}{sign}"),
                stack: Some((fmt(value + upper), fmt(value - lower))),
                after: format!("{suffix}{alt}"),
            }
        }
    };
    if driven {
        text.before = format!("({}", text.before);
        text.after = format!("{})", text.after);
    }
    text
}

fn dir(a: Pos2, b: Pos2) -> Vec2 {
    let d = b - a;
    if d.length() > 1e-6 {
        d.normalized()
    } else {
        vec2(1.0, 0.0)
    }
}

/// Galleys of a dimension label, laid out in its own (unrotated) frame.
struct LabelGalleys {
    before: std::sync::Arc<egui::Galley>,
    stack: Option<[std::sync::Arc<egui::Galley>; 2]>,
    after: std::sync::Arc<egui::Galley>,
    size: Vec2,
}

fn label_galleys(
    painter: &egui::Painter,
    text: &DimText,
    font: f32,
    color: Color32,
) -> LabelGalleys {
    let main = FontId::proportional(font);
    let small = FontId::proportional(font * 0.72);
    let before = painter.layout_no_wrap(text.before.clone(), main.clone(), color);
    let after = painter.layout_no_wrap(text.after.clone(), main, color);
    let stack = text.stack.as_ref().map(|(u, l)| {
        [
            painter.layout_no_wrap(u.clone(), small.clone(), color),
            painter.layout_no_wrap(l.clone(), small.clone(), color),
        ]
    });
    let stack_w = stack
        .as_ref()
        .map_or(0.0, |[u, l]| u.size().x.max(l.size().x) + 3.0);
    let height = before
        .size()
        .y
        .max(stack.as_ref().map_or(0.0, |[u, l]| u.size().y + l.size().y));
    let size = vec2(before.size().x + stack_w + after.size().x, height);
    LabelGalleys {
        before,
        stack,
        after,
        size,
    }
}

/// The screen bounds of a `size` box centred at `center`, rotated by `angle`.
fn rotated_bounds(center: Pos2, size: Vec2, angle: f32) -> Rect {
    let rot = egui::emath::Rot2::from_angle(angle);
    let h = size / 2.0;
    let corners = [
        vec2(-h.x, -h.y),
        vec2(h.x, -h.y),
        vec2(h.x, h.y),
        vec2(-h.x, h.y),
    ];
    let mut r = Rect::NOTHING;
    for c in corners {
        r.extend_with(center + rot * c);
    }
    r
}

/// Where dimension and annotation graphics go: the screen (an egui painter), or a
/// [`Recorder`] that keeps them for output (PDF, SVG, DXF).
pub trait Pen {
    /// For laying out text.
    fn painter(&self) -> &egui::Painter;
    fn line(&self, points: &[Pos2], stroke: Stroke);
    /// A filled convex polygon (arrowheads).
    fn polygon(&self, points: Vec<Pos2>, color: Color32);
    fn dot(&self, at: Pos2, radius: f32, color: Color32);
    /// A paper-coloured patch behind text.
    fn mask(&self, points: Vec<Pos2>, color: Color32);
    /// Text laid out in `galley`, its top-left corner at `top_left`, rotated by `angle`.
    fn text(
        &self,
        top_left: Pos2,
        galley: std::sync::Arc<egui::Galley>,
        angle: f32,
        color: Color32,
    );
    /// A picked or hovered outline (screen only).
    fn highlight(&self, rect: Rect, color: Color32);
}

impl Pen for egui::Painter {
    fn painter(&self) -> &egui::Painter {
        self
    }

    fn line(&self, points: &[Pos2], stroke: Stroke) {
        if let [a, b] = points {
            self.line_segment([*a, *b], stroke);
        } else {
            self.add(egui::Shape::line(points.to_vec(), stroke));
        }
    }

    fn polygon(&self, points: Vec<Pos2>, color: Color32) {
        self.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }

    fn dot(&self, at: Pos2, radius: f32, color: Color32) {
        self.circle_filled(at, radius, color);
    }

    fn mask(&self, points: Vec<Pos2>, color: Color32) {
        self.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    }

    fn text(
        &self,
        top_left: Pos2,
        galley: std::sync::Arc<egui::Galley>,
        angle: f32,
        color: Color32,
    ) {
        self.add(egui::epaint::TextShape::new(top_left, galley, color).with_angle(angle));
    }

    fn highlight(&self, rect: Rect, color: Color32) {
        self.rect_stroke(rect, 2.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
}

/// Graphics recorded by a [`Recorder`], in its screen space.
#[derive(Clone, Debug, PartialEq)]
pub enum Recorded {
    Line(Vec<Pos2>, f32),
    Polygon(Vec<Pos2>),
    Mask(Vec<Pos2>),
    Dot(Pos2, f32),
    /// One line of text: the left end of its baseline, rotation and font size (multi-line
    /// notes give one per line).
    Text {
        baseline: Pos2,
        angle: f32,
        size: f32,
        text: String,
    },
}

/// A [`Pen`] that keeps what is drawn instead of showing it.
pub struct Recorder<'a> {
    painter: &'a egui::Painter,
    pub items: std::cell::RefCell<Vec<Recorded>>,
}

impl<'a> Recorder<'a> {
    pub fn new(painter: &'a egui::Painter) -> Self {
        Self {
            painter,
            items: Default::default(),
        }
    }

    pub fn take(self) -> Vec<Recorded> {
        self.items.into_inner()
    }
}

impl Pen for Recorder<'_> {
    fn painter(&self) -> &egui::Painter {
        self.painter
    }

    fn line(&self, points: &[Pos2], stroke: Stroke) {
        self.items
            .borrow_mut()
            .push(Recorded::Line(points.to_vec(), stroke.width));
    }

    fn polygon(&self, points: Vec<Pos2>, _color: Color32) {
        self.items.borrow_mut().push(Recorded::Polygon(points));
    }

    fn dot(&self, at: Pos2, radius: f32, _color: Color32) {
        self.items.borrow_mut().push(Recorded::Dot(at, radius));
    }

    fn mask(&self, points: Vec<Pos2>, _color: Color32) {
        self.items.borrow_mut().push(Recorded::Mask(points));
    }

    fn text(
        &self,
        top_left: Pos2,
        galley: std::sync::Arc<egui::Galley>,
        angle: f32,
        _color: Color32,
    ) {
        let size = galley
            .job
            .sections
            .first()
            .map_or(12.0, |s| s.format.font_id.size);
        let rot = egui::emath::Rot2::from_angle(angle);
        let mut items = self.items.borrow_mut();
        for placed in &galley.rows {
            let Some(first) = placed.row.glyphs.first() else {
                continue;
            };
            let text: String = placed.row.glyphs.iter().map(|g| g.chr).collect();
            let local = placed.pos.to_vec2() + vec2(first.pos.x, first.pos.y);
            items.push(Recorded::Text {
                baseline: top_left + rot * local,
                angle,
                size,
                text,
            });
        }
    }

    fn highlight(&self, _rect: Rect, _color: Color32) {}
}

/// Draw a label laid out by [`label_galleys`], centred at `center` and rotated by `angle`.
fn paint_label(
    pen: &dyn Pen,
    center: Pos2,
    angle: f32,
    g: &LabelGalleys,
    color: Color32,
    background: Option<Color32>,
) {
    let rot = egui::emath::Rot2::from_angle(angle);
    let origin = -g.size / 2.0;
    if let Some(bg) = background {
        let pad = vec2(3.0, 1.0);
        let (a, b) = (origin - pad, -origin + pad);
        let corners = [
            vec2(a.x, a.y),
            vec2(b.x, a.y),
            vec2(b.x, b.y),
            vec2(a.x, b.y),
        ]
        .map(|c| center + rot * c)
        .to_vec();
        pen.mask(corners, bg);
    }
    let place = |galley: &std::sync::Arc<egui::Galley>, local: Vec2| {
        let pos = center + rot * (origin + local);
        pen.text(pos, galley.clone(), angle, color);
    };
    let mid = g.size.y / 2.0;
    place(&g.before, vec2(0.0, mid - g.before.size().y / 2.0));
    let mut x = g.before.size().x;
    if let Some([u, l]) = &g.stack {
        let top = mid - (u.size().y + l.size().y) / 2.0;
        place(u, vec2(x + 2.0, top));
        place(l, vec2(x + 2.0, top + u.size().y));
        x += u.size().x.max(l.size().x) + 3.0;
    }
    place(&g.after, vec2(x, mid - g.after.size().y / 2.0));
}

/// Readable text direction along `u`: never upside down (vertical reads bottom to top).
fn reading_angle(u: Vec2) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let mut a = u.y.atan2(u.x);
    if a > FRAC_PI_2 - 1e-4 {
        a -= PI;
    } else if a < -FRAC_PI_2 - 1e-4 {
        a += PI;
    }
    a
}

/// Lay out a dimension. `placement` is the label offset from the anchor (world units). A
/// driven dimension shows the geometry's current measurement in parentheses.
pub fn layout_dimension(
    painter: &egui::Painter,
    doc: &Document,
    id: Option<ConstraintId>,
    c: &Constraint,
    placement: Option<DVec2>,
    map: &ScreenMap,
) -> Option<DimLayout> {
    let style = DimStyle::of(&doc.draft_style());
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
    let driven = id.is_some_and(|id| doc.is_driven(id));
    let text = dimension_label(doc, id, c);
    let galleys = label_galleys(painter, &text, style.text, Color32::WHITE);
    // ISO: linear dimension text above the line and parallel to it.
    let (text_pos, angle) = match &shape {
        DimShape::Linear { d, .. } if style.aligned => {
            let angle = reading_angle(dir(d[0], d[1]));
            let up = egui::emath::Rot2::from_angle(angle) * vec2(0.0, -1.0);
            (text_pos + up * (galleys.size.y / 2.0 + 1.5), angle)
        }
        _ => (text_pos, 0.0),
    };
    let text_rect = rotated_bounds(text_pos, galleys.size, angle).expand2(vec2(3.0, 1.0));
    Some(DimLayout {
        id,
        constraint: *c,
        anchor,
        label: text,
        text_pos,
        text_rect,
        angle,
        shape,
        driven,
        style,
    })
}

/// An arrowhead (or tick / dot) at `tip`, pointing along `direction`.
fn arrow(pen: &dyn Pen, tip: Pos2, direction: Vec2, color: Color32, style: &DimStyle) {
    let d = if direction.length() > 1e-6 {
        direction.normalized()
    } else {
        vec2(1.0, 0.0)
    };
    let len = style.arrow_len;
    let base = tip - d * len;
    let n = d.rot90() * (len / 3.0);
    match style.arrow {
        ArrowStyle::Closed => {
            pen.polygon(vec![tip, base + n, base - n], color);
        }
        ArrowStyle::Open => {
            let stroke = Stroke::new(1.0, color);
            pen.line(&[tip, base + n], stroke);
            pen.line(&[tip, base - n], stroke);
        }
        ArrowStyle::Tick => {
            // An oblique stroke through the end, at 45° to the line.
            let t = (d + d.rot90()).normalized() * (len * 0.5);
            pen.line(&[tip - t, tip + t], Stroke::new(1.6, color));
        }
        ArrowStyle::Dot => {
            pen.dot(tip, (len / 4.0).max(1.5), color);
        }
    }
}

/// Draw a dimension laid out by [`layout_dimension`].
pub fn paint_dimension(
    pen: &dyn Pen,
    dim: &DimLayout,
    state: AnnotationState,
    colors: &AnnotationColors,
) {
    let color = if dim.driven && state == AnnotationState::Normal {
        colors.driven
    } else {
        colors.color(state)
    };
    let style = &dim.style;
    let stroke = Stroke::new(1.0, color);
    match &dim.shape {
        DimShape::Linear { p, d } => {
            for i in 0..2 {
                let v = d[i] - p[i];
                if v.length() > style.gap {
                    let u = v.normalized();
                    pen.line(&[p[i] + u * style.gap, d[i] + u * style.overshoot], stroke);
                }
            }
            pen.line(&[d[0], d[1]], stroke);
            let u = dir(d[0], d[1]);
            let len = (d[1] - d[0]).length();
            // Where the label sits along the line (for aligned text, its foot on the line).
            let foot = d[0] + u * (dim.text_pos - d[0]).dot(u);
            let along = (foot - d[0]).dot(u);
            // Extend the dimension line to a label placed outside it.
            if along < 0.0 {
                pen.line(&[d[0], foot], stroke);
            } else if along > len {
                pen.line(&[d[1], foot], stroke);
            }
            if len > 2.5 * style.arrow_len || style.arrow == ArrowStyle::Tick {
                arrow(pen, d[0], -u, color, style);
                arrow(pen, d[1], u, color, style);
            } else {
                // Too tight: arrows point inward from outside.
                arrow(pen, d[0], u, color, style);
                arrow(pen, d[1], -u, color, style);
            }
        }
        DimShape::Radius { center, tip, from } => {
            pen.line(&[*from, *tip], stroke);
            if from == center {
                pen.line(&[*center, *tip], stroke);
            }
            // The arrow points along the leader: inward from an outside label, outward from
            // the centre when the label is inside.
            arrow(pen, *tip, *tip - *from, color, style);
        }
        DimShape::Diameter { ends } => {
            pen.line(&[ends[0], ends[1]], stroke);
            let u = dir(ends[0], ends[1]);
            let along = (dim.text_pos - ends[0]).dot(u);
            let len = (ends[1] - ends[0]).length();
            if along < 0.0 {
                pen.line(&[ends[0], dim.text_pos], stroke);
            } else if along > len {
                pen.line(&[ends[1], dim.text_pos], stroke);
            }
            arrow(pen, ends[0], -u, color, style);
            arrow(pen, ends[1], u, color, style);
        }
        DimShape::Angular {
            center,
            radius,
            start,
            sweep,
        } => {
            let steps = ((sweep.abs() / TAU32) * 64.0).ceil().clamp(2.0, 256.0) as usize;
            let points: Vec<Pos2> = (0..=steps)
                .map(|k| {
                    let a = start + sweep * k as f32 / steps as f32;
                    *center + vec2(a.cos(), a.sin()) * *radius
                })
                .collect();
            pen.line(&points, stroke);
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                let tangent = |a: f32| vec2(-a.sin(), a.cos()) * sweep.signum();
                arrow(pen, *first, -tangent(*start), color, style);
                arrow(pen, *last, tangent(start + sweep), color, style);
            }
        }
    }
    let galleys = label_galleys(pen.painter(), &dim.label, style.text, color);
    paint_label(
        pen,
        dim.text_pos,
        dim.angle,
        &galleys,
        color,
        Some(colors.text_background),
    );
    if state != AnnotationState::Normal {
        pen.highlight(dim.text_rect, color);
    }
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
    Tangent,
    Concentric,
    Coradial,
    Collinear,
    OnCurve,
    Symmetric,
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
            | Constraint::Equal { a, b }
            | Constraint::Tangent { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::Coradial { a, b }
            | Constraint::Collinear { a, b } => {
                let kind = match c {
                    Constraint::Parallel { .. } => Parallel,
                    Constraint::Perpendicular { .. } => Perpendicular,
                    Constraint::Tangent { .. } => Tangent,
                    Constraint::Concentric { .. } => Concentric,
                    Constraint::Coradial { .. } => Coradial,
                    Constraint::Collinear { .. } => Collinear,
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
            Constraint::PointOnCurve { point, .. } => {
                place(point, point_base(point), id, OnCurve, &mut out)
            }
            Constraint::Symmetric { axis, .. } => {
                place(axis, line_base(axis), id, Symmetric, &mut out)
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
    // Quiet by default: a soft tile without a border; framed when hovered or picked.
    painter.rect_filled(rect, 4.0, colors.glyph_background.gamma_multiply(0.9));
    if state != AnnotationState::Normal {
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
    let c = rect.center();
    let s = Stroke::new(
        1.3,
        if state == AnnotationState::Normal {
            color.gamma_multiply(0.85)
        } else {
            color
        },
    );
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
        GlyphKind::Tangent => {
            painter.circle_stroke(c + vec2(0.0, 1.2), 3.3, s);
            line((-5.0, -2.1), (5.0, -2.1));
        }
        GlyphKind::Concentric => {
            painter.circle_stroke(c, 4.6, s);
            painter.circle_stroke(c, 2.0, s);
        }
        GlyphKind::Coradial => {
            painter.circle_stroke(c, 4.6, s);
            line((0.0, 0.0), (3.3, -3.3));
            painter.circle_filled(c, 1.1, color);
        }
        GlyphKind::Collinear => {
            line((-5.0, 3.5), (-1.2, 0.8));
            line((1.2, -0.8), (5.0, -3.5));
        }
        GlyphKind::OnCurve => {
            let arc: Vec<Pos2> = (0..=8)
                .map(|k| {
                    let a = (200.0 + 140.0 * k as f32 / 8.0).to_radians();
                    c + vec2(0.0, 5.5) + vec2(a.cos(), a.sin()) * 6.0
                })
                .collect();
            painter.add(egui::Shape::line(arc, s));
            painter.circle_filled(c + vec2(0.0, -0.5), 1.8, color);
        }
        GlyphKind::Symmetric => {
            line((0.0, -5.0), (0.0, 5.0));
            line((-4.5, -2.5), (-4.5, 2.5));
            line((4.5, -2.5), (4.5, 2.5));
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
    fn dimension_texts() {
        let e = EntityId(1);
        let mm = Units::default();
        let text = |c: Constraint| dimension_text(&c, &mm);
        let distance = |value| Constraint::Distance { a: e, b: e, value };
        assert_eq!(text(distance(42.0)), "42");
        // Round-off is not rounding.
        assert_eq!(text(distance(0.1 + 0.2)), "0.3");
        assert_eq!(text(distance(12.34567)), "~12.346");
        let arch = Units::architectural();
        assert_eq!(dimension_text(&distance(18.5 * 25.4), &arch), "1'-6 1/2\"");
        assert_eq!(dimension_text(&distance(100.0), &arch), "~3 15/16\"");
        assert_eq!(
            text(Constraint::Diameter {
                curve: e,
                value: 25.0
            }),
            "Ø25"
        );
        assert_eq!(
            text(Constraint::Radius {
                curve: e,
                value: 2.5
            }),
            "R2.5"
        );
        assert_eq!(
            text(Constraint::Angle {
                a: e,
                b: e,
                degrees: 30.0
            }),
            "30°"
        );
    }

    #[test]
    fn labels_carry_notes_tolerances_and_alternate_units() {
        use drift_core::{DimNote, LengthFormat};
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(25.0, 0.0));
        let c = doc.add_point(DVec2::new(0.0, 10.0));
        let circle = doc.add_circle(c, 3.0).unwrap();
        let len = doc
            .add_constraint(Constraint::Distance { a, b, value: 25.0 })
            .unwrap();
        let dia = doc
            .add_constraint(Constraint::Diameter {
                curve: circle,
                value: 6.0,
            })
            .unwrap();
        let label = |doc: &Document, id| {
            let c = *doc.constraint(id).unwrap();
            dimension_label(doc, Some(id), &c)
        };
        assert_eq!(label(&doc, len).plain(), "25");
        let note = |prefix: &str, suffix: &str, tolerance| DimNote {
            prefix: prefix.into(),
            suffix: suffix.into(),
            tolerance,
        };
        doc.set_dim_note(dia, Some(note("4×", "THRU", Tolerance::None)));
        assert_eq!(label(&doc, dia).plain(), "4× Ø6 THRU");
        doc.set_dim_note(len, Some(note("", "", Tolerance::Symmetric(0.1))));
        assert_eq!(label(&doc, len).plain(), "25 ±0.1");
        doc.set_dim_note(len, Some(note("", "", Tolerance::Fit("H7".into()))));
        assert_eq!(label(&doc, len).plain(), "25 H7");
        doc.set_dim_note(
            len,
            Some(note(
                "",
                "",
                Tolerance::Deviation {
                    upper: 0.1,
                    lower: 0.05,
                },
            )),
        );
        let t = label(&doc, len);
        assert_eq!(t.before, "25");
        assert_eq!(t.stack, Some(("+0.1".into(), "−0.05".into())));
        doc.set_dim_note(
            len,
            Some(note(
                "",
                "",
                Tolerance::Limits {
                    upper: 0.2,
                    lower: 0.1,
                },
            )),
        );
        assert_eq!(label(&doc, len).stack, Some(("25.2".into(), "24.9".into())));
        doc.set_dim_note(len, None);
        let mut style = doc.draft_style();
        style.alt_units = Some(LengthFormat::Inch);
        doc.set_draft_style(style);
        assert_eq!(label(&doc, len).plain(), "25 [0.984\"]");
        doc.set_driven(len, true).unwrap();
        assert_eq!(label(&doc, len).plain(), "(25 [0.984\"])");
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
