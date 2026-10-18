//! Views on the canvas: their crops and names (Revit's crop regions), which never print. A
//! crop is picked by its edge or name (geometry under the pointer wins), dragged by its edge
//! and reframed by its grips; View → View crops shows or hides them all, a view's own Crop
//! switch hides it alone.

use drift_core::sheets::scale_label;
use drift_core::{Bounds, DVec2, View, ViewId};

use crate::overlay::ScreenMap;
use crate::*;

/// How near (points) a grip, and a crop's edge, count.
const GRIP_REACH: f32 = 8.0;
const EDGE_REACH: f32 = 5.0;
/// The smallest a crop can be made, in paper mm at its view's scale.
const MIN_CROP: f64 = 10.0;

/// What a drag on a crop does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CropGrip {
    /// Move the crop over the model.
    Move,
    /// Move an edge or corner: which way each axis' edge goes (-1 the lower, 1 the upper, 0
    /// neither).
    Resize(i8, i8),
}

/// A drag on a crop in progress: the view as it was, the world point it started at, and
/// what the dragged edge or corner snapped to.
#[derive(Clone, Debug)]
pub(crate) struct CropDrag {
    view: ViewId,
    grip: CropGrip,
    was: View,
    from: DVec2,
    snap: Option<drift_geom::Snap>,
}

/// The points of `crop` a drag by `grip` carries: the dragged corner, or the middle of the
/// dragged edge (only its own axis counts); for a move, the corners.
fn dragged_points(crop: Bounds, grip: CropGrip) -> Vec<DVec2> {
    let pick = |d: i8, lo: f64, hi: f64| match d {
        -1 => lo,
        1 => hi,
        _ => (lo + hi) * 0.5,
    };
    match grip {
        CropGrip::Move => vec![
            crop.min,
            DVec2::new(crop.max.x, crop.min.y),
            crop.max,
            DVec2::new(crop.min.x, crop.max.y),
        ],
        CropGrip::Resize(dx, dy) => vec![DVec2::new(
            pick(dx, crop.min.x, crop.max.x),
            pick(dy, crop.min.y, crop.max.y),
        )],
    }
}

/// `was` after a drag of `d` (world mm) by `grip`.
pub(crate) fn dragged_crop(was: &View, grip: CropGrip, d: DVec2) -> View {
    let mut v = was.clone();
    match grip {
        CropGrip::Move => v.center = was.center + d,
        CropGrip::Resize(dx, dy) => {
            let b = was.crop();
            let (mut min, mut max) = (b.min, b.max);
            let least = MIN_CROP * was.scale;
            // The dragged edges follow the pointer, never past the opposite ones.
            match dx {
                -1 => min.x = (min.x + d.x).min(max.x - least),
                1 => max.x = (max.x + d.x).max(min.x + least),
                _ => {}
            }
            match dy {
                -1 => min.y = (min.y + d.y).min(max.y - least),
                1 => max.y = (max.y + d.y).max(min.y + least),
                _ => {}
            }
            v.center = (min + max) * 0.5;
            v.size = max - min;
        }
    }
    v
}

/// The colour of crops that aren't picked.
fn crop_colour(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(0xa8, 0x8c, 0xff)
    } else {
        egui::Color32::from_rgb(0x6a, 0x4c, 0xd8)
    }
}

impl DriftApp {
    /// Whether a view's crop is on the canvas: shown, or picked (from the Views list, say).
    pub(crate) fn crop_shown(&self, id: ViewId, view: &View) -> bool {
        self.picked_view == Some(id) || (self.show_view_crops && view.crop_visible)
    }

