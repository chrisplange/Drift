//! Interactive tools as small state machines over [`Editor`].
//!
//! Tools receive pointer input already converted to world space and snapped, so they are
//! independent of the UI toolkit and can be tested by feeding them events.

use std::f64::consts::{PI, TAU};

use drift_core::{Bounds, DVec2, Emphasis, EntityId, Shape, SketchItem, arc_shape};

use drift_solver::Param;

use crate::editor::{Anchor, Editor, Pointer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Add to the selection.
    pub shift: bool,
    /// Toggle in the selection (Ctrl, or Cmd on macOS).
    pub command: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// The pointer moved (or the frame was redrawn) with no button action.
    Hover(Pointer),
    Click(Pointer, Modifiers),
    DoubleClick(Pointer),
    /// A primary-button drag began. The pointer is at the press position.
    DragStart(Pointer, Modifiers),
    Drag(Pointer),
    DragEnd(Pointer, Modifiers),
    /// Finish the current operation (right click or Enter).
    Finish,
}

/// What the overlay should draw for the tool, beyond its preview geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    SelectionBox { rect: Bounds, crossing: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Line,
    Rectangle,
    Circle,
    Arc,
    Point,
}

impl ToolKind {
    pub const ALL: [ToolKind; 6] = [
        ToolKind::Select,
        ToolKind::Line,
        ToolKind::Rectangle,
        ToolKind::Circle,
        ToolKind::Arc,
        ToolKind::Point,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Circle => "Circle",
            ToolKind::Arc => "Arc",
            ToolKind::Point => "Point",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            ToolKind::Select => 'S',
            ToolKind::Line => 'L',
            ToolKind::Rectangle => 'R',
            ToolKind::Circle => 'C',
            ToolKind::Arc => 'A',
            ToolKind::Point => 'P',
        }
    }

    pub fn create(self) -> Box<dyn Tool> {
        match self {
            ToolKind::Select => Box::<SelectTool>::default(),
            ToolKind::Line => Box::<LineTool>::default(),
            ToolKind::Rectangle => Box::<RectangleTool>::default(),
            ToolKind::Circle => Box::<CircleTool>::default(),
            ToolKind::Arc => Box::<ArcTool>::default(),
            ToolKind::Point => Box::<PointTool>::default(),
        }
    }
}

pub trait Tool {
    fn kind(&self) -> ToolKind;

    /// One-line instruction for the status bar.
    fn hint(&self) -> String;

    fn handle(&mut self, input: Input, ed: &mut Editor);

    /// Abandon the operation in progress (for the select tool with nothing in progress: clear
    /// the selection). Returns false if there was nothing to cancel.
    fn cancel(&mut self, ed: &mut Editor) -> bool;

    /// True while an operation is in progress (a drag, or a shape with points already placed).
    fn is_busy(&self) -> bool;

    /// Geometry to draw for the operation in progress.
    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        Vec::new()
    }

    /// A live measurement to show next to the cursor, e.g. a length.
    fn measurement(&self, _pointer: &Pointer) -> Option<String> {
        None
    }

    fn overlay(&self) -> Option<Overlay> {
        None
    }

    /// Whether the pointer should snap right now.
    fn wants_snap(&self) -> bool {
        true
    }

    /// Entities that must not be snapped to (e.g. the geometry being dragged).
    fn snap_excludes(&self, _ed: &Editor, _id: EntityId) -> bool {
        false
    }
}

fn preview(shape: Shape) -> SketchItem {
    SketchItem::new(shape).with_emphasis(Emphasis::Preview)
}

fn preview_vertex(at: DVec2) -> SketchItem {
    preview(Shape::Point(at)).as_vertex()
}

fn format_len(v: f64) -> String {
    format!("{v:.4}")
}

// ---------------------------------------------------------------------------------------------
// Select

#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectDrag {
    Box {
        start: DVec2,
        current: DVec2,
    },
    Move {
        /// World position the drag is following: the dragged point, or the grab position.
        last: DVec2,
        /// Set when a single point is being dragged; it snaps and can merge on drop.
        point: Option<EntityId>,
    },
}

#[derive(Default)]
pub struct SelectTool {
    drag: Option<SelectDrag>,
}

impl SelectTool {
    fn dragged_point(&self) -> Option<EntityId> {
        match self.drag {
            Some(SelectDrag::Move { point, .. }) => point,
            _ => None,
        }
    }
}

impl Tool for SelectTool {
    fn is_busy(&self) -> bool {
        self.drag.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Select
    }

