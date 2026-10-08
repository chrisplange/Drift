//! Drafting annotations on the canvas: notes and leaders, centre marks and centre lines, hole
//! and chamfer callouts, and ordinate dimensions. Their text and leaders are laid out in screen
//! space (like dimensions); centre marks and centre lines are world geometry drawn by the GPU.

use drift_core::{
    Anchor, Annotation, AnnotationId, DVec2, Document, DraftStyle, Emphasis, EntityId, EntityKind,
    LeaderEnd, LineKind, OrdinateAxis, Shape, SketchItem,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, vec2};

use crate::annotations::{AnnotationColors, AnnotationState, DimStyle, PT_PER_MM, Pen};
use crate::overlay::ScreenMap;

/// Paper millimetres a centre line runs past its circle.
const CENTRE_OVERRUN_MM: f64 = 2.5;
/// Gap between a measured point and an ordinate's leader, in logical points.
const ORDINATE_GAP: f32 = 4.0;

/// Text of an annotation and where it sits on screen.
#[derive(Clone, Debug)]
pub struct NoteText {
    pub text: String,
    pub center: Pos2,
    /// Rotation (screen, radians): ordinate X labels read vertically.
    pub angle: f32,
    pub rect: Rect,
}

/// An annotation laid out for drawing and picking.
#[derive(Clone, Debug)]
pub struct NoteLayout {
    pub id: AnnotationId,
    pub text: Option<NoteText>,
    /// A leader polyline, from its tip (on the geometry) to the text.
    pub leader: Vec<Pos2>,
    pub end: LeaderEnd,
    /// Centre lines, on screen (for picking; the GPU draws them).
    pub segments: Vec<[Pos2; 2]>,
}

impl NoteLayout {
    /// Whether `p` picks this annotation.
    pub fn hit(&self, p: Pos2) -> bool {
        if self
            .text
            .as_ref()
            .is_some_and(|t| t.rect.expand(2.0).contains(p))
        {
            return true;
        }
        let near = |a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = if ab.length_sq() > 0.0 {
                ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a + ab * t - p).length() <= 4.0
        };
        self.segments.iter().any(|[a, b]| near(*a, *b))
            || self.leader.windows(2).any(|w| near(w[0], w[1]))
    }

    /// Where an in-place text editor goes.
    pub fn text_rect(&self) -> Option<Rect> {
        self.text.as_ref().map(|t| t.rect)
    }
}

/// The centre of a circle or arc, or a point's position.
fn centre(doc: &Document, id: EntityId) -> Option<(DVec2, f64)> {
    match doc.get(id)?.kind {
        EntityKind::Circle { center, radius } => Some((doc.point(center).ok()?, radius)),
        EntityKind::Arc { center, start, .. } => {
            let c = doc.point(center).ok()?;
            Some((c, c.distance(doc.point(start).ok()?)))
        }
        EntityKind::Point(p) => Some((p, 0.0)),
        EntityKind::Line { .. } => None,
    }
}

fn line_ends(doc: &Document, id: EntityId) -> Option<(DVec2, DVec2)> {
    match doc.shape(id)? {
        Shape::Line { a, b } => Some((a, b)),
        _ => None,
    }
}

/// World segments of a centre mark or centre line.
pub fn centre_segments(doc: &Document, a: &Annotation) -> Vec<(DVec2, DVec2)> {
    let overrun = CENTRE_OVERRUN_MM * doc.draft_style().scale;
    match *a {
        Annotation::CentreMark { curve } => {
            let Some((c, r)) = centre(doc, curve) else {
                return Vec::new();
            };
            let reach = r + overrun.max(0.1 * r);
            vec![
                (c - DVec2::X * reach, c + DVec2::X * reach),
                (c - DVec2::Y * reach, c + DVec2::Y * reach),
            ]
        }
        Annotation::Centreline { a, b } => {
            if let (Some((a1, a2)), Some((b1, b2))) = (line_ends(doc, a), line_ends(doc, b)) {
                // Midway between two lines: pair the ends that point the same way.
                let (b1, b2) = if (a2 - a1).dot(b2 - b1) >= 0.0 {
                    (b1, b2)
                } else {
                    (b2, b1)
                };
                let (p, q) = ((a1 + b1) * 0.5, (a2 + b2) * 0.5);
                let u = (q - p).normalize_or_zero();
                return vec![(p - u * overrun, q + u * overrun)];
            }
            let (Some((p, rp)), Some((q, rq))) = (centre(doc, a), centre(doc, b)) else {
                return Vec::new();
            };
            let u = (q - p).normalize_or_zero();
            vec![(p - u * (rp + overrun), q + u * (rq + overrun))]
        }
        _ => Vec::new(),
    }
}

