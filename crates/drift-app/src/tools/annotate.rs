//! Drafting tools: notes with leaders, centre marks and centre lines, hole and chamfer
//! callouts, and ordinate dimensions.

use drift_core::{Annotation, Leader, OrdinateAxis};

use super::*;
use crate::notes;

/// Paper millimetres between the geometry and a new row of ordinate labels.
const ORDINATE_CLEARANCE_MM: f64 = 8.0;

fn add_annotation(ed: &mut Editor, a: Annotation, label: &str) -> Option<drift_core::AnnotationId> {
    ed.doc.begin();
    match ed.doc.add_annotation(a) {
        Ok(id) => {
            ed.commit(label);
            ed.selection.clear();
            ed.selected_fills.clear();
            ed.selected_annotation = Some(id);
            Some(id)
        }
        Err(e) => {
            ed.doc.rollback();
            ed.notice = Some(e.to_string());
            None
        }
    }
}

/// The curve or point under the pointer, preferring circles and arcs (centre lines often
/// cross them).
fn pick_prefer_round(ed: &Editor, p: &Pointer, points: bool) -> Option<EntityId> {
    let ok = |id: EntityId, round: bool| {
        ed.doc.get(id).is_some_and(|e| {
            if round {
                is_round(e.kind)
            } else {
                points || !e.kind.is_point()
            }
        })
    };
    ed.index
        .pick(p.raw, p.tolerance, |id| ok(id, true))
        .or_else(|| ed.index.pick(p.raw, p.tolerance, |id| ok(id, false)))
}

fn preview_leader(from: DVec2, to: DVec2) -> SketchItem {
    preview(Shape::Line { a: from, b: to })
}

// ---------------------------------------------------------------------------------------------
// Note

/// Click empty space for a note, or click geometry and then the text position for a note
/// with a leader. The text is typed on the canvas.
#[derive(Default)]
pub struct NoteTool {
    /// What its leaders point at (geometry clicked before the text; Shift+click adds more).
    targets: Vec<drift_core::Anchor>,
    hover: Option<DVec2>,
}

impl Tool for NoteTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Note
    }

    fn hint(&self) -> String {
        match self.targets.len() {
            0 => "Click to place a note, or click geometry first for a leader to it".into(),
            1 => "Click where the text goes · Shift+click more geometry for more leaders".into(),
            n => format!("{n} leaders · click where the text goes · Shift+click for another"),
        }
    }

    fn is_busy(&self) -> bool {
        !self.targets.is_empty()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => self.hover = Some(p.pos()),
            Input::Click(p, mods) => {
                // Geometry first (or, with leaders started, Shift+geometry): another leader.
                if (self.targets.is_empty() || mods.shift)
                    && let Some(to) = leader_target(ed, &p)
                {
                    self.targets.push(to);
                    return;
                }
                let leaders: Vec<Leader> = std::mem::take(&mut self.targets)
                    .into_iter()
                    .map(Leader::to)
                    .collect();
                let Some(at) = ed.doc.note_at(p.pos(), &leaders) else {
                    return;
                };
                let note = Annotation::Note {
                    text: String::new(),
                    at,
                    leaders,
                    style: Default::default(),
                };
                ed.pending_note_edit = add_annotation(ed, note, "Note");
            }
            Input::Finish => self.targets.clear(),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let busy = !self.targets.is_empty();
        self.targets.clear();
        busy
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let Some(to) = self.hover else {
            return Vec::new();
        };
        self.targets
            .iter()
            .filter_map(|t| ed.doc.anchor_pos(t))
            .flat_map(|tip| [preview_leader(tip, to), preview_vertex(tip)])
            .collect()
    }
}

/// What a leader clicked at `p` points to: a point under the pointer, else the spot on the
/// line or curve under it (whatever the automatic relations setting), else nothing.
fn leader_target(ed: &Editor, p: &Pointer) -> Option<drift_core::Anchor> {
    if let Some(point) = p.snapped_point() {
        return Some(drift_core::Anchor::Point(point));
    }
    let curve = match select::pick_target_with(ed, p, crate::editor::PickFilter::ALL)? {
        select::Target::Entity(id) | select::Target::Midpoint(id) => id,
    };
    if ed.doc.get(curve)?.kind.is_point() {
        return Some(drift_core::Anchor::Point(curve));
    }
    // A spot on the curve: where the pointer is, onto the curve.
    let at = drift_geom::nearest_point(&ed.doc.shape(curve)?, p.raw);
    Some(notes::anchor_at(&ed.doc, None, Some(curve), at))
}

