//! A Blender-style modal rotate (R) of the selection, run by the select tool.

use super::*;

/// Pointer angles within this many degrees of a multiple of 15° stick to it.
const MAGNET_DEG: f64 = 2.0;

pub(super) struct Spin {
    pub(super) ids: Vec<EntityId>,
    pub(super) points: BTreeSet<EntityId>,
    pub(super) pivot: DVec2,
    /// The direction the turn is measured from: 0° (pointing right), so angles are absolute
    /// whatever the pointer's position when the rotate began; or, rotating by reference, the
    /// direction from the pivot to a picked point (so it turns to meet the pointer's).
    pub(super) reference: DVec2,
    /// The pointer's latest position.
    pub(super) pointer: DVec2,
    /// Degrees from the pointer (counter-clockwise).
    pub(super) angle: f64,
    pub(super) typed: Option<f64>,
    pub(super) picking_pivot: bool,
    /// Picking the reference point (R again): the turn is then measured from it.
    pub(super) picking_reference: bool,
    /// The angle the geometry is turned to now (so an unchanged angle isn't re-applied).
    pub(super) applied: Option<f64>,
    /// Copies of blocks turning too (as they were when the rotate began).
    pub(super) instances: Vec<(drift_core::InstanceId, drift_core::Instance)>,
    /// The picked sketch text and annotations turning too, as they were.
    pub(super) riders: crate::picks::Riders,
    /// Begun by dragging a turn handle: letting go places it.
    pub(super) by_handle: bool,
}

impl Spin {
    /// Picking a point (the pivot or the reference): nothing is turned meanwhile.
    pub(super) fn picking(&self) -> bool {
        self.picking_pivot || self.picking_reference
    }

    pub(super) fn current(&self) -> f64 {
        self.typed.unwrap_or(self.angle)
    }

    /// Re-apply the turn from the original geometry (if it changed).
    pub(super) fn apply(&mut self, ed: &mut Editor) {
        let angle = if self.picking() { 0.0 } else { self.current() };
        if self.applied == Some(angle) {
            return;
        }
        self.applied = Some(angle);
        ed.doc.rollback();
        ed.doc.begin();
        if angle != 0.0 {
            if !self.ids.is_empty() {
                ed.rotate_geometry(&self.ids, &self.points, self.pivot, angle);
            }
            ed.rotate_instances(&self.instances, self.pivot, angle);
            ed.place_riders(&self.riders, &turn(self.pivot, angle));
        }
        ed.region_motion = Some(crate::editor::RegionMotion::Turn {
            pivot: self.pivot,
            degrees: angle,
        });
        ed.sync();
    }

    /// Follow the pointer: its angle about the pivot, from 0° (pointing right). Exact snaps
    /// give exact angles; otherwise nearby multiples of 15° stick (the grid and inference
    /// lines don't: they'd make the angle jump). Close to the pivot the angle is undefined,
    /// so it stays put.
    pub(super) fn track(&mut self, p: &Pointer) {
        self.pointer = p.exact_snap().map_or(p.raw, |s| s.at);
        if (self.pointer - self.pivot).length() <= p.tolerance {
            return;
        }
        let (a, b) = (self.reference, self.pointer - self.pivot);
        if a.length() < 1e-12 || b.length() < 1e-12 {
            return;
        }
        let mut deg = a.angle_to(b).to_degrees();
        if p.exact_snap().is_none() {
            let snapped = (deg / 15.0).round() * 15.0;
            if (deg - snapped).abs() <= MAGNET_DEG {
                deg = snapped;
            }
        }
        self.angle = deg;
    }
}

/// Turning by `degrees` about `pivot`.
fn turn(pivot: DVec2, degrees: f64) -> impl Fn(DVec2) -> DVec2 {
    let rotor = drift_core::blocks::exact_rotor(degrees);
    move |p| pivot + rotor.rotate(p - pivot)
}

