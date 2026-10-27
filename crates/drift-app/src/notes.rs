//! Drafting annotations on the canvas: notes and leaders, centre marks and centre lines, hole
//! and chamfer callouts, and ordinate dimensions. Their text and leaders are laid out in screen
//! space (like dimensions); centre marks and centre lines are world geometry drawn by the GPU.

use drift_core::{
    Anchor, Annotation, AnnotationId, DVec2, Document, DraftStyle, Emphasis, EntityId, EntityKind,
    Leader, LeaderEnd, LeaderShape, LineKind, NoteFrame, NoteStyle, OrdinateAxis, Shape,
    SketchItem, TextAlign,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, vec2};

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
    /// How it's set: size (logical points), weight and slant, the width it wraps to, and how
    /// its lines align.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub wrap: f32,
    pub align: egui::Align,
    /// The height of its first line (where leaders land).
    pub first_line: f32,
}

impl NoteText {
    /// The text laid out in `color`.
    pub fn galley(&self, painter: &egui::Painter, color: Color32) -> std::sync::Arc<egui::Galley> {
        galley(
            painter,
            &self.text,
            self.size,
            self.bold,
            self.italic,
            self.wrap,
            self.align,
            color,
        )
    }
}

/// Lay out text in the drafting font.
#[allow(clippy::too_many_arguments)]
fn galley(
    painter: &egui::Painter,
    text: &str,
    size: f32,
    bold: bool,
    italic: bool,
    wrap: f32,
    align: egui::Align,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: crate::fonts::drafting_font(size, bold),
            color,
            italics: italic,
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap;
    job.halign = align;
    painter.layout_job(job)
}

/// A leader as drawn: from its tip (on the geometry) to the text.
#[derive(Clone, Debug)]
pub struct LeaderPath {
    pub points: Vec<Pos2>,
    pub end: LeaderEnd,
    /// A bent leader's bend (its handle).
    pub bend: Option<Pos2>,
    /// Where the leader meets its shoulder (the handle that sets the shoulder's length), and
    /// where the shoulder meets the text.
    pub shoulder: Option<Pos2>,
    pub landing: Pos2,
    /// A curved leader's middle (its handle), and the ends of the chord it bows from.
    pub curve: Option<(Pos2, [Pos2; 2])>,
}

/// A piece of a section or elevation mark, on screen, in drawing order.
#[derive(Clone, Debug)]
pub enum Mark {
    /// A thin line through these points.
    Stroke(Vec<Pos2>),
    /// A filled convex shape (a flag's pointer, a cutting line's heavy end).
    Solid(Vec<Pos2>),
    /// A paper-coloured patch (the inside of a flag's ring).
    Blank(Vec<Pos2>),
    /// Text at a size (logical points).
    Text(NoteText, f32),
}

/// An annotation laid out for drawing and picking.
#[derive(Clone, Debug)]
pub struct NoteLayout {
    pub id: AnnotationId,
    pub text: Option<NoteText>,
    /// Leaders, in the note's order.
    pub leaders: Vec<LeaderPath>,
    /// A note's frame round its text, and whether paper shows behind the text.
    pub frame: NoteFrame,
    pub mask: bool,
    /// Centre lines, on screen (for picking; the GPU draws them).
    pub segments: Vec<[Pos2; 2]>,
    /// The pieces of a section or elevation mark.
    pub marks: Vec<Mark>,
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
        let on_mark = |m: &Mark| match m {
            Mark::Stroke(points) => points.windows(2).any(|w| near(w[0], w[1])),
            Mark::Blank(points) => Rect::from_points(points).contains(p),
            Mark::Solid(_) | Mark::Text(..) => false,
        };
        self.segments.iter().any(|[a, b]| near(*a, *b))
            || self.leader_at(p).is_some()
            || self.marks.iter().any(on_mark)
    }

    /// The leader under `p` (its index), if any.
    pub fn leader_at(&self, p: Pos2) -> Option<usize> {
        let near = |a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = if ab.length_sq() > 0.0 {
                ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a + ab * t - p).length() <= 4.0
        };
        self.leaders
            .iter()
            .position(|l| l.points.windows(2).any(|w| near(w[0], w[1])))
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
        EntityKind::Ellipse { center, major, .. }
        | EntityKind::EllipseArc { center, major, .. } => {
            let c = doc.point(center).ok()?;
            Some((c, c.distance(doc.point(major).ok()?)))
        }
        EntityKind::Point(p) => Some((p, 0.0)),
        EntityKind::Line { .. } | EntityKind::Bezier { .. } => None,
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
    shown: &dyn Fn(AnnotationId) -> bool,
) -> Vec<SketchItem> {
    let mut out = Vec::new();
    for (id, a) in doc.annotations().filter(|(id, _)| shown(*id)) {
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
            drift_geom::intersect::line_line(pa, ua, pb, ub)
                .map(|x| (x.at.distance(pa), x.at.distance(pb)))
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
        Annotation::Note { at, leaders, .. } => doc.note_text_pos(*at, leaders),
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
        // A mark is dragged whole, by its first end.
        Annotation::Section { a, .. } => Some(*a),
        Annotation::Elevation { at, .. } => Some(*at),
    }
}