// ---------------------------------------------------------------------------------------------
// Centre marks and centre lines

/// Click a circle or arc for a centre mark; then another circle, arc or point for a centre line
/// through both centres. Two lines give a centre line midway between them.
#[derive(Default)]
pub struct CentreMarkTool {
    first: Option<EntityId>,
}

fn is_round(kind: EntityKind) -> bool {
    matches!(kind, EntityKind::Circle { .. } | EntityKind::Arc { .. })
}

impl Tool for CentreMarkTool {
    fn kind(&self) -> ToolKind {
        ToolKind::CentreMark
    }

    fn hint(&self) -> String {
        match self.first {
            Some(_) => "Click another circle, arc, point or line for a centre line between them \
                        (Esc to stop)"
                .into(),
            None => {
                "Click a circle or arc for a centre mark, or two lines for a centre line".into()
            }
        }
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                ed.hovered = pick_prefer_round(ed, &p, true);
            }
            Input::Click(p, _) => {
                let Some(hit) = pick_prefer_round(ed, &p, true) else {
                    self.first = None;
                    ed.highlight.clear();
                    return;
                };
                let Some(kind) = ed.doc.get(hit).map(|e| e.kind) else {
                    return;
                };
                let first_kind = self.first.and_then(|f| ed.doc.get(f)).map(|e| e.kind);
                let is_line = |k: EntityKind| matches!(k, EntityKind::Line { .. });
                match (self.first, first_kind) {
                    (Some(a), Some(k)) if a != hit && is_line(k) == is_line(kind) => {
                        add_annotation(ed, Annotation::Centreline { a, b: hit }, "Centre line");
                        self.first = None;
                    }
                    _ => {
                        let marked = ed.doc.annotations().any(
                            |(_, x)| matches!(x, Annotation::CentreMark { curve } if *curve == hit),
                        );
                        if is_round(kind) && !marked {
                            add_annotation(
                                ed,
                                Annotation::CentreMark { curve: hit },
                                "Centre mark",
                            );
                        }
                        self.first = Some(hit);
                    }
                }
                ed.highlight = self.first.into_iter().collect();
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        ed.highlight.clear();
        self.first.take().is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// Callouts

#[derive(Clone, Copy, Debug, PartialEq)]
enum CalloutTarget {
    Hole(EntityId),
    Chamfer(EntityId),
}

/// Click a hole (circle) or a chamfer (line), then where the text goes.
#[derive(Default)]
pub struct CalloutTool {
    target: Option<CalloutTarget>,
    hover: Option<DVec2>,
}

impl CalloutTool {
    fn anchor(&self, ed: &Editor) -> Option<DVec2> {
        match self.target? {
            CalloutTarget::Hole(c) => match ed.doc.shape(c)? {
                Shape::Circle { center, .. } | Shape::Arc { center, .. } => Some(center),
                _ => None,
            },
            CalloutTarget::Chamfer(l) => match ed.doc.shape(l)? {
                Shape::Line { a, b } => Some((a + b) * 0.5),
                _ => None,
            },
        }
    }
}

impl Tool for CalloutTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Callout
    }

    fn hint(&self) -> String {
        match self.target {
            Some(_) => "Click where the callout text goes".into(),
            None => "Click a hole for 4× Ø6 THRU, or a chamfer line for 2 × 45°".into(),
        }
    }

    fn is_busy(&self) -> bool {
        self.target.is_some()
    }

