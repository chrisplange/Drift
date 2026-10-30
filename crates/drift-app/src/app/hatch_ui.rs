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
    /// The angle handle's arm points against the pattern's direction (so the handle stays
    /// inside the area): a pattern runs both ways along it.
    pub flipped: bool,
}

/// Length of the arm to the angle handle, and how near a press counts (points).
const ARM: f32 = 56.0;
const REACH: f32 = 9.0;
/// How far inside a picked shape's outline its origin handle must be to show where the origin
/// is (points).
const INSET: f32 = 14.0;
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

/// The colour of a hatch's handles: their own, so they aren't taken for the transform box's.
pub(crate) fn handle_color(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(0xc5, 0x8a, 0xff)
    } else {
        egui::Color32::from_rgb(0x8a, 0x3f, 0xd1)
    }
}

impl DriftApp {
    /// The one picked hatch whose pattern can be aligned: a hatch picked alone, or a hatched
    /// shape picked (its fill with its outline).
    pub(crate) fn aligned_hatch(&self) -> Option<FillId> {
        let ed = &self.editor;
        if ed.selected_fills.len() != 1
            || !ed.selected_instances.is_empty()
            || !ed.selected_texts.is_empty()
        {
            return None;
        }
        let id = *ed.selected_fills.iter().next()?;
        if !matches!(ed.doc.fill(id)?.kind, FillKind::Hatch(_)) {
            return None;
        }
        let alone = ed.selection.is_empty()
            || ed
                .shape_outline(id)
                .is_some_and(|o| ed.selection.iter().all(|c| o.contains(c)));
        alone.then_some(id)
    }