    fn hint(&self) -> String {
        match self.drag {
            Some(SelectDrag::Box { .. }) => {
                "Drag right for window selection, left for crossing selection".into()
            }
            Some(SelectDrag::Move { point: Some(_), .. }) => {
                "Drop on another point to join them · Esc to cancel".into()
            }
            Some(SelectDrag::Move { .. }) => "Moving selection · Esc to cancel".into(),
            None => "Click to select · Shift to add · Ctrl to toggle · drag to move or box-select"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                ed.hovered = match self.drag {
                    Some(SelectDrag::Box { .. }) => None,
                    Some(SelectDrag::Move { point, .. }) => point,
                    None => ed.index.pick(p.raw, p.tolerance, |_| true),
                };
            }
            Input::Click(p, mods) => {
                let hit = ed.index.pick(p.raw, p.tolerance, |_| true);
                match hit {
                    Some(id) if mods.command => {
                        if !ed.selection.remove(&id) {
                            ed.selection.insert(id);
                        }
                    }
                    Some(id) if mods.shift => {
                        ed.selection.insert(id);
                    }
                    Some(id) => {
                        ed.selection = [id].into();
                    }
                    None if !mods.shift && !mods.command => ed.selection.clear(),
                    None => {}
                }
            }
            Input::DoubleClick(_) | Input::Finish => {}
            Input::DragStart(p, mods) => match ed.index.pick(p.raw, p.tolerance, |_| true) {
                Some(id) => {
                    if !ed.selection.contains(&id) {
                        if !mods.shift && !mods.command {
                            ed.selection.clear();
                        }
                        ed.selection.insert(id);
                    }
                    let is_point = ed.doc.get(id).is_some_and(|e| e.kind.is_point());
                    let point = (is_point && ed.selection.len() == 1).then_some(id);
                    let last = match point {
                        Some(pt) => ed.doc.point(pt).unwrap_or(p.raw),
                        None => p.raw,
                    };
                    ed.doc.begin();
                    self.drag = Some(SelectDrag::Move { last, point });
                }
                None => {
                    self.drag = Some(SelectDrag::Box {
                        start: p.raw,
                        current: p.raw,
                    });
                }
            },
            Input::Drag(p) => match &mut self.drag {
                Some(SelectDrag::Box { current, .. }) => *current = p.raw,
                Some(SelectDrag::Move { last, point }) => {
                    let target = if point.is_some() { p.pos() } else { p.raw };
                    let delta = target - *last;
                    if delta != DVec2::ZERO {
                        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                        // The grabbed geometry follows the pointer; constraints move the rest.
                        let dragged: Vec<Param> = ed
                            .doc
                            .defining_points(ids.iter().copied())
                            .into_iter()
                            .flat_map(Param::point)
                            .collect();
                        if ed.drag(&ids, delta, &dragged) {
                            // Track where the geometry actually went (constraints may hold it).
                            *last = match point {
                                Some(pt) => ed.doc.point(*pt).unwrap_or(target),
                                None => target,
                            };
                        }
                    }
                }
                None => {}
            },
            Input::DragEnd(p, mods) => match self.drag.take() {
                Some(SelectDrag::Box { start, .. }) => {
                    let mut rect = Bounds::from_point(start);
                    rect.include(p.raw);
                    let crossing = p.raw.x < start.x;
                    let found = ed.index.select_in_rect(&rect, crossing);
                    if !mods.shift && !mods.command {
                        ed.selection.clear();
                    }
                    ed.selection.extend(found);
                }
                Some(SelectDrag::Move { point, .. }) => {
                    if let (Some(from), Some(into)) = (point, p.snapped_point())
                        && from != into
                        && ed.doc.merge_points(from, into).is_ok()
                    {
                        ed.selection = [into].into();
                    }
                    ed.commit("Move");
                }
                None => {}
            },
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        match self.drag.take() {
            Some(SelectDrag::Move { .. }) => {
                ed.doc.rollback();
                ed.sync();
                true
            }
            Some(SelectDrag::Box { .. }) => true,
            None if !ed.selection.is_empty() => {
                ed.selection.clear();
                true
            }
            None => false,
        }
    }

    fn overlay(&self) -> Option<Overlay> {
        match self.drag {
            Some(SelectDrag::Box { start, current }) => {
                let mut rect = Bounds::from_point(start);
                rect.include(current);
                Some(Overlay::SelectionBox {
                    rect,
                    crossing: current.x < start.x,
                })
            }
            _ => None,
        }
    }