/// GPU items for every centre mark and centre line.
pub fn centre_items(
    doc: &Document,
    selected: Option<AnnotationId>,
    hovered: Option<AnnotationId>,
) -> Vec<SketchItem> {
    let mut out = Vec::new();
    for (id, a) in doc.annotations() {
        let emphasis = if selected == Some(id) {
            Emphasis::Selected
        } else if hovered == Some(id) {
            Emphasis::Hovered
        } else {
            Emphasis::Normal
        };
        for (p, q) in centre_segments(doc, a) {
            out.push(
                SketchItem::new(Shape::Line { a: p, b: q })
                    .with_kind(LineKind::Center)
                    .with_emphasis(emphasis),
            );
        }
    }
    out
}

/// The text of a hole callout: `4× Ø6 THRU` (counting visible circles of the same size).
pub fn hole_callout_text(doc: &Document, curve: EntityId, suffix: &str) -> Option<String> {
    let (_, r) = centre(doc, curve)?;
    let same = doc
        .iter()
        .filter(|(id, e)| {
            matches!(e.kind, EntityKind::Circle { .. })
                && e.style == LineKind::Solid
                && doc.is_visible(*id)
                && drift_solver::radius_of(doc, *id)
                    .is_some_and(|x| (x - r).abs() <= 1e-9 * r.max(1.0))
        })
        .count();
    let units = doc.units();
    let d = doc.draft_style().length(&units, 2.0 * r);
    let count = if same > 1 {
        format!("{same}× ")
    } else {
        String::new()
    };
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    Some(format!("{count}Ø{d}{suffix}"))
}

/// The text of a chamfer callout: `2 × 45°` for equal legs, else `2 × 3`. Legs are measured
/// to the corner the chamfer cuts (where the lines on either side would meet).
pub fn chamfer_text(doc: &Document, line: EntityId) -> Option<String> {
    let EntityKind::Line { a, b } = doc.get(line)?.kind else {
        return None;
    };
    let (pa, pb) = (doc.point(a).ok()?, doc.point(b).ok()?);
    // The other line at each end, as a direction away from the chamfer.
    let neighbour = |point: EntityId, at: DVec2| -> Option<DVec2> {
        doc.users_of(point)
            .filter(|u| *u != line)
            .find_map(|u| line_ends(doc, u))
            .map(|(p, q)| {
                if p.distance(at) < q.distance(at) {
                    q - p
                } else {
                    p - q
                }
            })
    };
    let units = doc.units();
    let style = doc.draft_style();
    let len = |v: f64| style.length(&units, v);
    let legs = match (neighbour(a, pa), neighbour(b, pb)) {
        (Some(ua), Some(ub)) => {
            // Extend both neighbours backwards to their crossing (the cut-off corner).
            let denom = ua.perp_dot(ub);
            if denom.abs() < 1e-12 * ua.length() * ub.length() {
                None
            } else {
                let t = (pb - pa).perp_dot(ub) / denom;
                let corner = pa + ua * t;
                Some((corner.distance(pa), corner.distance(pb)))
            }
        }
        _ => None,
    };
    let (la, lb) = legs.unwrap_or_else(|| {
        let d = pb - pa;
        (d.x.abs(), d.y.abs())
    });
    let tol = 1e-9 * la.max(lb).max(1.0);
    Some(if (la - lb).abs() <= tol {
        format!("{} × 45°", len(la))
    } else {
        format!("{} × {}", len(la), len(lb))
    })
}

/// Where an annotation's label is anchored in the world (what dragging moves).
pub fn label_world(doc: &Document, a: &Annotation) -> Option<DVec2> {
    match a {
        Annotation::Note { at, leader, .. } => match leader {
            Some(l) => Some(doc.anchor_pos(&l.to)? + *at),
            None => Some(*at),
        },
        Annotation::HoleCallout { curve, at, .. } => Some(centre(doc, *curve)?.0 + *at),
        Annotation::ChamferCallout { line, at } => {
            let (p, q) = line_ends(doc, *line)?;
            Some((p + q) * 0.5 + *at)
        }
        Annotation::Ordinate {
            origin,
            point,
            axis,
            at,
        } => {
            let (o, p) = (doc.ref_point(*origin)?, doc.ref_point(*point)?);
            Some(match axis {
                OrdinateAxis::X => DVec2::new(p.x, o.y + at),
                OrdinateAxis::Y => DVec2::new(o.x + at, p.y),
            })
        }
        Annotation::CentreMark { .. } | Annotation::Centreline { .. } => None,
    }
}

