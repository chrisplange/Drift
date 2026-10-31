//! Temporary dimensions on the canvas (as in Revit): drawn dashed by the picked curve. Click
//! the value to type a new one (the picked curve moves to suit), the lock beside it to make
//! it a real dimension, and drag the square at its far end onto other geometry to measure to
//! that instead.

use crate::annotations::{AnnotationColors, AnnotationState, DimLayout, Pen};
use crate::*;
use drift_edit::temp_dims::TempDim;

/// How far beside the picked line its length shows (points).
const LENGTH_OUT_PT: f64 = 26.0;
/// The far end's square: drawn this size, and picked this close (points).
const HANDLE_PT: f32 = 7.0;

/// The part of a temporary dimension under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TempPart {
    Value,
    Lock,
    End,
}

/// The temporary dimensions shown: kept while the same curve stays picked, so a far end
/// moved onto other geometry stays there.
#[derive(Default)]
pub(crate) struct TempDims {
    pub(crate) subject: Option<drift_core::EntityId>,
    pub(crate) dims: Vec<TempDim>,
    /// The one whose value is being typed: its index, the text, and whether to focus it.
    pub(crate) edit: Option<(usize, String, bool)>,
    /// The one whose far end is being dragged.
    drag: Option<usize>,
    hover: Option<(usize, TempPart)>,
    /// Where each one's value showed last frame (screen; by index).
    pub(crate) values: Vec<(usize, egui::Pos2)>,
}

/// A temporary dimension as laid out this frame.
pub(crate) struct TempLayout {
    index: usize,
    dim: DimLayout,
    /// Its label's offset from its anchor (world), for making it permanent where it shows.
    placement: Option<DVec2>,
    lock: egui::Rect,
    end: Option<egui::Pos2>,
}

/// Draws dimension lines dashed.
struct Dashed<'a>(&'a egui::Painter);

impl Pen for Dashed<'_> {
    fn painter(&self) -> &egui::Painter {
        self.0
    }

    fn line(&self, points: &[egui::Pos2], stroke: egui::Stroke) {
        crate::overlay::dashed(self.0, points, stroke, 4.0, 3.0);
    }

    fn polygon(&self, points: Vec<egui::Pos2>, color: egui::Color32) {
        Pen::polygon(self.0, points, color);
    }

    fn dot(&self, at: egui::Pos2, radius: f32, color: egui::Color32) {
        Pen::dot(self.0, at, radius, color);
    }

    fn mask(&self, points: Vec<egui::Pos2>, color: egui::Color32) {
        Pen::mask(self.0, points, color);
    }

    fn text(
        &self,
        top_left: egui::Pos2,
        galley: std::sync::Arc<egui::Galley>,
        angle: f32,
        color: egui::Color32,
    ) {
        Pen::text(self.0, top_left, galley, angle, color);
    }

    fn highlight(&self, rect: egui::Rect, color: egui::Color32) {
        Pen::highlight(self.0, rect, color);
    }
}

/// An open padlock: click to lock the dimension in (make it permanent).
fn paint_lock(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, rect.center().y),
        egui::pos2(rect.right() - 1.0, rect.bottom()),
    );
    painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Inside);
    // The shackle, open: raised on its left leg.
    let (r, cx) = (body.width() * 0.3, body.center().x);
    let top = rect.top() + r;
    let arc: Vec<egui::Pos2> = (0..=8)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 8.0);
            egui::pos2(cx - r * a.cos(), top - r * a.sin() + 1.0)
        })
        .collect();
    painter.add(egui::Shape::line(arc, stroke));
    painter.line_segment(
        [
            egui::pos2(cx - r, top + 1.0),
            egui::pos2(cx - r, body.top()),
        ],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx + r, top + 1.0), egui::pos2(cx + r, top + 3.0)],
        stroke,
    );
}

