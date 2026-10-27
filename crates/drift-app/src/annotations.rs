//! Dimensions and relation glyphs on the canvas: layout (for hit testing) and painting.
//!
//! Dimension labels are stored as an offset in world units from an anchor on the measured
//! geometry (see [`dimension_anchor`]), so they travel with the geometry. Everything else (gaps,
//! arrow sizes, text) is in screen space so annotations stay readable at any zoom.

use std::collections::HashMap;
use std::f32::consts::TAU as TAU32;

use drift_core::{
    ArrowStyle, Constraint, ConstraintId, DVec2, Document, DraftStyle, EntityId, EntityKind,
    LineKind, Tolerance, Units,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::overlay::ScreenMap;

/// Default distance of a dimension label from the geometry, in logical points.
const DEFAULT_OFFSET_PT: f64 = 28.0;
/// The least a linear dimension's line stands off what it measures, on screen (points): it
/// keeps to the side it was placed on, however close it is dragged.
pub const RELIEF_PT: f32 = 12.0;
/// How far around a dimension's text a click still lands on it (points).
pub const TEXT_REACH: f32 = 6.0;
const GLYPH_SIZE: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationState {
    Normal,
    Hovered,
    Selected,
    Conflict,
    Preview,
}

#[derive(Clone, Copy)]
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

    /// The colours dimensions and notes are drawn in: the drawing's own colour if it has one
    /// that shows against the canvas, otherwise white on the dark canvas and charcoal on the
    /// light one. (Relation glyphs keep the theme's colours.)
    pub fn drafting(dark: bool, color: Option<drift_core::Rgb>) -> Self {
        let theme = if dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(0x33, 0x36, 0x3a)
        };
        let normal = color
            .and_then(|c| crate::editor::contrast(c.0, dark))
            .map_or(theme, |[r, g, b]| Color32::from_rgb(r, g, b));
        Self {
            normal,
            ..Self::new(dark)
        }
    }

    /// The same colours, faded: for dimensions and annotations hidden on the canvas, while
    /// hidden ones are shown.
    pub fn faded(&self) -> Self {
        let f = |c: Color32| c.gamma_multiply(0.4);
        Self {
            normal: f(self.normal),
            hovered: f(self.hovered),
            selected: f(self.selected),
            conflict: f(self.conflict),
            preview: f(self.preview),
            driven: f(self.driven),
            ..*self
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

/// A label offset for a newly placed dimension: as far from the geometry as the pointer is,
/// with the number in the middle of the dimension line. (Dragging the label afterwards moves
/// it along the line too.)
pub fn centred_offset(doc: &Document, c: &Constraint, offset: DVec2) -> DVec2 {
    let p = |id| doc.ref_point(id);
    let across = |a: Option<DVec2>, b: Option<DVec2>| match (a, b) {
        (Some(a), Some(b)) if a != b => {
            let n = (b - a).normalize().perp();
            n * offset.dot(n)
        }
        _ => offset,
    };
    match *c {
        Constraint::Distance { a, b, .. } => across(p(a), p(b)),
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point);
            across(q, q.and_then(|q| foot(doc, q, line)))
        }
        Constraint::HorizontalDistance { .. } => DVec2::new(0.0, offset.y),
        Constraint::VerticalDistance { .. } => DVec2::new(offset.x, 0.0),
        _ => offset,
    }
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
        Constraint::Angle { a, b, degrees } => {
            // Inside the dimensioned angle, between the lines themselves where it can be.
            let bisector =
                angle_sectors(doc, a, b, degrees).map_or(diagonal, |sectors| sectors[0].bisector());
            bisector * d * 2.0
        }
        _ => DVec2::ZERO,
    }
}

