//! Interactive tools as small state machines over [`Editor`].
//!
//! Tools receive pointer input already converted to world space and snapped, so they are
//! independent of the UI toolkit and can be tested by feeding them events.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use drift_core::{
    Bounds, Constraint, DVec2, Emphasis, EntityId, EntityKind, Shape, SketchItem, arc_shape,
};
use drift_solver::Param;

use crate::editor::{Anchor, Editor, Guide, GuideKind, Pointer};

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
    SelectionBox {
        rect: Bounds,
        crossing: bool,
    },
    /// A modal move: the base point and where it has been moved to (`None` while the user is
    /// picking a new base point).
    Move {
        base: DVec2,
        to: Option<DVec2>,
    },
}

/// Keys that tools can claim (Blender-style modal keys). Unclaimed keys keep their global
/// meaning (e.g. X toggles construction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKey {
    /// Lock to the X axis.
    X,
    /// Lock to the Y axis.
    Y,
    /// Grab: move the selection.
    G,
    /// Pick the base point of a move.
    B,
}

/// A world axis a drawing tool can lock the segment being drawn to (Blender-style X / Y keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn dir(self) -> DVec2 {
        match self {
            Axis::X => DVec2::X,
            Axis::Y => DVec2::Y,
        }
    }

    /// The relation a segment locked to this axis gets.
    fn relation(self) -> GuideKind {
        match self {
            Axis::X => GuideKind::Horizontal,
            Axis::Y => GuideKind::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Line,
    Rectangle,
    Circle,
    /// Centre-point arc.
    Arc,
    ThreePointArc,
    TangentArc,
    Point,
    Dimension,
}

impl ToolKind {
    pub const ALL: [ToolKind; 9] = [
        ToolKind::Select,
        ToolKind::Line,
        ToolKind::Rectangle,
        ToolKind::Circle,
        ToolKind::Arc,
        ToolKind::ThreePointArc,
        ToolKind::TangentArc,
        ToolKind::Point,
        ToolKind::Dimension,
    ];

    /// The arc tools, in the order pressing A cycles through them.
    pub const ARCS: [ToolKind; 3] = [ToolKind::Arc, ToolKind::ThreePointArc, ToolKind::TangentArc];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Circle => "Circle",
            ToolKind::Arc => "Arc",
            ToolKind::ThreePointArc => "3-pt arc",
            ToolKind::TangentArc => "Tangent arc",
            ToolKind::Point => "Point",
            ToolKind::Dimension => "Dimension",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            ToolKind::Select => 'S',
            ToolKind::Line => 'L',
            ToolKind::Rectangle => 'R',
            ToolKind::Circle => 'C',
            // A selects the centre arc; pressed again it cycles through the arc tools.
            ToolKind::Arc | ToolKind::ThreePointArc | ToolKind::TangentArc => 'A',
            ToolKind::Point => 'P',
            ToolKind::Dimension => 'D',
        }
    }

    pub fn create(self) -> Box<dyn Tool> {
        match self {
            ToolKind::Select => Box::<SelectTool>::default(),
            ToolKind::Line => Box::<LineTool>::default(),
            ToolKind::Rectangle => Box::<RectangleTool>::default(),
            ToolKind::Circle => Box::<CircleTool>::default(),
            ToolKind::Arc => Box::<ArcTool>::default(),
            ToolKind::ThreePointArc => Box::<ThreePointArcTool>::default(),
            ToolKind::TangentArc => Box::<TangentArcTool>::default(),
            ToolKind::Point => Box::<PointTool>::default(),
            ToolKind::Dimension => Box::<DimensionTool>::default(),
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

    /// Adjust the pointer after snapping (e.g. lock a line horizontal).
    fn refine_pointer(&self, _pointer: &mut Pointer) {}

    /// A modal key was pressed with the pointer at `pointer` (if over the canvas). Returns
    /// false if the tool doesn't use the key, so it can do something else.
    fn key(&mut self, _key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        false
    }

    /// True during a modal operation (a grab): other shortcuts are suspended and right-click
    /// cancels it.
    fn is_modal(&self) -> bool {
        false
    }

    /// The existing point a chain of curves (polyline, tangent arcs) currently ends at, so
    /// switching between line and tangent arc can continue from it.
    fn chain_end(&self) -> Option<EntityId> {
        None
    }

    /// Continue a chain from an existing point (after switching tools mid-chain).
    fn continue_from(&mut self, _point: EntityId, _ed: &mut Editor) {}

    /// The active axis lock and the point the locked segment runs from, for the overlay.
    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        None
    }

    /// Labels of the values the user can type right now (at most two), e.g. length and angle.
    fn fields(&self) -> &'static [&'static str] {
        &[]
    }

    /// The values typed so far (`None` = not typed), for previews and measurements.
    fn set_typed(&mut self, _values: [Option<f64>; 2]) {}

    /// Enter was pressed while typing: apply the typed values.
    fn commit_typed(&mut self, _pointer: &Pointer, _ed: &mut Editor) {}

    /// A dimension being placed (and its label offset), for the overlay to preview.
    fn pending_dimension(
        &self,
        _ed: &Editor,
        _pointer: Option<&Pointer>,
    ) -> Option<(Constraint, DVec2)> {
        None
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

/// What the select tool would pick under the pointer: points first, then line midpoints,
/// then curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Entity(EntityId),
    Midpoint(EntityId),
}

impl Target {
    /// The id a dimension uses for this pick when it's a point-like reference: a point, or a
    /// line standing for its midpoint.
    fn point_ref(self, ed: &Editor) -> Option<EntityId> {
        match self {
            Target::Midpoint(line) => Some(line),
            Target::Entity(id) => ed.doc.get(id).filter(|e| e.kind.is_point()).map(|_| id),
        }
    }
}

/// Midpoints are picked within this fraction of the normal pick radius.
const MIDPOINT_PICK: f64 = 0.6;

fn pick_target(ed: &Editor, p: &Pointer) -> Option<Target> {
    let is_point = |id: EntityId| ed.doc.get(id).is_some_and(|e| e.kind.is_point());
    if let Some(id) = ed.index.pick(p.raw, p.tolerance, is_point) {
        return Some(Target::Entity(id));
    }
    // A tighter radius than for picking, so clicking near (not on) the middle of a line still
    // picks the line.
    if let Some(line) = ed
        .index
        .pick_midpoint(p.raw, MIDPOINT_PICK * p.tolerance, |_| true)
    {
        return Some(Target::Midpoint(line));
    }
    ed.index
        .pick(p.raw, p.tolerance, |_| true)
        .map(Target::Entity)
}

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

/// A Blender-style modal move (G): the selection follows the pointer from a base point until a
/// click or Enter confirms it, or Esc / right-click cancels. X / Y lock an axis, numbers give
/// the exact offset, and B picks a new base point.
struct Grab {
    ids: Vec<EntityId>,
    /// Defining points of the selection: these move rigidly with the offset.
    points: BTreeSet<EntityId>,
    base: DVec2,
    /// Where the pointer is (the move's target before locks and typed values).
    target: DVec2,
    picking_base: bool,
    lock: Option<Axis>,
    typed: [Option<f64>; 2],
    /// The offset the geometry has been moved by so far in the open transaction.
    applied: DVec2,
}

impl Grab {
    /// The offset for the pointer at `target`, honouring the lock and typed values.
    fn offset(&self, target: DVec2) -> DVec2 {
        let d = target - self.base;
        match self.lock {
            Some(axis) => {
                let u = axis.dir();
                u * self.typed[0].unwrap_or(d.dot(u))
            }
            None if any_typed(self.typed) => {
                DVec2::new(self.typed[0].unwrap_or(0.0), self.typed[1].unwrap_or(0.0))
            }
            None => d,
        }
    }

    fn dragged(&self) -> Vec<Param> {
        self.points.iter().copied().flat_map(Param::point).collect()
    }

    /// Move the geometry to `offset` (live, in steps from the last offset so constraints
    /// follow smoothly).
    fn apply(&mut self, offset: DVec2, ed: &mut Editor) {
        let step = offset - self.applied;
        if step != DVec2::ZERO && ed.drag(&self.ids, step, &self.dragged()) {
            self.applied = offset;
        }
    }
}

#[derive(Default)]
pub struct SelectTool {
    drag: Option<SelectDrag>,
    grab: Option<Grab>,
}

impl SelectTool {
    /// Start a modal move of the selection from the pointer.
    fn start_grab(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        if self.drag.is_some() || self.grab.is_some() {
            return false;
        }
        if ed.selection.is_empty() {
            ed.notice = Some("Select something to move first".into());
            return true;
        }
        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
        let points = ed.doc.defining_points(ids.iter().copied());
        ed.doc.begin();
        self.grab = Some(Grab {
            ids,
            points,
            base: p.pos(),
            target: p.pos(),
            picking_base: false,
            lock: None,
            typed: [None; 2],
            applied: DVec2::ZERO,
        });
        true
    }

    /// Confirm the move for the pointer at `target`. The final offset is applied again in one
    /// step from the original geometry, so typed and snapped moves land exactly.
    fn confirm_grab(&mut self, target: DVec2, ed: &mut Editor) {
        let Some(grab) = self.grab.take() else { return };
        let offset = grab.offset(target);
        ed.doc.rollback();
        if offset == DVec2::ZERO {
            ed.sync();
            return;
        }
        ed.doc.begin();
        let dragged = grab.dragged();
        if !ed.drag(&grab.ids, offset, &dragged) {
            // Too far for one solve (constraints pull the rest along): go in steps.
            const STEPS: u32 = 16;
            for _ in 0..STEPS {
                ed.drag(&grab.ids, offset / f64::from(STEPS), &dragged);
            }
        }
        ed.commit("Move");
    }

    fn cancel_grab(&mut self, ed: &mut Editor) -> bool {
        if self.grab.take().is_none() {
            return false;
        }
        ed.doc.rollback();
        ed.sync();
        true
    }

    fn dragged_point(&self) -> Option<EntityId> {
        match self.drag {
            Some(SelectDrag::Move { point, .. }) => point,
            _ => None,
        }
    }
}

impl Tool for SelectTool {
    fn is_busy(&self) -> bool {
        self.drag.is_some() || self.grab.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Select
    }

    fn is_modal(&self) -> bool {
        self.grab.is_some()
    }

    fn key(&mut self, key: ToolKey, pointer: Option<&Pointer>, ed: &mut Editor) -> bool {
        match (key, &mut self.grab) {
            (ToolKey::G, None) => match pointer {
                Some(p) => self.start_grab(p, ed),
                None => false,
            },
            (ToolKey::X | ToolKey::Y, Some(grab)) if !grab.picking_base => {
                let axis = if key == ToolKey::X { Axis::X } else { Axis::Y };
                grab.lock = (grab.lock != Some(axis)).then_some(axis);
                let offset = grab.offset(grab.target);
                grab.apply(offset, ed);
                true
            }
            (ToolKey::B, Some(grab)) => {
                // Put the geometry back while the new base is picked.
                grab.apply(DVec2::ZERO, ed);
                grab.picking_base = true;
                true
            }
            // Swallow other modal keys during a grab.
            (_, Some(_)) => true,
            _ => false,
        }
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        let grab = self.grab.as_ref()?;
        Some((grab.base, grab.lock?))
    }