/// The annotation with its label moved to `to` (world).
pub fn with_label_at(doc: &Document, a: &Annotation, to: DVec2) -> Option<Annotation> {
    let mut a = a.clone();
    match &mut a {
        Annotation::Note { at, leader, .. } => {
            *at = match leader {
                Some(l) => to - doc.anchor_pos(&l.to)?,
                None => to,
            };
        }
        Annotation::HoleCallout { curve, at, .. } => *at = to - centre(doc, *curve)?.0,
        Annotation::ChamferCallout { line, at } => {
            let (p, q) = line_ends(doc, *line)?;
            *at = to - (p + q) * 0.5;
        }
        Annotation::Ordinate {
            origin, axis, at, ..
        } => {
            let o = doc.ref_point(*origin)?;
            *at = match axis {
                OrdinateAxis::X => to.y - o.y,
                OrdinateAxis::Y => to.x - o.x,
            };
        }
        Annotation::CentreMark { .. } | Annotation::Centreline { .. } => return None,
    }
    Some(a)
}

/// The value an ordinate dimension shows.
pub fn ordinate_value(
    doc: &Document,
    origin: EntityId,
    point: EntityId,
    axis: OrdinateAxis,
) -> Option<f64> {
    let (o, p) = (doc.ref_point(origin)?, doc.ref_point(point)?);
    Some(match axis {
        OrdinateAxis::X => p.x - o.x,
        OrdinateAxis::Y => p.y - o.y,
    })
}

/// Text centred at `center`, rotated by `angle`, with its screen bounds.
fn place_text(
    painter: &egui::Painter,
    text: String,
    center: Pos2,
    angle: f32,
    font: f32,
) -> NoteText {
    let galley = painter.layout(
        text.clone(),
        FontId::proportional(font),
        Color32::WHITE,
        f32::INFINITY,
    );
    let rot = egui::emath::Rot2::from_angle(angle);
    let h = galley.size() / 2.0;
    let mut rect = Rect::NOTHING;
    for c in [
        vec2(-h.x, -h.y),
        vec2(h.x, -h.y),
        vec2(h.x, h.y),
        vec2(-h.x, h.y),
    ] {
        rect.extend_with(center + rot * c);
    }
    NoteText {
        text,
        center,
        angle,
        rect: rect.expand2(vec2(3.0, 1.0)),
    }
}

/// A leader from `tip` to a text box: it lands on the nearer side of the box with a short
/// horizontal shoulder.
fn leader_to(tip: Pos2, text: &NoteText) -> Vec<Pos2> {
    let side = if tip.x <= text.center.x {
        text.rect.left()
    } else {
        text.rect.right()
    };
    let landing = egui::pos2(side, text.center.y);
    let shoulder = egui::pos2(
        landing.x + if tip.x <= text.center.x { -8.0 } else { 8.0 },
        landing.y,
    );
    vec![tip, shoulder, landing]
}

