//! Handles on a picked hatch, on the canvas: one moves the pattern's origin (it snaps, so a
//! tile joint or a brick course can sit on a corner), the other turns the pattern. Revit
//! aligns and moves its model patterns the same way.

use drift_core::{Bounds, DVec2, FillId, FillKind};
use drift_geom::regions::RegionTolerance;

use crate::overlay::OverlayColors;
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HatchGrip {
    Origin,
    Angle,
}

/// Where a picked hatch's handles are.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HatchHandles {
    pub fill: FillId,
    /// The origin handle, in the sketch and on screen.
    pub at: DVec2,
    pub origin: egui::Pos2,
    /// The handle that turns the pattern, at the end of an arm along its direction.
    pub angle: egui::Pos2,
    pub degrees: f64,
}

/// Length of the arm to the angle handle, and how near a press counts (points).
const ARM: f32 = 56.0;
const REACH: f32 = 9.0;
/// The angle handle turns in these steps (Alt: freely).
const ANGLE_STEP: f64 = 15.0;

/// Whether a hatch origin lies in an area's bounds. An origin on the outline (a corner, an
/// edge) counts as in, whichever side of it rounding left the bounds.
fn origin_in(origin: DVec2, b: &Bounds) -> bool {
    let pad = RegionTolerance::for_extent(b.min.abs().max(b.max.abs()).max_element()).snap;
    origin.x >= b.min.x - pad
        && origin.x <= b.max.x + pad
        && origin.y >= b.min.y - pad
        && origin.y <= b.max.y + pad
}

impl HatchHandles {
    fn grip_at(&self, p: egui::Pos2) -> Option<HatchGrip> {
        if p.distance(self.angle) <= REACH {
            Some(HatchGrip::Angle)
        } else if p.distance(self.origin) <= REACH {
            Some(HatchGrip::Origin)
        } else {
            None
        }
    }
}

impl DriftApp {
    /// The handles of the one picked hatch, while the Select tool is idle.
    pub(crate) fn hatch_handles(&mut self, map: &ScreenMap) -> Option<HatchHandles> {
        let ed = &mut self.editor;
        if self.tool.kind() != ToolKind::Select
            || (self.tool.is_busy() && self.hatch_drag.is_none())
            || !ed.selection.is_empty()
            || ed.selected_fills.len() != 1
        {
            return None;
        }
        let id = *ed.selected_fills.iter().next()?;
        let FillKind::Hatch(hatch) = &ed.doc.fill(id)?.kind else {
            return None;
        };
        let (origin, degrees) = (hatch.origin, hatch.angle);
        let shapes = ed.fill_shapes();
        let region = &shapes.get(&id).filter(|s| !s.lost)?.region;
        // An origin away from the area (a new hatch's is the sketch's) is shown in the middle
        // of the area: dragging the handle puts the origin where it is dropped.
        let near = origin_in(origin, &region.bounds);
        let moving = self.hatch_drag == Some((id, HatchGrip::Origin));
        let at = if near || moving {
            origin
        } else {
            region.centroid
        };
        let screen = map.to_screen(at);
        let a = degrees.to_radians();
        Some(HatchHandles {
            fill: id,
            at,
            origin: screen,
            angle: screen + egui::vec2(a.cos() as f32, -a.sin() as f32) * ARM,
            degrees,
        })
    }

