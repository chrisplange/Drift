//! Select: pick, box-select, drag, and the Blender-style modal move (G).

use super::*;

/// What the select tool would pick under the pointer: points first, then line midpoints,
/// then curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Entity(EntityId),
    Midpoint(EntityId),
}

impl Target {
    /// The id a dimension uses for this pick when it's a point-like reference: a point, or a
    /// line standing for its midpoint.
    pub(super) fn point_ref(self, ed: &Editor) -> Option<EntityId> {
        match self {
            Target::Midpoint(line) => Some(line),
            Target::Entity(id) => ed.doc.get(id).filter(|e| e.kind.is_point()).map(|_| id),
        }
    }
}

/// Midpoints are picked within this fraction of the normal pick radius.
pub(super) const MIDPOINT_PICK: f64 = 0.6;

pub(super) fn pick_target(ed: &Editor, p: &Pointer) -> Option<Target> {
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
pub(super) enum SelectDrag {
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
pub(super) struct Grab {
    pub(super) ids: Vec<EntityId>,
    /// Defining points of the selection: these move rigidly with the offset.
    pub(super) points: BTreeSet<EntityId>,
    pub(super) base: DVec2,
    /// Where the pointer is (the move's target before locks and typed values).
    pub(super) target: DVec2,
    pub(super) picking_base: bool,
    pub(super) lock: Option<Axis>,
    pub(super) typed: [Option<f64>; 2],
    /// The offset the geometry has been moved by so far in the open transaction.
    pub(super) applied: DVec2,
}

impl Grab {
    /// The offset for the pointer at `target`, honouring the lock and typed values.
    pub(super) fn offset(&self, target: DVec2) -> DVec2 {
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

    pub(super) fn dragged(&self) -> Vec<Param> {
        self.points.iter().copied().flat_map(Param::point).collect()
    }

    /// Move the geometry to `offset` (live, in steps from the last offset so constraints
    /// follow smoothly).
    pub(super) fn apply(&mut self, offset: DVec2, ed: &mut Editor) {
        let step = offset - self.applied;
        if step != DVec2::ZERO && ed.drag(&self.ids, step, &self.dragged()) {
            self.applied = offset;
        }
    }
}

#[derive(Default)]
pub struct SelectTool {
    pub(super) drag: Option<SelectDrag>,
    pub(super) grab: Option<Grab>,
}

impl SelectTool {
    /// Start a modal move of the selection from the pointer.
    pub(super) fn start_grab(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
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
    pub(super) fn confirm_grab(&mut self, target: DVec2, ed: &mut Editor) {
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

    pub(super) fn cancel_grab(&mut self, ed: &mut Editor) -> bool {
        if self.grab.take().is_none() {
            return false;
        }
        ed.doc.rollback();
        ed.sync();
        true
    }

    pub(super) fn dragged_point(&self) -> Option<EntityId> {
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

    fn measurement(&self, p: &Pointer) -> Option<String> {
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
            p.units.format_readout(d.x),
            p.units.format_readout(d.y),
            p.units.format_readout(d.length())
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