impl DriftApp {
    /// Lay out the temporary dimensions for this frame, refreshing them when what's picked
    /// changes. None unless the preference is on and one curve is picked with the select
    /// tool at rest.
    pub(crate) fn layout_temp_dims(
        &mut self,
        painter: &egui::Painter,
        map: &ScreenMap,
    ) -> Vec<TempLayout> {
        let idle = self.show_temp_dims
            && self.tool.kind() == ToolKind::Select
            && !self.tool.is_busy()
            && self.drawing.label_drag.is_none();
        let subject = if idle {
            self.drawing.editor.temp_dims_subject()
        } else {
            None
        };
        if subject != self.drawing.temp_dims.subject {
            self.drawing.temp_dims = TempDims {
                subject,
                dims: if subject.is_some() {
                    self.drawing.editor.temp_dims()
                } else {
                    Vec::new()
                },
                ..Default::default()
            };
        }
        self.drawing.temp_dims.values.clear();
        let doc = &self.drawing.editor.doc;
        let out = LENGTH_OUT_PT / map.camera.scale;
        let mut layouts = Vec::new();
        for (index, t) in self.drawing.temp_dims.dims.iter().enumerate() {
            let Some(c) = t.measured(&self.drawing.editor) else {
                continue;
            };
            let placement = drift_edit::temp_dims::label_spot(&self.drawing.editor, t, out)
                .zip(drift_edit::labels::dimension_anchor(doc, &c))
                .map(|(spot, anchor)| spot - anchor);
            let Some(dim) = annotations::layout_dimension(painter, doc, None, &c, placement, map)
            else {
                continue;
            };
            let lock = egui::Rect::from_center_size(
                dim.text_rect.right_center() + egui::vec2(10.0, 0.0),
                egui::vec2(10.0, 12.0),
            );
            let end = t
                .reference_point(&self.drawing.editor)
                .map(|p| map.to_screen(p));
            self.drawing
                .temp_dims
                .values
                .push((index, dim.text_rect.center()));
            layouts.push(TempLayout {
                index,
                dim,
                placement,
                lock,
                end,
            });
        }
        layouts
    }