/// Where a radius dimension's arrow lands, where its leader starts, and where its text goes,
/// for a label dragged to `label` (screen points, y down). On an arc (`sweep`: its start and counter-clockwise sweep,
/// radians) the arrow always lands on the arc itself: toward the label where the arc is,
/// else straight through the centre from the far side, else radially at the arc's nearer
/// end, with the text on the leader's end (as a linear dimension's text keeps to its line).
fn radius_leader(
    center: Pos2,
    label: Pos2,
    r: f32,
    sweep: Option<(f64, f64)>,
) -> (Pos2, Pos2, Pos2) {
    let v = dir(center, label);
    let outside = (label - center).length() > r;
    let toward = |u: Vec2| (center + u * r, if outside { label } else { center }, label);
    let Some((start, sweep)) = sweep else {
        return toward(v);
    };
    // Screen y is down; arc angles are counter-clockwise with y up.
    let screen = |a: f64| vec2(a.cos() as f32, -a.sin() as f32);
    let on_arc = |u: Vec2| {
        let a = f64::from(-u.y).atan2(f64::from(u.x));
        (a - start).rem_euclid(std::f64::consts::TAU) <= sweep + 1e-9
    };
    if on_arc(v) {
        return toward(v);
    }
    if on_arc(-v) {
        // The leader runs from the label through the centre to the arc.
        return (center - v * r, label, label);
    }
    // Neither way meets the arc: the leader stays radial, through whichever end is nearer
    // the label, and runs out as far as the label (clear of the arc), the text at its end.
    let ends = [screen(start), screen(start + sweep)];
    let u = if ends[0].dot(v) >= ends[1].dot(v) {
        ends[0]
    } else {
        ends[1]
    };
    let from = center + u * (label - center).dot(u).max(r + RELIEF_PT);
    (center + u * r, from, from)
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
pub(crate) fn foot(doc: &Document, q: DVec2, line: EntityId) -> Option<DVec2> {
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

/// Unit direction along a line from `vertex` (where it meets the other line of an angle)
/// toward the line itself: its end farther from the vertex.
fn ray_from(doc: &Document, line: EntityId, vertex: DVec2) -> Option<DVec2> {
    let (a, b) = line_points(doc, line)?;
    let far = if a.distance_squared(vertex) >= b.distance_squared(vertex) {
        a
    } else {
        b
    };
    (far - vertex)
        .try_normalize()
        .or_else(|| (b - a).try_normalize())
}

/// One of the four angles two crossing lines make: from the direction `from`, turning
/// `sweep` radians (counter-clockwise positive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sector {
    pub from: DVec2,
    pub sweep: f64,
}

impl Sector {
    pub fn bisector(&self) -> DVec2 {
        DVec2::from_angle(self.from.y.atan2(self.from.x) + self.sweep / 2.0)
    }

    pub fn to(&self) -> DVec2 {
        DVec2::from_angle(self.from.y.atan2(self.from.x) + self.sweep)
    }
}

/// The two angles between lines `a` and `b` that measure `degrees` (they are opposite each
/// other across the vertex). The first starts on line `a` itself, and when the angle between
/// the lines as drawn is the one dimensioned it ends on line `b` too; the second is its
/// mirror image through the vertex.
pub fn angle_sectors(
    doc: &Document,
    a: EntityId,
    b: EntityId,
    degrees: f64,
) -> Option<[Sector; 2]> {
    let vertex = angle_vertex(doc, a, b)?;
    let (r1, r2) = (ray_from(doc, a, vertex)?, ray_from(doc, b, vertex)?);
    // From line a's ray to line b's, the short way round.
    let between = r1.perp_dot(r2).atan2(r1.dot(r2));
    let corner = between.abs().to_degrees();
    let sweep = if (corner - degrees).abs() <= (180.0 - corner - degrees).abs() {
        between
    } else {
        // The dimension is of the other angle: from line a to line b carried on through
        // the vertex.
        between - std::f64::consts::PI * between.signum()
    };
    Some([Sector { from: r1, sweep }, Sector { from: -r1, sweep }])
}

/// The order of two lines for an angle dimension that measures the angle between them as
/// they are drawn (at their corner), rather than its supplement.
pub fn corner_order(doc: &Document, a: EntityId, b: EntityId) -> (EntityId, EntityId) {
    let corner = (|| {
        let vertex = angle_vertex(doc, a, b)?;
        let (r1, r2) = (ray_from(doc, a, vertex)?, ray_from(doc, b, vertex)?);
        Some(r1.perp_dot(r2).atan2(r1.dot(r2)).abs().to_degrees())
    })();
    let measured = drift_solver::measure(doc, &Constraint::Angle { a, b, degrees: 0.0 });
    match (corner, measured) {
        // The solver measures a → b; the other order measures the supplement.
        (Some(corner), Some(m)) if (m - corner).abs() > (180.0 - m - corner).abs() => (b, a),
        _ => (a, b),
    }
}

/// How far drawn geometry runs from `at` in the direction `out` (a unit vector): along lines
/// through `at` parallel to `out`, and on along lines that carry on from their ends (a wall
/// drawn in pieces). An extension line from `at` going `out` would run on top of it that far.
/// 0 when no line does. Construction lines and lines on hidden layers don't count.
pub(crate) fn geometry_run(doc: &Document, at: DVec2, out: DVec2) -> f64 {
    let tol = 1e-6 * (1.0 + at.abs().max_element());
    let lines: Vec<(DVec2, DVec2)> = doc
        .iter()
        .filter(|(id, e)| e.style != LineKind::Construction && doc.is_visible(*id))
        .filter_map(|(_, e)| match e.kind {
            EntityKind::Line { a, b } => Some((doc.point(a).ok()?, doc.point(b).ok()?)),
            _ => None,
        })
        .filter(|(a, b)| {
            let len = (*b - *a).length();
            len > tol && (*b - *a).perp_dot(out).abs() <= 1e-6 * len
        })
        .collect();
    let mut reach = 0.0;
    for _ in 0..64 {
        let tip = at + out * reach;
        let next = lines
            .iter()
            .filter(|(a, _)| (tip - *a).perp_dot(out).abs() <= tol)
            .filter_map(|(a, b)| {
                let (ta, tb) = ((*a - at).dot(out), (*b - at).dot(out));
                (ta.min(tb) <= reach + tol && reach <= ta.max(tb) + tol).then_some(ta.max(tb))
            })
            .fold(reach, f64::max);
        if next <= reach + tol {
            break;
        }
        reach = next;
    }
    reach
}

#[derive(Clone, Debug)]
pub enum DimShape {
    /// Extension lines from the measured points `p` to the dimension line ends `d`. Each is
    /// drawn from `start` along it from `p` (a gap past the measured point, or past geometry
    /// it would run along, or where it was put by hand). `ends` are the measured points in
    /// the sketch and the extension lines' directions there, for dragging their starts.
    Linear {
        p: [Pos2; 2],
        d: [Pos2; 2],
        start: [f32; 2],
        ends: [Option<(DVec2, DVec2)>; 2],
    },
    /// A leader from `from` to the arrow tip `tip` on the curve.
    Radius { center: Pos2, tip: Pos2, from: Pos2 },
    /// A line across the circle through its centre, arrows at both ends.
    Diameter { ends: [Pos2; 2] },
    /// An arc around `center` from screen angle `start` sweeping `sweep` (radians, y-down),
    /// with an extension line to each end the measured line doesn't reach.
    Angular {
        extensions: [Option<[Pos2; 2]>; 2],
        center: Pos2,
        radius: f32,
        start: f32,
        sweep: f32,
    },
}

impl DimShape {
    /// A linear dimension's extension line at end `i` (0 or 1), from where it starts to the
    /// dimension line (without its overshoot), if it is drawn.
    pub fn extension_line(&self, i: usize) -> Option<[Pos2; 2]> {
        let DimShape::Linear { p, d, start, .. } = self else {
            return None;
        };
        let v = d[i] - p[i];
        let len = v.length();
        (len > 1e-3 && len > start[i]).then(|| [p[i] + v / len * start[i], d[i]])
    }

    /// Grips at the starts of a linear dimension's extension lines: the end, where the grip
    /// is, and the measured point and direction in the sketch a drag measures along.
    pub fn extension_grips(&self) -> Vec<(usize, Pos2, DVec2, DVec2)> {
        let DimShape::Linear { p, d, start, ends } = self else {
            return Vec::new();
        };
        (0..2)
            .filter_map(|i| {
                let (at, out) = ends[i]?;
                Some((i, p[i] + dir(p[i], d[i]) * start[i], at, out))
            })
            .collect()
    }
}

/// Logical points per millimetre on paper (96 dpi), for drafting sizes on screen.
pub const PT_PER_MM: f32 = (96.0 / drift_core::units::MM_PER_INCH) as f32;

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

/// A dimension's text: the main part, an optional stacked pair (deviations or limits), a
/// trailing part, and a line under the dimension line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimText {
    pub before: String,
    pub stack: Option<(String, String)>,
    pub after: String,
    pub below: String,
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
    /// `None` for a dimension that doesn't exist yet (a tool preview), or a property's.
    pub id: Option<ConstraintId>,
    /// The property dimension it draws (a wall's thickness), if it is one.
    pub property: Option<drift_core::PropertyDimId>,
    pub constraint: Constraint,
    pub label: DimText,
    /// The label's centre.
    pub text_pos: Pos2,
    /// The point a drag of the label moves: on the dimension line for a linear dimension
    /// (the text may sit above it), else the label's centre.
    pub handle: Pos2,
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

/// The style a dimension is drawn with: the drawing's, with the dimension's own settings in
/// place (a dimension not made yet has none).
pub fn dimension_style(doc: &Document, id: Option<ConstraintId>) -> DraftStyle {
    id.map_or_else(
        || doc.draft_style(),
        |id| doc.draft_style_for(drift_core::AnnotationRef::Dimension(id)),
    )
}

/// The full label of a dimension: value (measured, in parentheses, if driven; marked Σ if
/// linked to an expression), prefix, suffix, tolerance and alternate units.
pub fn dimension_label(doc: &Document, id: Option<ConstraintId>, c: &Constraint) -> DimText {
    let units = doc.units();
    let style = dimension_style(doc, id);
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
        // Text instead of the value (`EQ`): no tolerance or alternate units, which are of
        // the value.
        _ if !note.text.is_empty() => DimText {
            before: format!("{mark}{prefix}{}{suffix}", note.text),
            ..Default::default()
        },
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
            ..Default::default()
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
                ..Default::default()
            }
        }
    };
    if driven {
        text.before = format!("({}", text.before);
        text.after = format!("{})", text.after);
    }
    text.below = note.below;
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