/// Lay out every annotation.
pub fn layout_annotations(
    painter: &egui::Painter,
    doc: &Document,
    map: &ScreenMap,
) -> Vec<NoteLayout> {
    let style: DraftStyle = doc.draft_style();
    let font = DimStyle::of(&style).text;
    let s = |p: DVec2| map.to_screen(p);
    let mut out = Vec::new();
    for (id, a) in doc.annotations() {
        let mut layout = NoteLayout {
            id,
            text: None,
            leader: Vec::new(),
            end: LeaderEnd::Arrow,
            segments: centre_segments(doc, a)
                .into_iter()
                .map(|(p, q)| [s(p), s(q)])
                .collect(),
        };
        match a {
            Annotation::Note { text, at, leader } => {
                let target = leader.and_then(|l| doc.anchor_pos(&l.to));
                let center = s(target.unwrap_or(DVec2::ZERO) + *at);
                let shown = if text.is_empty() { " " } else { text };
                let t = place_text(painter, shown.to_owned(), center, 0.0, font);
                if let (Some(l), Some(tip)) = (leader, target) {
                    layout.leader = leader_to(s(tip), &t);
                    layout.end = l.end;
                }
                layout.text = Some(t);
            }
            Annotation::CentreMark { .. } | Annotation::Centreline { .. } => {}
            Annotation::HoleCallout { curve, at, suffix } => {
                let (Some((c, r)), Some(text)) =
                    (centre(doc, *curve), hole_callout_text(doc, *curve, suffix))
                else {
                    continue;
                };
                let t = place_text(painter, text, s(c + *at), 0.0, font);
                let dir = (*at).normalize_or(DVec2::X);
                layout.leader = leader_to(s(c + dir * r), &t);
                layout.text = Some(t);
            }
            Annotation::ChamferCallout { line, at } => {
                let (Some((p, q)), Some(text)) = (line_ends(doc, *line), chamfer_text(doc, *line))
                else {
                    continue;
                };
                let mid = (p + q) * 0.5;
                let t = place_text(painter, text, s(mid + *at), 0.0, font);
                layout.leader = leader_to(s(mid), &t);
                layout.text = Some(t);
            }
            Annotation::Ordinate {
                origin,
                point,
                axis,
                at,
            } => {
                let (Some(o), Some(p), Some(v)) = (
                    doc.ref_point(*origin),
                    doc.ref_point(*point),
                    ordinate_value(doc, *origin, *point, *axis),
                ) else {
                    continue;
                };
                let units = doc.units();
                let text = if v.abs() < 1e-12 {
                    "0".to_owned()
                } else {
                    style.length(&units, v)
                };
                let (end, angle) = match axis {
                    OrdinateAxis::X => (DVec2::new(p.x, o.y + at), -std::f32::consts::FRAC_PI_2),
                    OrdinateAxis::Y => (DVec2::new(o.x + at, p.y), 0.0),
                };
                let (sp, se) = (s(p), s(end));
                let away = (se - sp).normalized();
                let away = if away.is_finite() {
                    away
                } else {
                    vec2(0.0, 1.0)
                };
                // The label sits just past the leader's end, reading along it.
                let probe = place_text(painter, text.clone(), se, angle, font);
                let half = match axis {
                    OrdinateAxis::X => probe.rect.height() / 2.0,
                    OrdinateAxis::Y => probe.rect.width() / 2.0,
                };
                let t = place_text(painter, text, se + away * (half + 2.0), angle, font);
                layout.leader = vec![sp + away * ORDINATE_GAP, se];
                layout.end = LeaderEnd::None;
                layout.text = Some(t);
            }
        }
        out.push(layout);
    }
    out
}

/// Draw an annotation's text and leader (centre lines are drawn by the GPU).
pub fn paint_annotation(
    pen: &dyn Pen,
    layout: &NoteLayout,
    state: AnnotationState,
    colors: &AnnotationColors,
    style: &DimStyle,
) {
    let color = match state {
        AnnotationState::Normal => colors.normal,
        AnnotationState::Hovered => colors.hovered,
        AnnotationState::Selected => colors.selected,
        AnnotationState::Conflict => colors.conflict,
        AnnotationState::Preview => colors.preview,
    };
    let stroke = Stroke::new(1.0, color);
    if layout.leader.len() >= 2 {
        pen.line(&layout.leader, stroke);
        let (tip, next) = (layout.leader[0], layout.leader[1]);
        let d = (tip - next).normalized();
        let d = if d.is_finite() { d } else { vec2(1.0, 0.0) };
        match layout.end {
            LeaderEnd::Arrow => {
                let len = style.arrow_len;
                let base = tip - d * len;
                let n = d.rot90() * (len / 3.0);
                pen.polygon(vec![tip, base + n, base - n], color);
            }
            LeaderEnd::Dot => {
                pen.dot(tip, (style.arrow_len / 3.5).max(2.0), color);
            }
            LeaderEnd::None => {}
        }
    }
    if let Some(t) = &layout.text {
        let galley = pen.painter().layout(
            t.text.clone(),
            FontId::proportional(style.text),
            color,
            f32::INFINITY,
        );
        let rot = egui::emath::Rot2::from_angle(t.angle);
        let pos = t.center + rot * (-galley.size() / 2.0);
        pen.text(pos, galley, t.angle, color);
        if state != AnnotationState::Normal {
            pen.highlight(t.rect, color);
        }
    }
    if state != AnnotationState::Normal && layout.text.is_none() {
        // Centre marks: mark their ends so a picked one shows.
        for [a, b] in &layout.segments {
            pen.dot(*a, 2.0, color);
            pen.dot(*b, 2.0, color);
        }
    }
}