    pub(crate) fn paint_temp_dims(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
        layouts: &[TempLayout],
        dark: bool,
    ) {
        let colors = AnnotationColors::drafting(dark, self.drawing.editor.doc.draft_style().color);
        let colors = AnnotationColors {
            normal: colors.normal.gamma_multiply(0.8),
            ..colors
        };
        let pen = Dashed(painter);
        let hover = self.drawing.temp_dims.hover;
        for l in layouts {
            let editing = self
                .drawing
                .temp_dims
                .edit
                .as_ref()
                .is_some_and(|e| e.0 == l.index);
            let state = if hover == Some((l.index, TempPart::Value)) || editing {
                AnnotationState::Hovered
            } else {
                AnnotationState::Normal
            };
            annotations::paint_dimension(&pen, &l.dim, state, &colors);
            let lock = if hover == Some((l.index, TempPart::Lock)) {
                colors.hovered
            } else {
                colors.normal
            };
            paint_lock(painter, l.lock, lock);
            if let Some(end) = l.end {
                let color = if hover == Some((l.index, TempPart::End))
                    || self.drawing.temp_dims.drag == Some(l.index)
                {
                    colors.hovered
                } else {
                    colors.normal
                };
                let square = egui::Rect::from_center_size(end, egui::Vec2::splat(HANDLE_PT));
                painter.rect_filled(square, 1.0, colors.text_background);
                painter.rect_stroke(
                    square,
                    1.0,
                    egui::Stroke::new(1.2, color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // A far end being dragged: a line to where it would go.
        if let Some(i) = self.drawing.temp_dims.drag
            && let Some(from) = layouts.iter().find(|l| l.index == i).and_then(|l| l.end)
            && let Some(to) = self.drawing.view.pointer.map(|p| map.to_screen(p.raw))
        {
            crate::overlay::dashed(
                painter,
                &[from, to],
                egui::Stroke::new(1.0, colors.hovered),
                4.0,
                3.0,
            );
        }
    }

    /// Pointer input on temporary dimensions; true if it took this frame's input.
    pub(crate) fn temp_dim_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        layouts: &[TempLayout],
        pos: Option<egui::Pos2>,
    ) -> bool {
        let value_at = |p: egui::Pos2| {
            layouts
                .iter()
                .find(|l| l.dim.text_rect.expand(annotations::TEXT_REACH).contains(p))
        };
        let lock_at = |p: egui::Pos2| layouts.iter().find(|l| l.lock.expand(3.0).contains(p));
        let end_at = |p: egui::Pos2| {
            layouts
                .iter()
                .find(|l| l.end.is_some_and(|e| e.distance(p) <= HANDLE_PT))
        };
        let part_at = |p: egui::Pos2| {
            lock_at(p)
                .map(|l| (l.index, TempPart::Lock))
                .or_else(|| end_at(p).map(|l| (l.index, TempPart::End)))
                .or_else(|| value_at(p).map(|l| (l.index, TempPart::Value)))
        };
        self.drawing.temp_dims.hover = pos.and_then(part_at);
        match self.drawing.temp_dims.hover.map(|h| h.1) {
            Some(TempPart::Value) => ui.ctx().set_cursor_icon(egui::CursorIcon::Text),
            Some(TempPart::Lock) => ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand),
            Some(TempPart::End) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None if self.drawing.temp_dims.drag.is_some() => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            None => {}
        }
        let now = ui.input(|i| i.time);

        // Dragging a far end onto other geometry.
        if self.drawing.temp_dims.drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(l) = end_at(origin)
        {
            self.drawing.temp_dims.drag = Some(l.index);
        }
        if let Some(i) = self.drawing.temp_dims.drag {
            if !response.dragged_by(PointerButton::Primary) {
                self.drawing.temp_dims.drag = None;
                let dropped = release_pos(ui, PointerButton::Primary).or(pos);
                let target = dropped.and_then(|at| {
                    let p = self.make_pointer(map, at, true);
                    drift_edit::tools::reference_at(&self.drawing.editor, &p)
                });
                let moved = target.and_then(|id| {
                    let t = self.drawing.temp_dims.dims.get(i)?;
                    self.drawing.editor.retarget_temp_dim(t, id)
                });
                match moved {
                    Some(t) => self.drawing.temp_dims.dims[i] = t,
                    None => self.set_status(
                        "Drop the end on a point, or on a line beside the picked one, to measure \
                         to it"
                            .into(),
                        true,
                        now,
                    ),
                }
            }
            return true;
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(at) = release_pos(ui, PointerButton::Primary).or(pos)
            && let Some((index, part)) = part_at(at)
            && let Some(l) = layouts.iter().find(|l| l.index == index)
        {
            match part {
                TempPart::Lock => self.keep_temp_dim(index, l.placement, now),
                TempPart::Value | TempPart::End => {
                    if let Some(value) = l.dim.constraint.value() {
                        let text = self.drawing.editor.doc.units().format_length(value);
                        self.drawing.temp_dims.edit = Some((index, text, true));
                    }
                }
            }
            return true;
        }
        false
    }

    /// Make a temporary dimension a real one, where it shows.
    fn keep_temp_dim(&mut self, index: usize, placement: Option<DVec2>, now: f64) {
        let Some(t) = self.drawing.temp_dims.dims.get(index).copied() else {
            return;
        };
        match self.drawing.editor.make_temp_dim_permanent(&t, placement) {
            Ok((_, driven)) => {
                self.drawing.temp_dims.dims.remove(index);
                self.drawing.temp_dims.edit = None;
                let text = if driven {
                    "Added as a driven (reference) dimension: the sketch already holds this"
                } else {
                    "Dimension added"
                };
                self.set_status(text.into(), false, now);
            }
            Err(e) => self.set_status(format!("Can't add it: {e}"), true, now),
        }
    }

    /// The value editor of a temporary dimension: Enter moves the picked curve to suit.
    pub(crate) fn temp_dim_editor(
        &mut self,
        ctx: &egui::Context,
        layouts: &[TempLayout],
        now: f64,
    ) {
        let Some((index, mut text, focus)) = self.drawing.temp_dims.edit.take() else {
            return;
        };
        let Some(at) = layouts
            .iter()
            .find(|l| l.index == index)
            .map(|l| l.dim.text_rect.center())
        else {
            return;
        };
        let field = egui::Id::new("temp_dim_edit_field");
        let area = egui::Area::new(egui::Id::new("temp_dim_edit"))
            .fixed_pos(at - egui::vec2(45.0, 11.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(field)
                        .desired_width(90.0)
                        .horizontal_align(egui::Align::Center),
                );
                if focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, field) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(text.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ctx, field);
                    }
                }
            });
        let (enter, escape, outside) = ctx.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !area.response.rect.contains(p)),
            )
        });
        if !focus && enter {
            let t = self.drawing.temp_dims.dims.get(index).copied();
            let result = self
                .drawing
                .editor
                .evaluate_text(&text, drift_core::Kind::Length)
                .map_err(|e| e.to_string())
                .and_then(|v| match t {
                    Some(t) => self.drawing.editor.set_temp_dim(&t, v.value),
                    None => Ok(()),
                });
            if let Err(e) = result {
                self.set_status(format!("Can't use {text}: {e}"), true, now);
            }
            return;
        }
        if focus || !(escape || outside) {
            self.drawing.temp_dims.edit = Some((index, text, false));
        }
    }
}
