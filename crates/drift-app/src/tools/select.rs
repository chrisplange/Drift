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

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SelectDrag {
    Box {
        start: DVec2,
        current: DVec2,
    },
    /// Dragging the selection: it follows the pointer's motion; when the pointer snaps, the
    /// base point (the point, midpoint or centre nearest the grab) lands on the snap.
    Move {
        ids: Vec<EntityId>,
        points: BTreeSet<EntityId>,
        /// Pointer where the drag began.
        start: DVec2,
        base: DVec2,
        /// Offset applied so far in the open transaction.
        applied: DVec2,
        /// Set when a single point is being dragged; it can merge on drop.
        point: Option<EntityId>,
    },
    /// Dragging a circle's edge: the radius follows the pointer (drag its centre to move it).
    Radius {
        circle: EntityId,
        center: DVec2,
    },
}

/// A Blender-style modal move (G): the selection follows the pointer from a base point until a
/// click or Enter confirms it, or Esc / right-click cancels. X / Y lock an axis, numbers give
/// the exact offset, and B picks a new base point.
pub(super) struct Grab {
    pub(super) ids: Vec<EntityId>,
    /// Defining points of the selection: these move rigidly with the offset.
    pub(super) points: BTreeSet<EntityId>,
    /// The point that lands on snap targets: a selected point, a line's midpoint, a
    /// circle's centre or a region's centroid by default; B picks another.
    pub(super) base: DVec2,
    /// Pointer where the move (or the base point) began.
    pub(super) start: DVec2,
    /// The pointer's offset before locks and typed values (see `move_offset`).
    pub(super) delta: DVec2,
    pub(super) picking_base: bool,
    /// A region being moved (its seed point follows it).
    pub(super) region: bool,
    pub(super) lock: Option<Axis>,
    pub(super) typed: [Option<f64>; 2],
    /// The offset the geometry has been moved by so far in the open transaction.
    pub(super) applied: DVec2,
}

impl Grab {
    /// The offset for the latest pointer position, honouring the lock and typed values.
    pub(super) fn offset(&self) -> DVec2 {
        let d = self.delta;
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
        // A selected region moves its boundary, based on its centroid.
        let region = if ed.selection.is_empty() {
            ed.region()
        } else {
            None
        };
        let (ids, base) = match &region {
            Some(r) => (ed.region_curves(r), r.centroid),
            None => {
                let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                let base = ed.move_base(&ids, p.raw, false);
                (ids, base)
            }
        };
        if ids.is_empty() {
            ed.notice = Some("Select something to move first".into());
            return true;
        }
        let points = ed.doc.defining_points(ids.iter().copied());
        ed.doc.begin();
        self.grab = Some(Grab {
            ids,
            points,
            base,
            start: p.raw,
            delta: DVec2::ZERO,
            picking_base: false,
            region: region.is_some(),
            lock: None,
            typed: [None; 2],
            applied: DVec2::ZERO,
        });
        true
    }