/// Font size for drafting text at the document's style (logical points).
pub fn text_size(doc: &Document) -> f32 {
    (doc.draft_style().text_height as f32 * PT_PER_MM).clamp(8.0, 40.0)
}

/// The anchor for a leader tip at the pointer: a snapped point, a spot on a curve, or free.
pub fn anchor_at(
    doc: &Document,
    point: Option<EntityId>,
    curve: Option<EntityId>,
    at: DVec2,
) -> Anchor {
    if let Some(p) = point {
        return Anchor::Point(p);
    }
    if let Some(c) = curve {
        match doc.shape(c) {
            Some(Shape::Line { a, b }) => {
                let d = b - a;
                let t = if d.length_squared() > 0.0 {
                    ((at - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return Anchor::Curve { curve: c, t };
            }
            Some(Shape::Circle { center, .. } | Shape::Arc { center, .. }) => {
                let v = at - center;
                return Anchor::Curve {
                    curve: c,
                    t: v.y.atan2(v.x),
                };
            }
            _ => {}
        }
    }
    Anchor::Free(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hole_callouts_count_equal_holes() {
        let mut doc = Document::new();
        let mut circles = Vec::new();
        for x in [0.0, 10.0, 20.0] {
            let c = doc.add_point(DVec2::new(x, 0.0));
            circles.push(doc.add_circle(c, 3.0).unwrap());
        }
        let c = doc.add_point(DVec2::new(40.0, 0.0));
        let big = doc.add_circle(c, 5.0).unwrap();
        assert_eq!(
            hole_callout_text(&doc, circles[0], "THRU").as_deref(),
            Some("3× Ø6 THRU")
        );
        assert_eq!(hole_callout_text(&doc, big, "").as_deref(), Some("Ø10"));
    }

    #[test]
    fn chamfer_callouts_measure_legs_to_the_corner() {
        let mut doc = Document::new();
        // A 10 × 10 corner at the origin, chamfered 2 × 2 (and 2 × 3).
        let p = [(10.0, 0.0), (2.0, 0.0), (0.0, 2.0), (0.0, 10.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        doc.add_line(p[0], p[1]).unwrap();
        let chamfer = doc.add_line(p[1], p[2]).unwrap();
        doc.add_line(p[2], p[3]).unwrap();
        assert_eq!(chamfer_text(&doc, chamfer).as_deref(), Some("2 × 45°"));
        doc.set_point(p[2], DVec2::new(0.0, 3.0)).unwrap();
        assert_eq!(chamfer_text(&doc, chamfer).as_deref(), Some("2 × 3"));
    }

    #[test]
    fn centre_marks_and_lines() {
        let mut doc = Document::new();
        let c = doc.add_point(DVec2::new(5.0, 5.0));
        let circle = doc.add_circle(c, 4.0).unwrap();
        let mark = centre_segments(&doc, &Annotation::CentreMark { curve: circle });
        assert_eq!(mark.len(), 2);
        assert!((mark[0].0.distance(mark[0].1) - 2.0 * (4.0 + 2.5)).abs() < 1e-12);
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let bottom = doc.add_line(p[0], p[1]).unwrap();
        let top = doc.add_line(p[2], p[3]).unwrap(); // runs the other way
        let mid = centre_segments(&doc, &Annotation::Centreline { a: bottom, b: top });
        assert_eq!(mid.len(), 1);
        assert!((mid[0].0.y - 2.0).abs() < 1e-12 && (mid[0].1.y - 2.0).abs() < 1e-12);
        assert!((mid[0].0.x + 2.5).abs() < 1e-12 && (mid[0].1.x - 12.5).abs() < 1e-12);
    }

    #[test]
    fn ordinates_and_anchors() {
        let mut doc = Document::new();
        let o = doc.add_point(DVec2::new(1.0, 1.0));
        let p = doc.add_point(DVec2::new(26.0, 11.0));
        assert_eq!(ordinate_value(&doc, o, p, OrdinateAxis::X), Some(25.0));
        assert_eq!(ordinate_value(&doc, o, p, OrdinateAxis::Y), Some(10.0));
        let line = doc.add_line(o, p).unwrap();
        let a = anchor_at(&doc, None, Some(line), DVec2::new(13.5, 6.0));
        assert_eq!(doc.anchor_pos(&a), Some(DVec2::new(13.5, 6.0)));
        assert_eq!(
            anchor_at(&doc, Some(p), None, DVec2::ZERO),
            Anchor::Point(p)
        );
    }
}
