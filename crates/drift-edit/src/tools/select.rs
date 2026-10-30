//! Select: pick, box-select, drag, and the Blender-style modal move (G).

use super::picking::{Pick, Purpose, Target, target_at};
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SelectDrag {
    Box {
        start: DVec2,
        current: DVec2,
    },
    /// Dragging a sketch text by its body.
    Text {
        id: drift_core::TextId,
        /// Pointer and the text's start where the drag began.
        start: DVec2,
        from: DVec2,
    },
    /// Dragging a fillet's arc: it is re-sized in place, its lines keeping their directions.
    Fillet {
        arc: EntityId,
    },
    /// Alt-dragging a side of an offset: the offset's distance (`var`) follows the pointer's
    /// distance from the curve that side is measured from, and every side moves.
    Offset {
        family: Vec<ConstraintId>,
        from: EntityId,
    },
    /// Dragging a circle's edge: the radius follows the pointer (drag its centre to move it).
    Radius {
        circle: EntityId,
        center: DVec2,
    },
    /// Dragging an opening along its wall: where the press was along its segment, and where
    /// its first jamb was then.
    Opening {
        id: drift_core::OpeningId,
        press: f64,
        from: f64,
    },
    /// Dragging a transform handle: the geometry scales (a corner) or stretches (a side).
    Scale {
        frame: crate::transform::TransformBox,
        points: BTreeSet<EntityId>,
        /// The handle's side along each axis.
        handle: (i8, i8),
        /// Pointer where the drag began.
        press: DVec2,
        /// The scale applied now: its anchor, factors, and whether dimensions scaled too.
        applied: Option<(DVec2, DVec2, bool)>,
    },
}

/// A move of the selection, by G or by dragging it with the mouse: one path for both, so X / Y
/// lock an axis, typed numbers give the exact offset and the base point lands on snaps the same
/// way. With G (a Blender-style modal move) the selection follows the pointer from a base point
/// until a click or Enter confirms it, or Esc / right-click cancels, and B picks a new base
/// point. A mouse drag ends when the button is let go.
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
    pub(super) lock: Option<Axis>,
    pub(super) typed: [Option<f64>; 2],
    /// The offset the geometry has been moved by so far in the open transaction.
    pub(super) applied: DVec2,
    /// Copies of blocks moving too (as they were when the move began).
    pub(super) instances: Vec<(drift_core::InstanceId, drift_core::Instance)>,
    /// The picked sketch text and annotations moving too, as they were.
    pub(super) riders: crate::picks::Riders,
    /// A picked area or hatch moving: it lets go of curves that aren't moving with it
    /// (with automatic relations off; see [`Editor::detach_for_move`]).
    pub(super) detach: bool,
    /// Moving by dragging with the mouse: it ends when the button is let go. None for G.
    pub(super) by_mouse: Option<MouseMove>,
}

/// A move by dragging with the mouse.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct MouseMove {
    /// Set when a single point is being dragged; it can merge on drop.
    pub(super) point: Option<EntityId>,
}

impl Grab {
    /// A move of `ids` (their defining points `points`) from `base`, the pointer at `start`:
    /// a G move, with nothing else moving along.
    pub(super) fn new(
        ids: Vec<EntityId>,
        points: BTreeSet<EntityId>,
        base: DVec2,
        start: DVec2,
    ) -> Self {
        Self {
            ids,
            points,
            base,
            start,
            delta: DVec2::ZERO,
            picking_base: false,
            lock: None,
            typed: [None; 2],
            applied: DVec2::ZERO,
            instances: Vec::new(),
            riders: Default::default(),
            detach: false,
            by_mouse: None,
        }
    }

    /// A drag of the selection with the mouse, as [`new`](Self::new).
    fn dragged_by_mouse(
        ids: Vec<EntityId>,
        base: DVec2,
        start: DVec2,
        point: Option<EntityId>,
        ed: &Editor,
    ) -> Self {
        let points = ed.doc.defining_points(ids.iter().copied());
        Self {
            instances: ed.picked_instances(),
            riders: ed.riders(),
            by_mouse: Some(MouseMove { point }),
            ..Self::new(ids, points, base, start)
        }
    }

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
        if step == DVec2::ZERO {
            return;
        }
        if self.ids.is_empty() || ed.drag(&self.ids, step, &self.dragged()) {
            ed.move_instances(&self.instances, offset);
            ed.place_riders(&self.riders, &|p| p + offset);
            self.applied = offset;
            ed.region_motion = Some(crate::editor::RegionMotion::Move(offset));
        }
    }
}

#[derive(Default)]
pub struct SelectTool {
    pub(super) drag: Option<SelectDrag>,
    pub(super) grab: Option<Grab>,
    pub(super) spin: Option<super::rotate::Spin>,
}