    /// The handles of the one picked hatch (alone, or a hatched shape's), while the Select
    /// tool is idle.
    pub(crate) fn hatch_handles(&mut self, map: &ScreenMap) -> Option<HatchHandles> {
        if self.tool.kind() != ToolKind::Select
            || (self.tool.is_busy() && self.hatch_drag.is_none())
            || self.align_edge.is_some()
        {
            return None;
        }
        let id = self.aligned_hatch()?;
        let ed = &mut self.editor;
        // A whole shape picked has the transform box round it.
        let boxed = !ed.selection.is_empty();
        let FillKind::Hatch(hatch) = &ed.doc.fill(id)?.kind else {
            return None;
        };
        let (origin, degrees) = (hatch.origin, hatch.angle);
        let shapes = ed.fill_shapes();
        let region = &shapes.get(&id).filter(|s| !s.lost)?.region;
        // An origin away from the area (a new hatch's is the sketch's) is shown in the middle
        // of the area: dragging the handle puts the origin where it is dropped. With the
        // transform box round the shape, so is one on its outline (a corner), clear of the
        // box's handles.
        let near = if boxed {
            let m = f64::from(INSET) / map.camera.scale;
            [(m, 0.0), (-m, 0.0), (0.0, m), (0.0, -m)]
                .iter()
                .all(|&(x, y)| region.contains(origin + DVec2::new(x, y)))
        } else {
            origin_in(origin, &region.bounds)
        };
        let moving = self.hatch_drag == Some((id, HatchGrip::Origin));
        let at = if near || moving {
            origin
        } else {
            region.centroid
        };
        let screen = map.to_screen(at);
        let a = degrees.to_radians();
        let arm = egui::vec2(a.cos() as f32, -a.sin() as f32) * ARM;
        // The arm into the area, so the knob stays clear of the transform box's handles.
        let flipped = !region.contains(map.to_world(screen + arm))
            && region.contains(map.to_world(screen - arm));
        Some(HatchHandles {
            fill: id,
            at,
            origin: screen,
            angle: if flipped { screen - arm } else { screen + arm },
            degrees,
            flipped,
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
        if let Some(grip) = over {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            response.clone().on_hover_text_at_pointer(match grip {
                HatchGrip::Origin => {
                    "Pattern origin: drag to move the pattern (it snaps to points)"
                }
                HatchGrip::Angle => {
                    "Pattern angle: drag to turn the pattern (in 15° steps; Alt: freely)"
                }
            });
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
                let mut degrees = v.y.atan2(v.x).to_degrees();
                if h.flipped {
                    degrees += 180.0;
                }
                hatch.angle = if snap {
                    (degrees / ANGLE_STEP).round() * ANGLE_STEP
                } else {
                    (degrees * 10.0).round() / 10.0
                };
                hatch.angle = (hatch.angle + 180.0).rem_euclid(360.0) - 180.0;
                if hatch.angle <= -180.0 {
                    hatch.angle += 360.0;
                }
            }
        }
        self.editor.doc.set_fill(id, fill);
    }

    /// The Fill section's Pattern alignment rows for one hatch: its origin (typed, or put on
    /// a corner or the centre of the area), its angle (typed, or along one of the outline's
    /// edges), and Reset.
    pub(crate) fn pattern_alignment(&mut self, ui: &mut egui::Ui, id: FillId) {
        const UNITS: &str = "units and variables work, e.g. 2in or =width/2";
        let Some(FillKind::Hatch(h)) = self.editor.doc.fill(id).map(|f| f.kind.clone()) else {
            return;
        };
        let units = self.editor.doc.units();
        let bounds = self.editor.fill_shapes().get(&id).map(|s| s.region.bounds);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Pattern alignment")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        let mut origin = None;
        let mut angle = None;
        let mut typed = Vec::new();
        theme::row(ui, "Origin", |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (axis, name, v) in [(0, "X", h.origin.x), (1, "Y", h.origin.y)] {
                let key = egui::Id::new(("hatch_origin", id.0, axis));
                let tip = format!("Pattern origin {name}: type a coordinate ({UNITS})");
                if let Some(text) =
                    app::panels::value_field(ui, key, &units.format_length(v), 84.0, &tip)
                {
                    typed.push((Some(axis), text));
                }
            }
        });
        if let Some(b) = bounds {
            theme::row(ui, "", |ui| {
                if let Some(at) = anchor_grid(ui, &b) {
                    origin = Some(at);
                }
                ui.label(
                    egui::RichText::new("Origin on a corner or the centre")
                        .small()
                        .weak(),
                );
            });
        }
        let mut edge = false;
        theme::row(ui, "Angle", |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let key = egui::Id::new(("hatch_angle", id.0));
            let tip = "Pattern angle: type an angle (anticlockwise from +X)";
            if let Some(text) =
                app::panels::value_field(ui, key, &units.format_angle(h.angle), 84.0, tip)
            {
                typed.push((None, text));
            }
            let waiting = self.align_edge == Some(id);
            if ui
                .add(egui::Button::new("Align to edge…").selected(waiting))
                .on_hover_text(
                    "Click one of the shape's edges: the pattern runs along it, its origin on \
                     the edge's start (as courses of brick or tile are set out along a wall)",
                )
                .clicked()
            {
                edge = true;
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(theme::LABEL_WIDTH + ui.spacing().item_spacing.x);
            if ui
                .button("Reset")
                .on_hover_text("Back to the drawing's origin and 0°")
                .clicked()
            {
                origin = Some(DVec2::ZERO);
                angle = Some(0.0);
            }
        });
        ui.label(
            egui::RichText::new(
                "On the canvas, drag the square handle to move the pattern (it snaps to points) \
                 and the round one to turn it. Alt-click or double-click inside a shape to pick \
                 its fill alone.",
            )
            .small()
            .weak(),
        );
        for (axis, text) in typed {
            let kind = match axis {
                Some(_) => drift_core::units::Kind::Length,
                None => drift_core::units::Kind::Angle,
            };
            match self.editor.evaluate_text(&text, kind) {
                Ok(v) => match axis {
                    Some(axis) => {
                        let mut o = origin.unwrap_or(h.origin);
                        o[axis] = v.value;
                        origin = Some(o);
                    }
                    None => angle = Some(v.value),
                },
                Err(e) => self.editor.out.notice = Some(format!("Can't use {text}: {e}")),
            }
        }
        if edge {
            self.align_edge = if self.align_edge == Some(id) {
                None
            } else {
                Some(id)
            };
        }
        if origin.is_some() || angle.is_some() {
            self.editor.align_hatch(id, origin, angle);
        }
    }