    fn wants_snap(&self) -> bool {
        self.target.is_some()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => {
                self.hover = Some(p.pos());
                if self.target.is_none() {
                    ed.hovered = pick_prefer_round(ed, &p, false);
                }
            }
            Input::Click(p, _) => match self.target {
                None => {
                    let hit = pick_prefer_round(ed, &p, false);
                    self.target = match hit.and_then(|h| Some((h, ed.doc.get(h)?.kind))) {
                        Some((h, k)) if is_round(k) => Some(CalloutTarget::Hole(h)),
                        Some((h, EntityKind::Line { .. })) => Some(CalloutTarget::Chamfer(h)),
                        _ => {
                            ed.notice = Some("Click a circle (hole) or a line (chamfer)".into());
                            None
                        }
                    };
                    ed.highlight = hit.into_iter().collect();
                }
                Some(target) => {
                    let Some(base) = self.anchor(ed) else {
                        self.target = None;
                        return;
                    };
                    let at = p.pos() - base;
                    let a = match target {
                        CalloutTarget::Hole(curve) => Annotation::HoleCallout {
                            curve,
                            at,
                            suffix: "THRU".into(),
                        },
                        CalloutTarget::Chamfer(line) => Annotation::ChamferCallout { line, at },
                    };
                    add_annotation(ed, a, "Callout");
                    self.target = None;
                    ed.highlight.clear();
                }
            },
            Input::Finish => self.target = None,
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        ed.highlight.clear();
        self.target.take().is_some()
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let tip = match self.target {
            Some(CalloutTarget::Hole(c)) => match ed.doc.shape(c) {
                Some(Shape::Circle { center, radius } | Shape::Arc { center, radius, .. }) => self
                    .hover
                    .map(|h| center + (h - center).normalize_or(DVec2::X) * radius),
                _ => None,
            },
            Some(CalloutTarget::Chamfer(_)) => self.anchor(ed),
            None => None,
        };
        match (tip, self.hover) {
            (Some(tip), Some(to)) => vec![preview_leader(tip, to)],
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Ordinate dimensions

/// Click the origin point, then the points to dimension. X / Y choose which coordinate.
pub struct OrdinateTool {
    origin: Option<EntityId>,
    axis: OrdinateAxis,
}

impl Default for OrdinateTool {
    fn default() -> Self {
        Self {
            origin: None,
            axis: OrdinateAxis::X,
        }
    }
}

impl OrdinateTool {
    /// Where new labels go: in line with the previous ordinate of this axis, or a row clear
    /// of the geometry (below it for X, left of it for Y).
    fn label_offset(&self, ed: &Editor, origin: EntityId) -> f64 {
        let previous = ed.doc.annotations().filter_map(|(_, a)| match a {
            Annotation::Ordinate {
                origin: o,
                axis,
                at,
                ..
            } if *o == origin && *axis == self.axis => Some(*at),
            _ => None,
        });
        if let Some(at) = previous.last() {
            return at;
        }
        let o = ed.doc.ref_point(origin).unwrap_or_default();
        let clear = ORDINATE_CLEARANCE_MM * ed.doc.draft_style().scale;
        let b = ed.bounds();
        match self.axis {
            OrdinateAxis::X => b.min.y - o.y - clear,
            OrdinateAxis::Y => b.min.x - o.x - clear,
        }
    }

    fn add(&self, ed: &mut Editor, origin: EntityId, point: EntityId) {
        let at = self.label_offset(ed, origin);
        let a = Annotation::Ordinate {
            origin,
            point,
            axis: self.axis,
            at,
        };
        add_annotation(ed, a, "Ordinate dimension");
    }
}

impl Tool for OrdinateTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Ordinate
    }

    fn hint(&self) -> String {
        let axis = match self.axis {
            OrdinateAxis::X => "X (press Y for Y)",
            OrdinateAxis::Y => "Y (press X for X)",
        };
        match self.origin {
            None => format!("Click the origin point for ordinate dimensions in {axis}"),
            Some(_) => format!("Click points to dimension their {axis}; Esc to finish"),
        }
    }

    fn is_busy(&self) -> bool {
        self.origin.is_some()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Click(p, _) = input {
            let Some(point) = p.snapped_point() else {
                ed.notice = Some("Click an existing point".into());
                return;
            };
            match self.origin {
                None => {
                    self.origin = Some(point);
                    // The origin itself reads 0.
                    self.add(ed, point, point);
                    ed.highlight = [point].into();
                }
                Some(origin) => self.add(ed, origin, point),
            }
        }
        if input == Input::Finish {
            self.origin = None;
            ed.highlight.clear();
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        ed.highlight.clear();
        self.origin.take().is_some()
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match key {
            ToolKey::X => self.axis = OrdinateAxis::X,
            ToolKey::Y => self.axis = OrdinateAxis::Y,
            _ => return false,
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Section and elevation marks

/// X / Y while a mark's line is being drawn: lock it to that axis, or unlock it if it
/// already is. Not claimed before the first click.
fn toggle_lock(lock: &mut Option<Axis>, drawing: bool, key: ToolKey) -> bool {
    let axis = match key {
        ToolKey::X => Axis::X,
        ToolKey::Y => Axis::Y,
        _ => return false,
    };
    if !drawing {
        return false;
    }
    *lock = if *lock == Some(axis) {
        None
    } else {
        Some(axis)
    };
    true
}

/// Two clicks: the ends of a section's cutting line. It gets the next free letter; which way
/// it looks, and the sheet it refers to, are set in the inspector.
#[derive(Default)]
pub struct SectionTool {
    first: Option<DVec2>,
    hover: Option<DVec2>,
    /// X / Y: the cutting line runs along that axis.
    lock: Option<Axis>,
}

impl Tool for SectionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Section
    }

    fn hint(&self) -> String {
        if self.first.is_some() {
            "Click where the cutting line ends · X / Y: along an axis".into()
        } else {
            "Click where the section's cutting line starts".into()
        }
    }

    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        if let Some(from) = self.first {
            super::line::steer(from, self.lock, p);
        }
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        toggle_lock(&mut self.lock, self.first.is_some(), key)
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        Some((self.first?, self.lock?))
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => self.hover = Some(p.pos()),
            Input::Click(p, _) => match self.first.take() {
                None => self.first = Some(p.pos()),
                Some(a) if a != p.pos() => {
                    let mark = Annotation::Section {
                        a,
                        b: p.pos(),
                        label: notes::next_mark_label(&ed.doc, true),
                        sheet: String::new(),
                        flip: false,
                        view: None,
                    };
                    add_annotation(ed, mark, "Section mark");
                    self.lock = None;
                }
                Some(a) => self.first = Some(a),
            },
            Input::Finish => {
                self.first = None;
                self.lock = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.lock = None;
        self.first.take().is_some()
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        match (self.first, self.hover) {
            (Some(a), Some(b)) if a != b => vec![preview_leader(a, b)],
            _ => Vec::new(),
        }
    }
}

/// Click where an elevation's flag goes, then towards what it looks at (along an axis with
/// X / Y, in the document's angle steps when angle snapping is on; Alt: freely). It gets
/// the next free number.
#[derive(Default)]
pub struct ElevationTool {
    at: Option<DVec2>,
    hover: Option<DVec2>,
    /// X / Y: it looks along that axis.
    lock: Option<Axis>,
}

impl ElevationTool {
    /// The direction from the flag to the (steered) pointer, in degrees.
    fn aim(at: DVec2, p: &Pointer) -> Option<f64> {
        let v = p.pos() - at;
        if v.length() <= p.tolerance {
            return None;
        }
        let degrees = v.y.atan2(v.x).to_degrees();
        // Steered onto an axis or an angle step, it is exact; otherwise to a tenth.
        Some(match p.guide {
            Some(_) => (degrees * 1e6).round() / 1e6,
            None => (degrees * 10.0).round() / 10.0,
        })
    }
}

impl Tool for ElevationTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Elevation
    }

    fn hint(&self) -> String {
        if self.at.is_some() {
            "Click towards what the elevation looks at · X / Y: along an axis · Alt: any angle"
                .into()
        } else {
            "Click where the elevation mark goes".into()
        }
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        if let Some(from) = self.at {
            // Aiming: no snap target decides it, only the axis or the angle step.
            p.snap = None;
            super::line::steer(from, self.lock, p);
        }
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        toggle_lock(&mut self.lock, self.at.is_some(), key)
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        Some((self.at?, self.lock?))
    }

    fn is_busy(&self) -> bool {
        self.at.is_some()
    }

    fn wants_snap(&self) -> bool {
        // The second click only aims.
        self.at.is_none()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => self.hover = Some(p.pos()),
            Input::Click(p, _) => match self.at {
                None => self.at = Some(p.pos()),
                Some(at) => {
                    let Some(angle) = Self::aim(at, &p) else {
                        return;
                    };
                    self.at = None;
                    self.lock = None;
                    let mark = Annotation::Elevation {
                        at,
                        angle,
                        label: notes::next_mark_label(&ed.doc, false),
                        sheet: String::new(),
                        view: None,
                    };
                    add_annotation(ed, mark, "Elevation mark");
                }
            },
            Input::Finish => {
                self.at = None;
                self.lock = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.lock = None;
        self.at.take().is_some()
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        match (self.at, self.hover) {
            (Some(a), Some(b)) if a != b => vec![preview_leader(a, b)],
            _ => Vec::new(),
        }
    }
}