    /// Drag a hatch handle. Returns true if the pointer input was used here.
    pub(crate) fn hatch_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        handles: Option<&HatchHandles>,
        snap: bool,
    ) -> bool {
        let Some(h) = handles else {
            if self.hatch_drag.take().is_some() {
                self.editor.commit("Edit hatch");
            }
            return false;
        };
        if self.hatch_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(grip) = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|p| h.grip_at(p))
        {
            self.hatch_drag = Some((h.fill, grip));
            self.editor.doc.begin();
        }
        if let Some((id, grip)) = self.hatch_drag {
            if let Some(p) = response.interact_pointer_pos() {
                self.move_hatch_grip(id, grip, h, map, p, snap);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.hatch_drag = None;
                self.editor.commit(match grip {
                    HatchGrip::Origin => "Move hatch pattern",
                    HatchGrip::Angle => "Turn hatch pattern",
                });
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return true;
        }
        let over = response.hover_pos().and_then(|p| h.grip_at(p));
        if over.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            // A click on a handle isn't a click on the canvas behind it.
            return response.clicked_by(PointerButton::Primary);
        }
        false
    }

    fn move_hatch_grip(
        &mut self,
        id: FillId,
        grip: HatchGrip,
        h: &HatchHandles,
        map: &ScreenMap,
        p: egui::Pos2,
        snap: bool,
    ) {
        let Some(mut fill) = self.editor.doc.fill(id).cloned() else {
            return;
        };
        let FillKind::Hatch(hatch) = &mut fill.kind else {
            return;
        };
        let raw = map.to_world(p);
        match grip {
            HatchGrip::Origin => {
                // Onto points, curves and the grid, like anything else that is placed.
                let options = drift_geom::SnapOptions {
                    tolerance: PICK_RADIUS / self.camera.scale,
                    grid: self
                        .grid_snap
                        .then(|| drift_render::grid_info(&self.camera, self.imperial()).minor),
                    from: None,
                };
                let found = snap
                    .then(|| self.editor.snap(raw, options, |_| false))
                    .flatten();
                self.hatch_snap = found.filter(|s| s.kind != drift_geom::SnapKind::Grid);
                hatch.origin = found.map_or(raw, |s| s.at);
            }
            HatchGrip::Angle => {
                let v = raw - h.at;
                if v.length_squared() <= 0.0 {
                    return;
                }
                let degrees = v.y.atan2(v.x).to_degrees();
                hatch.angle = if snap {
                    (degrees / ANGLE_STEP).round() * ANGLE_STEP
                } else {
                    (degrees * 10.0).round() / 10.0
                };
                if hatch.angle <= -180.0 {
                    hatch.angle += 360.0;
                }
            }
        }
        self.editor.doc.set_fill(id, fill);
    }

    pub(crate) fn paint_hatch_handles(
        &mut self,
        painter: &egui::Painter,
        map: &ScreenMap,
        h: &HatchHandles,
        colors: &OverlayColors,
    ) {
        let color = colors.window;
        let halo = colors.label_bg;
        let dragging = self.hatch_drag.map(|d| d.1);
        painter.line_segment([h.origin, h.angle], egui::Stroke::new(1.0, color));
        // The origin: a cross in a square. The angle: a round knob.
        let square = egui::Rect::from_center_size(h.origin, egui::Vec2::splat(11.0));
        painter.rect_filled(square, 2.0, halo);
        painter.rect_stroke(
            square,
            2.0,
            egui::Stroke::new(1.5, color),
            egui::StrokeKind::Middle,
        );
        for d in [egui::vec2(3.0, 0.0), egui::vec2(0.0, 3.0)] {
            painter.line_segment([h.origin - d, h.origin + d], egui::Stroke::new(1.0, color));
        }
        painter.circle_filled(h.angle, 5.5, halo);
        painter.circle_stroke(h.angle, 5.5, egui::Stroke::new(1.5, color));
        match dragging {
            Some(HatchGrip::Angle) => {
                let text = format!("{}°", (h.degrees * 10.0).round() / 10.0);
                overlay::cursor_label(painter, h.angle, &[text], colors);
            }
            Some(HatchGrip::Origin) => {
                if let Some(snap) = &self.hatch_snap {
                    overlay::snap_marker(painter, map, snap, colors.snap);
                }
            }
            None => self.hatch_snap = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_on_the_outline_is_in_whatever_the_rounding() {
        let v = DVec2::new;
        let b = |min_y: f64| Bounds {
            min: v(-188.96, min_y),
            max: v(141.24, 76.2),
        };
        // A corner origin, with the bottom edge rounded either way (as a drag left it).
        for min_y in [0.0, 7.1e-15, -7.1e-15] {
            assert!(origin_in(v(0.0, 0.0), &b(min_y)), "{min_y}");
        }
        assert!(!origin_in(v(0.0, -0.01), &b(0.0)));
        assert!(!origin_in(v(500.0, 30.0), &b(0.0)));
    }
}