    fn fields(&self) -> &'static [&'static str] {
        match &self.grab {
            Some(grab) if !grab.picking_base => {
                if grab.lock.is_some() {
                    &["Distance"]
                } else {
                    &["dX", "dY"]
                }
            }
            _ => &[],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        if let Some(grab) = &mut self.grab {
            grab.typed = values;
        }
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        if let Some(grab) = &self.grab {
            // The pointer may be off the canvas while typing: keep the last target.
            let target = if grab.typed.iter().any(Option::is_some) {
                grab.target
            } else {
                p.pos()
            };
            self.confirm_grab(target, ed);
        }
    }

    fn measurement(&self, _p: &Pointer) -> Option<String> {
        let grab = self.grab.as_ref()?;
        if grab.picking_base {
            return Some("Base point".into());
        }
        let d = grab.offset(grab.target);
        let lock = grab
            .lock
            .map_or(String::new(), |axis| format!("  [{axis:?} locked]"));
        Some(format!(
            "dX {}  dY {}  D {}{lock}",
            format_len(d.x),
            format_len(d.y),
            format_len(d.length())
        ))
    }

    fn hint(&self) -> String {
        if let Some(grab) = &self.grab {
            return if grab.picking_base {
                "Click the base point (snaps to points) · Esc: cancel".into()
            } else {
                let lock = match grab.lock {
                    Some(axis) => format!("{axis:?} locked"),
                    None => "X / Y: lock".into(),
                };
                format!(
                    "Move: click / Enter to place · type offset · {lock} · B: base · Esc: cancel"
                )
            };
        }
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
        if let Some(grab) = &mut self.grab {
            ed.hovered = None;
            ed.hovered_midpoint = None;
            match input {
                Input::Hover(p) | Input::Drag(p) => {
                    grab.target = p.pos();
                    if !grab.picking_base {
                        grab.apply(grab.offset(p.pos()), ed);
                    }
                }
                // Blender confirms on press; a click that turned into a drag still counts.
                Input::Click(p, _) | Input::DragStart(p, _) => {
                    if grab.picking_base {
                        grab.base = p.pos();
                        grab.target = p.pos();
                        grab.picking_base = false;
                    } else {
                        self.confirm_grab(p.pos(), ed);
                    }
                }
                Input::Finish if !grab.picking_base => {
                    let target = grab.target;
                    self.confirm_grab(target, ed);
                }
                _ => {}
            }
            return;
        }
        match input {
            Input::Hover(p) => {
                ed.hovered_midpoint = None;
                ed.hovered = match self.drag {
                    Some(SelectDrag::Box { .. }) => None,
                    Some(SelectDrag::Move { point, .. }) => point,
                    None => match pick_target(ed, &p) {
                        Some(Target::Entity(id)) => Some(id),
                        Some(Target::Midpoint(line)) => {
                            ed.hovered_midpoint = Some(line);
                            None
                        }
                        None => None,
                    },
                };
            }
            Input::Click(p, mods) => {
                if let Some(Target::Midpoint(line)) = pick_target(ed, &p) {
                    if mods.command {
                        if !ed.selected_midpoints.remove(&line) {
                            ed.selected_midpoints.insert(line);
                        }
                    } else {
                        if !mods.shift {
                            ed.selection.clear();
                            ed.selected_midpoints.clear();
                        }
                        ed.selected_midpoints.insert(line);
                    }
                    return;
                }
                if !mods.shift && !mods.command {
                    ed.selected_midpoints.clear();
                }
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
                    // Dropped on a midpoint: keep it there.
                    if let (Some(from), Some(line)) = (point, p.midpoint_of())
                        && ed.doc.constraints_on(from).all(|c| {
                            ed.doc.constraint(c)
                                != Some(&Constraint::Midpoint { point: from, line })
                        })
                    {
                        let _ = ed
                            .doc
                            .add_constraint(Constraint::Midpoint { point: from, line });
                    }
                    // Dropped on a curve: keep it on the curve (refused for its own curves).
                    if let (Some(from), Some(curve)) = (point, p.on_curve())
                        && ed.doc.constraints_on(from).all(|c| {
                            ed.doc.constraint(c)
                                != Some(&Constraint::PointOnCurve { point: from, curve })
                        })
                    {
                        let _ = ed
                            .doc
                            .add_constraint(Constraint::PointOnCurve { point: from, curve });
                    }
                    ed.commit("Move");
                }
                None => {}
            },
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        if self.cancel_grab(ed) {
            return true;
        }
        match self.drag.take() {
            Some(SelectDrag::Move { .. }) => {
                ed.doc.rollback();
                ed.sync();
                true
            }
            Some(SelectDrag::Box { .. }) => true,
            None if !ed.selection.is_empty() || !ed.selected_midpoints.is_empty() => {
                ed.selection.clear();
                ed.selected_midpoints.clear();
                true
            }
            None => false,
        }
    }

    fn overlay(&self) -> Option<Overlay> {
        if let Some(grab) = &self.grab {
            return Some(Overlay::Move {
                base: grab.base,
                to: (!grab.picking_base).then(|| grab.base + grab.offset(grab.target)),
            });
        }
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
        self.dragged_point().is_some() || self.grab.is_some()
    }

    fn snap_excludes(&self, ed: &Editor, id: EntityId) -> bool {
        if let Some(grab) = &self.grab {
            // Picking a base point: anything, typically a point of the selection. Moving:
            // nothing that moves with the selection.
            return !grab.picking_base
                && (grab.points.contains(&id)
                    || grab.ids.contains(&id)
                    || ed
                        .doc
                        .get(id)
                        .is_some_and(|e| e.kind.points().any(|q| grab.points.contains(&q))));
        }
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
// Shared helpers for drawing tools

/// Field labels for typing the first point of a shape.
const XY: &[&str] = &["X", "Y"];

/// The typed coordinates, falling back to the pointer for any not typed.
fn typed_point(typed: [Option<f64>; 2], p: &Pointer) -> DVec2 {
    let at = p.pos();
    DVec2::new(typed[0].unwrap_or(at.x), typed[1].unwrap_or(at.y))
}

fn any_typed(typed: [Option<f64>; 2]) -> bool {
    typed.iter().any(Option::is_some)
}

/// A pointer at an exact location, without snaps (typed input creates new points).
fn exact(p: &Pointer, at: DVec2) -> Pointer {
    Pointer::new(at, p.tolerance)
}

/// Add a relation if automatic relations are on; failures (e.g. redundancy) are ignored and
/// the commit's solve decides whether the whole step is acceptable.
fn auto_relation(ed: &mut Editor, relation: Option<GuideKind>, line: EntityId) {
    if !ed.auto_relations {
        return;
    }
    let constraint = match relation {
        Some(GuideKind::Horizontal) => Constraint::Horizontal { line },
        Some(GuideKind::Vertical) => Constraint::Vertical { line },
        _ => return,
    };
    let _ = ed.doc.add_constraint(constraint);
}

/// Unit vector at `degrees`, exact for multiples of 90° (where `cos`/`sin` would leave ~1e-16
/// residue that the solver would then spread onto already-placed points).
fn exact_direction(degrees: f64) -> DVec2 {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() <= 1e-12 {
        match (quarter.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::from_angle(degrees.to_radians())
    }
}

/// Horizontal/vertical relation implied by a typed angle that is a multiple of 90°.
fn relation_for_angle(degrees: f64) -> Option<GuideKind> {
    let quarter = degrees / 90.0;
    if (quarter - quarter.round()).abs() > 1e-12 {
        return None;
    }
    Some(if (quarter.round() as i64).rem_euclid(2) == 0 {
        GuideKind::Horizontal
    } else {
        GuideKind::Vertical
    })
}

// ---------------------------------------------------------------------------------------------
// Line (polyline chain)

#[derive(Default)]
pub struct LineTool {
    last: Option<Anchor>,
    chain_start: Option<EntityId>,
    typed: [Option<f64>; 2],
    /// Axis the current segment is locked to; cleared when the segment is placed.
    lock: Option<Axis>,
}

impl LineTool {
    /// Where the next point goes: typed length/angle from the last point, else the pointer.
    fn target(&self, p: &Pointer) -> DVec2 {
        let Some(last) = self.last else {
            return typed_point(self.typed, p);
        };
        if !any_typed(self.typed) {
            return p.pos();
        }
        let d = p.pos() - last.pos;
        let len = self.typed[0].unwrap_or(d.length());
        let dir = match (self.typed[1], self.lock) {
            (Some(degrees), _) => exact_direction(degrees),
            // Locked: along the axis, towards the pointer's side.
            (None, Some(axis)) => {
                let u = axis.dir();
                if d.dot(u) < 0.0 { -u } else { u }
            }
            (None, None) if d.length() > 0.0 => d.normalize(),
            (None, None) => DVec2::X,
        };
        last.pos + dir * len
    }

    /// Place the next point at `anchor`. `length` adds a length dimension; `relation` an
    /// automatic horizontal/vertical relation.
    fn place(
        &mut self,
        anchor: Anchor,
        relation: Option<GuideKind>,
        length: Option<f64>,
        ed: &mut Editor,
    ) {
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
        let Ok(line) = ed.doc.add_line(a, b) else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        match self.lock {
            // An explicit axis lock always gets its relation.
            Some(axis) => {
                let _ = ed.doc.add_constraint(match axis {
                    Axis::X => Constraint::Horizontal { line },
                    Axis::Y => Constraint::Vertical { line },
                });
            }
            None => auto_relation(ed, relation, line),
        }
        if let Some(value) = length {
            let _ = ed.doc.add_constraint(Constraint::Distance { a, b, value });
        }
        if !ed.commit("Line") {
            return;
        }
        // Locks are per segment.
        self.lock = None;
        let start = *self.chain_start.get_or_insert(a);
        if b == start {
            // Closed the loop.
            self.last = None;
            self.chain_start = None;
        } else {
            self.last = Some(Anchor {
                pos: ed.doc.point(b).unwrap_or(anchor.pos),
                existing: Some(b),
                midpoint_of: None,
                on_curve: None,
            });
        }
    }

    fn finish(&mut self) {
        self.last = None;
        self.chain_start = None;
        self.lock = None;
    }

    /// Lock (or, pressed again, unlock) the segment in progress to an axis. False before the
    /// first point.
    fn lock_axis(&mut self, axis: Axis) -> bool {
        if self.last.is_none() {
            return false;
        }
        self.lock = if self.lock == Some(axis) {
            None
        } else {
            Some(axis)
        };
        true
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
            None => "Click the start point, or type X / Y".into(),
            Some(_) => match self.lock {
                Some(axis) => format!(
                    "Locked to the {axis:?} axis ({axis:?} again to unlock) · click the next point \
                     or type a length"
                ),
                None => "Click the next point, or type a length (Tab: angle) · X / Y: lock to \
                         an axis · double-click, right-click or Enter to finish"
                    .into(),
            },
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => {
                self.place(p.anchor(), p.auto_relation(), None, ed)
            }
            Input::DragStart(p, _) if self.last.is_none() => self.place(p.anchor(), None, None, ed),
            Input::DoubleClick(_) | Input::Finish => self.finish(),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.last.is_some();
        self.finish();
        had
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        let Some(last) = self.last else { return };
        if let Some(axis) = self.lock {
            // Project onto the axis through the last point. A snap target off the axis still
            // sets how far along it goes (shown with a guide line from the target); it is only
            // reused if it lies exactly on the axis.
            let u = axis.dir();
            let target = p.pos();
            let at = last.pos + u * (target - last.pos).dot(u);
            let reference = p.snap.map(|s| s.at).filter(|q| *q != at);
            if reference.is_some() {
                p.snap = None;
            }
            p.guide = Some(Guide {
                at,
                kind: axis.relation(),
                from: [Some(last.pos), reference],
            });
            return;
        }
        if p.snapped_point().is_some() {
            return;
        }
        let at = p.pos();
        let d = at - last.pos;
        let tol = p.tolerance;
        let (kind, at) = if d.y.abs() <= tol && d.x.abs() > tol {
            (GuideKind::Horizontal, DVec2::new(at.x, last.pos.y))
        } else if d.x.abs() <= tol && d.y.abs() > tol {
            (GuideKind::Vertical, DVec2::new(last.pos.x, at.y))
        } else {
            return;
        };
        let other = p.guide.and_then(|g| g.from[0]);
        p.guide = Some(Guide {
            at,
            kind,
            from: [Some(last.pos), other],
        });
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.last.is_some() {
            &["Length", "Angle"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match key {
            ToolKey::X => self.lock_axis(Axis::X),
            ToolKey::Y => self.lock_axis(Axis::Y),
            _ => false,
        }
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        Some((self.last?.pos, self.lock?))
    }

    fn chain_end(&self) -> Option<EntityId> {
        self.last?.existing
    }

    fn continue_from(&mut self, point: EntityId, ed: &mut Editor) {
        if let Ok(pos) = ed.doc.point(point) {
            self.last = Some(Anchor {
                pos,
                existing: Some(point),
                midpoint_of: None,
                on_curve: None,
            });
            self.chain_start = None;
        }
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let target = self.target(p);
        let anchor = Anchor::at(target);
        if self.last.is_none() {
            self.place(anchor, None, None, ed);
        } else {
            if self.typed[1].is_some() {
                // A typed angle overrides the lock.
                self.lock = None;
            }
            let relation = match self.typed[1] {
                Some(degrees) => relation_for_angle(degrees),
                None => p.auto_relation(),
            };
            let length = self.typed[0].filter(|v| *v > 0.0);
            self.place(anchor, relation, length, ed);
        }
        self.typed = [None; 2];
    }
    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        match (self.last, pointer) {
            (Some(last), Some(p)) => vec![
                preview(Shape::Line {
                    a: last.pos,
                    b: self.target(p),
                }),
                preview_vertex(last.pos),
            ],
            _ => Vec::new(),
        }
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let last = self.last?;
        let d = self.target(p) - last.pos;
        let angle = d.y.atan2(d.x).to_degrees();
        let lock = self
            .lock
            .map_or(String::new(), |axis| format!("  [{axis:?} locked]"));
        Some(format!("L {}  A {angle:.2}°{lock}", format_len(d.length())))
    }
}

// ---------------------------------------------------------------------------------------------
// Rectangle (two opposite corners)

#[derive(Default)]
pub struct RectangleTool {
    first: Option<Anchor>,
    typed: [Option<f64>; 2],
}

impl RectangleTool {
    fn second_corner(&self, p: &Pointer) -> DVec2 {
        let Some(first) = self.first else {
            return typed_point(self.typed, p);
        };
        let d = p.pos() - first.pos;
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        DVec2::new(
            self.typed[0].map_or(d.x, |w| sign(d.x) * w.abs()),
            self.typed[1].map_or(d.y, |h| sign(d.y) * h.abs()),
        ) + first.pos
    }

    fn create(&mut self, second: Anchor, dims: [Option<f64>; 2], ed: &mut Editor) {
        let Some(first) = self.first else { return };
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
        let mut lines = Vec::new();
        for i in 0..4 {
            match ed.doc.add_line(corners[i], corners[(i + 1) % 4]) {
                Ok(l) => lines.push(l),
                Err(_) => {
                    ed.doc.rollback();
                    ed.sync();
                    self.first = None;
                    return;
                }
            }
        }
        for (i, line) in lines.iter().enumerate() {
            let relation = if i % 2 == 0 {
                GuideKind::Horizontal
            } else {
                GuideKind::Vertical
            };
            auto_relation(ed, Some(relation), *line);
        }
        if let Some(w) = dims[0] {
            let _ = ed.doc.add_constraint(Constraint::Distance {
                a: p0,
                b: p1,
                value: w.abs(),
            });
        }
        if let Some(h) = dims[1] {
            let _ = ed.doc.add_constraint(Constraint::Distance {
                a: p1,
                b: p2,
                value: h.abs(),
            });
        }
        ed.commit("Rectangle");
        self.first = None;
    }
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
            None => "Click the first corner, or type X / Y".into(),
            Some(_) => "Click the opposite corner, or type a width (Tab: height)".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.first.is_none() => p,
            _ => return,
        };
        if self.first.is_none() {
            self.first = Some(p.anchor());
        } else {
            self.create(p.anchor(), [None; 2], ed);
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.first.take().is_some()
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.first.is_some() {
            &["Width", "Height"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let corner = self.second_corner(p);
        if self.first.is_none() {
            self.first = Some(exact(p, corner).anchor());
        } else {
            let dims = self.typed.map(|v| v.filter(|x| *x != 0.0));
            self.create(exact(p, corner).anchor(), dims, ed);
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(first), Some(p)) = (self.first, pointer) else {
            return Vec::new();
        };
        let (a, c) = (first.pos, self.second_corner(p));
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
        let d = self.second_corner(p) - self.first?.pos;
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
    typed: [Option<f64>; 2],
}

impl CircleTool {
    fn radius(&self, p: &Pointer) -> f64 {
        match (self.center, self.typed[0]) {
            (Some(_), Some(diameter)) => diameter.abs() / 2.0,
            (Some(c), None) => p.pos().distance(c.pos),
            (None, _) => 0.0,
        }
    }

    fn create(&mut self, radius: f64, dimension: bool, ed: &mut Editor) {
        let Some(center) = self.center else { return };
        if radius == 0.0 {
            return;
        }
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        match ed.doc.add_circle(c, radius) {
            Ok(circle) => {
                if dimension {
                    let _ = ed.doc.add_constraint(Constraint::Diameter {
                        curve: circle,
                        value: 2.0 * radius,
                    });
                }
                ed.commit("Circle");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        self.center = None;
    }
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
            None => "Click the centre, or type X / Y".into(),
            Some(_) => "Click a point on the circle, or type the diameter".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.center.is_none() => p,
            _ => return,
        };
        match self.center {
            None => self.center = Some(p.anchor()),
            Some(center) => {
                let radius = p.pos().distance(center.pos);
                self.create(radius, false, ed);
            }
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.center.take().is_some()
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.center.is_some() {
            &["Diameter"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match self.center {
            None => self.center = Some(exact(p, typed_point(self.typed, p)).anchor()),
            Some(_) => {
                let radius = self.radius(p);
                self.create(radius, self.typed[0].is_some(), ed);
            }
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        vec![
            preview(Shape::Circle {
                center: center.pos,
                radius: self.radius(p),
            }),
            preview_vertex(center.pos),
        ]
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        self.center?;
        let r = self.radius(p);
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
    typed: [Option<f64>; 2],
    /// A radius typed for the start point, to add as a dimension when the arc is created.
    radius_dimension: Option<f64>,
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

    /// Where the start point goes for a pointer position (typed radius, pointer direction).
    fn start_for(&self, p: &Pointer) -> DVec2 {
        let center = self.center.map_or(DVec2::ZERO, |c| c.pos);
        match self.typed[0] {
            Some(r) => {
                let d = p.pos() - center;
                let dir = if d.length() > 0.0 {
                    d.normalize()
                } else {
                    DVec2::X
                };
                center + dir * r.abs()
            }
            None => p.pos(),
        }
    }

    /// Centre, start and end of the arc for the pointer position, ordered counter-clockwise.
    fn geometry(&self, p: &Pointer) -> Option<(DVec2, DVec2, DVec2)> {
        let center = self.center?.pos;
        let start = self.start?.pos;
        let radius = start.distance(center);
        let end = match self.typed[0] {
            // Typed sweep (degrees, negative = clockwise) from the start.
            Some(sweep) => {
                let a = (start - center).y.atan2((start - center).x) + sweep.to_radians();
                center + radius * DVec2::from_angle(a)
            }
            None => {
                let dir = p.pos() - center;
                if dir == DVec2::ZERO {
                    return None;
                }
                center + dir.normalize() * radius
            }
        };
        let clockwise = match self.typed[0] {
            Some(sweep) => sweep < 0.0,
            None => self.swept < 0.0,
        };
        Some(if clockwise {
            (center, end, start)
        } else {
            (center, start, end)
        })
    }

    fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let (Some(center), Some(start)) = (self.center, self.start) else {
            return;
        };
        let Some((_, s, e)) = self.geometry(p) else {
            return;
        };
        let clockwise = s != start.pos;
        let radius = start.pos.distance(center.pos);
        // Reuse a snapped point for the free end only if it lies exactly on the circle.
        let free = Anchor {
            pos: if clockwise { s } else { e },
            existing: p
                .snapped_point()
                .filter(|_| self.typed[0].is_none())
                .filter(|_| {
                    (p.pos().distance(center.pos) - radius).abs() <= 1e-9 * radius.max(1.0)
                }),
            midpoint_of: None,
            on_curve: None,
        };
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        let st = start.resolve(&mut ed.doc);
        let fr = free.resolve(&mut ed.doc);
        let (first, last) = if clockwise { (fr, st) } else { (st, fr) };
        match ed.doc.add_arc(c, first, last) {
            Ok(arc) => {
                if let Some(value) = self.radius_dimension {
                    let _ = ed
                        .doc
                        .add_constraint(Constraint::Radius { curve: arc, value });
                }
                ed.commit("Arc");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        self.reset();
    }

    fn set_start(&mut self, anchor: Anchor) {
        self.start = Some(anchor);
        self.swept = 0.0;
        self.last_angle = self.angle_of(anchor.pos);
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
            (None, _) => "Click the arc centre, or type X / Y".into(),
            (Some(_), None) => "Click the start point (sets the radius), or type the radius".into(),
            _ => "Click the end point, or type the sweep angle · move around the centre to choose \
                  the direction"
                .into(),
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
        match (self.center, self.start) {
            (None, _) => self.center = Some(p.anchor()),
            (Some(center), None) => {
                if p.pos() != center.pos {
                    self.set_start(p.anchor());
                }
            }
            _ => self.create(&p, ed),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.center.is_some();
        self.reset();
        had
    }

    fn fields(&self) -> &'static [&'static str] {
        match (self.center, self.start) {
            (None, _) => XY,
            (Some(_), None) => &["Radius"],
            _ => &["Sweep"],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match (self.center, self.start) {
            (None, _) => self.center = Some(exact(p, typed_point(self.typed, p)).anchor()),
            (Some(_), None) => {
                let at = self.start_for(p);
                self.radius_dimension = self.typed[0].map(f64::abs).filter(|r| *r > 0.0);
                self.typed = [None; 2];
                self.set_start(Anchor::at(at));
            }
            _ => self.create(p, ed),
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(center.pos)];
        if self.start.is_none() {
            items.push(preview(Shape::Line {
                a: center.pos,
                b: self.start_for(p),
            }));
            return items;
        }
        if let Some((c, s, e)) = self.geometry(p)
            && s != e
        {
            items.push(preview(arc_shape(c, s, e)));
            items.push(preview_vertex(s));
            items.push(preview_vertex(e));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let center = self.center?.pos;
        match self.start {
            None => Some(format!(
                "R {}",
                format_len(self.start_for(p).distance(center))
            )),
            Some(start) => {
                let r = start.pos.distance(center);
                let (c, s, e) = self.geometry(p)?;
                let Shape::Arc { sweep, .. } = arc_shape(c, s, e) else {
                    return None;
                };
                Some(format!("R {}  {:.2}°", format_len(r), sweep.to_degrees()))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Three-point arc

/// Centre of the circle through three points, or `None` if they are (nearly) collinear.
/// Computed relative to `a` to keep precision far from the origin.
fn circumcenter(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let (b, c) = (b - a, c - a);
    let d = 2.0 * b.perp_dot(c);
    if d.abs() <= 1e-12 * b.length() * c.length() || d == 0.0 {
        return None;
    }
    let (bb, cc) = (b.length_squared(), c.length_squared());
    Some(a + DVec2::new(c.y * bb - b.y * cc, b.x * cc - c.x * bb) / d)
}

/// Order a start/end pair counter-clockwise for an arc from `s` to `e` that passes `m`'s side
/// of the chord: (centre, first, last), and whether they were swapped.
fn order_through(center: DVec2, s: DVec2, e: DVec2, m: DVec2) -> (DVec2, DVec2, DVec2, bool) {
    // Counter-clockwise from s to e runs on the right of the chord s → e.
    if (e - s).perp_dot(m - s) < 0.0 {
        (center, s, e, false)
    } else {
        (center, e, s, true)
    }
}

/// Start point, end point, then a point on the arc (sets the bulge and radius).
#[derive(Default)]
pub struct ThreePointArcTool {
    start: Option<Anchor>,
    end: Option<Anchor>,
    typed: [Option<f64>; 2],
}

impl ThreePointArcTool {
    /// End point for the pointer: typed length/angle from the start, else the pointer.
    fn end_for(&self, p: &Pointer) -> DVec2 {
        let Some(start) = self.start else {
            return p.pos();
        };
        if !any_typed(self.typed) {
            return p.pos();
        }
        let d = p.pos() - start.pos;
        let len = self.typed[0].unwrap_or(d.length());
        let dir = match self.typed[1] {
            Some(degrees) => exact_direction(degrees),
            None if d.length() > 0.0 => d.normalize(),
            None => DVec2::X,
        };
        start.pos + dir * len
    }

    /// (centre, first, last, swapped) for the pointer, honouring a typed radius.
    fn geometry(&self, p: &Pointer) -> Option<(DVec2, DVec2, DVec2, bool)> {
        let (s, e) = (self.start?.pos, self.end?.pos);
        let m = p.pos();
        match self.typed[0] {
            Some(r) => {
                // The shorter arc of radius r, bulging towards the pointer.
                let chord = e - s;
                let half = chord.length() / 2.0;
                let r = r.abs();
                if r < half || half == 0.0 {
                    return None;
                }
                let mut n = chord.perp().normalize();
                if n.dot(m - s) < 0.0 {
                    n = -n;
                }
                let h = (r * r - half * half).sqrt();
                let mid = s + chord / 2.0;
                let center = mid - n * h;
                Some(order_through(center, s, e, mid + n * (r - h)))
            }
            None => Some(order_through(circumcenter(s, e, m)?, s, e, m)),
        }
    }

    fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let (Some(start), Some(end)) = (self.start, self.end) else {
            return;
        };
        let Some((center, _, _, swapped)) = self.geometry(p) else {
            ed.notice = Some("The three points are in line: move off the line".into());
            return;
        };
        let radius = self.typed[0].map(f64::abs);
        // A third point picked on an existing point stays on the arc.
        let through = p.snapped_point().filter(|_| radius.is_none());
        ed.doc.begin();
        let c = ed.doc.add_point(center);
        let s = start.resolve(&mut ed.doc);
        let e = end.resolve(&mut ed.doc);
        let (first, last) = if swapped { (e, s) } else { (s, e) };
        match ed.doc.add_arc(c, first, last) {
            Ok(arc) => {
                if let Some(value) = radius {
                    let _ = ed
                        .doc
                        .add_constraint(Constraint::Radius { curve: arc, value });
                }
                if let Some(point) = through
                    && point != s
                    && point != e
                {
                    let _ = ed
                        .doc
                        .add_constraint(Constraint::PointOnCurve { point, curve: arc });
                }
                ed.commit("3-point arc");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        *self = Self::default();
    }
}

impl Tool for ThreePointArcTool {
    fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ThreePointArc
    }

    fn hint(&self) -> String {
        match (self.start, self.end) {
            (None, _) => "Click the arc's start point, or type X / Y".into(),
            (Some(_), None) => "Click the end point, or type the chord length (Tab: angle)".into(),
            _ => "Click a point on the arc (sets its size and side), or type the radius".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.start.is_none() => p,
            _ => return,
        };
        match (self.start, self.end) {
            (None, _) => self.start = Some(p.anchor()),
            (Some(start), None) => {
                if p.pos() != start.pos {
                    self.end = Some(p.anchor());
                }
            }
            _ => self.create(&p, ed),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.start.is_some();
        *self = Self::default();
        had
    }

    fn fields(&self) -> &'static [&'static str] {
        match (self.start, self.end) {
            (None, _) => XY,
            (Some(_), None) => &["Length", "Angle"],
            _ => &["Radius"],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match (self.start, self.end) {
            (None, _) => self.start = Some(exact(p, typed_point(self.typed, p)).anchor()),
            (Some(start), None) => {
                let at = self.end_for(p);
                if at != start.pos {
                    self.end = Some(Anchor::at(at));
                }
            }
            _ => self.create(p, ed),
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(start), Some(p)) = (self.start, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(start.pos)];
        match self.end {
            None => items.push(preview(Shape::Line {
                a: start.pos,
                b: self.end_for(p),
            })),
            Some(end) => {
                items.push(preview_vertex(end.pos));
                match self.geometry(p) {
                    Some((c, s, e, _)) => items.push(preview(arc_shape(c, s, e))),
                    None => items.push(preview(Shape::Line {
                        a: start.pos,
                        b: end.pos,
                    })),
                }
            }
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let start = self.start?.pos;
        if self.end.is_none() {
            let d = self.end_for(p) - start;
            return Some(format!(
                "Chord {}  A {:.2}°",
                format_len(d.length()),
                d.y.atan2(d.x).to_degrees()
            ));
        }
        let (c, s, e, _) = self.geometry(p)?;
        let Shape::Arc { sweep, .. } = arc_shape(c, s, e) else {
            return None;
        };
        Some(format!(
            "R {}  {:.2}°",
            format_len(c.distance(s)),
            sweep.to_degrees()
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Tangent arc

/// The directions a curve leaves `point` in, for each line or arc ending there: (curve, unit
/// direction continuing past the end).
fn tangents_at(doc: &drift_core::Document, point: EntityId) -> Vec<(EntityId, DVec2)> {
    let Ok(p) = doc.point(point) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, e) in doc.iter() {
        let dir = match e.kind {
            EntityKind::Line { a, b } if a == point || b == point => {
                let other = if a == point { b } else { a };
                doc.point(other).ok().map(|q| p - q)
            }
            EntityKind::Arc { center, start, end } if start == point || end == point => {
                doc.point(center).ok().map(|c| {
                    // Counter-clockwise travel direction at the end; reversed at the start.
                    let along = (p - c).perp();
                    if end == point { along } else { -along }
                })
            }
            _ => None,
        };
        if let Some(d) = dir.filter(|d| d.length() > 0.0) {
            out.push((id, d.normalize()));
        }
    }
    out
}

/// An arc leaving `from` along `dir`: centre, end and whether it turns left (counter-clockwise),
/// from a target end point and/or a typed radius and sweep (degrees, positive turns left).
fn tangent_arc(
    from: DVec2,
    dir: DVec2,
    target: DVec2,
    radius: Option<f64>,
    sweep: Option<f64>,
) -> Option<(DVec2, DVec2, bool)> {
    let n = dir.perp(); // left of the travel direction
    let d = target - from;
    let nd = n.dot(d);
    // Radius and side through the target point (centre on the normal, equidistant).
    let through = (nd.abs() > 1e-12 * d.length()).then(|| {
        let s = d.length_squared() / (2.0 * nd);
        (s.abs(), s > 0.0)
    });
    let (r, left) = match (radius, sweep) {
        (Some(r), Some(sweep)) => (r.abs(), sweep > 0.0),
        (Some(r), None) => (r.abs(), nd >= 0.0),
        (None, Some(sweep)) => (through?.0, sweep > 0.0),
        (None, None) => through?,
    };
    if r <= 0.0 {
        return None;
    }
    let center = from + n * if left { r } else { -r };
    let end = match sweep {
        Some(sweep) => center + DVec2::from_angle(sweep.to_radians()).rotate(from - center),
        None if radius.is_some() => {
            let v = target - center;
            if v == DVec2::ZERO {
                return None;
            }
            center + v.normalize() * r
        }
        None => target,
    };
    (end != from).then_some((center, end, left))
}

/// An arc tangent to the line or arc it starts from. Chains: each arc starts where the last
/// ended. Pressing L continues with lines.
#[derive(Default)]
pub struct TangentArcTool {
    /// The existing point the arc starts at, and where it is.
    start: Option<(EntityId, DVec2)>,
    /// Curves ending at the start, with the direction each leaves it in.
    tangents: Vec<(EntityId, DVec2)>,
    typed: [Option<f64>; 2],
}

impl TangentArcTool {
    fn begin_at(&mut self, point: EntityId, ed: &Editor) -> bool {
        let tangents = tangents_at(&ed.doc, point);
        let Ok(pos) = ed.doc.point(point) else {
            return false;
        };
        if tangents.is_empty() {
            return false;
        }
        self.start = Some((point, pos));
        self.tangents = tangents;
        true
    }

    /// The curve to be tangent to and its direction: the one best aligned with the pointer.
    fn tangent_for(&self, target: DVec2) -> Option<(EntityId, DVec2)> {
        let (_, from) = self.start?;
        let towards = (target - from).normalize_or_zero();
        self.tangents
            .iter()
            .copied()
            .max_by(|a, b| a.1.dot(towards).total_cmp(&b.1.dot(towards)))
    }

    /// (curve, centre, end, left) for the pointer and typed values.
    fn geometry(&self, p: &Pointer) -> Option<(EntityId, DVec2, DVec2, bool)> {
        let (_, from) = self.start?;
        let (curve, dir) = self.tangent_for(p.pos())?;
        let (center, end, left) = tangent_arc(from, dir, p.pos(), self.typed[0], self.typed[1])?;
        Some((curve, center, end, left))
    }

    fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let Some((point, _)) = self.start else {
            return;
        };
        let Some((curve, center, end, left)) = self.geometry(p) else {
            ed.notice = Some("Move off the tangent line to bend the arc".into());
            return;
        };
        let typed = any_typed(self.typed);
        let anchor = if typed {
            Anchor::at(end)
        } else {
            // Reuse a snapped point only if the arc ends exactly on it.
            let mut a = p.anchor();
            a.pos = end;
            a.existing = a.existing.filter(|_| p.pos() == end);
            a
        };
        let radius = self.typed[0].map(f64::abs);
        ed.doc.begin();
        let c = ed.doc.add_point(center);
        let e = anchor.resolve(&mut ed.doc);
        let (first, last) = if left { (point, e) } else { (e, point) };
        let Ok(arc) = ed.doc.add_arc(c, first, last) else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        let _ = ed
            .doc
            .add_constraint(Constraint::Tangent { a: curve, b: arc });
        if let Some(value) = radius {
            let _ = ed
                .doc
                .add_constraint(Constraint::Radius { curve: arc, value });
        }
        if !ed.commit("Tangent arc") {
            return;
        }
        // Chain: the next arc continues from this one's end.
        self.typed = [None; 2];
        if !self.begin_at(e, ed) {
            self.start = None;
        }
        // Only the new arc counts as the curve to continue from.
        self.tangents.retain(|(id, _)| *id == arc);
    }
}

impl Tool for TangentArcTool {
    fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::TangentArc
    }

    fn hint(&self) -> String {
        match self.start {
            None => "Click the end of a line or arc to start a tangent arc".into(),
            Some(_) => "Click the arc's end, or type radius (Tab: sweep) · L: lines · Enter /                         right-click to finish"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => match self.start {
                None => match p.snapped_point() {
                    Some(point) if self.begin_at(point, ed) => {}
                    _ => {
                        ed.notice = Some("Start a tangent arc on the end of a line or arc".into());
                    }
                },
                Some(_) => self.create(&p, ed),
            },
            Input::DragStart(p, _) if self.start.is_none() => {
                if let Some(point) = p.snapped_point() {
                    self.begin_at(point, ed);
                }
            }
            Input::DoubleClick(_) | Input::Finish => *self = Self::default(),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.start.is_some();
        *self = Self::default();
        had
    }

    fn chain_end(&self) -> Option<EntityId> {
        self.start.map(|(point, _)| point)
    }

    fn continue_from(&mut self, point: EntityId, ed: &mut Editor) {
        self.begin_at(point, ed);
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.start.is_some() {
            &["Radius", "Sweep"]
        } else {
            &[]
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        self.create(p, ed);
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some((_, from)), Some(p)) = (self.start, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(from)];
        if let Some((_, center, end, left)) = self.geometry(p) {
            let (s, e) = if left { (from, end) } else { (end, from) };
            items.push(preview(arc_shape(center, s, e)));
            items.push(preview_vertex(end));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let (_, from) = self.start?;
        let (_, center, end, left) = self.geometry(p)?;
        let (s, e) = if left { (from, end) } else { (end, from) };
        let Shape::Arc { sweep, .. } = arc_shape(center, s, e) else {
            return None;
        };
        let sweep = if left { sweep } else { -sweep };
        Some(format!(
            "R {}  {:.2}°",
            format_len(center.distance(from)),
            sweep.to_degrees()
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Point

#[derive(Default)]
pub struct PointTool {
    typed: [Option<f64>; 2],
}

impl Tool for PointTool {
    fn is_busy(&self) -> bool {
        false
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Point
    }

    fn hint(&self) -> String {
        "Click to place a point, or type X / Y".into()
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

    fn fields(&self) -> &'static [&'static str] {
        XY
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let at = typed_point(self.typed, p);
        ed.doc.begin();
        ed.doc.add_point(at);
        ed.commit("Point");
        self.typed = [None; 2];
    }
}

// ---------------------------------------------------------------------------------------------
// Smart Dimension

/// What a dimension measures, before its exact type is chosen by where the label goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DimBase {
    /// Two points (or a line's ends): aligned, horizontal or vertical by label position.
    Points(EntityId, EntityId),
    /// A circle (diameter) or an arc (radius).
    Circular(EntityId, bool),
    Angle(EntityId, EntityId),
    PointLine(EntityId, EntityId),
}

#[derive(Default)]
pub struct DimensionTool {
    /// The first entity (or midpoint) clicked, waiting for a possible second.
    first: Option<Target>,
    /// The dimension being placed.
    base: Option<DimBase>,
}

impl DimensionTool {
    /// The base implied by clicking one thing alone (a midpoint needs a second pick).
    fn single(ed: &Editor, t: Target) -> Option<DimBase> {
        let Target::Entity(e) = t else { return None };
        match ed.doc.get(e)?.kind {
            EntityKind::Line { a, b } => Some(DimBase::Points(a, b)),
            EntityKind::Circle { .. } => Some(DimBase::Circular(e, true)),
            EntityKind::Arc { .. } => Some(DimBase::Circular(e, false)),
            EntityKind::Point(_) => None,
        }
    }

    /// The base implied by clicking `first` then `second`. Midpoints act as points.
    fn pair(ed: &Editor, first: Target, second: Target) -> Option<DimBase> {
        let line_of = |t: Target| match t {
            Target::Entity(id) => match ed.doc.get(id)?.kind {
                EntityKind::Line { a, b } => Some((id, a, b)),
                _ => None,
            },
            Target::Midpoint(_) => None,
        };
        match (
            first.point_ref(ed),
            second.point_ref(ed),
            line_of(first),
            line_of(second),
        ) {
            (Some(p), Some(q), ..) if p != q => Some(DimBase::Points(p, q)),
            (.., Some((l1, ..)), Some((l2, ..))) => Some(DimBase::Angle(l1, l2)),
            (Some(p), None, None, Some((l, a, b))) | (None, Some(p), Some((l, a, b)), None)
                if p != l && p != a && p != b =>
            {
                Some(DimBase::PointLine(p, l))
            }
            _ => None,
        }
    }

    /// The dimension for `base` with its label at `at`, at the geometry's current value.
    fn dimension(ed: &Editor, base: DimBase, at: DVec2) -> Option<Constraint> {
        let c = match base {
            DimBase::Points(a, b) => {
                let (pa, pb) = (ed.doc.ref_point(a)?, ed.doc.ref_point(b)?);
                let (lo, hi) = (pa.min(pb), pa.max(pb));
                let inside_x = at.x >= lo.x && at.x <= hi.x;
                let inside_y = at.y >= lo.y && at.y <= hi.y;
                if inside_x && !inside_y {
                    Constraint::HorizontalDistance { a, b, value: 0.0 }
                } else if inside_y && !inside_x {
                    Constraint::VerticalDistance { a, b, value: 0.0 }
                } else {
                    Constraint::Distance { a, b, value: 0.0 }
                }
            }
            DimBase::Circular(curve, true) => Constraint::Diameter { curve, value: 0.0 },
            DimBase::Circular(curve, false) => Constraint::Radius { curve, value: 0.0 },
            DimBase::Angle(a, b) => Constraint::Angle { a, b, degrees: 0.0 },
            DimBase::PointLine(point, line) => Constraint::PointLineDistance {
                point,
                line,
                value: 0.0,
            },
        };
        let value = drift_solver::measure(&ed.doc, &c)?;
        Some(c.with_value(value))
    }

    fn place(&mut self, ed: &mut Editor, base: DimBase, at: DVec2) {
        self.reset(ed);
        let Some(c) = Self::dimension(ed, base, at) else {
            return;
        };
        let offset = crate::annotations::dimension_anchor(&ed.doc, &c).map(|anchor| at - anchor);
        match ed.add_dimension(c, offset) {
            Ok(id) => ed.pending_edit = Some(id),
            Err(e) => ed.notice = Some(format!("{}: {e}", c.type_name())),
        }
    }

    fn reset(&mut self, ed: &mut Editor) {
        self.first = None;
        self.base = None;
        ed.highlight.clear();
        ed.selected_midpoints.clear();
    }

    /// Show what has been picked: entities highlighted, midpoints marked.
    fn show_pick(ed: &mut Editor, t: Target, first: bool) {
        if first {
            ed.highlight.clear();
            ed.selected_midpoints.clear();
        }
        match t {
            Target::Entity(id) => {
                ed.highlight.insert(id);
            }
            Target::Midpoint(line) => {
                ed.selected_midpoints.insert(line);
            }
        }
    }
}

impl Tool for DimensionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Dimension
    }

    fn is_busy(&self) -> bool {
        self.first.is_some() || self.base.is_some()
    }

    fn hint(&self) -> String {
        match (self.first, self.base) {
            (None, None) => {
                "Click a line, circle or arc, or two points / lines / a point and a line".into()
            }
            (Some(_), None) => "Click a second point or line".into(),
            _ => "Click to place the dimension, or click another entity to change it".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                ed.hovered = None;
                ed.hovered_midpoint = None;
                match pick_target(ed, &p) {
                    Some(Target::Entity(id)) => ed.hovered = Some(id),
                    Some(Target::Midpoint(line)) => ed.hovered_midpoint = Some(line),
                    None => {}
                }
            }
            Input::Click(p, _) => {
                let picked = pick_target(ed, &p);
                // A second pick combines with the first; otherwise a click places.
                if let (Some(first), Some(t)) = (self.first, picked)
                    && t != first
                    && let Some(base) = Self::pair(ed, first, t)
                {
                    self.base = Some(base);
                    self.first = None;
                    Self::show_pick(ed, t, false);
                    return;
                }
                if let Some(base) = self.base {
                    self.place(ed, base, p.raw);
                    return;
                }
                if let Some(t) = picked {
                    self.first = Some(t);
                    self.base = Self::single(ed, t);
                    Self::show_pick(ed, t, true);
                }
            }
            Input::Finish => self.reset(ed),
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        let had = self.is_busy();
        self.reset(ed);
        had
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn pending_dimension(
        &self,
        ed: &Editor,
        pointer: Option<&Pointer>,
    ) -> Option<(Constraint, DVec2)> {
        let at = pointer?.raw;
        let c = Self::dimension(ed, self.base?, at)?;
        let anchor = crate::annotations::dimension_anchor(&ed.doc, &c)?;
        Some((c, at - anchor))
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
        tool.refine_pointer(&mut p);
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

    fn line_relations(ed: &Editor) -> Vec<Constraint> {
        ed.doc
            .constraints()
            .map(|(_, c)| *c)
            .filter(|c| {
                matches!(
                    c,
                    Constraint::Horizontal { .. } | Constraint::Vertical { .. }
                )
            })
            .collect()
    }

    #[test]
    fn axis_locks_apply_per_segment() {
        let mut ed = Editor::default();
        let mut tool = LineTool::default();
        assert!(
            !tool.lock_axis(Axis::X),
            "nothing to lock before the first point"
        );
        click(&mut ed, &mut tool, v(0.0, 0.0));
        assert!(tool.lock_axis(Axis::X));
        assert_eq!(tool.axis_lock(), Some((v(0.0, 0.0), Axis::X)));
        // Far off the axis: the end is projected onto it.
        click(&mut ed, &mut tool, v(30.0, 17.0));
        assert_eq!(tool.axis_lock(), None, "the lock ends with the segment");
        assert!(tool.lock_axis(Axis::Y));
        click(&mut ed, &mut tool, v(12.0, -20.0));
        // A third, unlocked segment gets no relation.
        click(&mut ed, &mut tool, v(50.0, 3.0));
        tool.handle(Input::Finish, &mut ed);
        let ends: Vec<_> = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Line { a, b } => {
                    Some((ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(ends[0], (v(0.0, 0.0), v(30.0, 0.0)));
        assert_eq!(ends[1], (v(30.0, 0.0), v(30.0, -20.0)));
        assert_eq!(ends[2].1, v(50.0, 3.0));
        assert!(matches!(
            line_relations(&ed)[..],
            [Constraint::Horizontal { .. }, Constraint::Vertical { .. }]
        ));
    }

    #[test]
    fn axis_lock_toggles_and_works_with_typed_lengths() {
        let mut ed = Editor::default();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(10.0, 10.0));
        tool.lock_axis(Axis::Y);
        tool.lock_axis(Axis::Y);
        assert_eq!(tool.axis_lock(), None, "pressing the key again unlocks");
        tool.lock_axis(Axis::X);
        tool.lock_axis(Axis::Y);
        assert_eq!(
            tool.axis_lock().map(|l| l.1),
            Some(Axis::Y),
            "the other key switches"
        );
        // Pointer below and to the side: a typed length goes straight down.
        let p = ptr(&ed, &tool, v(14.0, 2.0));
        tool.set_typed([Some(25.0), None]);
        tool.commit_typed(&p, &mut ed);
        let b = ed
            .doc
            .iter()
            .find_map(|(_, e)| match e.kind {
                EntityKind::Line { b, .. } => Some(b),
                _ => None,
            })
            .unwrap();
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, -15.0));
        assert!(matches!(
            line_relations(&ed)[..],
            [Constraint::Vertical { .. }]
        ));
    }

    #[test]
    fn a_locked_end_lines_up_with_a_snapped_point() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let marker = ed.doc.add_point(v(40.0, 25.0));
        ed.commit("Point");
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        tool.lock_axis(Axis::X);
        let p = ptr(&ed, &tool, v(40.1, 25.1)); // snaps to the marker point
        assert_eq!(p.pos(), v(40.0, 0.0));
        assert_eq!(p.snapped_point(), None, "an off-axis point isn't reused");
        assert_eq!(p.guide.unwrap().from[1], Some(v(40.0, 25.0)));
        tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
        assert!(ed.doc.contains(marker));
        assert_eq!(points(&ed.doc), 3);
    }

    /// A selected line from (0, 0) to (10, 0) and an unselected one ending at (30, 20):
    /// (editor, selected line's end points).
    fn grab_fixture() -> (Editor, [EntityId; 2]) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 0.0));
        let line = ed.doc.add_line(a, b).unwrap();
        let c = ed.doc.add_point(v(50.0, 50.0));
        let d = ed.doc.add_point(v(30.0, 20.0));
        ed.doc.add_line(c, d).unwrap();
        ed.commit("Setup");
        ed.selection = [line].into();
        (ed, [a, b])
    }

    fn grab(ed: &mut Editor, tool: &mut SelectTool, at: DVec2) {
        let p = ptr(ed, tool, at);
        assert!(tool.key(ToolKey::G, Some(&p), ed));
        assert!(tool.is_modal());
    }

    fn hover(ed: &mut Editor, tool: &mut SelectTool, at: DVec2) {
        let p = ptr(ed, tool, at);
        tool.handle(Input::Hover(p), ed);
    }

    fn ends(ed: &Editor, [a, b]: [EntityId; 2]) -> [DVec2; 2] {
        [ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()]
    }

    #[test]
    fn grab_follows_the_pointer_and_confirms_in_one_undo_step() {
        let (mut ed, pts) = grab_fixture();
        let mut tool = SelectTool::default();
        grab(&mut ed, &mut tool, v(5.0, 0.0));
        hover(&mut ed, &mut tool, v(8.0, 3.0));
        assert_eq!(ends(&ed, pts), [v(3.0, 3.0), v(13.0, 3.0)], "live preview");
        click(&mut ed, &mut tool, v(8.0, 3.0));
        assert!(!tool.is_modal());
        assert_eq!(ends(&ed, pts), [v(3.0, 3.0), v(13.0, 3.0)]);
        assert!(ed.undo());
        assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
    }

    #[test]
    fn grab_takes_axis_locks_and_typed_offsets() {
        let (mut ed, pts) = grab_fixture();
        let mut tool = SelectTool::default();
        grab(&mut ed, &mut tool, v(5.0, 0.0));
        assert!(tool.key(ToolKey::Y, None, &mut ed));
        assert_eq!(tool.fields(), ["Distance"]);
        hover(&mut ed, &mut tool, v(9.0, 7.0));
        assert_eq!(ends(&ed, pts)[0], v(0.0, 7.0), "locked to Y");
        tool.set_typed([Some(-4.25), None]);
        let p = ptr(&ed, &tool, v(9.0, 7.0));
        tool.commit_typed(&p, &mut ed);
        assert_eq!(ends(&ed, pts), [v(0.0, -4.25), v(10.0, -4.25)]);

        // Unlocked: dX and dY.
        grab(&mut ed, &mut tool, v(5.0, 0.0));
        assert_eq!(tool.fields(), ["dX", "dY"]);
        tool.set_typed([Some(0.1), Some(0.2)]);
        let p = ptr(&ed, &tool, v(40.0, 40.0));
        tool.commit_typed(&p, &mut ed);
        assert_eq!(ends(&ed, pts)[1], v(10.0 + 0.1, -4.25 + 0.2));
    }

    #[test]
    fn grab_from_a_picked_base_point_lands_exactly_on_a_snap() {
        let (mut ed, pts) = grab_fixture();
        let mut tool = SelectTool::default();
        grab(&mut ed, &mut tool, v(4.0, 1.0));
        hover(&mut ed, &mut tool, v(20.0, 9.0));
        assert!(tool.key(ToolKey::B, None, &mut ed));
        // Picking the base point puts the geometry back and can snap to the selection's own end.
        hover(&mut ed, &mut tool, v(10.1, 0.1));
        assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
        click(&mut ed, &mut tool, v(10.1, 0.1));
        assert!(tool.is_modal(), "picking the base doesn't place the move");
        // Moving snaps to the other line's end, but not to the moving line.
        hover(&mut ed, &mut tool, v(30.2, 19.9));
        click(&mut ed, &mut tool, v(30.2, 19.9));
        assert_eq!(ends(&ed, pts), [v(20.0, 20.0), v(30.0, 20.0)]);
    }

    #[test]
    fn cancelling_a_grab_restores_the_geometry() {
        let (mut ed, pts) = grab_fixture();
        let mut tool = SelectTool::default();
        grab(&mut ed, &mut tool, v(5.0, 0.0));
        hover(&mut ed, &mut tool, v(25.0, 13.0));
        assert!(tool.cancel(&mut ed));
        assert!(!tool.is_modal());
        assert_eq!(ends(&ed, pts), [v(0.0, 0.0), v(10.0, 0.0)]);
        // The selection survives the cancel.
        assert!(!ed.selection.is_empty());
        // No move was recorded: one undo removes the setup itself.
        assert!(ed.undo());
        assert_eq!(ed.doc.iter().count(), 0);
        // G with nothing selected only explains.
        let p = ptr(&ed, &tool, v(0.0, 0.0));
        assert!(tool.key(ToolKey::G, Some(&p), &mut ed));
        assert!(!tool.is_modal());
        assert!(ed.notice.take().is_some());
    }

    /// The single arc in the document: (centre, start, end) positions.
    fn the_arc(ed: &Editor) -> (EntityId, [DVec2; 3]) {
        let arcs: Vec<_> = ed
            .doc
            .iter()
            .filter_map(|(id, e)| match e.kind {
                EntityKind::Arc { center, start, end } => {
                    Some((id, [center, start, end].map(|q| ed.doc.point(q).unwrap())))
                }
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 1, "expected one arc");
        arcs[0]
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn three_point_arc_bulges_through_the_third_point() {
        for (third, center, start, end) in [
            // Above the chord: counter-clockwise from the end back to the start.
            (v(10.0, 5.0), v(10.0, -7.5), v(20.0, 0.0), v(0.0, 0.0)),
            // Below: counter-clockwise from the start.
            (v(10.0, -5.0), v(10.0, 7.5), v(0.0, 0.0), v(20.0, 0.0)),
        ] {
            let mut ed = Editor::default();
            let mut tool = ThreePointArcTool::default();
            click(&mut ed, &mut tool, v(0.0, 0.0));
            click(&mut ed, &mut tool, v(20.0, 0.0));
            click(&mut ed, &mut tool, third);
            let (_, [c, s, e]) = the_arc(&ed);
            assert!(
                close(c, center) && close(s, start) && close(e, end),
                "{c} {s} {e}"
            );
            assert!(!tool.is_busy());
        }
    }

    #[test]
    fn three_point_arc_takes_typed_values_and_refuses_collinear_points() {
        let mut ed = Editor::default();
        let mut tool = ThreePointArcTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        let p = ptr(&ed, &tool, v(3.0, 1.0));
        tool.set_typed([Some(20.0), Some(0.0)]);
        tool.commit_typed(&p, &mut ed);
        // Collinear third point: refused, still waiting.
        click(&mut ed, &mut tool, v(40.0, 0.0));
        assert!(ed.notice.take().is_some());
        assert!(tool.is_busy());
        // Typed radius, bulging towards the pointer (above).
        let p = ptr(&ed, &tool, v(8.0, 30.0));
        tool.set_typed([Some(12.5), None]);
        tool.commit_typed(&p, &mut ed);
        let (arc, [c, ..]) = the_arc(&ed);
        assert!(close(c, v(10.0, -7.5)), "{c}");
        assert!(ed.doc.constraints().any(|(_, k)| *k
            == Constraint::Radius {
                curve: arc,
                value: 12.5
            }));
    }

    /// A line from (0, 0) to (10, 0): (editor, its end point).
    fn line_end_fixture() -> (Editor, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 0.0));
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        (ed, b)
    }

    fn tangents(ed: &Editor) -> usize {
        ed.doc
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::Tangent { .. }))
            .count()
    }

    #[test]
    fn tangent_arcs_continue_lines_and_chain() {
        let (mut ed, end) = line_end_fixture();
        let mut tool = TangentArcTool::default();
        click(&mut ed, &mut tool, v(10.1, 0.1)); // snaps to the line's end
        assert_eq!(tool.chain_end(), Some(end));
        click(&mut ed, &mut tool, v(20.0, 10.0));
        let (_, [c, s, e]) = the_arc(&ed);
        // Leaves along +X, turning left: a quarter circle about (10, 10).
        assert!(close(c, v(10.0, 10.0)) && close(s, v(10.0, 0.0)) && close(e, v(20.0, 10.0)));
        assert_eq!(tangents(&ed), 1);
        // Chained: the next arc leaves the first one's end going up, and turns right.
        click(&mut ed, &mut tool, v(30.0, 20.0));
        let centers: Vec<DVec2> = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Arc { center, .. } => Some(ed.doc.point(center).unwrap()),
                _ => None,
            })
            .collect();
        assert!(
            centers.iter().any(|c| close(*c, v(30.0, 10.0))),
            "{centers:?}"
        );
        assert_eq!(tangents(&ed), 2);
        assert!(ed.analysis().conflicts.is_empty());
        tool.handle(Input::Finish, &mut ed);
        assert!(!tool.is_busy());
    }

    #[test]
    fn tangent_arc_takes_a_typed_radius_and_sweep() {
        let (mut ed, _) = line_end_fixture();
        let mut tool = TangentArcTool::default();
        click(&mut ed, &mut tool, v(10.0, 0.0));
        let p = ptr(&ed, &tool, v(14.0, 3.0));
        tool.set_typed([Some(5.0), Some(-90.0)]); // right turn
        tool.commit_typed(&p, &mut ed);
        let (arc, [c, s, e]) = the_arc(&ed);
        assert!(close(c, v(10.0, -5.0)), "{c}");
        // Clockwise from the line's end, so stored counter-clockwise from the far end.
        assert!(close(s, v(15.0, -5.0)) && close(e, v(10.0, 0.0)), "{s} {e}");
        assert!(ed.doc.constraints().any(|(_, k)| *k
            == Constraint::Radius {
                curve: arc,
                value: 5.0
            }));
    }

    #[test]
    fn tangent_arc_needs_a_curve_end_to_start_from() {
        let mut ed = Editor::default();
        ed.doc.begin();
        ed.doc.add_point(v(0.0, 0.0));
        ed.commit("Point");
        let mut tool = TangentArcTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        assert!(!tool.is_busy());
        assert!(ed.notice.take().is_some());
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
        click(&mut ed, &mut sel, v(3.0, 0.1));
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

    // ---- M3: automatic relations, typed input, Smart Dimension ----

    fn constraints(ed: &Editor) -> Vec<Constraint> {
        ed.doc.constraints().map(|(_, c)| *c).collect()
    }

    fn with_auto_relations() -> Editor {
        Editor::new(Document::new())
    }

    #[test]
    fn near_horizontal_line_snaps_and_gets_a_relation() {
        let mut ed = with_auto_relations();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(10.0, 0.3)); // within tolerance of horizontal
        let (_, line) = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .unwrap();
        let EntityKind::Line { a, b } = line.kind else {
            unreachable!()
        };
        assert_eq!(ed.doc.point(b).unwrap().y, ed.doc.point(a).unwrap().y);
        assert!(matches!(
            constraints(&ed)[..],
            [Constraint::Horizontal { .. }]
        ));
        // Without automatic relations the line is still drawn straight, but unconstrained.
        let mut ed = Editor::default();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        click(&mut ed, &mut tool, v(10.0, 0.3));
        assert_eq!(ed.doc.constraint_count(), 0);
    }

    #[test]
    fn typed_length_and_angle_make_an_exact_dimensioned_line() {
        let mut ed = with_auto_relations();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        tool.set_typed([Some(25.0), Some(90.0)]);
        let p = ptr(&ed, &tool, v(3.0, 3.0));
        tool.commit_typed(&p, &mut ed);
        assert_eq!(lines(&ed.doc), 1);
        let end = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Point(p) if p != v(0.0, 0.0) => Some(p),
                _ => None,
            })
            .next()
            .unwrap();
        assert_eq!(end, v(0.0, 25.0));
        assert!(
            ed.doc
                .iter()
                .any(|(_, e)| e.kind == EntityKind::Point(v(0.0, 0.0)))
        );
        let cs = constraints(&ed);
        assert!(cs.iter().any(|c| matches!(c, Constraint::Vertical { .. })));
        assert!(
            cs.iter()
                .any(|c| matches!(c, Constraint::Distance { value, .. } if *value == 25.0))
        );
        // The chain continues from the new end.
        assert!(tool.is_busy());
    }

    #[test]
    fn typed_coordinates_place_points_exactly() {
        let mut ed = Editor::default();
        let mut tool = PointTool::default();
        tool.set_typed([Some(12.5), Some(-3.25)]);
        let p = ptr(&ed, &tool, v(100.0, 100.0));
        tool.commit_typed(&p, &mut ed);
        let (_, point) = ed.doc.iter().next().unwrap();
        assert_eq!(point.kind, EntityKind::Point(v(12.5, -3.25)));
    }

    #[test]
    fn rectangle_is_constrained_square_and_typed_size_is_dimensioned() {
        let mut ed = with_auto_relations();
        let mut tool = RectangleTool::default();
        click(&mut ed, &mut tool, v(0.0, 0.0));
        tool.set_typed([Some(40.0), Some(25.0)]);
        let p = ptr(&ed, &tool, v(10.0, 10.0));
        tool.commit_typed(&p, &mut ed);
        assert_eq!(ed.bounds().max, v(40.0, 25.0));
        let cs = constraints(&ed);
        assert_eq!(
            cs.iter()
                .filter(|c| matches!(c, Constraint::Horizontal { .. }))
                .count(),
            2
        );
        assert_eq!(
            cs.iter()
                .filter(|c| matches!(c, Constraint::Vertical { .. }))
                .count(),
            2
        );
        assert_eq!(
            cs.iter()
                .filter(|c| matches!(c, Constraint::Distance { .. }))
                .count(),
            2
        );
        // Only the position is left free.
        assert_eq!(ed.analysis().dof, Some(2));
    }

    #[test]
    fn smart_dimension_picks_the_type_from_the_label_position() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 5.0));
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        let mut tool = DimensionTool::default();
        let on_line = v(2.0, 1.0); // on the line, away from its midpoint

        // Label above the line, within its x range: horizontal distance.
        click(&mut ed, &mut tool, on_line);
        click(&mut ed, &mut tool, v(5.0, 20.0));
        assert!(matches!(
            constraints(&ed)[..],
            [Constraint::HorizontalDistance { value, .. }] if value == 10.0
        ));
        let id = ed.pending_edit.take().expect("value editor requested");
        assert_eq!(ed.doc.placement(id), Some(v(0.0, 17.5)));

        // Label to the right, within its y range: vertical distance.
        click(&mut ed, &mut tool, on_line);
        click(&mut ed, &mut tool, v(30.0, 2.0));
        assert!(matches!(
            constraints(&ed)[1],
            Constraint::VerticalDistance { value, .. } if value == 5.0
        ));

        // Now the length is implied: an aligned dimension is refused as redundant.
        click(&mut ed, &mut tool, on_line);
        click(&mut ed, &mut tool, v(12.0, -4.0));
        assert_eq!(ed.doc.constraint_count(), 2);
        assert!(
            ed.notice
                .take()
                .is_some_and(|n| n.contains("already implied"))
        );
        // ...and offered as a driven dimension instead.
        let (driven, placement) = ed.pending_driven.take().expect("driven prompt");
        assert!(matches!(driven, Constraint::Distance { .. }));
        let id = ed.add_driven_dimension(driven, placement).unwrap();
        assert!(ed.doc.is_driven(id));
        assert!((driven.value().unwrap() - 125f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn smart_dimension_angles_diameters_and_point_line_distances() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let o = ed.doc.add_point(v(0.0, 0.0));
        let x = ed.doc.add_point(v(20.0, 0.0));
        let y = ed.doc.add_point(v(10.0, 10.0));
        let l1 = ed.doc.add_line(o, x).unwrap();
        let l2 = ed.doc.add_line(o, y).unwrap();
        let c = ed.doc.add_point(v(50.0, 0.0));
        let circle = ed.doc.add_circle(c, 4.0).unwrap();
        let q = ed.doc.add_point(v(30.0, 7.0));
        ed.commit("Setup");
        let mut tool = DimensionTool::default();

        click(&mut ed, &mut tool, v(14.0, 0.0)); // on l1
        click(&mut ed, &mut tool, v(7.0, 7.0)); // on l2
        click(&mut ed, &mut tool, v(8.0, 3.0)); // place
        assert!(constraints(&ed).iter().any(|c| matches!(c,
            Constraint::Angle { a, b, degrees } if *a == l1 && *b == l2 && (degrees - 45.0).abs() < 1e-9)));

        click(&mut ed, &mut tool, v(54.0, 0.0)); // on the circle
        click(&mut ed, &mut tool, v(60.0, 6.0));
        assert!(constraints(&ed).iter().any(|cc| matches!(cc,
            Constraint::Diameter { curve, value } if *curve == circle && *value == 8.0)));

        click(&mut ed, &mut tool, v(30.0, 7.0)); // the point
        click(&mut ed, &mut tool, v(16.0, 0.0)); // l1
        click(&mut ed, &mut tool, v(35.0, 3.0));
        assert!(constraints(&ed).iter().any(|cc| matches!(cc,
            Constraint::PointLineDistance { point, line, value } if *point == q && *line == l1 && *value == 7.0)));
    }

    // ---- Midpoints ----

    /// A horizontal line 0..20 and a separate slanted line; returns (base, base midpoint pos,
    /// slanted line's end points).
    fn midpoint_fixture() -> (Editor, EntityId, [EntityId; 2]) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(20.0, 0.0));
        let base = ed.doc.add_line(a, b).unwrap();
        let c = ed.doc.add_point(v(13.0, 6.0));
        let d = ed.doc.add_point(v(25.0, 14.0));
        ed.doc.add_line(c, d).unwrap();
        ed.commit("Setup");
        (ed, base, [c, d])
    }

    #[test]
    fn midpoints_can_be_hovered_picked_and_constrained() {
        let (mut ed, base, [c, _]) = midpoint_fixture();
        let mut sel = SelectTool::default();
        let near_mid = v(10.1, 0.1);
        sel.handle(Input::Hover(ptr(&ed, &sel, near_mid)), &mut ed);
        assert_eq!(ed.hovered_midpoint, Some(base));
        assert_eq!(ed.hovered, None);
        click(&mut ed, &mut sel, near_mid);
        assert_eq!(ed.selected_midpoints, [base].into());
        // Shift-click the other line's end point.
        let p = ptr(&ed, &sel, v(13.1, 6.1));
        sel.handle(
            Input::Click(
                p,
                Modifiers {
                    shift: true,
                    command: false,
                },
            ),
            &mut ed,
        );
        assert_eq!(ed.selection, [c].into());
        let midpoint = ed
            .relation_options()
            .into_iter()
            .find(|o| o.label == "Midpoint")
            .expect("Midpoint offered");
        ed.add_constraints("Midpoint", &midpoint.constraints)
            .unwrap();
        // Nothing is fixed, so the smallest change moves both: check the relation holds.
        let EntityKind::Line { a, b } = ed.doc.get(base).unwrap().kind else {
            unreachable!()
        };
        let mid = (ed.doc.point(a).unwrap() + ed.doc.point(b).unwrap()) * 0.5;
        assert!((ed.doc.point(c).unwrap() - mid).length() < 1e-12);
        // Esc clears both kinds of selection.
        assert!(sel.cancel(&mut ed));
        assert!(ed.selection.is_empty() && ed.selected_midpoints.is_empty());
    }

    #[test]
    fn point_and_line_selection_offers_midpoint() {
        let (mut ed, base, [c, _]) = midpoint_fixture();
        ed.selection = [base, c].into();
        assert!(ed.relation_options().iter().any(|o| o.label == "Midpoint"));
    }

    #[test]
    fn starting_a_line_on_a_midpoint_adds_the_relation() {
        let (mut ed, base, _) = midpoint_fixture();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(10.1, 0.2)); // snaps to the base's midpoint
        click(&mut ed, &mut tool, v(10.0, -15.0));
        let Some(Constraint::Midpoint { point, line }) = ed
            .doc
            .constraints()
            .map(|(_, c)| *c)
            .find(|c| matches!(c, Constraint::Midpoint { .. }))
        else {
            panic!("no midpoint relation")
        };
        assert_eq!(line, base);
        // Dragging the base (weighting its points, as the select tool does) carries the new
        // line's start along with its midpoint.
        let EntityKind::Line { a, b } = ed.doc.get(base).unwrap().kind else {
            unreachable!()
        };
        let dragged: Vec<Param> = [a, b].into_iter().flat_map(Param::point).collect();
        ed.doc.begin();
        assert!(ed.drag(&[base], v(4.0, 0.0), &dragged));
        ed.commit("Move");
        let mid = (ed.doc.point(a).unwrap() + ed.doc.point(b).unwrap()) * 0.5;
        assert!((mid - v(14.0, 0.0)).length() < 1e-5, "{mid}");
        assert!((ed.doc.point(point).unwrap() - mid).length() < 1e-9);
    }

    /// A circle of radius 10 at the origin: (editor, circle).
    fn circle_fixture() -> (Editor, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(c, 10.0).unwrap();
        ed.commit("Circle");
        (ed, circle)
    }

    fn on_curve_relations(ed: &Editor) -> Vec<(EntityId, EntityId)> {
        ed.doc
            .constraints()
            .filter_map(|(_, c)| match *c {
                Constraint::PointOnCurve { point, curve } => Some((point, curve)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_line_ending_on_a_curve_stays_on_it() {
        let (mut ed, circle) = circle_fixture();
        let mut tool = LineTool::default();
        click(&mut ed, &mut tool, v(30.0, 0.0));
        click(&mut ed, &mut tool, v(6.1, 8.1)); // near (6, 8) on the circle
        tool.handle(Input::Finish, &mut ed);
        let [(point, curve)] = on_curve_relations(&ed)[..] else {
            panic!("expected one on-curve relation")
        };
        assert_eq!(curve, circle);
        assert!((ed.doc.point(point).unwrap().length() - 10.0).abs() < 1e-9);
        // Growing the circle carries the end along.
        ed.doc.begin();
        let _ = ed.doc.add_constraint(Constraint::Radius {
            curve: circle,
            value: 15.0,
        });
        ed.commit("Radius");
        let EntityKind::Circle { center, radius } = ed.doc.get(circle).unwrap().kind else {
            unreachable!()
        };
        assert!((radius - 15.0).abs() < 1e-9);
        let from_centre = ed.doc.point(point).unwrap() - ed.doc.point(center).unwrap();
        assert!((from_centre.length() - 15.0).abs() < 1e-9);

        // Without automatic relations the point is only placed there.
        let (mut ed, _) = circle_fixture();
        ed.auto_relations = false;
        let mut tool = LineTool::default();
        let mut p = ptr(&ed, &tool, v(30.0, 0.0));
        p.relations = false;
        tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
        let mut p = ptr(&ed, &tool, v(6.1, 8.1));
        p.relations = false;
        tool.handle(Input::Click(p, Modifiers::default()), &mut ed);
        assert!(on_curve_relations(&ed).is_empty());
    }

    #[test]
    fn dropping_a_point_on_a_curve_constrains_it() {
        let (mut ed, circle) = circle_fixture();
        ed.doc.begin();
        let p = ed.doc.add_point(v(20.0, 20.0));
        ed.commit("Point");
        let mut sel = SelectTool::default();
        let m = Modifiers::default();
        let grab = ptr(&ed, &sel, v(20.0, 20.0));
        sel.handle(Input::DragStart(grab, m), &mut ed);
        let to = ptr(&ed, &sel, v(-6.1, 8.1));
        assert_eq!(to.on_curve(), Some(circle));
        sel.handle(Input::Drag(to), &mut ed);
        sel.handle(Input::DragEnd(to, m), &mut ed);
        assert_eq!(on_curve_relations(&ed), [(p, circle)]);
        assert!((ed.doc.point(p).unwrap().length() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn dropping_an_end_point_on_a_midpoint_constrains_it() {
        let (mut ed, base, [c, _]) = midpoint_fixture();
        let mut sel = SelectTool::default();
        let m = Modifiers::default();
        let grab = ptr(&ed, &sel, v(13.0, 6.0));
        sel.handle(Input::DragStart(grab, m), &mut ed);
        let to = ptr(&ed, &sel, v(10.2, 0.2));
        assert_eq!(to.midpoint_of(), Some(base));
        sel.handle(Input::Drag(to), &mut ed);
        sel.handle(Input::DragEnd(to, m), &mut ed);
        assert_eq!(ed.doc.point(c).unwrap(), v(10.0, 0.0));
        assert!(ed.doc.constraints().any(|(_, k)| *k
            == Constraint::Midpoint {
                point: c,
                line: base
            }));
    }

    #[test]
    fn smart_dimension_measures_to_midpoints() {
        // Base 0..20 on the x axis (midpoint (10, 0)); a second line from (13, 6) to (25, 14),
        // midpoint (19, 10).
        let (mut ed, base, [c, _]) = midpoint_fixture();
        let other = ed
            .doc
            .iter()
            .find(|(id, e)| matches!(e.kind, EntityKind::Line { .. }) && *id != base)
            .map(|(id, _)| id)
            .unwrap();
        let mut tool = DimensionTool::default();
        let base_mid = v(10.1, 0.1);

        // Midpoint -> point, label above both (inside their x range): horizontal distance.
        click(&mut ed, &mut tool, base_mid);
        assert_eq!(
            ed.selected_midpoints,
            [base].into(),
            "picked midpoint is marked"
        );
        click(&mut ed, &mut tool, v(13.1, 6.1));
        click(&mut ed, &mut tool, v(11.0, 20.0));
        assert!(constraints(&ed).contains(&Constraint::HorizontalDistance {
            a: base,
            b: c,
            value: 3.0
        }));
        assert!(
            ed.selected_midpoints.is_empty(),
            "pick marks cleared after placing"
        );

        // Midpoint -> midpoint, aligned.
        click(&mut ed, &mut tool, base_mid);
        click(&mut ed, &mut tool, v(19.05, 10.05));
        click(&mut ed, &mut tool, v(25.0, -5.0)); // outside both ranges: aligned
        let expected = v(10.0, 0.0).distance(v(19.0, 10.0));
        assert!(constraints(&ed).iter().any(|k| matches!(k,
            Constraint::Distance { a, b, value } if *a == base && *b == other
                && (value - expected).abs() < 1e-12)));

        // Midpoint -> another line: perpendicular distance.
        click(&mut ed, &mut tool, v(19.05, 10.05)); // other's midpoint
        click(&mut ed, &mut tool, v(4.0, 0.0)); // the base line (away from its midpoint)
        click(&mut ed, &mut tool, v(25.0, 5.0));
        assert!(constraints(&ed).iter().any(|k| matches!(k,
            Constraint::PointLineDistance { point, line, value } if *point == other
                && *line == base && (value - 10.0).abs() < 1e-12)));
    }

    #[test]
    fn panel_offers_distances_to_selected_midpoints() {
        let (mut ed, base, [c, _]) = midpoint_fixture();
        ed.selected_midpoints = [base].into();
        ed.selection = [c].into();
        let labels: Vec<_> = ed.dimension_options().iter().map(|o| o.label).collect();
        assert_eq!(
            labels,
            ["Distance", "Horizontal distance", "Vertical distance"]
        );
        let distance = &ed.dimension_options()[0].constraints[0];
        assert_eq!(distance.value(), Some(v(10.0, 0.0).distance(v(13.0, 6.0))));
    }
}