/// Galleys of a dimension label, laid out in its own (unrotated) frame: the value's row,
/// and under it the `below` line, `gap` lower.
struct LabelGalleys {
    before: std::sync::Arc<egui::Galley>,
    stack: Option<[std::sync::Arc<egui::Galley>; 2]>,
    after: std::sync::Arc<egui::Galley>,
    below: Option<std::sync::Arc<egui::Galley>>,
    /// The value's row: its width and height.
    row: Vec2,
    gap: f32,
    size: Vec2,
}

/// The gap between a label's row and its `below` line: room for the dimension line to run
/// between them where the text sits above the line (ISO), else a little space.
fn below_gap(straddles: bool) -> f32 {
    if straddles { 3.0 } else { 1.0 }
}

fn label_galleys(
    painter: &egui::Painter,
    text: &DimText,
    font: f32,
    color: Color32,
    straddles: bool,
) -> LabelGalleys {
    let main = crate::fonts::drafting_font(font, false);
    let small = crate::fonts::drafting_font(font * 0.72, false);
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
    let row = vec2(before.size().x + stack_w + after.size().x, height);
    let below = (!text.below.is_empty())
        .then(|| painter.layout_no_wrap(text.below.clone(), small.clone(), color));
    let gap = below_gap(straddles);
    let size = match &below {
        Some(b) => vec2(row.x.max(b.size().x), row.y + gap + b.size().y),
        None => row,
    };
    LabelGalleys {
        before,
        stack,
        after,
        below,
        row,
        gap,
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
        bold: bool,
        italic: bool,
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
        let format = galley.job.sections.first().map(|s| &s.format);
        let size = format.map_or(12.0, |f| f.font_id.size);
        let bold = format.is_some_and(|f| crate::fonts::is_bold(&f.font_id.family));
        let italic = format.is_some_and(|f| f.italics);
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
                bold,
                italic,
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
    // The row, centred across the label; the line under it, too.
    let mid = g.row.y / 2.0;
    let left = (g.size.x - g.row.x) / 2.0;
    place(&g.before, vec2(left, mid - g.before.size().y / 2.0));
    let mut x = left + g.before.size().x;
    if let Some([u, l]) = &g.stack {
        let top = mid - (u.size().y + l.size().y) / 2.0;
        place(u, vec2(x + 2.0, top));
        place(l, vec2(x + 2.0, top + u.size().y));
        x += u.size().x.max(l.size().x) + 3.0;
    }
    place(&g.after, vec2(x, mid - g.after.size().y / 2.0));
    if let Some(b) = &g.below {
        place(b, vec2((g.size.x - b.size().x) / 2.0, g.row.y + g.gap));
    }
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

/// Where a linear dimension's number goes along its line (`along` from the first end of a
/// line `len` long; `half` is half the text's extent along it). Between the extension lines
/// it is drawn to the middle; where it doesn't fit there, it goes just outside, on the side
/// it was nearer. A number placed outside stays where it was put.
fn settle_label(along: f32, len: f32, half: f32, style: &DimStyle) -> f32 {
    if !(0.0..=len).contains(&along) {
        return along;
    }
    let headless = matches!(style.arrow, ArrowStyle::Tick | ArrowStyle::Dot);
    let arrows_fit = len > 2.5 * style.arrow_len || style.arrow == ArrowStyle::Tick;
    // Text above the line only has to clear the extension lines; text in the line also has
    // to leave the arrows visible.
    let room = if style.aligned || headless {
        2.0 * half + 4.0
    } else {
        2.0 * half + 2.0 * style.arrow_len + 6.0
    };
    let mid = len / 2.0;
    if room <= len && arrows_fit {
        if (along - mid).abs() <= (len * 0.15).max(8.0) {
            mid
        } else {
            along
        }
    } else {
        // Clear of the arrow, which points inward from outside when the line is short.
        let lead = half + 4.0 + if arrows_fit { 0.0 } else { style.arrow_len };
        if along < mid - 0.5 { -lead } else { len + lead }
    }
}

/// Where a dimension line goes across its extension lines: at `at`, but at least
/// [`RELIEF_PT`] beyond the span `lo..=hi` its ends lie in, on the side it is on (the far
/// side of `hi` when it is on neither).
fn relieved(at: f32, lo: f32, hi: f32) -> f32 {
    if at < lo || (at <= hi && at - lo < hi - at) {
        at.min(lo - RELIEF_PT)
    } else {
        at.max(hi + RELIEF_PT)
    }
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
    let style = DimStyle::of(&dimension_style(doc, id));
    let anchor = dimension_anchor(doc, c)?;
    let offset = placement.unwrap_or_else(|| default_offset(doc, c, map.camera.scale));
    let label = map.to_screen(anchor + offset);
    let s = |p: DVec2| map.to_screen(p);
    // Points, or lines standing for their midpoints.
    let p = |id| doc.ref_point(id);

    let linear = |p1: Pos2, p2: Pos2| {
        let u = dir(p1, p2);
        let n = u.rot90();
        let d = relieved((label - p1).dot(n), 0.0, 0.0);
        let d1 = p1 + n * d;
        let d2 = p2 + n * d;
        let along = (label - d1).dot(u);
        (
            DimShape::Linear {
                p: [p1, p2],
                d: [d1, d2],
                start: [style.gap; 2],
                ends: [None; 2],
            },
            d1 + u * along,
        )
    };
    // The measured points in the sketch, and a direction square to the dimension line.
    let measured: Option<([DVec2; 2], DVec2)> = match *c {
        Constraint::Distance { a, b, .. } => p(a).zip(p(b)).map(|(a, b)| ([a, b], (b - a).perp())),
        Constraint::PointLineDistance { point, line, .. } => {
            p(point).and_then(|q| foot(doc, q, line).map(|f| ([q, f], (f - q).perp())))
        }
        Constraint::HorizontalDistance { a, b, .. } => {
            p(a).zip(p(b)).map(|(a, b)| ([a, b], DVec2::Y))
        }
        Constraint::VerticalDistance { a, b, .. } => {
            p(a).zip(p(b)).map(|(a, b)| ([a, b], DVec2::X))
        }
        _ => None,
    };

    let (shape, text_pos) = match *c {
        Constraint::Distance { a, b, .. } => linear(s(p(a)?), s(p(b)?)),
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point)?;
            linear(s(q), s(foot(doc, q, line)?))
        }
        Constraint::HorizontalDistance { a, b, .. } => {
            let (p1, p2) = (s(p(a)?), s(p(b)?));
            let y = relieved(label.y, p1.y.min(p2.y), p1.y.max(p2.y));
            let d = [pos2(p1.x, y), pos2(p2.x, y)];
            let shape = DimShape::Linear {
                p: [p1, p2],
                d,
                start: [style.gap; 2],
                ends: [None; 2],
            };
            (shape, pos2(label.x, y))
        }
        Constraint::VerticalDistance { a, b, .. } => {
            let (p1, p2) = (s(p(a)?), s(p(b)?));
            let x = relieved(label.x, p1.x.min(p2.x), p1.x.max(p2.x));
            let d = [pos2(x, p1.y), pos2(x, p2.y)];
            let shape = DimShape::Linear {
                p: [p1, p2],
                d,
                start: [style.gap; 2],
                ends: [None; 2],
            };
            (shape, pos2(x, label.y))
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            let center = s(center_of(doc, curve)?);
            let r = (drift_solver::radius_of(doc, curve)? * map.camera.scale) as f32;
            let v = dir(center, label);
            if matches!(c, Constraint::Radius { .. }) {
                let sweep = match doc.shape(curve) {
                    Some(drift_core::Shape::Arc { start, sweep, .. }) => Some((start, sweep)),
                    _ => None,
                };
                let (tip, from, text) = radius_leader(center, label, r, sweep);
                (DimShape::Radius { center, tip, from }, text)
            } else {
                let ends = [center - v * r, center + v * r];
                (DimShape::Diameter { ends }, label)
            }
        }
        Constraint::Angle { a, b, degrees } => {
            let center = s(anchor);
            // Of the two angles that measure this, the one the label sits in.
            let sectors = angle_sectors(doc, a, b, degrees)?;
            let toward = vec2(label.x - center.x, -(label.y - center.y));
            let toward = DVec2::new(f64::from(toward.x), f64::from(toward.y));
            let sector = if sectors[1].bisector().dot(toward) > sectors[0].bisector().dot(toward) {
                sectors[1]
            } else {
                sectors[0]
            };
            // Screen angles are y-down, so world counter-clockwise is negative.
            let start = (-sector.from.y).atan2(sector.from.x) as f32;
            let sweep = -(sector.sweep as f32);
            let radius = (label - center).length().max(12.0);
            // Where an end of the arc is off its line, an extension line reaches it.
            let reach = |line: EntityId, dir: DVec2| -> Option<[Pos2; 2]> {
                let (pa, pb) = line_points(doc, line)?;
                let (sa, sb) = (s(pa), s(pb));
                let tip = center + vec2(dir.x as f32, -dir.y as f32) * radius;
                let along = sb - sa;
                let t = if along.length_sq() > 0.0 {
                    (tip - sa).dot(along) / along.length_sq()
                } else {
                    0.0
                };
                if (-0.001..=1.001).contains(&t) {
                    return None;
                }
                let near = if t < 0.0 { sa } else { sb };
                let out = (tip - near).normalized();
                Some([near + out * 2.0, tip + out * 5.0])
            };
            (
                DimShape::Angular {
                    extensions: [reach(a, sector.from), reach(b, sector.to())],
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
    let mut shape = shape;
    if let (
        DimShape::Linear {
            p: sp,
            d,
            start,
            ends,
        },
        Some((world, n)),
    ) = (&mut shape, measured)
        && let Some(n) = n.try_normalize()
    {
        // Where each extension line starts: where it was put by hand, else a gap past the
        // end of geometry it would run along (toward the dimension line), else a gap from
        // the measured point.
        let manual = id.map_or([None; 2], |id| doc.extension_starts(id));
        for i in 0..2 {
            let v = d[i] - sp[i];
            let toward = (map.to_world(d[i]) - world[i]).dot(n);
            if v.length() < 1e-3 || toward == 0.0 {
                continue;
            }
            let out = n * toward.signum();
            let u = v.normalized();
            let along = |t: f64| (s(world[i] + out * t) - sp[i]).dot(u);
            start[i] = match manual[i] {
                Some(t) => along(t),
                None => match geometry_run(doc, world[i], out) {
                    run if run > 0.0 => along(run) + style.gap,
                    _ => style.gap,
                },
            };
            ends[i] = Some((world[i], out));
        }
    }
    let driven = id.is_some_and(|id| doc.is_driven(id));
    let text = dimension_label(doc, id, c);
    let straddles = matches!(shape, DimShape::Linear { .. }) && style.aligned;
    let galleys = label_galleys(painter, &text, style.text, Color32::WHITE, straddles);
    let text_pos = match &shape {
        DimShape::Linear { d, .. } => {
            let u = dir(d[0], d[1]);
            let half = if style.aligned {
                galleys.size.x / 2.0
            } else {
                (u.x.abs() * galleys.size.x + u.y.abs() * galleys.size.y) / 2.0
            };
            let along = settle_label(
                (text_pos - d[0]).dot(u),
                (d[1] - d[0]).length(),
                half,
                &style,
            );
            d[0] + u * along
        }
        _ => text_pos,
    };
    let handle = text_pos;
    // ISO: linear dimension text above the line and parallel to it.
    let (text_pos, angle) = match &shape {
        DimShape::Linear { d, .. } if style.aligned => {
            let angle = reading_angle(dir(d[0], d[1]));
            let up = egui::emath::Rot2::from_angle(angle) * vec2(0.0, -1.0);
            // The row's foot just above the line (and a `below` line just under it).
            (
                text_pos + up * (galleys.row.y + 1.5 - galleys.size.y / 2.0),
                angle,
            )
        }
        _ => (text_pos, 0.0),
    };
    let text_rect = rotated_bounds(text_pos, galleys.size, angle).expand2(vec2(3.0, 1.0));
    Some(DimLayout {
        id,
        property: None,
        constraint: *c,
        label: text,
        text_pos,
        handle,
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
        DimShape::Linear { d, .. } => {
            for i in 0..2 {
                if let Some([from, to]) = dim.shape.extension_line(i) {
                    pen.line(&[from, to + dir(from, to) * style.overshoot], stroke);
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
            extensions,
            center,
            radius,
            start,
            sweep,
        } => {
            for ext in extensions.iter().flatten() {
                pen.line(ext, stroke);
            }
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
    let straddles = matches!(dim.shape, DimShape::Linear { .. }) && style.aligned;
    let galleys = label_galleys(pen.painter(), &dim.label, style.text, color, straddles);
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
            Constraint::Fix { point, .. }
            | Constraint::FixX { point, .. }
            | Constraint::FixY { point, .. } => place(point, point_base(point), id, Fix, &mut out),
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

    #[test]
    fn dimensions_take_the_drawings_colour_or_the_canvas_default() {
        use drift_core::Rgb;
        // White on the dark canvas, charcoal on the light one; relation glyphs keep theirs.
        assert_eq!(
            AnnotationColors::drafting(true, None).normal,
            Color32::WHITE
        );
        let charcoal = AnnotationColors::drafting(false, None).normal;
        assert_eq!(charcoal, Color32::from_rgb(0x33, 0x36, 0x3a));
        assert_ne!(AnnotationColors::new(true).normal, Color32::WHITE);
        // A chosen colour shows on both canvases...
        let red = Some(Rgb([0xd0, 0x30, 0x30]));
        assert_eq!(
            AnnotationColors::drafting(true, red).normal,
            Color32::from_rgb(0xd0, 0x30, 0x30)
        );
        assert_eq!(
            AnnotationColors::drafting(false, red).normal,
            Color32::from_rgb(0xd0, 0x30, 0x30)
        );
        // ...unless it would vanish into one: black on the dark canvas falls back to white.
        let black = Some(Rgb([0, 0, 0]));
        assert_eq!(
            AnnotationColors::drafting(true, black).normal,
            Color32::WHITE
        );
        assert_eq!(
            AnnotationColors::drafting(false, black).normal,
            Color32::BLACK
        );
    }

    use super::*;

    #[test]
    fn radius_arrows_land_on_the_arc() {
        use std::f64::consts::FRAC_PI_2;
        let c = pos2(0.0, 0.0);
        let near = |a: Pos2, b: Pos2| (a - b).length() < 1e-3;
        let h = 10.0 / 2f32.sqrt();
        // A quarter arc from 0° to 90° (screen: right, then up).
        let quarter = Some((0.0, FRAC_PI_2));
        // Label beside the arc: the arrow points at it, the leader starts at the label.
        let label = pos2(20.0, -20.0);
        let (tip, from, text) = radius_leader(c, label, 10.0, quarter);
        assert!(near(tip, pos2(h, -h)) && near(from, label) && near(text, label));
        // Label across the centre from the arc (#115): the leader runs through the centre to
        // the arc, not to the empty side of its circle.
        let label = pos2(-20.0, 20.0);
        let (tip, from, text) = radius_leader(c, label, 10.0, quarter);
        assert!(near(tip, pos2(h, -h)), "{tip:?}");
        assert!(near(from, label) && near(text, label));
        // Label where neither way meets the arc: a radial leader through the nearer end, out
        // as far as the label, with the text on its end.
        let (tip, from, text) = radius_leader(c, pos2(40.0, 40.0), 10.0, quarter);
        assert!(near(tip, pos2(10.0, 0.0)), "{tip:?}");
        assert!(near(from, pos2(40.0, 0.0)) && near(text, from), "{from:?}");
        let (tip, from, _) = radius_leader(c, pos2(-40.0, -40.0), 10.0, quarter);
        assert!(near(tip, pos2(0.0, -10.0)), "{tip:?}");
        assert!(near(from, pos2(0.0, -40.0)), "{from:?}");
        // ...and clear of the arc when the label is close in.
        let (_, from, _) = radius_leader(c, pos2(5.0, 5.0), 10.0, quarter);
        assert!(near(from, pos2(10.0 + RELIEF_PT, 0.0)), "{from:?}");
        // Inside the arc the leader starts at the centre.
        let (tip, from, _) = radius_leader(c, pos2(3.0, -3.0), 10.0, quarter);
        assert!(near(tip, pos2(h, -h)) && near(from, c));
        // A circle points wherever the label is.
        let (tip, _, _) = radius_leader(c, pos2(-20.0, 20.0), 10.0, None);
        assert!(near(tip, pos2(-h, h)));
    }

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
            ..Default::default()
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
        // A line under the dimension line, and text in place of the value (no tolerance,
        // which is of the value).
        doc.set_dim_note(
            len,
            Some(DimNote {
                suffix: "TYP".into(),
                tolerance: Tolerance::Symmetric(0.1),
                below: "TO FACE OF STUD".into(),
                ..Default::default()
            }),
        );
        let t = label(&doc, len);
        assert_eq!(
            (t.plain().as_str(), t.below.as_str()),
            ("25 ±0.1 TYP", "TO FACE OF STUD")
        );
        doc.set_dim_note(
            len,
            Some(DimNote {
                text: "EQ".into(),
                tolerance: Tolerance::Symmetric(0.1),
                ..Default::default()
            }),
        );
        assert_eq!(label(&doc, len).plain(), "EQ");
        doc.set_dim_note(len, None);
        let mut style = doc.draft_style();
        style.alt_units = Some(LengthFormat::Inch);
        doc.set_draft_style(style);
        assert_eq!(label(&doc, len).plain(), "25 [0.984\"]");
        doc.set_driven(len, true).unwrap();
        assert_eq!(label(&doc, len).plain(), "(25 [0.984\"])");
    }

    #[test]
    fn dimension_lines_keep_clear_of_what_they_measure() {
        // Placed well clear: where it was put.
        assert_eq!(relieved(40.0, 100.0, 100.0), 40.0);
        assert_eq!(relieved(160.0, 100.0, 100.0), 160.0);
        // Dragged close (or onto the geometry): held off on its side.
        assert_eq!(relieved(95.0, 100.0, 100.0), 100.0 - RELIEF_PT);
        assert_eq!(relieved(104.0, 100.0, 100.0), 100.0 + RELIEF_PT);
        assert_eq!(relieved(100.0, 100.0, 100.0), 100.0 + RELIEF_PT);
        // Between two ends at different heights: off the nearer one, outside both.
        assert_eq!(relieved(105.0, 100.0, 140.0), 100.0 - RELIEF_PT);
        assert_eq!(relieved(130.0, 100.0, 140.0), 140.0 + RELIEF_PT);
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

    #[test]
    fn numbers_sit_mid_line_or_just_outside_a_tight_one() {
        // Text in the line, with arrowheads.
        let style = DimStyle {
            aligned: false,
            arrow: ArrowStyle::Closed,
            ..DimStyle::default()
        };
        let a = style.arrow_len;
        // Room to spare: near the middle is the middle; well off it stays where it was put.
        assert!((settle_label(95.0, 200.0, 20.0, &style) - (100.0)).abs() < 1e-3);
        assert!((settle_label(40.0, 200.0, 20.0, &style) - (40.0)).abs() < 1e-3);
        // Placed outside the extension lines: left alone.
        assert!((settle_label(-30.0, 200.0, 20.0, &style) - (-30.0)).abs() < 1e-3);
        assert!((settle_label(260.0, 200.0, 20.0, &style) - (260.0)).abs() < 1e-3);
        // Too tight for the text and its arrows: just outside, on the nearer side (after
        // the far end when it was in the middle).
        let len = 40.0 + a;
        assert!((settle_label(len / 2.0, len, 20.0, &style) - (len + 24.0)).abs() < 1e-3);
        assert!((settle_label(3.0, len, 20.0, &style) - (-24.0)).abs() < 1e-3);
        // Shorter than its arrows, which then point in from outside: clear of them too.
        let len = 2.0 * a;
        assert!((settle_label(a, len, 20.0, &style) - (len + 24.0 + a)).abs() < 1e-3);
        // Text above the line only has to fit between the extension lines.
        let iso = DimStyle {
            aligned: true,
            ..style
        };
        let len = 44.0 + a;
        assert!((settle_label(len / 2.0 + 2.0, len, 20.0, &iso) - (len / 2.0)).abs() < 1e-3);
    }

    /// A painter for laying out text, on a context that never draws.
    fn painter() -> egui::Painter {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        egui::Painter::new(
            ctx,
            egui::LayerId::background(),
            Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0)),
        )
    }

    /// The extension lines a dimension draws: the lines running across its dimension line
    /// (on screen), each as (its start, its end past the dimension line).
    fn extension_lines(painter: &egui::Painter, dim: &DimLayout) -> Vec<[Pos2; 2]> {
        let DimShape::Linear { d, .. } = &dim.shape else {
            panic!("not linear");
        };
        let along = dir(d[0], d[1]);
        let pen = Recorder::new(painter);
        paint_dimension(
            &pen,
            dim,
            AnnotationState::Normal,
            &AnnotationColors::new(true),
        );
        pen.take()
            .into_iter()
            .filter_map(|r| match r {
                Recorded::Line(pts, _) if pts.len() == 2 => {
                    let v = pts[1] - pts[0];
                    // Square to the dimension line, and not an arrowhead.
                    (v.length() > 8.0 && v.normalized().dot(along).abs() < 1e-3)
                        .then(|| [pts[0], pts[1]])
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn extension_lines_start_past_the_geometry_they_run_along() {
        // Two upright lines of different lengths, dimensioned between their bottom ends with
        // the dimension line above both: each extension line starts a gap above its own
        // line's top, not at the bottom (as drafted by hand, and as Revit does).
        let painter = painter();
        let mut doc = Document::new();
        let a0 = doc.add_point(DVec2::new(0.0, 0.0));
        let a_mid = doc.add_point(DVec2::new(0.0, 50.0));
        let a1 = doc.add_point(DVec2::new(0.0, 100.0));
        // The left one in two pieces, end to end.
        let left = doc.add_line(a0, a_mid).unwrap();
        doc.add_line(a_mid, a1).unwrap();
        let b0 = doc.add_point(DVec2::new(40.0, 0.0));
        let b1 = doc.add_point(DVec2::new(40.0, 60.0));
        doc.add_line(b0, b1).unwrap();
        let map = ScreenMap {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0)),
            camera: drift_render::Camera {
                center: DVec2::new(20.0, 60.0),
                scale: 2.0,
            },
        };
        let gap = DimStyle::default().gap;
        let s = |x: f64, y: f64| map.to_screen(DVec2::new(x, y));
        let near = |a: Pos2, b: Pos2| a.distance(b) < 0.01;
        let check = |c: Constraint, label: DVec2, starts: [Pos2; 2]| {
            let id = doc.constraints().find(|(_, k)| **k == c).map(|(id, _)| id);
            let dim = layout_dimension(&painter, &doc, id, &c, Some(label), &map).unwrap();
            let lines = extension_lines(&painter, &dim);
            assert_eq!(lines.len(), 2, "{c:?}: {lines:?}");
            for (line, start) in lines.iter().zip(starts) {
                assert!(
                    near(line[0], start),
                    "{c:?}: {line:?} should start at {start:?}"
                );
            }
        };
        let value = 40.0;
        // Above: past the tops (the left one past both its pieces).
        let up = DVec2::new(0.0, 130.0);
        let past_tops = [
            s(0.0, 100.0) - vec2(0.0, gap),
            s(40.0, 60.0) - vec2(0.0, gap),
        ];
        check(
            Constraint::HorizontalDistance {
                a: a0,
                b: b0,
                value,
            },
            up,
            past_tops,
        );
        check(
            Constraint::Distance {
                a: a0,
                b: b0,
                value,
            },
            up,
            past_tops,
        );
        // Picked at the tops instead: the same.
        let tops = Constraint::HorizontalDistance {
            a: a1,
            b: b1,
            value,
        };
        check(tops, DVec2::new(0.0, 50.0), past_tops);
        // Below: the lines run away from the dimension line, so from the picked points.
        let bottoms = [s(0.0, 0.0) + vec2(0.0, gap), s(40.0, 0.0) + vec2(0.0, gap)];
        let down = DVec2::new(0.0, -30.0);
        check(
            Constraint::HorizontalDistance {
                a: a0,
                b: b0,
                value,
            },
            down,
            bottoms,
        );
        // Point to line: past the tops from the point and from the foot on the line.
        let pl = Constraint::PointLineDistance {
            point: b0,
            line: left,
            value,
        };
        check(pl, up, [past_tops[1], past_tops[0]]);

        // Set by hand: where it was put, from the measured point toward the dimension line.
        let mut doc2 = doc.clone();
        let h = doc2
            .add_constraint(Constraint::HorizontalDistance {
                a: a0,
                b: b0,
                value,
            })
            .unwrap();
        doc2.set_extension_start(h, 0, Some(30.0)).unwrap();
        let c = *doc2.constraint(h).unwrap();
        let dim = layout_dimension(&painter, &doc2, Some(h), &c, Some(up), &map).unwrap();
        let lines = extension_lines(&painter, &dim);
        assert!(near(lines[0][0], s(0.0, 30.0)), "{lines:?}");
        assert!(near(lines[1][0], past_tops[1]), "{lines:?}");
        // It keeps its distance when the geometry moves.
        doc2.set_point(a0, DVec2::new(-5.0, 10.0)).unwrap();
        let c = *doc2.constraint(h).unwrap();
        let dim = layout_dimension(&painter, &doc2, Some(h), &c, Some(up), &map).unwrap();
        let lines = extension_lines(&painter, &dim);
        assert!(near(lines[0][0], s(-5.0, 40.0)), "{lines:?}");
    }

    #[test]
    fn new_dimensions_are_centred_on_their_line() {
        let mut doc = Document::new();
        doc.begin();
        let a = doc.add_point(DVec2::new(0.0, 0.0));
        let b = doc.add_point(DVec2::new(40.0, 0.0));
        let c = doc.add_point(DVec2::new(40.0, 30.0));
        doc.commit("Points");
        let off = DVec2::new(13.0, 7.0);
        let value = 0.0;
        // Along the measured direction the offset goes; across it, it is kept.
        let d = centred_offset(&doc, &Constraint::Distance { a, b, value }, off);
        assert!(d.distance(DVec2::new(0.0, 7.0)) < 1e-12);
        let d = centred_offset(&doc, &Constraint::Distance { a, b: c, value }, off);
        assert!(d.dot(DVec2::new(40.0, 30.0)).abs() < 1e-9 && d.length() > 1.0);
        let h = Constraint::HorizontalDistance { a, b: c, value };
        assert_eq!(centred_offset(&doc, &h, off), DVec2::new(0.0, 7.0));
        let v = Constraint::VerticalDistance { a, b: c, value };
        assert_eq!(centred_offset(&doc, &v, off), DVec2::new(13.0, 0.0));
    }
}