/// The annotation with its label moved to `to` (world).
pub fn with_label_at(doc: &Document, a: &Annotation, to: DVec2) -> Option<Annotation> {
    let mut a = a.clone();
    match &mut a {
        Annotation::Note { at, leaders, .. } => *at = doc.note_at(to, leaders)?,
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
        Annotation::Section { a, b, .. } => {
            *b += to - *a;
            *a = to;
        }
        Annotation::Elevation { at, .. } => *at = to,
    }
    Some(a)
}

/// What a section or elevation mark's flag reads, over and under its rule: the linked view's
/// number and sheet number (`–` and `–` while the view is on no sheet), or else the mark's
/// own label and sheet. Worked out as it is drawn, so renumbering a sheet or moving a view
/// follows. `None` for other annotations.
pub fn mark_reference(doc: &Document, a: &Annotation) -> Option<(String, String)> {
    let (label, sheet, view) = match a {
        Annotation::Section {
            label, sheet, view, ..
        }
        | Annotation::Elevation {
            label, sheet, view, ..
        } => (label, sheet, *view),
        _ => return None,
    };
    Some(match view {
        Some(v) => doc
            .view_reference(v)
            .unwrap_or_else(|| ("–".to_owned(), "–".to_owned())),
        None => (label.clone(), sheet.clone()),
    })
}

/// The next unused label for a new section (`A`, `B` … then `A1` …) or elevation (`1`,
/// `2` …).
pub fn next_mark_label(doc: &Document, section: bool) -> String {
    let used: Vec<&str> = doc
        .annotations()
        .filter_map(|(_, a)| match a {
            Annotation::Section { label, .. } if section => Some(label.as_str()),
            Annotation::Elevation { label, .. } if !section => Some(label.as_str()),
            _ => None,
        })
        .collect();
    let free = |label: &String| !used.contains(&label.as_str());
    if section {
        (0..)
            .flat_map(|round: u32| {
                ('A'..='Z').map(move |c| {
                    if round == 0 {
                        c.to_string()
                    } else {
                        format!("{c}{round}")
                    }
                })
            })
            .find(free)
            .expect("an unbounded supply")
    } else {
        (1u32..)
            .map(|n| n.to_string())
            .find(free)
            .expect("an unbounded supply")
    }
}