/// Where a rotate turns about: a picked region's or hatch's centroid, a single curve's centre
/// (a circle's or arc's centre, a line's midpoint), or the middle of a larger selection.
fn rotate_pivot(ed: &mut Editor, ids: &[EntityId], near: DVec2) -> Option<DVec2> {
    if let Some(r) = ed.region() {
        let outline = ed.region_curves(&r);
        if ids.iter().all(|id| outline.contains(id)) {
            return Some(r.centroid);
        }
    }
    if ed.selection.is_empty()
        && let Some((_, centre)) = ed.selected_fill_curves()
    {
        return Some(centre);
    }
    if let [_] = ids {
        return Some(ed.move_base(ids, near, false));
    }
    let mut bounds = Bounds::EMPTY;
    for id in ids {
        if let Some(shape) = ed.index.shape(*id) {
            let b = shape.bounds();
            bounds.include(b.min);
            bounds.include(b.max);
        }
    }
    (!bounds.is_empty()).then(|| bounds.center())
}

impl SelectTool {
    /// Start rotating the selection about its centre.
    pub(super) fn start_spin(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        if self.drag.is_some() || self.grab.is_some() || self.spin.is_some() {
            return false;
        }
        // A picked fill or hatch turns with the curves around it.
        let instances = ed.picked_instances();
        let riders = ed.riders();
        let ids: Vec<EntityId> = if ed.selection.is_empty() {
            match ed.selected_fill_curves() {
                Some((ids, _)) if ed.refuse_locked(&ids) => return false,
                Some((ids, _)) => ids,
                None if !instances.is_empty() || !riders.is_empty() => Vec::new(),
                None => return false,
            }
        } else {
            ed.selection.iter().copied().collect()
        };
        let pivot = if ids.is_empty() {
            // Only copies of blocks: one turns about its insertion point, several about
            // their middle.
            match instances.as_slice() {
                // Only text and annotations: a lone text turns about its anchor, more about
                // their middle.
                [] => riders.centre(),
                [(_, i)] => Some(i.at),
                _ => {
                    let mut b = Bounds::EMPTY;
                    for (id, _) in &instances {
                        let ib = ed.instance_bounds(*id);
                        b.include(ib.min);
                        b.include(ib.max);
                    }
                    (!b.is_empty()).then(|| b.center())
                }
            }
        } else {
            rotate_pivot(ed, &ids, p.raw)
        };
        let Some(pivot) = pivot else {
            return false;
        };
        let points = ed.doc.defining_points(ids.iter().copied());
        ed.doc.begin();
        self.spin = Some(Spin {
            ids,
            points,
            pivot,
            reference: DVec2::X,
            pointer: p.raw,
            angle: 0.0,
            typed: None,
            picking_pivot: false,
            picking_reference: false,
            applied: Some(0.0),
            instances,
            riders,
            by_handle: false,
        });
        true
    }

    /// Start turning the geometry in a transform box about its middle, from the pointer
    /// (dragging a turn handle).
    pub(super) fn start_turn(
        &mut self,
        p: &Pointer,
        frame: &crate::transform::TransformBox,
        ed: &mut Editor,
    ) -> bool {
        if self.drag.is_some() || self.grab.is_some() || self.spin.is_some() {
            return false;
        }
        let pivot = frame.bounds.center();
        let reference = p.raw - pivot;
        if reference.length() < 1e-12 {
            return false;
        }
        let points = ed.doc.defining_points(frame.ids.iter().copied());
        ed.doc.begin();
        self.spin = Some(Spin {
            ids: frame.ids.clone(),
            points,
            pivot,
            reference,
            pointer: p.raw,
            angle: 0.0,
            typed: None,
            picking_pivot: false,
            picking_reference: false,
            applied: Some(0.0),
            instances: Vec::new(),
            riders: Default::default(),
            by_handle: true,
        });
        true
    }