    /// A view's crop on screen.
    fn crop_rect(map: &ScreenMap, view: &View) -> egui::Rect {
        let b = view.crop();
        egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max))
    }

    /// Where a view's name goes: over its crop's top-left corner.
    fn crop_label(painter: &egui::Painter, rect: egui::Rect, view: &View) -> egui::Rect {
        let text = format!("{} · {}", view.title(), scale_label(view.scale));
        let galley =
            painter.layout_no_wrap(text, egui::FontId::proportional(12.0), egui::Color32::WHITE);
        egui::Rect::from_min_size(
            rect.left_top() - egui::vec2(0.0, galley.size().y + 3.0),
            galley.size(),
        )
    }

    /// What a press at `pos` would take hold of: a grip of the picked view's crop, else the
    /// edge or name of a crop on the canvas (the picked one first).
    pub(crate) fn crop_grip_at(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
        pos: egui::Pos2,
    ) -> Option<(ViewId, CropGrip)> {
        let doc = &self.editor.doc;
        if let Some((id, view)) = self
            .picked_view
            .and_then(|id| doc.view(id).map(|v| (id, v)))
        {
            let rect = Self::crop_rect(map, view);
            if let Some((_, (dx, dy))) = Self::view_handles(rect)
                .into_iter()
                .find(|(at, _)| at.distance(pos) <= GRIP_REACH)
            {
                return Some((id, CropGrip::Resize(dx, dy)));
            }
        }
        let on_edge = |rect: egui::Rect| {
            rect.expand(EDGE_REACH).contains(pos) && !rect.shrink(EDGE_REACH).contains(pos)
        };
        let mut shown: Vec<(ViewId, &View)> = doc
            .views()
            .filter(|(id, v)| self.crop_shown(*id, v))
            .collect();
        // The picked one first, then the last drawn (on top).
        shown.sort_by_key(|(id, _)| self.picked_view != Some(*id));
        shown
            .into_iter()
            .find(|(_, v)| {
                let rect = Self::crop_rect(map, v);
                on_edge(rect) || Self::crop_label(painter, rect, v).contains(pos)
            })
            .map(|(id, _)| (id, CropGrip::Move))
    }

    /// Pick, move and reframe crops on the canvas with the select tool. Returns true if it
    /// took this frame's pointer input.
    pub(crate) fn view_crop_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        pos: Option<egui::Pos2>,
        snap: bool,
    ) -> bool {
        if let Some(drag) = self.crop_drag.clone() {
            // The crop has the pointer: nothing under it is hovered meanwhile.
            self.editor.hovered = None;
            if let Some(p) = response.interact_pointer_pos() {
                let (d, found) = self.snap_crop_drag(&drag, map.to_world(p) - drag.from, snap);
                if let Some(c) = &mut self.crop_drag {
                    c.snap = found;
                }
                let moved = dragged_crop(&drag.was, drag.grip, d);
                self.editor.doc.set_view(drag.view, moved);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.crop_drag = None;
                self.editor.commit(match drag.grip {
                    CropGrip::Move => "Move view crop",
                    CropGrip::Resize(..) => "Reframe view",
                });
            }
            return true;
        }
        if self.tool.kind() != ToolKind::Select || self.tool.is_busy() {
            return false;
        }
        let painter = ui.painter_at(map.rect);
        // Geometry under the pointer wins over a crop's edge (a crop's grips win over it).
        let wins =
            |grip: CropGrip, geometry: bool| matches!(grip, CropGrip::Resize(..)) || !geometry;
        let geometry = self.editor.hovered.is_some();
        if response.drag_started_by(PointerButton::Primary)
            && let Some(from) = ui.input(|i| i.pointer.press_origin())
            && let Some((view, grip)) = self.crop_grip_at(&painter, map, from)
            && wins(grip, geometry)
            && let Some(was) = self.editor.doc.view(view).cloned()
        {
            self.picked_view = Some(view);
            self.crop_drag = Some(CropDrag {
                view,
                grip,
                was,
                from: map.to_world(from),
                snap: None,
            });
            self.editor.doc.begin();
            return true;
        }
        let under = pos.and_then(|p| self.crop_grip_at(&painter, map, p));
        if response.clicked_by(PointerButton::Primary) {
            if let Some((view, grip)) = under
                && wins(grip, geometry)
            {
                self.pick_view_alone(view);
                return true;
            }
            // A click anywhere else lets the view go.
            self.picked_view = None;
        }
        if response.hovered()
            && let Some((_, grip)) = under
            && wins(grip, geometry)
        {
            ui.ctx().set_cursor_icon(match grip {
                CropGrip::Resize(0, _) => egui::CursorIcon::ResizeVertical,
                CropGrip::Resize(_, 0) => egui::CursorIcon::ResizeHorizontal,
                CropGrip::Resize(dx, dy) if dx == dy => egui::CursorIcon::ResizeNeSw,
                CropGrip::Resize(..) => egui::CursorIcon::ResizeNwSe,
                CropGrip::Move => egui::CursorIcon::Move,
            });
        }
        false
    }

    /// A crop dragged by `d` (world mm), snapped: the dragged edge or corner (for a move,
    /// whichever corner is nearest a snap) onto points, curves and the grid, as anything
    /// placed does. Returns the drag to make, and the snap it found. Alt drags freely.
    fn snap_crop_drag(
        &self,
        drag: &CropDrag,
        d: DVec2,
        snap: bool,
    ) -> (DVec2, Option<drift_geom::Snap>) {
        if !snap {
            return (d, None);
        }
        let options = drift_geom::SnapOptions {
            tolerance: PICK_RADIUS / self.camera.scale,
            grid: self
                .grid_snap
                .then(|| drift_render::grid_info(&self.camera, self.imperial()).minor),
        };
        let best = dragged_points(drag.was.crop(), drag.grip)
            .into_iter()
            .filter_map(|p| {
                let at = p + d;
                let s = self.editor.snap(at, options, |_| false)?;
                Some(((s.at - at).length(), s, s.at - at))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((_, s, nudge)) => (d + nudge, Some(s)),
            None => (d, None),
        }
    }

    /// The crops and names of the views on the canvas, and the picked one's grips.
    pub(crate) fn paint_view_crops(&self, painter: &egui::Painter, map: &ScreenMap, dark: bool) {
        let accent = theme::accent(dark);
        let colour = crop_colour(dark);
        let hover = (self.tool.kind() == ToolKind::Select)
            .then(|| painter.ctx().input(|i| i.pointer.hover_pos()))
            .flatten()
            .filter(|p| map.rect.contains(*p))
            .and_then(|p| self.crop_grip_at(painter, map, p))
            .map(|(id, _)| id);
        for (id, view) in self.editor.doc.views() {
            if !self.crop_shown(id, view) {
                continue;
            }
            let picked = self.picked_view == Some(id);
            let rect = Self::crop_rect(map, view);
            if !rect.expand(40.0).intersects(map.rect) {
                continue;
            }
            // A picked view whose crop is hidden shows faintly.
            let faint = !(self.show_view_crops && view.crop_visible);
            let mut c = if picked { accent } else { colour };
            if faint {
                c = c.gamma_multiply(0.55);
            } else if !picked && hover != Some(id) {
                c = c.gamma_multiply(0.8);
            }
            let width = if picked || hover == Some(id) {
                1.5
            } else {
                1.0
            };
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &corners,
                egui::Stroke::new(width, c),
                8.0,
                5.0,
            ));
            let label = Self::crop_label(painter, rect, view);
            // On a patch of the canvas's colour, to read over hatching.
            let ground = if dark {
                egui::Color32::from_rgba_unmultiplied(0x1c, 0x1d, 0x21, 225)
            } else {
                egui::Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 225)
            };
            painter.rect_filled(label.expand2(egui::vec2(3.0, 1.0)), 3.0, ground);
            painter.text(
                label.min,
                egui::Align2::LEFT_TOP,
                format!("{} · {}", view.title(), scale_label(view.scale)),
                egui::FontId::proportional(12.0),
                c,
            );
            if picked {
                for (at, _) in Self::view_handles(rect) {
                    let knob = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
                    painter.rect_filled(knob, 1.0, egui::Color32::WHITE);
                    painter.rect_stroke(
                        knob,
                        1.0,
                        egui::Stroke::new(1.5, accent),
                        egui::StrokeKind::Middle,
                    );
                }
            }
        }
        // What a dragged crop snapped to, over its grips.
        if let Some(s) = self.crop_drag.as_ref().and_then(|d| d.snap.as_ref()) {
            let colour = crate::overlay::OverlayColors::new(dark).snap;
            crate::overlay::snap_marker(painter, map, s, colour);
        }
    }

    /// Pick a view on the canvas, and nothing else (its bar shows in the context bar).
    pub(crate) fn pick_view_alone(&mut self, view: ViewId) {
        self.picked_view = Some(view);
        self.editor.selection.clear();
        self.editor.selected_fills.clear();
        self.editor.selected_instances.clear();
        self.editor.selected_annotation = None;
        self.selected_constraint = None;
    }

    /// Change a view (from the Views list or the context bar). Typing into its name merges
    /// into one undo step.
    pub(crate) fn update_view(&mut self, id: ViewId, view: View, typing: bool) {
        if self.editor.doc.view(id) == Some(&view) {
            return;
        }
        let label = if typing {
            "Edit view text"
        } else {
            "Edit view"
        };
        self.editor.doc.begin();
        self.editor.doc.set_view(id, view);
        let tx = self.editor.doc.commit(label);
        if typing && self.editor.history.undo_label() == Some(label) {
            self.editor.history.merge_into_last(tx);
        } else {
            self.editor.history.push(tx);
        }
        self.editor.sync();
    }

    /// The context bar for a view picked on the canvas: its name and scale, whether its crop
    /// shows, and where to place it.
    pub(crate) fn view_actions(&mut self, ui: &mut egui::Ui, id: ViewId) {
        let Some(original) = self.editor.doc.view(id).cloned() else {
            return;
        };
        let mut view = original.clone();
        let mut typing = false;
        ui.label(egui::RichText::new("View").weak());
        typing |= ui
            .add(
                egui::TextEdit::singleline(&mut view.name)
                    .hint_text("Name")
                    .desired_width(120.0),
            )
            .changed();
        self.scale_menu(ui, "bar_view_scale", &mut view.scale);
        ui.checkbox(&mut view.crop_visible, "Crop")
            .on_hover_text("Show its crop and name on the canvas (they never print)");
        let sheets = self.editor.doc.sheets_showing(id).len();
        let placed = match sheets {
            0 => "Not on a sheet".to_owned(),
            1 => "On 1 sheet".to_owned(),
            n => format!("On {n} sheets"),
        };
        ui.label(egui::RichText::new(placed).weak());
        if ui
            .button("Sheets…")
            .on_hover_text("Place it on a sheet, from the Sheets tab")
            .clicked()
        {
            self.show_panel(crate::dock::Panel::Sheets);
        }
        self.update_view(id, view, typing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_move_and_reframe_without_turning_inside_out() {
        let crop = drift_core::Bounds {
            min: DVec2::new(0.0, 0.0),
            max: DVec2::new(1000.0, 500.0),
        };
        let view = View::new("Plan", crop, 50.0);
        let moved = dragged_crop(&view, CropGrip::Move, DVec2::new(10.0, -20.0));
        assert_eq!(moved.center, DVec2::new(510.0, 230.0));
        assert_eq!(moved.size, view.size);
        // The right edge out by 200: the left stays put.
        let wider = dragged_crop(&view, CropGrip::Resize(1, 0), DVec2::new(200.0, 99.0));
        assert_eq!(wider.crop().min, crop.min);
        assert_eq!(wider.crop().max, DVec2::new(1200.0, 500.0));
        // Dragged past the opposite edge: it stops 10 paper mm (500 world mm at 1:50) short.
        let squashed = dragged_crop(&view, CropGrip::Resize(0, 1), DVec2::new(0.0, -2000.0));
        assert_eq!(squashed.crop().min.y, 0.0);
        assert_eq!(squashed.crop().max.y, 500.0);
        let corner = dragged_crop(&view, CropGrip::Resize(-1, -1), DVec2::new(-100.0, -50.0));
        assert_eq!(corner.crop().min, DVec2::new(-100.0, -50.0));
        assert_eq!(corner.crop().max, crop.max);
    }
}