impl SelectTool {
    /// Start a modal move of the selection from the pointer.
    pub(super) fn start_grab(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        if self.drag.is_some() || self.grab.is_some() {
            return false;
        }
        // A selected region moves its boundary, based on its centroid.
        // A picked region (its outline is selected with it) moves from its centroid.
        let region = ed.region().filter(|r| {
            let outline = ed.region_curves(r);
            ed.selection.iter().all(|id| outline.contains(id))
        });
        // Lines picked by their midpoints move whole, as a drag of the midpoint moves them.
        let midpoints: Vec<EntityId> = ed
            .selected_midpoints
            .iter()
            .copied()
            .filter(|l| ed.doc.get(*l).is_some())
            .collect();
        let picked_geometry = !ed.selection.is_empty() || !midpoints.is_empty();
        // A picked fill or hatch moves the curves around it (it follows them).
        let fill = if !picked_geometry && region.is_none() {
            ed.selected_fill_curves()
        } else {
            None
        };
        let instances = ed.picked_instances();
        let riders = ed.riders();
        let (ids, base) = match (&region, fill) {
            (Some(r), _) => (ed.region_curves(r), r.centroid),
            (None, Some((ids, centre))) => (ids, centre),
            // Only copies of blocks: their insertion point is the base.
            (None, None) if !picked_geometry && !instances.is_empty() => {
                (Vec::new(), instances[0].1.at)
            }
            // Only text and annotations: the first text's anchor (or note's label) is the base.
            (None, None) if !picked_geometry && !riders.is_empty() => {
                (Vec::new(), riders.base().unwrap_or(p.raw))
            }
            // Only midpoints: the one nearest the pointer is the base.
            (None, None) if ed.selection.is_empty() && !midpoints.is_empty() => {
                let middle = |l: &EntityId| match ed.doc.shape(*l) {
                    Some(Shape::Line { a, b }) => Some((a + b) / 2.0),
                    _ => None,
                };
                let base = midpoints
                    .iter()
                    .filter_map(middle)
                    .min_by(|a, b| a.distance(p.raw).total_cmp(&b.distance(p.raw)))
                    .unwrap_or(p.raw);
                (midpoints, base)
            }
            (None, None) => {
                let mut ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                ids.extend(midpoints.into_iter().filter(|l| !ed.selection.contains(l)));
                let base = ed.move_base(&ids, p.raw, false);
                (ids, base)
            }
        };
        if ids.is_empty() && instances.is_empty() && riders.is_empty() {
            ed.out.notice = Some("Select something to move first".into());
            return true;
        }
        if ed.refuse_locked(&ids) {
            return true;
        }
        let area = region.is_some() || (!picked_geometry && !ed.selected_fills.is_empty());
        ed.doc.begin();
        if area {
            ed.detach_for_move(&ids);
        }
        let points = ed.doc.defining_points(ids.iter().copied());
        self.grab = Some(Grab {
            instances,
            riders,
            detach: area,
            ..Grab::new(ids, points, base, p.raw)
        });
        true
    }

    /// A drag that starts on a transform handle scales, stretches or turns what the box is
    /// around. True when it did.
    fn start_handle_drag(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        use crate::transform::Handle;
        let Some(frame) = ed.transform_box() else {
            return false;
        };
        match frame.handle_at(
            p.raw,
            crate::transform::handle_reach(p.tolerance),
            p.tolerance,
        ) {
            Some(Handle::Scale { x, y }) => {
                let points = ed.doc.defining_points(frame.ids.iter().copied());
                ed.doc.begin();
                self.drag = Some(SelectDrag::Scale {
                    frame,
                    points,
                    handle: (x, y),
                    press: p.raw,
                    applied: None,
                });
                true
            }
            Some(Handle::Turn { .. }) => self.start_turn(p, &frame, ed),
            None => false,
        }
    }

    /// A drag that starts inside a picked area moves it (its outline), from its centroid.
    /// True when it did.
    fn start_region_drag(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        let Some(region) = ed.region().filter(|r| r.contains(p.raw)) else {
            return false;
        };
        let ids = ed.region_curves(&region);
        if !ed.selection.iter().all(|id| ids.contains(id)) || ids.iter().any(|id| ed.is_locked(*id))
        {
            return false;
        }
        ed.doc.begin();
        ed.detach_for_move(&ids);
        let points = ed.doc.defining_points(ids.iter().copied());
        self.grab = Some(Grab {
            detach: true,
            by_mouse: Some(MouseMove::default()),
            ..Grab::new(ids, points, region.centroid, p.raw)
        });
        true
    }

    /// Confirm the move for the pointer at `target`. The final offset is applied again in one
    /// step from the original geometry, so typed and snapped moves land exactly.
    pub(super) fn confirm_grab(&mut self, ed: &mut Editor) {
        let Some(grab) = self.grab.take() else { return };
        let offset = grab.offset();
        ed.region_motion = None;
        // A picked region moves with its outline.
        if let Some(seed) = &mut ed.selected_region {
            *seed += offset;
        }
        ed.doc.rollback();
        if offset == DVec2::ZERO {
            ed.sync();
            return;
        }
        ed.doc.begin();
        ed.move_instances(&grab.instances, offset);
        // Applied again from the original geometry: let go of the neighbours again too
        // (the outline's points are new ones then).
        let dragged = if grab.detach {
            ed.detach_for_move(&grab.ids);
            ed.doc
                .defining_points(grab.ids.iter().copied())
                .iter()
                .copied()
                .flat_map(Param::point)
                .collect()
        } else {
            grab.dragged()
        };
        if !grab.ids.is_empty() && !ed.drag(&grab.ids, offset, &dragged) {
            // Too far for one solve (constraints pull the rest along): go in steps.
            const STEPS: u32 = 16;
            for _ in 0..STEPS {
                ed.drag(&grab.ids, offset / f64::from(STEPS), &dragged);
            }
        }
        // After the geometry, so notes land where they're taken whatever their leaders did.
        ed.place_riders(&grab.riders, &|p| p + offset);
        ed.commit("Move");
    }

