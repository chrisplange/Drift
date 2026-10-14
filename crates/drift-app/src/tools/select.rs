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
    let hidden = ed.hidden_handles();
    let is_point =
        |id: EntityId| !hidden.contains(&id) && ed.doc.get(id).is_some_and(|e| e.kind.is_point());
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
        .pick(p.raw, p.tolerance, |id| !hidden.contains(&id))
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
        /// Copies of blocks moving too (as they were when the drag began).
        instances: Vec<(drift_core::InstanceId, drift_core::Instance)>,
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
    /// Dragging a circle's edge: the radius follows the pointer (drag its centre to move it).
    Radius {
        circle: EntityId,
        center: DVec2,
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
    pub(super) lock: Option<Axis>,
    pub(super) typed: [Option<f64>; 2],
    /// The offset the geometry has been moved by so far in the open transaction.
    pub(super) applied: DVec2,
    /// Copies of blocks moving too (as they were when the move began).
    pub(super) instances: Vec<(drift_core::InstanceId, drift_core::Instance)>,
    /// The picked sketch text moving too, and where its anchor was.
    pub(super) text: Option<(drift_core::TextId, DVec2)>,
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
        if step == DVec2::ZERO {
            return;
        }
        if self.ids.is_empty() || ed.drag(&self.ids, step, &self.dragged()) {
            ed.move_instances(&self.instances, offset);
            if let Some((id, from)) = self.text {
                ed.place_text(id, from + offset);
            }
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
        // A picked fill or hatch moves the curves around it (it follows them).
        let fill = if ed.selection.is_empty() && region.is_none() {
            ed.selected_fill_curves()
        } else {
            None
        };
        let instances = ed.picked_instances();
        let text = ed
            .selected_text
            .and_then(|id| ed.doc.text(id).map(|t| (id, t.at)));
        let (ids, base) = match (&region, fill) {
            (Some(r), _) => (ed.region_curves(r), r.centroid),
            (None, Some((ids, centre))) => (ids, centre),
            // Only copies of blocks: their insertion point is the base.
            (None, None) if ed.selection.is_empty() && !instances.is_empty() => {
                (Vec::new(), instances[0].1.at)
            }
            // Only a text: its anchor is the base.
            (None, None) if ed.selection.is_empty() && text.is_some() => {
                (Vec::new(), text.map_or(p.raw, |t| t.1))
            }
            (None, None) => {
                let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                let base = ed.move_base(&ids, p.raw, false);
                (ids, base)
            }
        };
        if ids.is_empty() && instances.is_empty() && text.is_none() {
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
            lock: None,
            typed: [None; 2],
            applied: DVec2::ZERO,
            instances,
            text,
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
        let points = ed.doc.defining_points(ids.iter().copied());
        ed.doc.begin();
        self.drag = Some(SelectDrag::Move {
            ids,
            points,
            start: p.raw,
            base: region.centroid,
            applied: DVec2::ZERO,
            point: None,
            instances: Vec::new(),
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
        if let Some((id, from)) = grab.text {
            ed.place_text(id, from + offset);
        }
        let dragged = grab.dragged();
        if !grab.ids.is_empty() && !ed.drag(&grab.ids, offset, &dragged) {
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
        ed.region_motion = None;
        ed.doc.rollback();
        ed.sync();
        true
    }

    /// The points moving with a drag or grab (never snap targets).
    fn moving(&self) -> Option<(&BTreeSet<EntityId>, &[EntityId])> {
        if let Some(grab) = &self.grab {
            return (!grab.picking_base).then_some((&grab.points, grab.ids.as_slice()));
        }
        if let Some(spin) = &self.spin {
            return (!spin.picking_pivot).then_some((&spin.points, spin.ids.as_slice()));
        }
        match &self.drag {
            Some(SelectDrag::Move { points, ids, .. }) => Some((points, ids.as_slice())),
            _ => None,
        }
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
        self.grab.is_some() || self.spin.is_some()
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
        if let Some(spin) = &self.spin {
            return if spin.picking_pivot { &[] } else { &["Angle"] };
        }
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
            } else {
                format!("Angle {}", p.units.format_angle(spin.current()))
            });
        }
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
        if let Some(spin) = &self.spin {
            return if spin.picking_pivot {
                "Click the pivot (snaps to points) · Esc: cancel".into()
            } else {
                "Rotate: click / Enter to place · type an angle · 15° steps stick · B: pivot · \
                 Esc: cancel"
                    .into()
            };
        }
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
            Some(SelectDrag::Text { .. }) => "Moving text · Esc to cancel".into(),
            Some(SelectDrag::Radius { .. }) => {
                "Resizing the circle (drag its centre to move it) · Esc to cancel".into()
            }
            Some(SelectDrag::Fillet { .. }) => {
                "Resizing the fillet in steps (Alt: freely) · Esc to cancel".into()
            }
            Some(SelectDrag::Scale { .. }) => "Scaling: dimensions hold · Shift: keep the \
                 shape · Ctrl: about the middle · Alt: scale the dimensions too · Esc to cancel"
                .into(),
            None => "Click to select · Shift to add · Ctrl to toggle · drag to move or box-select"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if self.spin_input(input, ed) {
            return;
        }
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
                    Some(SelectDrag::Fillet { arc }) => Some(*arc),
                    Some(SelectDrag::Text { .. } | SelectDrag::Scale { .. }) => None,
                    None => match pick_target(ed, &p) {
                        Some(Target::Entity(id)) => Some(id),
                        Some(Target::Midpoint(line)) => {
                            ed.hovered_midpoint = Some(line);
                            None
                        }
                        None => None,
                    },
                };
                let free =
                    ed.hovered.is_none() && ed.hovered_midpoint.is_none() && self.drag.is_none();
                ed.hovered_text = if free {
                    ed.text_at(p.raw, p.tolerance)
                } else {
                    None
                };
                ed.hovered_instance = if free && ed.hovered_text.is_none() {
                    ed.instance_at(p.raw, p.tolerance)
                } else {
                    None
                };
                // Inside a fill (and not on a curve or a copy of a block): the fill.
                ed.hovered_fill = if free && ed.hovered_instance.is_none() {
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
                let hit = {
                    let hidden = ed.hidden_handles();
                    ed.index
                        .pick(p.raw, p.tolerance, |id| ed.pickable(id, &hidden))
                };
                // Sketch text (anywhere in its box).
                if hit.is_none()
                    && let Some(t) = ed.text_at(p.raw, p.tolerance)
                {
                    ed.selected_text = Some(t);
                    if !mods.shift && !mods.command {
                        ed.selection.clear();
                        ed.selected_instances.clear();
                        ed.selected_fills.clear();
                        ed.selected_region = None;
                    }
                    return;
                }
                ed.selected_text = None;
                // A copy of a block (on its curves, or inside it).
                let copy = if hit.is_none() {
                    ed.instance_at(p.raw, p.tolerance)
                } else {
                    None
                };
                if let Some(c) = copy {
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
            }
            // Double-click a group's member to edit inside the group.
            Input::DoubleClick(p) => {
                // Sketch text: type into it where it stands.
                if ed
                    .index
                    .pick(p.raw, p.tolerance, |id| !ed.is_locked(id))
                    .is_none()
                    && let Some(t) = ed.text_at(p.raw, p.tolerance)
                {
                    ed.selected_text = Some(t);
                    ed.editing_text = Some(t);
                    ed.text_caret_at = Some(p.raw);
                    return;
                }
                if ed
                    .index
                    .pick(p.raw, p.tolerance, |id| !ed.is_locked(id))
                    .is_none()
                    && let Some(c) = ed.instance_at(p.raw, p.tolerance)
                {
                    ed.edit_block(c);
                    return;
                }
                if let Some(id) = {
                    let hidden = ed.hidden_handles();
                    ed.index
                        .pick(p.raw, p.tolerance, |id| ed.pickable(id, &hidden))
                } && let Some(g) = ed.doc.group_of(id)
                    && ed.open_group != Some(g)
                {
                    ed.open_group = Some(g);
                    ed.selection = [id].into();
                }
            }
            Input::Finish => {}
            Input::DragStart(p, mods) => {
                if self.start_handle_drag(&p, ed) {
                    return;
                }
                let hidden = ed.hidden_handles();
                let hit = ed
                    .index
                    .pick(p.raw, p.tolerance, |id| ed.pickable(id, &hidden));
                match hit {
                    Some(id) => {
                        if !ed.selection.contains(&id) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                                ed.selected_instances.clear();
                            }
                            let set = ed.pick_set(id);
                            ed.selection.extend(set);
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
                        let points = ed.doc.defining_points(ids.iter().copied());
                        let instances = ed.picked_instances();
                        ed.doc.begin();
                        self.drag = Some(SelectDrag::Move {
                            ids,
                            points,
                            start: p.raw,
                            base,
                            applied: DVec2::ZERO,
                            point,
                            instances,
                        });
                    }
                    // Sketch text: drag it by its body.
                    None if ed.text_at(p.raw, p.tolerance).is_some() => {
                        let id = ed.text_at(p.raw, p.tolerance).expect("checked");
                        ed.selected_text = Some(id);
                        let from = ed.doc.text(id).map_or(p.raw, |t| t.at);
                        ed.doc.begin();
                        self.drag = Some(SelectDrag::Text {
                            id,
                            start: p.raw,
                            from,
                        });
                    }
                    // A copy of a block: drag it (and the rest of the selection) by its
                    // insertion point.
                    None if ed.instance_at(p.raw, p.tolerance).is_some() => {
                        let c = ed.instance_at(p.raw, p.tolerance).expect("checked");
                        if !ed.selected_instances.contains(&c) {
                            if !mods.shift && !mods.command {
                                ed.selection.clear();
                                ed.selected_instances.clear();
                                ed.selected_fills.clear();
                            }
                            ed.selected_instances.insert(c);
                        }
                        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                        let points = ed.doc.defining_points(ids.iter().copied());
                        let base = ed.doc.instance(c).map_or(p.raw, |i| i.at);
                        let instances = ed.picked_instances();
                        ed.doc.begin();
                        self.drag = Some(SelectDrag::Move {
                            ids,
                            points,
                            start: p.raw,
                            base,
                            applied: DVec2::ZERO,
                            point: None,
                            instances,
                        });
                    }
                    None if self.start_region_drag(&p, ed) => {}
                    None => {
                        // Dragging inside a picked fill moves its area; elsewhere, box-select.
                        let picked_fill = ed
                            .fill_at(p.raw)
                            .is_some_and(|f| ed.selected_fills.contains(&f));
                        match picked_fill.then(|| ed.selected_fill_curves()).flatten() {
                            Some((ids, centre)) => {
                                let points = ed.doc.defining_points(ids.iter().copied());
                                ed.doc.begin();
                                self.drag = Some(SelectDrag::Move {
                                    ids,
                                    points,
                                    start: p.raw,
                                    base: centre,
                                    applied: DVec2::ZERO,
                                    point: None,
                                    instances: Vec::new(),
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
                    let mut scale =
                        crate::transform::scale_for(*handle, anchor, *press, p.raw, p.shift);
                    if dims && p.round_scale {
                        // Round sizes, in steps that suit the zoom (as fillets are sized).
                        let step = super::corners::round_step(
                            p.tolerance * 1.5,
                            p.units.length.is_metric(),
                        );
                        let size = frame.bounds.max - frame.bounds.min;
                        scale = crate::transform::round_scale(scale, *handle, size, step, p.shift);
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
                    instances,
                    ..
                }) => {
                    let offset = crate::modify::move_offset(*start, *base, &p);
                    let step = offset - *applied;
                    if step != DVec2::ZERO {
                        // The grabbed geometry follows the pointer; constraints move the rest.
                        let dragged: Vec<Param> =
                            points.iter().copied().flat_map(Param::point).collect();
                        if ids.is_empty() || ed.drag(ids, step, &dragged) {
                            ed.move_instances(instances, offset);
                            *applied = offset;
                            ed.region_motion = Some(crate::editor::RegionMotion::Move(offset));
                        }
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
                    let found: Vec<EntityId> = ed
                        .index
                        .select_in_rect(&rect, crossing)
                        .into_iter()
                        .filter(|id| ed.pickable(*id, &hidden))
                        .collect();
                    if !mods.shift && !mods.command {
                        ed.selection.clear();
                        ed.selected_instances.clear();
                    }
                    ed.selection.extend(found);
                    ed.expand_groups();
                    let copies = ed.instances_in(&rect, crossing);
                    ed.selected_instances.extend(copies);
                }
                Some(SelectDrag::Move { point, applied, .. }) => {
                    ed.region_motion = None;
                    if let Some(seed) = &mut ed.selected_region {
                        *seed += applied;
                    }
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
                            ed.commit("Scale");
                        } else {
                            ed.doc.rollback();
                            ed.notice = Some("Can't scale that far".into());
                        }
                    }
                    ed.sync();
                }
                Some(SelectDrag::Fillet { .. }) => {
                    ed.commit("Fillet radius");
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
                SelectDrag::Move { .. }
                | SelectDrag::Radius { .. }
                | SelectDrag::Fillet { .. }
                | SelectDrag::Text { .. }
                | SelectDrag::Scale { .. },
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
                || ed.selected_text.is_some() =>
            {
                ed.selected_text = None;
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