    /// Confirm the move for the pointer at `target`. The final offset is applied again in one
    /// step from the original geometry, so typed and snapped moves land exactly.
    pub(super) fn confirm_grab(&mut self, ed: &mut Editor) {
        let Some(grab) = self.grab.take() else { return };
        let offset = grab.offset();
        if grab.region
            && let Some(seed) = &mut ed.selected_region
        {
            *seed += offset;
        }
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

    /// The points moving with a drag or grab (never snap targets).
    fn moving(&self) -> Option<(&BTreeSet<EntityId>, &[EntityId])> {
        if let Some(grab) = &self.grab {
            return (!grab.picking_base).then_some((&grab.points, grab.ids.as_slice()));
        }
        match &self.drag {
            Some(SelectDrag::Move { points, ids, .. }) => Some((points, ids.as_slice())),
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
                let offset = grab.offset();
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

    fn commit_typed(&mut self, _p: &Pointer, ed: &mut Editor) {
        // The pointer may be off the canvas while typing: the last offset stands.
        if self.grab.is_some() {
            self.confirm_grab(ed);
        }
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let grab = self.grab.as_ref()?;
        if grab.picking_base {
            return Some("Base point".into());
        }
        let d = grab.offset();
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
        match &self.drag {
            Some(SelectDrag::Box { .. }) => {
                "Drag right for window selection, left for crossing selection".into()
            }
            Some(SelectDrag::Move { point: Some(_), .. }) => {
                "Drop on another point to join them · Esc to cancel".into()
            }
            Some(SelectDrag::Move { .. }) => "Moving selection · Esc to cancel".into(),
            Some(SelectDrag::Radius { .. }) => {
                "Resizing the circle (drag its centre to move it) · Esc to cancel".into()
            }
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
                    if !grab.picking_base {
                        grab.delta = crate::modify::move_offset(grab.start, grab.base, &p);
                        let offset = grab.offset();
                        grab.apply(offset, ed);
                    }
                }
                // Blender confirms on press; a click that turned into a drag still counts.
                Input::Click(p, _) | Input::DragStart(p, _) => {
                    if grab.picking_base {
                        grab.base = p.pos();
                        grab.start = p.raw;
                        grab.delta = DVec2::ZERO;
                        grab.picking_base = false;
                    } else {
                        grab.delta = crate::modify::move_offset(grab.start, grab.base, &p);
                        self.confirm_grab(ed);
                    }
                }
                Input::Finish if !grab.picking_base => self.confirm_grab(ed),
                _ => {}
            }
            return;
        }
        match input {
            Input::Hover(p) => {
                ed.hovered_midpoint = None;
                ed.hovered = match &self.drag {
                    Some(SelectDrag::Box { .. }) => None,
                    Some(SelectDrag::Move { point, .. }) => *point,
                    Some(SelectDrag::Radius { circle, .. }) => Some(*circle),
                    None => match pick_target(ed, &p) {
                        Some(Target::Entity(id)) => Some(id),
                        Some(Target::Midpoint(line)) => {
                            ed.hovered_midpoint = Some(line);
                            None
                        }
                        None => None,
                    },
                };
                // Inside a fill (and not on a curve): the fill.
                ed.hovered_fill =
                    if ed.hovered.is_none() && ed.hovered_midpoint.is_none() && self.drag.is_none()
                    {
                        ed.fill_at(p.raw)
                    } else {
                        None
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
                let hit = ed.index.pick(p.raw, p.tolerance, |id| !ed.is_locked(id));
                // Clicking inside a fill (not on a curve) picks the fill; inside another
                // closed area, the region.
                let fill = if hit.is_none() {
                    ed.fill_at(p.raw)
                } else {
                    None
                };
                match fill {
                    Some(f) if mods.command => {
                        if !ed.selected_fills.remove(&f) {
                            ed.selected_fills.insert(f);
                        }
                    }
                    Some(f) if mods.shift => {
                        ed.selected_fills.insert(f);
                    }
                    Some(f) => {
                        ed.selected_fills = [f].into();
                        ed.selection.clear();
                        ed.selected_region = None;
                        return;
                    }
                    None if !mods.shift && !mods.command => ed.selected_fills.clear(),
                    None => {}
                }
                if fill.is_some() {
                    return;
                }
                ed.selected_region = match hit {
                    None => {
                        let regions = ed.regions();
                        drift_geom::regions::region_at(&regions, p.raw).map(|_| p.raw)
                    }
                    Some(_) => None,
                };
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
            Input::DragStart(p, mods) => {
                match ed.index.pick(p.raw, p.tolerance, |id| !ed.is_locked(id)) {
                    Some(id) => {
                        if !ed.selection.contains(&id) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                            }
                            ed.selection.insert(id);
                        }
                        // A lone circle grabbed by its edge resizes.
                        if ed.selection.len() == 1
                            && let Some(EntityKind::Circle { center, .. }) =
                                ed.doc.get(id).map(|e| e.kind)
                            && let Ok(center) = ed.doc.point(center)
                        {
                            ed.doc.begin();
                            self.drag = Some(SelectDrag::Radius { circle: id, center });
                            return;
                        }
                        let is_point = ed.doc.get(id).is_some_and(|e| e.kind.is_point());
                        let point = (is_point && ed.selection.len() == 1).then_some(id);
                        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                        let base = ed.move_base(&ids, p.raw, point.is_none());
                        let points = ed.doc.defining_points(ids.iter().copied());
                        ed.doc.begin();
                        self.drag = Some(SelectDrag::Move {
                            ids,
                            points,
                            start: p.raw,
                            base,
                            applied: DVec2::ZERO,
                            point,
                        });
                    }
                    None => {
                        self.drag = Some(SelectDrag::Box {
                            start: p.raw,
                            current: p.raw,
                        });
                    }
                }
            }
            Input::Drag(p) => match &mut self.drag {
                Some(SelectDrag::Box { current, .. }) => *current = p.raw,
                Some(SelectDrag::Radius { circle, center }) => {
                    let r = p.pos().distance(*center);
                    let current = drift_solver::radius_of(&ed.doc, *circle);
                    if r > 1e-9 && current.is_some_and(|c| c != r) {
                        ed.drag_radius(*circle, r);
                    }
                }
                Some(SelectDrag::Move {
                    ids,
                    points,
                    start,
                    base,
                    applied,
                    ..
                }) => {
                    let offset = crate::modify::move_offset(*start, *base, &p);
                    let step = offset - *applied;
                    if step != DVec2::ZERO {
                        // The grabbed geometry follows the pointer; constraints move the rest.
                        let dragged: Vec<Param> =
                            points.iter().copied().flat_map(Param::point).collect();
                        if ed.drag(ids, step, &dragged) {
                            *applied = offset;
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
                    let found: Vec<EntityId> = ed
                        .index
                        .select_in_rect(&rect, crossing)
                        .into_iter()
                        .filter(|id| !ed.is_locked(*id))
                        .collect();
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
                Some(SelectDrag::Radius { .. }) => {
                    ed.commit("Resize circle");
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
            Some(SelectDrag::Move { .. } | SelectDrag::Radius { .. }) => {
                ed.doc.rollback();
                ed.sync();
                true
            }
            Some(SelectDrag::Box { .. }) => true,
            None if !ed.selection.is_empty()
                || !ed.selected_midpoints.is_empty()
                || ed.selected_region.is_some()
                || !ed.selected_fills.is_empty() =>
            {
                ed.selection.clear();
                ed.selected_midpoints.clear();
                ed.selected_region = None;
                ed.selected_fills.clear();
                true
            }
            None => false,
        }
    }

    fn overlay(&self) -> Option<Overlay> {
        if let Some(grab) = &self.grab {
            return Some(Overlay::Move {
                base: grab.base,
                to: (!grab.picking_base).then(|| grab.base + grab.offset()),
            });
        }
        match &self.drag {
            Some(SelectDrag::Box { start, current }) => {
                let (start, current) = (*start, *current);
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
        matches!(self.drag, Some(SelectDrag::Move { .. })) || self.grab.is_some()
    }

    fn snap_excludes(&self, ed: &Editor, id: EntityId) -> bool {
        // Nothing that moves with the selection (picking a grab's base point: anything,
        // typically a point of the selection).
        self.moving().is_some_and(|(points, ids)| {
            points.contains(&id)
                || ids.contains(&id)
                || ed
                    .doc
                    .get(id)
                    .is_some_and(|e| e.kind.points().any(|q| points.contains(&q)))
        })
    }
}