    /// The mouse button let go of a drag of the selection: it stays where the drag took it.
    /// A lone point dropped on another point joins it; on a midpoint or a curve, it keeps to
    /// it.
    fn finish_mouse_move(&mut self, p: &Pointer, ed: &mut Editor) {
        let Some(grab) = self.grab.take() else { return };
        let point = grab.by_mouse.and_then(|m| m.point);
        ed.region_motion = None;
        if point.is_none() && grab.applied == DVec2::ZERO {
            // Not moved after all: nothing to keep (not even a join let go).
            ed.doc.rollback();
            ed.sync();
            return;
        }
        if let Some(seed) = &mut ed.selected_region {
            *seed += grab.applied;
        }
        if let (Some(from), Some(into)) = (point, p.snapped_point())
            && from != into
            && ed.doc.merge_points(from, into).is_ok()
        {
            ed.selection = [into].into();
        }
        // Dropped on a midpoint: keep it there.
        if let (Some(from), Some(line)) = (point, p.midpoint_of())
            && ed
                .doc
                .constraints_on(from)
                .all(|c| ed.doc.constraint(c) != Some(&Constraint::Midpoint { point: from, line }))
        {
            let _ = ed
                .doc
                .add_constraint(Constraint::Midpoint { point: from, line });
        }
        // Dropped on a curve: keep it on the curve (refused for its own curves).
        if let (Some(from), Some(curve)) = (point, p.on_curve())
            && ed.doc.constraints_on(from).all(|c| {
                ed.doc.constraint(c) != Some(&Constraint::PointOnCurve { point: from, curve })
            })
        {
            let _ = ed
                .doc
                .add_constraint(Constraint::PointOnCurve { point: from, curve });
        }
        ed.commit("Move");
    }

    pub(super) fn cancel_grab(&mut self, ed: &mut Editor) -> bool {
        if self.grab.take().is_none() {
            return false;
        }
        ed.region_motion = None;
        ed.doc.rollback();
        ed.sync();
        true
    }

    /// The points moving with a drag or grab (never snap targets): the defining points of
    /// everything moving.
    fn moving(&self) -> Option<&BTreeSet<EntityId>> {
        if let Some(grab) = &self.grab {
            return (!grab.picking_base).then_some(&grab.points);
        }
        if let Some(spin) = &self.spin {
            return (!spin.picking()).then_some(&spin.points);
        }
        None
    }
}

impl Tool for SelectTool {
    fn is_busy(&self) -> bool {
        self.drag.is_some() || self.grab.is_some() || self.spin.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Select
    }

    fn is_modal(&self) -> bool {
        // (A mouse drag ends with the button: it isn't modal, though X / Y lock it.)
        self.grab.as_ref().is_some_and(|g| g.by_mouse.is_none()) || self.spin.is_some()
    }