    pub(super) fn confirm_spin(&mut self, ed: &mut Editor) {
        let Some(spin) = self.spin.take() else { return };
        let angle = spin.current();
        ed.region_motion = None;
        ed.doc.rollback();
        if angle.abs() < 1e-12 || spin.picking() {
            ed.sync();
            return;
        }
        ed.doc.begin();
        ed.rotate_instances(&spin.instances, spin.pivot, angle);
        let (ok, removed) = if spin.ids.is_empty() {
            (true, Vec::new())
        } else {
            ed.rotate_geometry(&spin.ids, &spin.points, spin.pivot, angle)
        };
        if ok && let Some(seed) = &mut ed.selected_region {
            // A picked region turns with its outline.
            let rotor = drift_core::blocks::exact_rotor(angle);
            *seed = spin.pivot + rotor.rotate(*seed - spin.pivot);
        }
        if !ok {
            ed.doc.rollback();
            ed.sync();
            ed.out.notice = Some("Can't rotate it that way: its constraints don't allow it".into());
            return;
        }
        ed.place_riders(&spin.riders, &turn(spin.pivot, angle));
        if crate::editor::is_quarter_turn(angle) {
            ed.restore_after_rotation(&removed, angle);
        } else if !removed.is_empty() {
            ed.out.notice = Some(format!(
                "Rotated {}: {} horizontal / vertical relation(s) no longer applied and were \
                 removed",
                ed.doc.units().format_angle(angle),
                removed.len()
            ));
        }
        ed.commit("Rotate");
    }

    pub(super) fn cancel_spin(&mut self, ed: &mut Editor) -> bool {
        if self.spin.take().is_none() {
            return false;
        }
        ed.region_motion = None;
        ed.doc.rollback();
        ed.sync();
        true
    }

    /// Pointer input during a rotate. Returns false when not rotating.
    pub(super) fn spin_input(&mut self, input: Input, ed: &mut Editor) -> bool {
        let Some(spin) = &mut self.spin else {
            return false;
        };
        ed.hovered = None;
        ed.hovered_midpoint = None;
        match input {
            Input::Hover(p) | Input::Drag(p) => {
                if spin.picking() {
                    spin.pointer = p.pos();
                } else {
                    spin.track(&p);
                    spin.apply(ed);
                }
            }
            Input::Click(p, _) | Input::DragStart(p, _) => {
                if spin.picking_pivot {
                    spin.pivot = p.pos();
                    spin.pointer = p.pos();
                    spin.picking_pivot = false;
                    // From a new pivot, angles are absolute again until a reference is picked.
                    spin.reference = DVec2::X;
                    spin.angle = 0.0;
                    spin.typed = None;
                } else if spin.picking_reference {
                    // The reference: the turn is measured from the pivot's direction to it.
                    let to = p.pos() - spin.pivot;
                    if to.length() > p.tolerance {
                        spin.reference = to;
                        spin.pointer = p.pos();
                        spin.picking_reference = false;
                        spin.angle = 0.0;
                        spin.typed = None;
                    }
                } else {
                    self.confirm_spin(ed);
                }
            }
            Input::Finish => self.confirm_spin(ed),
            Input::DragEnd(..) if spin.by_handle => self.confirm_spin(ed),
            _ => {}
        }
        true
    }

    /// Modal keys during a rotate: B picks a new pivot; R rotates by reference (pick a point
    /// to turn from: it turns to meet the pointer); others are swallowed.
    pub(super) fn spin_key(&mut self, key: ToolKey, ed: &mut Editor) -> bool {
        let Some(spin) = &mut self.spin else {
            return false;
        };
        match key {
            ToolKey::B => {
                spin.picking_reference = false;
                spin.picking_pivot = true;
                spin.apply(ed);
            }
            ToolKey::R if !spin.picking_pivot => {
                spin.picking_reference = true;
                spin.typed = None;
                spin.apply(ed);
            }
            _ => {}
        }
        true
    }

    pub(super) fn spin_overlay(&self) -> Option<Overlay> {
        let spin = self.spin.as_ref()?;
        Some(Overlay::Rotate {
            // While a new pivot is placed, its marker follows the pointer.
            pivot: if spin.picking_pivot {
                spin.pointer
            } else {
                spin.pivot
            },
            from: spin.pivot + spin.reference,
            degrees: if spin.picking() {
                None
            } else {
                Some(spin.current())
            },
        })
    }
}