/// A flag: a ring at `c` holding the label (over the sheet it refers to, if given), with a
/// solid pointer towards `look` (a unit vector on screen).
fn flag(
    painter: &egui::Painter,
    c: Pos2,
    look: egui::Vec2,
    label: &str,
    sheet: &str,
    font: f32,
    marks: &mut Vec<Mark>,
) {
    let r = flag_radius(font);
    let side = look.rot90();
    marks.push(Mark::Solid(vec![
        c + side * r,
        c + look * (r * 1.62),
        c - side * r,
    ]));
    let ring: Vec<Pos2> = (0..=40)
        .map(|k| {
            let a = k as f32 / 40.0 * std::f32::consts::TAU;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    marks.push(Mark::Blank(ring.clone()));
    marks.push(Mark::Stroke(ring));
    let shown = if label.trim().is_empty() {
        " "
    } else {
        label.trim()
    };
    if sheet.trim().is_empty() {
        marks.push(Mark::Text(
            place_text(painter, shown.to_owned(), c, 0.0, font),
            font,
        ));
    } else {
        // The label over a rule, and under it the sheet (smaller, to fit the ring).
        marks.push(Mark::Stroke(vec![c - vec2(r, 0.0), c + vec2(r, 0.0)]));
        let small = font * 0.72;
        marks.push(Mark::Text(
            place_text(
                painter,
                shown.to_owned(),
                c - vec2(0.0, r * 0.45),
                0.0,
                font * 0.9,
            ),
            font * 0.9,
        ));
        marks.push(Mark::Text(
            place_text(
                painter,
                sheet.trim().to_owned(),
                c + vec2(0.0, r * 0.45),
                0.0,
                small,
            ),
            small,
        ));
    }
}

/// The radius of a mark's ring for a text size (logical points).
pub fn flag_radius(font: f32) -> f32 {
    font * 1.15 + 3.0
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
    let set = galley(
        painter,
        &text,
        font,
        false,
        false,
        f32::INFINITY,
        egui::Align::Center,
        Color32::WHITE,
    );
    let first_line = set.rows.first().map_or(font, |r| r.size.y);
    let galley = set;
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
        size: font,
        bold: false,
        italic: false,
        wrap: f32::INFINITY,
        align: egui::Align::Center,
        first_line,
    }
}

/// A note's text set in its style, centred at `center`.
fn place_note_text(
    painter: &egui::Painter,
    text: String,
    center: Pos2,
    style: &NoteStyle,
    font: f32,
) -> NoteText {
    let size = style
        .height
        .map_or(font, |h| (h as f32 * PT_PER_MM).clamp(4.0, 200.0));
    let wrap = style
        .wrap
        .map_or(f32::INFINITY, |w| (w as f32 * PT_PER_MM).max(size * 2.0));
    let align = match style.align {
        TextAlign::Left => egui::Align::Min,
        TextAlign::Center => egui::Align::Center,
        TextAlign::Right => egui::Align::Max,
    };
    let set = galley(
        painter,
        &text,
        size,
        style.bold,
        style.italic,
        wrap,
        align,
        Color32::WHITE,
    );
    let first_line = set.rows.first().map_or(size, |r| r.size.y);
    let rect = Rect::from_center_size(center, set.size());
    NoteText {
        text,
        center,
        angle: 0.0,
        rect: rect.expand2(vec2(3.0, 1.0)),
        size,
        bold: style.bold,
        italic: style.italic,
        wrap,
        align,
        first_line,
    }
}

/// A note's leader on screen: from its tip, through its bend or along its curve, to the
/// nearer side of the text's first line, with a shoulder if it has one.
fn note_leader(tip: Pos2, bend: Option<Pos2>, l: &Leader, text: &NoteText) -> LeaderPath {
    let from = bend.unwrap_or(tip);
    let left = from.x <= text.center.x;
    let landing = egui::pos2(
        if left {
            text.rect.left()
        } else {
            text.rect.right()
        },
        text.rect.top() + 1.0 + text.first_line / 2.0,
    );
    let reach = l.shoulder_len as f32 * PT_PER_MM;
    let shoulder = landing + vec2(if left { -reach } else { reach }, 0.0);
    let into = if l.shoulder { shoulder } else { landing };
    let mut points = vec![tip];
    let mut curve = None;
    match l.shape {
        LeaderShape::Straight => {}
        LeaderShape::Bent(_) => points.extend(bend),
        LeaderShape::Curved(k) => {
            // An arc through the tip and the shoulder, bowed by `k` of its length.
            let chord = into - tip;
            let n = chord.rot90() * k as f32;
            let mid = tip + chord * 0.5 + n;
            curve = Some((mid, [tip, into]));
            for i in 1..16 {
                let t = i as f32 / 16.0;
                // A quadratic through the bowed midpoint (control point = 2·mid − ends/2).
                let c = mid * 2.0 - (tip.to_vec2() + into.to_vec2()) * 0.5;
                let a = tip.to_vec2() * (1.0 - t) * (1.0 - t)
                    + c.to_vec2() * 2.0 * t * (1.0 - t)
                    + into.to_vec2() * t * t;
                points.push(a.to_pos2());
            }
        }
    }
    points.push(into);
    if l.shoulder {
        points.push(landing);
    }
    LeaderPath {
        points,
        end: l.end,
        bend,
        shoulder: l.shoulder.then_some(into),
        landing,
        curve,
    }
}

/// The bow (as [`LeaderShape::Curved`] holds it) that puts a curved leader's middle at `p`,
/// given the chord it bows from.
pub fn bow_through(p: Pos2, [a, b]: [Pos2; 2]) -> f64 {
    let chord = b - a;
    let len2 = chord.length_sq();
    if len2 <= 0.0 {
        return 0.0;
    }
    f64::from((p - (a + chord * 0.5)).dot(chord.rot90()) / len2).clamp(-1.0, 1.0)
}

/// A callout's leader, with an arrow.
fn callout_leader(tip: Pos2, text: &NoteText, end: LeaderEnd) -> LeaderPath {
    let points = leader_to(tip, text);
    LeaderPath {
        landing: *points.last().expect("three points"),
        points,
        end,
        bend: None,
        shoulder: None,
        curve: None,
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
    let s = |p: DVec2| map.to_screen(p);
    let mut out = Vec::new();
    for (id, a) in doc.annotations() {
        // The drawing's style, with the annotation's own settings in place.
        let style: DraftStyle = doc.draft_style_for(drift_core::AnnotationRef::Annotation(id));
        let font = DimStyle::of(&style).text;
        let mut layout = NoteLayout {
            id,
            text: None,
            leaders: Vec::new(),
            frame: NoteFrame::None,
            mask: false,
            segments: centre_segments(doc, a)
                .into_iter()
                .map(|(p, q)| [s(p), s(q)])
                .collect(),
            marks: Vec::new(),
        };
        let (label, sheet) = mark_reference(doc, a).unwrap_or_default();
        match a {
            Annotation::Note {
                text,
                at,
                leaders,
                style,
            } => {
                let Some(pos) = doc.note_text_pos(*at, leaders) else {
                    continue;
                };
                let shown = if text.is_empty() { " " } else { text };
                let t = place_note_text(painter, shown.to_owned(), s(pos), style, font);
                for l in leaders {
                    let Some(tip) = doc.anchor_pos(&l.to) else {
                        continue;
                    };
                    let bend = match l.shape {
                        LeaderShape::Bent(b) => Some(s(tip + b)),
                        _ => None,
                    };
                    layout.leaders.push(note_leader(s(tip), bend, l, &t));
                }
                layout.frame = style.frame;
                layout.mask = style.mask;
                layout.text = Some(t);
            }
            Annotation::CentreMark { .. } | Annotation::Centreline { .. } => {}
            Annotation::Section { a, b, flip, .. } => {
                let (sa, sb) = (s(*a), s(*b));
                let along = (sb - sa).normalized();
                let along = if along.is_finite() {
                    along
                } else {
                    vec2(1.0, 0.0)
                };
                // The section looks to the left of a → b (in the sketch, y up), or the right.
                let world =
                    (*b - *a).normalize_or(DVec2::X).perp() * if *flip { -1.0 } else { 1.0 };
                let look = vec2(world.x as f32, -world.y as f32);
                let r = flag_radius(font);
                // The cutting line, heavy where it leaves each flag.
                layout.marks.push(Mark::Stroke(vec![sa, sb]));
                let heavy = (r * 1.4).min((sb - sa).length() * 0.3);
                let w = look * 1.1;
                for (end, dir) in [(sa, along), (sb, -along)] {
                    layout.marks.push(Mark::Solid(vec![
                        end + w,
                        end + dir * heavy + w,
                        end + dir * heavy - w,
                        end - w,
                    ]));
                }
                for (end, out) in [(sa, -along), (sb, along)] {
                    flag(
                        painter,
                        end + out * r,
                        look,
                        &label,
                        &sheet,
                        font,
                        &mut layout.marks,
                    );
                }
            }
            Annotation::Elevation { at, angle, .. } => {
                let d = DVec2::from_angle(angle.to_radians());
                let look = vec2(d.x as f32, -d.y as f32);
                flag(
                    painter,
                    s(*at),
                    look,
                    &label,
                    &sheet,
                    font,
                    &mut layout.marks,
                );
            }
            Annotation::HoleCallout { curve, at, suffix } => {
                let (Some((c, r)), Some(text)) =
                    (centre(doc, *curve), hole_callout_text(doc, *curve, suffix))
                else {
                    continue;
                };
                let t = place_text(painter, text, s(c + *at), 0.0, font);
                let dir = (*at).normalize_or(DVec2::X);
                layout
                    .leaders
                    .push(callout_leader(s(c + dir * r), &t, LeaderEnd::Arrow));
                layout.text = Some(t);
            }
            Annotation::ChamferCallout { line, at } => {
                let (Some((p, q)), Some(text)) = (line_ends(doc, *line), chamfer_text(doc, *line))
                else {
                    continue;
                };
                let mid = (p + q) * 0.5;
                let t = place_text(painter, text, s(mid + *at), 0.0, font);
                layout
                    .leaders
                    .push(callout_leader(s(mid), &t, LeaderEnd::Arrow));
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
                layout.leaders.push(LeaderPath {
                    points: vec![sp + away * ORDINATE_GAP, se],
                    end: LeaderEnd::None,
                    bend: None,
                    shoulder: None,
                    landing: se,
                    curve: None,
                });
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
    for mark in &layout.marks {
        match mark {
            Mark::Stroke(points) => pen.line(points, stroke),
            Mark::Solid(points) => pen.polygon(points.clone(), color),
            Mark::Blank(points) => pen.mask(points.clone(), colors.text_background),
            Mark::Text(t, size) => {
                let galley = pen.painter().layout(
                    t.text.clone(),
                    crate::fonts::drafting_font(*size, false),
                    color,
                    f32::INFINITY,
                );
                pen.text(t.center - galley.size() / 2.0, galley, 0.0, color);
            }
        }
    }
    // Paper behind a masked note's text (and frame), under its leaders.
    if layout.mask
        && let Some(t) = &layout.text
    {
        let r = t.rect.expand(2.0);
        pen.mask(
            vec![
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ],
            colors.text_background,
        );
    }
    for leader in &layout.leaders {
        paint_leader(pen, leader, stroke, color, style);
    }
    if let Some(t) = &layout.text {
        if layout.frame != NoteFrame::None {
            pen.line(&frame_outline(t.rect.expand(2.0), layout.frame), stroke);
        }
        let galley = t.galley(pen.painter(), color);
        let rot = egui::emath::Rot2::from_angle(t.angle);
        // Centred on what the text covers: aligned (centre or right) lines start left of
        // the galley's origin, so its size alone would shift them sideways.
        let pos = t.center + rot * (-galley.rect.center().to_vec2());
        pen.text(pos, galley, t.angle, color);
        if state != AnnotationState::Normal {
            pen.highlight(t.rect, color);
        }
    }
    if state != AnnotationState::Normal && layout.text.is_none() && layout.marks.is_empty() {
        // Centre marks: mark their ends so a picked one shows.
        for [a, b] in &layout.segments {
            pen.dot(*a, 2.0, color);
            pen.dot(*b, 2.0, color);
        }
    }
}

/// A leader's line and its end mark.
pub fn paint_leader(
    pen: &dyn Pen,
    leader: &LeaderPath,
    stroke: Stroke,
    color: Color32,
    style: &DimStyle,
) {
    let points = &leader.points;
    if points.len() < 2 {
        return;
    }
    pen.line(points, stroke);
    let (tip, next) = (points[0], points[1]);
    let d = (tip - next).normalized();
    let d = if d.is_finite() { d } else { vec2(1.0, 0.0) };
    let len = style.arrow_len;
    let base = tip - d * len;
    let n = d.rot90() * (len / 3.0);
    match leader.end {
        LeaderEnd::Arrow => pen.polygon(vec![tip, base + n, base - n], color),
        LeaderEnd::OpenArrow => {
            pen.line(&[base + n, tip, base - n], stroke);
        }
        LeaderEnd::Dot => pen.dot(tip, (len / 3.5).max(2.0), color),
        LeaderEnd::None => {}
    }
}

/// The outline of a note's frame.
fn frame_outline(r: Rect, frame: NoteFrame) -> Vec<Pos2> {
    match frame {
        NoteFrame::Rounded => {
            let k = (r.height().min(r.width()) * 0.3).min(6.0);
            let mut points = Vec::new();
            for (c, a0) in [
                (r.right_top() + vec2(-k, k), -90.0_f32),
                (r.right_bottom() + vec2(-k, -k), 0.0),
                (r.left_bottom() + vec2(k, -k), 90.0),
                (r.left_top() + vec2(k, k), 180.0),
            ] {
                for i in 0..=6 {
                    let a = (a0 + 15.0 * i as f32).to_radians();
                    points.push(c + vec2(a.cos(), a.sin()) * k);
                }
            }
            points.push(points[0]);
            points
        }
        _ => vec![
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ],
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

    #[test]
    fn marks_take_the_next_free_label() {
        let mut doc = Document::new();
        assert_eq!(next_mark_label(&doc, true), "A");
        assert_eq!(next_mark_label(&doc, false), "1");
        for label in ["A", "C"] {
            doc.add_annotation(Annotation::Section {
                a: DVec2::ZERO,
                b: DVec2::X,
                label: label.into(),
                sheet: String::new(),
                flip: false,
                view: None,
            })
            .unwrap();
        }
        doc.add_annotation(Annotation::Elevation {
            at: DVec2::ZERO,
            angle: 0.0,
            label: "1".into(),
            sheet: String::new(),
            view: None,
        })
        .unwrap();
        // Gaps are filled; sections and elevations are numbered apart.
        assert_eq!(next_mark_label(&doc, true), "B");
        assert_eq!(next_mark_label(&doc, false), "2");
    }
}