    fn key(&mut self, key: ToolKey, pointer: Option<&Pointer>, ed: &mut Editor) -> bool {
        if self.spin.is_some() {
            return self.spin_key(key, ed);
        }
        if key == ToolKey::R && self.grab.is_none() {
            return match pointer {
                Some(p) => self.start_spin(p, ed),
                None => false,
            };
        }
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
            // (A mouse drag keeps the base it was grabbed by.)
            (ToolKey::B, Some(grab)) if grab.by_mouse.is_none() => {
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
        if let Some(spin) = &self.spin {
            return if spin.picking() { &[] } else { &["Angle"] };
        }
        match &self.grab {
            Some(grab) if !grab.picking_base => {
                if grab.lock.is_some() {
                    &["Distance"]
                } else if grab.by_mouse.is_some() {
                    // A mouse drag takes a typed distance once it's locked to an axis.
                    &[]
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
        if let Some(spin) = &mut self.spin {
            spin.typed = values[0];
        }
    }

    fn commit_typed(&mut self, _p: &Pointer, ed: &mut Editor) {
        // The pointer may be off the canvas while typing: the last offset stands.
        if self.grab.is_some() {
            self.confirm_grab(ed);
        }
        if self.spin.is_some() {
            self.confirm_spin(ed);
        }
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        if let Some(SelectDrag::Scale {
            applied,
            frame,
            handle,
            ..
        }) = &self.drag
        {
            let (_, s, dims) = (*applied)?;
            if dims {
                // The sizes the dimensions come to, along the axes that change.
                let size = (frame.bounds.max - frame.bounds.min) * s;
                let w = format!("W {}", p.units.format_readout(size.x));
                let h = format!("H {}", p.units.format_readout(size.y));
                let both = (s.x - s.y).abs() < 1e-9 || (handle.0 != 0 && handle.1 != 0);
                let text = match (handle.0 != 0, handle.1 != 0) {
                    _ if both => format!("{w}  {h}"),
                    (true, _) => w,
                    _ => h,
                };
                return Some(format!("{text} (dimensions too)"));
            }
            let percent = |f: f64| format!("{:.0}%", f * 100.0);
            let size = if (s.x - s.y).abs() < 1e-9 {
                percent(s.x)
            } else {
                format!("{} × {}", percent(s.x), percent(s.y))
            };
            return Some(format!("Scale {size}"));
        }
        if let Some(spin) = &self.spin {
            return Some(if spin.picking_pivot {
                "Pivot".into()
            } else if spin.picking_reference {
                "Reference".into()
            } else {
                format!("Angle {}", p.units.format_angle(spin.current()))
            });
        }
        let grab = self.grab.as_ref()?;
        if grab.picking_base {
            return Some("Base point".into());
        }
        if grab.by_mouse.is_some() && grab.lock.is_none() {
            return None;
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
        if let Some(spin) = &self.spin {
            return if spin.picking_pivot {
                "Click the pivot (snaps to points) · Esc: cancel".into()
            } else if spin.picking_reference {
                "Click the reference: a point to turn from, e.g. along an edge (snaps) · \
                 Esc: cancel"
                    .into()
            } else {
                "Rotate: click / Enter to place · type an angle · 15° steps stick · B: pivot · \
                 R: by reference · Esc: cancel"
                    .into()
            };
        }
        if let Some(grab) = &self.grab {
            let lock = match grab.lock {
                Some(axis) => format!("{axis:?} locked: type a distance"),
                None => "X / Y: lock".into(),
            };
            return if let Some(m) = grab.by_mouse {
                match m.point {
                    Some(_) => {
                        format!("Drop on another point to join them · {lock} · Esc to cancel")
                    }
                    None => format!("Moving selection · {lock} · Esc to cancel"),
                }
            } else if grab.picking_base {
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
            Some(SelectDrag::Text { .. }) => "Moving text · Esc to cancel".into(),
            Some(SelectDrag::Opening { .. }) => {
                "Sliding the opening along its wall (it keeps clear of corners; snaps to \
                 centred) · Esc to cancel"
                    .into()
            }
            Some(SelectDrag::Radius { .. }) => {
                "Resizing the circle (drag its centre to move it) · Esc to cancel".into()
            }
            Some(SelectDrag::Offset { .. }) => {
                "Changing the offset in steps: every side follows · Alt: freely · Esc to \
                 cancel"
                    .into()
            }
            Some(SelectDrag::Fillet { .. }) => {
                "Resizing the fillet in steps (Alt: freely) · Esc to cancel".into()
            }
            Some(SelectDrag::Scale { .. }) => "Scaling: dimensions hold · Shift: keep the \
                 shape · Ctrl: about the middle · Alt: scale the dimensions too · Esc to cancel"
                .into(),
            None => "Click to select · Shift adds · Ctrl toggles · drag to move or box-select · Tab: chain"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if self.spin_input(input, ed) {
            return;
        }
        if let Some(grab) = &mut self.grab {
            ed.hovered = grab.by_mouse.and_then(|m| m.point);
            ed.hovered_midpoint = None;
            if grab.by_mouse.is_some() {
                // A drag with the mouse: it follows the pointer until the button is let go.
                match input {
                    Input::Hover(p) | Input::Drag(p) => {
                        grab.delta = crate::modify::move_offset(grab.start, grab.base, &p);
                        let offset = grab.offset();
                        grab.apply(offset, ed);
                    }
                    Input::DragEnd(p, _) => self.finish_mouse_move(&p, ed),
                    _ => {}
                }
                return;
            }
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
                let target = match self.drag {
                    None => target_at(ed, &p, Purpose::Hover),
                    Some(_) => None,
                };
                ed.hovered = match &self.drag {
                    Some(SelectDrag::Radius { circle, .. }) => Some(*circle),
                    Some(SelectDrag::Fillet { arc }) => Some(*arc),
                    _ => target.and_then(Pick::entity),
                };
                ed.hovered_midpoint = target.and_then(Pick::midpoint);
                ed.hovered_text = target.and_then(Pick::text);
                ed.hovered_instance = target.and_then(Pick::copy);
                // Inside a fill (and not on a curve or a copy of a block): the fill.
                ed.hovered_fill = target.and_then(Pick::fill);
                // A chain highlighted by Tab lasts while the pointer stays on it.
                if let Some((_, set)) = &ed.chain_preview
                    && !ed.hovered.is_some_and(|h| set.contains(&h))
                {
                    ed.chain_preview = None;
                }
            }
            Input::Click(p, mods) => {
                // A chain highlighted by Tab: the click picks it (Shift adds it, Ctrl takes
                // it away).
                if let Some((_, set)) = ed.chain_preview.take()
                    && ed.hovered.is_some_and(|h| set.contains(&h))
                {
                    if mods.command {
                        ed.selection.retain(|id| !set.contains(id));
                    } else {
                        if !mods.shift {
                            ed.selection.clear();
                            ed.selected_fills.clear();
                            ed.selected_instances.clear();
                            ed.selected_region = None;
                        }
                        ed.selection.extend(set);
                        ed.expand_groups();
                    }
                    return;
                }
                // A picked opening's flip arrow: flip it.
                if let Some((o, flip)) = ed.opening_arrow_at(p.raw, p.tolerance) {
                    ed.flip_openings(&[o], flip);
                    return;
                }
                let target = target_at(ed, &p, Purpose::Click);
                if let Some(line) = target.and_then(Pick::midpoint) {
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
                    // (A click on a dimension or annotation is the app's: this one is off
                    // them.)
                    ed.selected_annotation = None;
                    ed.annotation_picks.clear();
                }
                let hit = target.and_then(Pick::entity);
                let filter = ed.prefs.pick_filter;
                // Sketch text (anywhere in its box): Shift adds it, Ctrl adds or takes it away.
                if let Some(t) = target.and_then(Pick::text) {
                    if mods.command {
                        if !ed.selected_texts.remove(&t) {
                            ed.selected_texts.insert(t);
                        }
                    } else if mods.shift {
                        ed.selected_texts.insert(t);
                    } else {
                        ed.selected_texts = [t].into();
                        ed.selection.clear();
                        ed.selected_instances.clear();
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                    }
                    return;
                }
                if !mods.shift && !mods.command {
                    ed.selected_texts.clear();
                }
                // A copy of a block (on its curves, or inside it).
                if let Some(c) = target.and_then(Pick::copy) {
                    if ed.open_group.is_some() {
                        ed.close_open_group();
                    }
                    if mods.command {
                        if !ed.selected_instances.remove(&c) {
                            ed.selected_instances.insert(c);
                        }
                    } else if mods.shift {
                        ed.selected_instances.insert(c);
                    } else {
                        ed.selected_instances = [c].into();
                        ed.selection.clear();
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                    }
                    return;
                }
                if !mods.shift && !mods.command {
                    ed.selected_instances.clear();
                }
                // Clicking inside a fill (not on a curve) picks the fill: a shape's fill picks
                // the shape, its outline with it (so Duplicate, Copy, Move and Delete act on
                // all of it). Inside another closed area, the region.
                let fill = target.and_then(Pick::fill);
                // Inside a wall's poché: the wall (the nearest, where walls merge).
                if let Some(Pick::Wall(w, _)) = target
                    && let Some(&first) = ed.wall_curves(w).first()
                {
                    let set = ed.wall_pick_set(w);
                    if mods.command && ed.selection.contains(&first) {
                        ed.selection.retain(|e| !set.contains(e));
                    } else if mods.shift || mods.command {
                        ed.selection.extend(set);
                    } else {
                        ed.selection = set;
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                    }
                    ed.sync_group_fills();
                    return;
                }
                // Alt-click inside a shape (not in a closed group): its fill alone, to align
                // its pattern without the transform handles.
                if mods.alt
                    && !mods.shift
                    && !mods.command
                    && let Some(f) = fill
                    && ed.fill_group(f).is_none()
                    && ed.shape_outline(f).is_some()
                {
                    ed.pick_fill_alone(f);
                    return;
                }
                // A shape in a group (not the open one) picks the whole group, as a click on
                // its outline does.
                if let Some(f) = fill
                    && ed.fill_group(f).is_some()
                    && let Some(&first) = ed.shape_outline(f).as_ref().and_then(|o| o.first())
                {
                    if let Some(g) = ed.open_group
                        && ed.doc.group_of(first) != Some(g)
                    {
                        ed.close_open_group();
                    }
                    let set = ed.pick_set(first);
                    if mods.command && ed.selection.contains(&first) {
                        ed.selection.retain(|e| !set.contains(e));
                    } else if mods.shift || mods.command {
                        ed.selection.extend(set);
                    } else {
                        ed.selection = set;
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                    }
                    ed.sync_group_fills();
                    return;
                }
                // (With edges filtered out, the fill alone.)
                let outline = fill
                    .filter(|_| filter.has(crate::editor::PickKind::Edges))
                    .and_then(|f| ed.shape_outline(f))
                    .unwrap_or_default();
                match fill {
                    Some(f) if mods.command => {
                        if ed.selected_fills.remove(&f) {
                            for c in &outline {
                                ed.selection.remove(c);
                            }
                        } else {
                            ed.selected_fills.insert(f);
                            ed.selection.extend(outline);
                        }
                    }
                    Some(f) if mods.shift => {
                        ed.selected_fills.insert(f);
                        ed.selection.extend(outline);
                    }
                    Some(f) => {
                        ed.selected_fills = [f].into();
                        ed.selection = outline.into_iter().collect();
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
                    // (A filled area was picked as its fill above: with areas picked only
                    // where filled, an empty outline's inside picks nothing.)
                    // (Not one outlined by hidden curves.)
                    None if filter.areas() && !ed.regions_need_fill() => {
                        ed.visible_region_at(p.raw).map(|_| p.raw)
                    }
                    _ => None,
                };
                // Picking outside the open group closes it.
                if let (Some(g), Some(id)) = (ed.open_group, hit)
                    && ed.doc.group_of(id) != Some(g)
                {
                    ed.close_open_group();
                }
                match hit {
                    Some(id) if mods.command => {
                        let set = ed.pick_set(id);
                        if ed.selection.contains(&id) {
                            ed.selection.retain(|e| !set.contains(e));
                        } else {
                            ed.selection.extend(set);
                        }
                    }
                    Some(id) if mods.shift => {
                        let set = ed.pick_set(id);
                        ed.selection.extend(set);
                    }
                    Some(id) => {
                        ed.selection = ed.pick_set(id);
                    }
                    // Inside a closed area (with no fill): its outline curves.
                    None => {
                        let curves: Vec<EntityId> = match ed.region() {
                            Some(r) => ed
                                .region_curves(&r)
                                .into_iter()
                                .filter(|id| !ed.is_locked(*id))
                                .collect(),
                            None => Vec::new(),
                        };
                        if !mods.shift && !mods.command {
                            ed.selection.clear();
                            if ed.open_group.is_some() {
                                ed.close_open_group();
                            }
                        }
                        ed.selection.extend(curves);
                        ed.expand_groups();
                    }
                }
                ed.sync_group_fills();
            }
            // Double-click a group's member to edit inside the group.
            Input::DoubleClick(p) => match target_at(ed, &p, Purpose::DoubleClick) {
                // Sketch text: type into it where it stands.
                Some(Pick::Text(t)) => {
                    ed.selected_texts = [t].into();
                    ed.editing_text = Some(t);
                    ed.text_caret_at = Some(p.raw);
                }
                Some(Pick::Copy(c)) => ed.edit_block(c),
                Some(Pick::Geometry(Target::Entity(id))) => {
                    if let Some(g) = ed.doc.group_of(id)
                        && ed.open_group != Some(g)
                    {
                        ed.open_group = Some(g);
                        ed.selection = [id].into();
                    }
                }
                // Inside a grouped shape's fill: into the group, with that shape picked. Inside
                // a shape of its own (or of the open group): its fill alone.
                Some(Pick::Wall(_, f) | Pick::Fill(f)) => {
                    if let Some(outline) = ed.shape_outline(f) {
                        match ed.fill_group(f) {
                            Some(g) => {
                                ed.open_group = Some(g);
                                ed.selected_fills = [f].into();
                                ed.selection = outline.into_iter().collect();
                                ed.selected_region = None;
                            }
                            None => ed.pick_fill_alone(f),
                        }
                    }
                }
                // (No midpoints for a double-click.)
                Some(Pick::Geometry(Target::Midpoint(_))) | None => {}
            },
            Input::Finish => {}
            Input::DragStart(p, mods) => {
                if self.start_handle_drag(&p, ed) {
                    return;
                }
                let target = target_at(ed, &p, Purpose::DragStart);
                match target {
                    // A side of an offset, on its own (with more picked, a drag moves the
                    // lot): change the offset.
                    Some(Pick::Geometry(Target::Entity(id)))
                        if (ed.selection.is_empty() || ed.selection.iter().eq([&id]))
                            && let Some((family, from)) = ed.offset_side_of(id) =>
                    {
                        ed.doc.begin();
                        self.drag = Some(SelectDrag::Offset { family, from });
                    }
                    Some(Pick::Geometry(Target::Entity(id))) => {
                        if !ed.selection.contains(&id) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                                ed.selected_instances.clear();
                                ed.selected_texts.clear();
                                ed.selected_annotation = None;
                                ed.annotation_picks.clear();
                            }
                            let set = ed.pick_set(id);
                            ed.selection.extend(set);
                        }
                        // An opening slides along its wall.
                        if let Some(o) = ed.opening_of(id)
                            && ed.selected_openings().len() == 1
                            && ed.selection == ed.opening_pick_set(o)
                            && let Some(op) = ed.doc.opening(o)
                            && let Some(host) = ed.opening_host(op.wall, op.seg)
                            && let Some((u0, _)) = ed.opening_span(o)
                        {
                            ed.doc.begin();
                            self.drag = Some(SelectDrag::Opening {
                                id: o,
                                press: host.seg.param(p.raw),
                                from: u0,
                            });
                            return;
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
                        // A lone fillet grabbed by its arc is re-sized where it is.
                        if ed.selection.len() == 1 && ed.fillet_at(id).is_some() {
                            ed.doc.begin();
                            self.drag = Some(SelectDrag::Fillet { arc: id });
                            return;
                        }
                        let is_point = ed.doc.get(id).is_some_and(|e| e.kind.is_point());
                        let point = (is_point && ed.selection.len() == 1).then_some(id);
                        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                        let base = ed.move_base(&ids, p.raw, point.is_none());
                        let grab = Grab::dragged_by_mouse(ids, base, p.raw, point, ed);
                        ed.doc.begin();
                        self.grab = Some(grab);
                    }
                    // Sketch text: drag it by its body (and the rest of what is picked with
                    // it).
                    Some(Pick::Text(id)) => {
                        if !ed.selected_texts.contains(&id) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                                ed.selected_instances.clear();
                                ed.selected_fills.clear();
                                ed.selected_texts.clear();
                                ed.selected_annotation = None;
                                ed.annotation_picks.clear();
                            }
                            ed.selected_texts.insert(id);
                        }
                        let from = ed.doc.text(id).map_or(p.raw, |t| t.at);
                        let alone = ed.selection.is_empty()
                            && ed.selected_instances.is_empty()
                            && ed.selected_texts.len() == 1
                            && ed.picked_notes().is_empty();
                        if alone {
                            ed.doc.begin();
                            self.drag = Some(SelectDrag::Text {
                                id,
                                start: p.raw,
                                from,
                            });
                        } else {
                            let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                            let grab = Grab::dragged_by_mouse(ids, from, p.raw, None, ed);
                            ed.doc.begin();
                            self.grab = Some(grab);
                        }
                    }
                    // A copy of a block: drag it (and the rest of the selection) by its
                    // insertion point.
                    Some(Pick::Copy(c)) => {
                        if !ed.selected_instances.contains(&c) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                                ed.selected_instances.clear();
                                ed.selected_fills.clear();
                            }
                            ed.selected_instances.insert(c);
                        }
                        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                        let base = ed.doc.instance(c).map_or(p.raw, |i| i.at);
                        let grab = Grab::dragged_by_mouse(ids, base, p.raw, None, ed);
                        ed.doc.begin();
                        self.grab = Some(grab);
                    }
                    _ if ed.prefs.pick_filter.areas() && self.start_region_drag(&p, ed) => {}
                    // (No midpoints for a drag start: one from a line's middle moves the line.)
                    _ => {
                        // Dragging inside a picked fill moves its area; elsewhere, box-select.
                        let picked_fill = target
                            .and_then(Pick::fill)
                            .is_some_and(|f| ed.selected_fills.contains(&f));
                        match picked_fill.then(|| ed.selected_fill_curves()).flatten() {
                            Some((ids, _)) if ed.refuse_locked(&ids) => {}
                            Some((ids, centre)) => {
                                ed.doc.begin();
                                ed.detach_for_move(&ids);
                                let points = ed.doc.defining_points(ids.iter().copied());
                                self.grab = Some(Grab {
                                    detach: true,
                                    by_mouse: Some(MouseMove::default()),
                                    ..Grab::new(ids, points, centre, p.raw)
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
                }
            }
            Input::Drag(p) => match &mut self.drag {
                Some(SelectDrag::Box { current, .. }) => *current = p.raw,
                Some(SelectDrag::Opening { id, press, from }) => {
                    let (o, press, from) = (*id, *press, *from);
                    if let Some(host) = ed
                        .doc
                        .opening(o)
                        .and_then(|op| ed.opening_host(op.wall, op.seg))
                    {
                        let mut u0 = from + host.seg.param(p.raw) - press;
                        // Centred, when near it.
                        for snap in ed.opening_snaps(o) {
                            if (u0 - snap).abs() <= 2.0 * p.tolerance {
                                u0 = snap;
                            }
                        }
                        ed.slide_opening(o, u0);
                    }
                }
                Some(SelectDrag::Scale {
                    frame,
                    points,
                    handle,
                    press,
                    applied,
                }) => {
                    // Shift keeps the shape, Ctrl scales about the middle, Alt scales the
                    // dimensions too (each as far as the stretch runs along it).
                    let dims = p.alt;
                    let anchor = frame.anchor(handle.0, handle.1, p.command);
                    let keep_shape = p.shift || frame.uniform;
                    let mut scale =
                        crate::transform::scale_for(*handle, anchor, *press, p.raw, keep_shape);
                    if dims && p.round_scale {
                        // Round sizes, in steps that suit the zoom (as fillets are sized).
                        let step = super::corners::round_step(
                            p.tolerance * 1.5,
                            p.units.length.is_metric(),
                        );
                        let size = frame.bounds.max - frame.bounds.min;
                        scale =
                            crate::transform::round_scale(scale, *handle, size, step, keep_shape);
                    }
                    let wanted = Some((anchor, scale, dims));
                    if wanted != *applied {
                        ed.doc.rollback();
                        ed.doc.begin();
                        if ed.scale_geometry(&frame.ids, points, anchor, scale, dims) {
                            *applied = wanted;
                        } else {
                            // Impossible: stay at the last scale that worked.
                            ed.doc.rollback();
                            ed.doc.begin();
                            if let Some((a, s, d)) = *applied {
                                ed.scale_geometry(&frame.ids, points, a, s, d);
                            }
                        }
                        ed.region_motion = applied.map(|(anchor, scale, _)| {
                            crate::editor::RegionMotion::Scale { anchor, scale }
                        });
                        ed.sync();
                    }
                }
                Some(SelectDrag::Text { id, start, from }) => {
                    if let Some(mut t) = ed.doc.text(*id).cloned() {
                        t.at = *from + (p.raw - *start);
                        ed.doc.set_text(*id, t);
                    }
                }
                Some(SelectDrag::Fillet { arc }) => {
                    if let Some(f) = ed.fillet_at(*arc) {
                        let (half, room) = f.half_angle_and_room();
                        let free = Pointer {
                            alt: p.alt,
                            units: p.units,
                            ..Pointer::new(p.raw, p.tolerance)
                        };
                        if let Some(r) =
                            super::corners::fillet_radius_at(f.corner, half, room, &free)
                            && (r - f.radius).abs() > 1e-9
                        {
                            ed.drag_fillet(*arc, r);
                        }
                    }
                }
                Some(SelectDrag::Offset { family, from }) => {
                    if let Some(Shape::Line { a, b }) = ed.doc.shape(*from) {
                        let u = (b - a).normalize_or_zero();
                        // In round steps for the zoom and the drawing's units, as a fillet
                        // drag; with Alt, freely (as precise as lengths show).
                        let d = (p.pos() - a).perp_dot(u).abs();
                        let d = if p.alt {
                            ed.shown_length(d)
                        } else {
                            let metric = ed.doc.units().length.is_metric();
                            let step = super::corners::round_step(p.tolerance * 1.5, metric);
                            ((d / step).round() * step).max(step)
                        };
                        ed.drag_offset(family, d);
                    }
                }
                Some(SelectDrag::Radius { circle, center }) => {
                    let r = p.pos().distance(*center);
                    let current = drift_solver::radius_of(&ed.doc, *circle);
                    if r > 1e-9 && current.is_some_and(|c| c != r) {
                        ed.drag_radius(*circle, r);
                    }
                }
                None => {}
            },
            Input::DragEnd(p, mods) => match self.drag.take() {
                Some(SelectDrag::Box { start, .. }) => {
                    let hidden = ed.hidden_handles();
                    let mut rect = Bounds::from_point(start);
                    rect.include(p.raw);
                    let crossing = p.raw.x < start.x;
                    let filter = ed.prefs.pick_filter;
                    // Points without edges: every point in the box (a box otherwise takes
                    // curves, and only standalone points).
                    let found: Vec<EntityId> = if !filter.geometry() && !filter.blocks() {
                        Vec::new()
                    } else if filter.has(crate::editor::PickKind::Points)
                        && !filter.has(crate::editor::PickKind::Edges)
                    {
                        ed.index
                            .query(rect)
                            .filter(|id| {
                                matches!(ed.index.shape(*id), Some(Shape::Point(q))
                                        if q.x >= rect.min.x && q.x <= rect.max.x
                                            && q.y >= rect.min.y && q.y <= rect.max.y)
                                    && ed.pickable(*id, &hidden)
                            })
                            .collect()
                    } else {
                        ed.index
                            .select_in_rect(&rect, crossing)
                            .into_iter()
                            .filter(|id| ed.pickable(*id, &hidden))
                            .collect()
                    };
                    let add = mods.shift || mods.command;
                    if !add {
                        ed.selection.clear();
                        ed.selected_instances.clear();
                        ed.selected_texts.clear();
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                        ed.selected_annotation = None;
                        ed.annotation_picks.clear();
                    }
                    ed.selection.extend(found);
                    ed.expand_groups();
                    if filter.blocks() {
                        let copies = ed.instances_in(&rect, crossing);
                        ed.selected_instances.extend(copies);
                    }
                    if filter.fills() {
                        let fills = ed.fills_in(&rect, crossing);
                        ed.selected_fills.extend(fills);
                    }
                    if filter.texts() {
                        let texts = ed.texts_in(&rect, crossing);
                        ed.selected_texts.extend(texts);
                    }
                    // Dimensions and annotations are laid out on screen: the app picks them.
                    ed.out.box_pick = Some(crate::picks::BoxPick {
                        rect,
                        crossing,
                        add,
                    });
                }
                Some(SelectDrag::Radius { .. }) => {
                    ed.commit("Resize circle");
                }
                Some(SelectDrag::Offset { .. }) => {
                    ed.commit("Change offset");
                }
                Some(SelectDrag::Scale {
                    frame,
                    points,
                    applied,
                    ..
                }) => {
                    ed.region_motion = None;
                    // Applied again in one step from the original geometry.
                    ed.doc.rollback();
                    if let Some((anchor, scale, dims)) =
                        applied.filter(|(_, s, _)| *s != DVec2::ONE)
                    {
                        ed.doc.begin();
                        if ed.scale_geometry(&frame.ids, &points, anchor, scale, dims) {
                            if let Some(seed) = &mut ed.selected_region {
                                *seed = anchor + (*seed - anchor) * scale;
                            }
                            // Dimensions held it all at its size: say so, and how to scale
                            // them with it, rather than seem to do nothing.
                            let held = !dims
                                && ed.bounds_of(&frame.ids).is_some_and(|after| {
                                    let (was, now) = (
                                        frame.bounds.max - frame.bounds.min,
                                        after.max - after.min,
                                    );
                                    let same = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.max(1.0);
                                    (scale.x == 1.0 || same(was.x, now.x))
                                        && (scale.y == 1.0 || same(was.y, now.y))
                                });
                            if held {
                                ed.doc.rollback();
                                ed.out.notice = Some(
                                    "Its size is held by a dimension: hold Alt while you scale \
                                     to change the dimension too, or edit its value"
                                        .into(),
                                );
                            } else {
                                ed.commit("Scale");
                            }
                        } else {
                            ed.doc.rollback();
                            ed.out.notice = Some("Can't scale that far".into());
                        }
                    }
                    ed.sync();
                }
                Some(SelectDrag::Fillet { .. }) => {
                    ed.commit("Fillet radius");
                }
                Some(SelectDrag::Opening { .. }) => {
                    ed.commit("Move opening");
                }
                Some(SelectDrag::Text { .. }) => {
                    ed.commit("Move text");
                }
                None => {}
            },
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        if self.cancel_grab(ed) || self.cancel_spin(ed) {
            return true;
        }
        match self.drag.take() {
            Some(
                SelectDrag::Radius { .. }
                | SelectDrag::Offset { .. }
                | SelectDrag::Fillet { .. }
                | SelectDrag::Text { .. }
                | SelectDrag::Scale { .. }
                | SelectDrag::Opening { .. },
            ) => {
                ed.region_motion = None;
                ed.doc.rollback();
                ed.sync();
                true
            }
            Some(SelectDrag::Box { .. }) => true,
            // Esc first leaves an open group.
            None if ed.open_group.is_some() => {
                ed.close_open_group();
                ed.expand_groups();
                true
            }
            None if !ed.selection.is_empty()
                || !ed.selected_midpoints.is_empty()
                || ed.selected_region.is_some()
                || !ed.selected_fills.is_empty()
                || !ed.selected_instances.is_empty()
                || ed.picks_annotations() =>
            {
                ed.selected_texts.clear();
                ed.selected_annotation = None;
                ed.annotation_picks.clear();
                ed.selected_instances.clear();
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
        if let Some(o) = self.spin_overlay() {
            return Some(o);
        }
        if let Some(grab) = self.grab.as_ref().filter(|g| g.by_mouse.is_none()) {
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
        // Moving, and rotating: a new pivot snaps to geometry and the grid; while turning,
        // a snap onto geometry gives an exact angle.
        self.grab.is_some() || self.spin.is_some()
    }

    fn snap_excludes(&self, ed: &Editor, id: EntityId) -> bool {
        // Nothing that moves with the selection (picking a grab's base point: anything,
        // typically a point of the selection). The moving points are every moving entity's
        // defining points, so set lookups cover the moving curves too.
        self.moving().is_some_and(|points| {
            points.contains(&id)
                || ed
                    .doc
                    .get(id)
                    .is_some_and(|e| e.kind.points().any(|q| points.contains(&q)))
        })
    }

    fn snap_excludes_copy(&self, id: drift_core::InstanceId) -> bool {
        let moving = match (&self.grab, &self.spin) {
            (Some(grab), _) if !grab.picking_base => &grab.instances,
            (None, Some(spin)) if !spin.picking() => &spin.instances,
            _ => return false,
        };
        moving.iter().any(|(i, _)| *i == id)
    }
}