    /// While Align to edge… waits: the outline curve under the pointer lights up, and a click
    /// on it aligns the pattern. Returns true if the pointer input was used here.
    pub(crate) fn align_edge_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        pos: Option<egui::Pos2>,
    ) -> bool {
        let Some(id) = self.align_edge else {
            return false;
        };
        // Its hatch no longer picked (or another tool taken up): it's over.
        if self.tool.kind() != ToolKind::Select || self.aligned_hatch() != Some(id) {
            self.align_edge = None;
            return false;
        }
        let Some(boundary) = self.editor.doc.fill(id).map(|f| f.boundary.clone()) else {
            self.align_edge = None;
            return false;
        };
        let tolerance = PICK_RADIUS / self.camera.scale;
        let curve_at = |app: &Self, p: egui::Pos2| {
            app.editor
                .index
                .pick(map.to_world(p), tolerance, |c| boundary.contains(&c))
        };
        let over = pos.and_then(|p| curve_at(self, p));
        self.editor.hovered = over;
        self.editor.hovered_fill = None;
        ui.ctx().set_cursor_icon(if over.is_some() {
            egui::CursorIcon::PointingHand
        } else {
            egui::CursorIcon::Crosshair
        });
        if response.clicked_by(PointerButton::Primary) {
            let at = release_pos(ui, PointerButton::Primary).or(pos);
            match at.and_then(|p| curve_at(self, p)) {
                Some(curve) => {
                    self.editor.align_hatch_to_edge(id, curve);
                    self.align_edge = None;
                }
                None => {
                    self.editor.out.notice = Some("Click one of the shape's own edges".into());
                }
            }
        }
        // Nothing else takes the pointer meanwhile (a drag doesn't box-select).
        response.clicked() || response.drag_started() || response.dragged() || over.is_some()
    }

    pub(crate) fn paint_hatch_handles(
        &mut self,
        painter: &egui::Painter,
        map: &ScreenMap,
        h: &HatchHandles,
        colors: &OverlayColors,
    ) {
        let color = handle_color(colors.dark);
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

/// A small square with the area's corners and centre: a click on one gives that point.
fn anchor_grid(ui: &mut egui::Ui, b: &Bounds) -> Option<DVec2> {
    const SIZE: f32 = 34.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    let color = ui.visuals().weak_text_color();
    let inner = rect.shrink(5.0);
    ui.painter().rect_stroke(
        inner,
        1.0,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Middle,
    );
    let spots = [
        ("Bottom left", egui::vec2(0.0, 1.0), b.min),
        (
            "Bottom right",
            egui::vec2(1.0, 1.0),
            DVec2::new(b.max.x, b.min.y),
        ),
        (
            "Top left",
            egui::vec2(0.0, 0.0),
            DVec2::new(b.min.x, b.max.y),
        ),
        ("Top right", egui::vec2(1.0, 0.0), b.max),
        ("Centre", egui::vec2(0.5, 0.5), (b.min + b.max) / 2.0),
    ];
    let mut picked = None;
    for (name, f, at) in spots {
        let c = inner.min + inner.size() * f;
        let spot = egui::Rect::from_center_size(c, egui::vec2(10.0, 10.0));
        let response = ui
            .interact(spot, ui.id().with(("anchor", name)), egui::Sense::click())
            .on_hover_text(format!("Pattern origin on the {}", name.to_lowercase()));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Origin {name}"))
        });
        let fill = if response.hovered() {
            handle_color(ui.visuals().dark_mode)
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        ui.painter().circle_filled(c, 3.5, fill);
        ui.painter()
            .circle_stroke(c, 3.5, egui::Stroke::new(1.0, color));
        if response.clicked() {
            picked = Some(at);
        }
    }
    picked
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