    fn wants_snap(&self) -> bool {
        self.dragged_point().is_some()
    }

    fn snap_excludes(&self, ed: &Editor, id: EntityId) -> bool {
        // Don't snap a dragged point to itself or to the curves that move with it.
        self.dragged_point().is_some_and(|p| {
            id == p
                || ed
                    .doc
                    .get(id)
                    .is_some_and(|e| e.kind.points().any(|q| q == p))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Line (polyline chain)

#[derive(Default)]
pub struct LineTool {
    last: Option<Anchor>,
    chain_start: Option<EntityId>,
}

impl LineTool {
    fn place(&mut self, p: &Pointer, ed: &mut Editor) {
        let anchor = p.anchor();
        let Some(last) = self.last else {
            self.last = Some(anchor);
            self.chain_start = None;
            return;
        };
        if anchor.pos == last.pos {
            return;
        }
        ed.doc.begin();
        let a = last.resolve(&mut ed.doc);
        let b = anchor.resolve(&mut ed.doc);
        if ed.doc.add_line(a, b).is_err() {
            ed.doc.rollback();
            ed.sync();
            return;
        }
        ed.commit("Line");
        let start = *self.chain_start.get_or_insert(a);
        if b == start {
            // Closed the loop.
            self.last = None;
            self.chain_start = None;
        } else {
            self.last = Some(Anchor {
                pos: anchor.pos,
                existing: Some(b),
            });
        }
    }
}

impl Tool for LineTool {
    fn is_busy(&self) -> bool {
        self.last.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Line
    }

    fn hint(&self) -> String {
        match self.last {
            None => "Click the start point".into(),
            Some(_) => "Click the next point · double-click, right-click or Enter to finish".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => self.place(&p, ed),
            Input::DragStart(p, _) if self.last.is_none() => self.place(&p, ed),
            Input::DoubleClick(_) | Input::Finish => {
                self.last = None;
                self.chain_start = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.chain_start = None;
        self.last.take().is_some()
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        match (self.last, pointer) {
            (Some(last), Some(p)) => vec![
                preview(Shape::Line {
                    a: last.pos,
                    b: p.pos(),
                }),
                preview_vertex(last.pos),
            ],
            _ => Vec::new(),
        }
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let last = self.last?;
        let d = p.pos() - last.pos;
        let angle = d.y.atan2(d.x).to_degrees();
        Some(format!("L {}  A {angle:.2}°", format_len(d.length())))
    }
}

// ---------------------------------------------------------------------------------------------
// Rectangle (two opposite corners)

#[derive(Default)]
pub struct RectangleTool {
    first: Option<Anchor>,
}

impl Tool for RectangleTool {
    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Rectangle
    }

    fn hint(&self) -> String {
        match self.first {
            None => "Click the first corner".into(),
            Some(_) => "Click the opposite corner".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.first.is_none() => p,
            _ => return,
        };
        let Some(first) = self.first else {
            self.first = Some(p.anchor());
            return;
        };
        let second = p.anchor();
        let (a, c) = (first.pos, second.pos);
        if a.x == c.x || a.y == c.y {
            return;
        }
        ed.doc.begin();
        let p0 = first.resolve(&mut ed.doc);
        let p1 = ed.doc.add_point(DVec2::new(c.x, a.y));
        let p2 = second.resolve(&mut ed.doc);
        let p3 = ed.doc.add_point(DVec2::new(a.x, c.y));
        let corners = [p0, p1, p2, p3];
        let ok = (0..4).all(|i| ed.doc.add_line(corners[i], corners[(i + 1) % 4]).is_ok());
        if ok {
            ed.commit("Rectangle");
        } else {
            ed.doc.rollback();
            ed.sync();
        }
        self.first = None;
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.first.take().is_some()
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(first), Some(p)) = (self.first, pointer) else {
            return Vec::new();
        };
        let (a, c) = (first.pos, p.pos());
        let corners = [a, DVec2::new(c.x, a.y), c, DVec2::new(a.x, c.y)];
        (0..4)
            .map(|i| {
                preview(Shape::Line {
                    a: corners[i],
                    b: corners[(i + 1) % 4],
                })
            })
            .collect()
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let d = p.pos() - self.first?.pos;
        Some(format!(
            "{} × {}",
            format_len(d.x.abs()),
            format_len(d.y.abs())
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Circle (centre, then a point on the circle)

#[derive(Default)]
pub struct CircleTool {
    center: Option<Anchor>,
}

impl Tool for CircleTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Circle
    }

    fn hint(&self) -> String {
        match self.center {
            None => "Click the centre".into(),
            Some(_) => "Click a point on the circle".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.center.is_none() => p,
            _ => return,
        };
        let Some(center) = self.center else {
            self.center = Some(p.anchor());
            return;
        };
        let radius = p.pos().distance(center.pos);
        if radius == 0.0 {
            return;
        }
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        if ed.doc.add_circle(c, radius).is_ok() {
            ed.commit("Circle");
        } else {
            ed.doc.rollback();
            ed.sync();
        }
        self.center = None;
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.center.take().is_some()
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        vec![
            preview(Shape::Circle {
                center: center.pos,
                radius: p.pos().distance(center.pos),
            }),
            preview_vertex(center.pos),
        ]
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let r = p.pos().distance(self.center?.pos);
        Some(format!("R {}  Ø {}", format_len(r), format_len(2.0 * r)))
    }
}

// ---------------------------------------------------------------------------------------------
// Arc (centre, start, end; direction follows the pointer)

#[derive(Default)]
pub struct ArcTool {
    center: Option<Anchor>,
    start: Option<Anchor>,
    /// Signed angle the pointer has swept around the centre since the start was placed.
    swept: f64,
    last_angle: f64,
}

impl ArcTool {
    fn angle_of(&self, at: DVec2) -> f64 {
        let c = self.center.map_or(DVec2::ZERO, |c| c.pos);
        (at - c).y.atan2((at - c).x)
    }

    fn track(&mut self, at: DVec2) {
        if self.start.is_some() {
            let angle = self.angle_of(at);
            let mut delta = angle - self.last_angle;
            if delta > PI {
                delta -= TAU;
            } else if delta < -PI {
                delta += TAU;
            }
            self.swept += delta;
            self.last_angle = angle;
        }
    }

    /// Centre, start and end of the arc for the pointer position, ordered counter-clockwise.
    fn geometry(&self, at: DVec2) -> Option<(DVec2, DVec2, DVec2)> {
        let center = self.center?.pos;
        let start = self.start?.pos;
        let radius = start.distance(center);
        let dir = at - center;
        if dir == DVec2::ZERO {
            return None;
        }
        let end = center + dir.normalize() * radius;
        Some(if self.swept < 0.0 {
            (center, end, start)
        } else {
            (center, start, end)
        })
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Tool for ArcTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Arc
    }

    fn hint(&self) -> String {
        match (self.center, self.start) {
            (None, _) => "Click the arc centre".into(),
            (Some(_), None) => "Click the start point (sets the radius)".into(),
            _ => "Click the end point · move around the centre to choose the direction".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Hover(p) | Input::Drag(p) => {
                self.track(p.pos());
                return;
            }
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.start.is_none() => p,
            _ => return,
        };
        self.track(p.pos());
        let Some(center) = self.center else {
            self.center = Some(p.anchor());
            return;
        };
        let Some(start) = self.start else {
            if p.pos() != center.pos {
                self.start = Some(p.anchor());
                self.swept = 0.0;
                self.last_angle = self.angle_of(p.pos());
            }
            return;
        };
        let Some((_, s, e)) = self.geometry(p.pos()) else {
            return;
        };
        let radius = start.pos.distance(center.pos);
        // Reuse a snapped point for the free end only if it lies exactly on the circle.
        let free = Anchor {
            pos: if self.swept < 0.0 { s } else { e },
            existing: p.snapped_point().filter(|_| {
                (p.pos().distance(center.pos) - radius).abs() <= 1e-9 * radius.max(1.0)
            }),
        };
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        let st = start.resolve(&mut ed.doc);
        let fr = free.resolve(&mut ed.doc);
        let (first, last) = if self.swept < 0.0 { (fr, st) } else { (st, fr) };
        if ed.doc.add_arc(c, first, last).is_ok() {
            ed.commit("Arc");
        } else {
            ed.doc.rollback();
            ed.sync();
        }
        self.reset();
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.center.is_some();
        self.reset();
        had
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(center.pos)];
        match self.geometry(p.pos()) {
            Some((c, s, e)) if s != e => {
                items.push(preview(arc_shape(c, s, e)));
                items.push(preview_vertex(s));
                items.push(preview_vertex(e));
            }
            _ => {
                items.push(preview(Shape::Line {
                    a: center.pos,
                    b: p.pos(),
                }));
            }
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let center = self.center?.pos;
        match self.start {
            None => Some(format!("R {}", format_len(p.pos().distance(center)))),
            Some(start) => {
                let r = start.pos.distance(center);
                let (c, s, e) = self.geometry(p.pos())?;
                let Shape::Arc { sweep, .. } = arc_shape(c, s, e) else {
                    return None;
                };
                Some(format!("R {}  {:.2}°", format_len(r), sweep.to_degrees()))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Point

#[derive(Default)]
pub struct PointTool;

impl Tool for PointTool {
    fn is_busy(&self) -> bool {
        false
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Point
    }

    fn hint(&self) -> String {
        "Click to place a point".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Click(p, _) = input
            && p.snapped_point().is_none()
        {
            ed.doc.begin();
            ed.doc.add_point(p.pos());
            ed.commit("Point");
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{Document, EntityKind};
    use drift_geom::SnapOptions;

    use super::*;

    const TOL: f64 = 0.5;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A snapped pointer at `at`, as the canvas would produce it.
    fn ptr(ed: &Editor, tool: &dyn Tool, at: DVec2) -> Pointer {
        let mut p = Pointer::new(at, TOL);
        if tool.wants_snap() {
            p.snap = ed.index.snap(
                at,
                SnapOptions {
                    tolerance: TOL,
                    grid: None,
                },
                |id| tool.snap_excludes(ed, id),
            );
        }
        p
    }

    fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
        let p = ptr(ed, tool, at);
        tool.handle(Input::Hover(p), ed);
        tool.handle(Input::Click(p, Modifiers::default()), ed);
    }

    fn count(doc: &Document, pred: impl Fn(&EntityKind) -> bool) -> usize {
        doc.iter().filter(|(_, e)| pred(&e.kind)).count()
    }

    fn lines(doc: &Document) -> usize {
        count(doc, |k| matches!(k, EntityKind::Line { .. }))
    }

    fn points(doc: &Document) -> usize {
        count(doc, EntityKind::is_point)
    }

    #[test]
    fn polyline_shares_points_and_closes_on_start() {
        let mut ed = Editor::default();
        let mut tool = LineTool::default();
        for at in [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.1, 0.2)] {
            click(&mut ed, &mut tool, at);
        }
        // The last click snapped to the start point and closed a triangle.
        assert_eq!(lines(&ed.doc), 3);
        assert_eq!(points(&ed.doc), 3);
        assert!(tool.last.is_none(), "chain finished after closing");
        // Each segment is its own undo step.
        assert!(ed.undo());
        assert_eq!(lines(&ed.doc), 2);
    }

    #[test]
    fn starting_a_line_on_an_existing_end_point_connects_to_it() {
        let mut ed = Editor::default();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(10.0, 0.0));
        tool.handle(Input::Finish, &mut ed);
        click(&mut ed, &mut tool, v(10.2, 0.1)); // snaps to the end point
        click(&mut ed, &mut tool, v(10.0, 5.0));
        assert_eq!(lines(&ed.doc), 2);
        assert_eq!(points(&ed.doc), 3);
    }

    #[test]
    fn rectangle_is_four_connected_lines_in_one_undo_step() {
        let mut ed = Editor::default();
        let mut tool = RectangleTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(30.0, 20.0));
        assert_eq!((lines(&ed.doc), points(&ed.doc)), (4, 4));
        assert_eq!(ed.bounds().max, v(30.0, 20.0));
        assert!(ed.undo());
        assert!(ed.doc.is_empty());
    }

    #[test]
    fn press_drag_release_draws_a_circle() {
        let mut ed = Editor::default();
        let mut tool = CircleTool::default();
        let m = Modifiers::default();
        let start = ptr(&ed, &tool, v(5.0, 5.0));
        tool.handle(Input::DragStart(start, m), &mut ed);
        let end = ptr(&ed, &tool, v(8.0, 9.0));
        tool.handle(Input::Drag(end), &mut ed);
        tool.handle(Input::DragEnd(end, m), &mut ed);
        let (_, circle) = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
            .unwrap();
        assert_eq!(
            circle.kind,
            EntityKind::Circle {
                center: drift_core::EntityId(1),
                radius: 5.0
            }
        );
    }

    #[test]
    fn arc_direction_follows_the_pointer() {
        for (path, expect_ccw) in [
            (vec![v(7.0, 7.0), v(0.0, 10.0)], true), // up and over: counter-clockwise
            (vec![v(7.0, -7.0), v(0.0, -10.0)], false), // down: clockwise
        ] {
            let mut ed = Editor::default();
            let mut tool = ArcTool::default();
            click(&mut ed, &mut tool, v(0.0, 0.0));
            click(&mut ed, &mut tool, v(10.0, 0.0));
            for at in &path[..path.len() - 1] {
                let p = ptr(&ed, &tool, *at);
                tool.handle(Input::Hover(p), &mut ed);
            }
            click(&mut ed, &mut tool, *path.last().unwrap());
            let (id, _) = ed
                .doc
                .iter()
                .find(|(_, e)| matches!(e.kind, EntityKind::Arc { .. }))
                .unwrap();
            let Some(Shape::Arc { start, sweep, .. }) = ed.doc.shape(id) else {
                panic!()
            };
            assert!((sweep - PI / 2.0).abs() < 1e-9, "quarter arc, got {sweep}");
            let expected_start = if expect_ccw { 0.0 } else { -PI / 2.0 };
            assert!((start - expected_start).abs() < 1e-9, "start {start}");
        }
    }

    #[test]
    fn select_click_box_and_move() {
        let mut ed = Editor::default();
        let mut rect = RectangleTool::default();
        click(&mut ed, &mut rect, v(0.0, 0.0));
        click(&mut ed, &mut rect, v(10.0, 10.0));
        let mut sel = SelectTool::default();
        let m = Modifiers::default();

        // Click the bottom edge.
        click(&mut ed, &mut sel, v(5.0, 0.1));
        assert_eq!(ed.selection.len(), 1);

        // Window-select everything (drag right).
        let a = ptr(&ed, &sel, v(-1.0, -1.0));
        sel.handle(Input::DragStart(a, m), &mut ed);
        let b = ptr(&ed, &sel, v(11.0, 11.0));
        sel.handle(Input::Drag(b), &mut ed);
        sel.handle(Input::DragEnd(b, m), &mut ed);
        assert_eq!(ed.selection.len(), 4);

        // Drag an edge: the whole selection moves as one undo step.
        let grab = ptr(&ed, &sel, v(5.0, 0.0));
        sel.handle(Input::DragStart(grab, m), &mut ed);
        let to = ptr(&ed, &sel, v(8.0, 4.0));
        sel.handle(Input::Drag(to), &mut ed);
        sel.handle(Input::DragEnd(to, m), &mut ed);
        assert_eq!(ed.bounds().min, v(3.0, 4.0));
        assert!(ed.undo());
        assert_eq!(ed.bounds().min, v(0.0, 0.0));
    }

    #[test]
    fn dragging_an_end_point_onto_another_joins_them() {
        let mut ed = Editor::default();
        let mut line = LineTool::default();
        click(&mut ed, &mut line, v(0.0, 0.0));
        click(&mut ed, &mut line, v(10.0, 0.0));
        line.handle(Input::Finish, &mut ed);
        click(&mut ed, &mut line, v(20.0, 0.0));
        click(&mut ed, &mut line, v(20.0, 10.0));
        assert_eq!(points(&ed.doc), 4);

        let mut sel = SelectTool::default();
        let m = Modifiers::default();
        let grab = ptr(&ed, &sel, v(20.0, 0.0));
        sel.handle(Input::DragStart(grab, m), &mut ed);
        let to = ptr(&ed, &sel, v(10.3, 0.2)); // snaps onto the first line's end
        sel.handle(Input::Drag(to), &mut ed);
        sel.handle(Input::DragEnd(to, m), &mut ed);
        assert_eq!(points(&ed.doc), 3, "points merged");
        assert!(ed.undo());
        assert_eq!(points(&ed.doc), 4);
    }

    #[test]
    fn escape_during_move_restores_geometry() {
        let mut ed = Editor::default();
        let mut tool = CircleTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(5.0, 0.0));
        let before = ed.doc.clone();
        let mut sel = SelectTool::default();
        let m = Modifiers::default();
        let grab = ptr(&ed, &sel, v(5.0, 0.0));
        sel.handle(Input::DragStart(grab, m), &mut ed);
        sel.handle(Input::Drag(ptr(&ed, &sel, v(15.0, 3.0))), &mut ed);
        assert_ne!(ed.doc, before);
        assert!(sel.cancel(&mut ed));
        assert_eq!(ed.doc, before);
        assert!(!ed.history.can_undo() || ed.history.undo_label() == Some("Circle"));
    }
}
