//! The canvas: pointer input, camera, and painting geometry and overlays.

use crate::*;

/// How near a press counts as on an extension line's grip (points).
const EXTENSION_GRIP_REACH: f32 = 6.0;

impl DriftApp {
    pub(crate) fn make_pointer(&self, map: &ScreenMap, pos: egui::Pos2, snap: bool) -> Pointer {
        let raw = map.to_world(pos);
        let mut pointer = Pointer::new(raw, PICK_RADIUS / self.drawing.view.camera.scale);
        pointer.relations = self.auto_relations;
        pointer.units = self.drawing.editor.doc.units();
        pointer.alt = !snap;
        (pointer.shift, pointer.command) = self.drawing.view.keys_held;
        pointer.round_scale = self.round_scale;
        pointer.angle_step = self.angle_snap.filter(|_| snap);
        pointer.perp_tangent = self.perp_tangent_snap;
        if snap && self.tool.wants_snap() {
            let options = SnapOptions {
                tolerance: pointer.tolerance,
                grid: self
                    .grid_snap
                    .then(|| grid_info(&self.drawing.view.camera, self.imperial()).minor),
                from: self
                    .perp_tangent_snap
                    .then(|| self.tool.snap_from())
                    .flatten(),
            };
            let ed = &self.drawing.editor;
            let tool = &self.tool;
            pointer.snap = ed.snap_excluding(
                raw,
                options,
                |id| tool.snap_excludes(ed, id),
                |copy| tool.snap_excludes_copy(copy),
            );
            // Inference lines apply unless an exact snap decides the position; with grid
            // snapping they start from the grid point, so both coordinates stay on the grid.
            if pointer.exact_snap().is_none() {
                let from = pointer.snap.map_or(raw, |grid| grid.at);
                pointer.guide = self.inference(from, pointer.tolerance);
            }
        }
        if snap {
            self.tool.refine_pointer(&mut pointer);
        }
        pointer
    }

    /// Align `at` horizontally / vertically with recently snapped points (inference lines).
    pub(crate) fn inference(&self, at: DVec2, tol: f64) -> Option<Guide> {
        let mut x_ref: Option<(f64, DVec2)> = None;
        let mut y_ref: Option<(f64, DVec2)> = None;
        for &q in &self.drawing.inference_points {
            if (at - q).length() <= tol {
                continue;
            }
            let (dx, dy) = ((at.x - q.x).abs(), (at.y - q.y).abs());
            if dx <= tol && x_ref.is_none_or(|(d, _)| dx < d) {
                x_ref = Some((dx, q));
            }
            if dy <= tol && y_ref.is_none_or(|(d, _)| dy < d) {
                y_ref = Some((dy, q));
            }
        }
        if x_ref.is_none() && y_ref.is_none() {
            return None;
        }
        Some(Guide {
            at: DVec2::new(
                x_ref.map_or(at.x, |(_, q)| q.x),
                y_ref.map_or(at.y, |(_, q)| q.y),
            ),
            kind: GuideKind::Aligned,
            from: [x_ref.map(|r| r.1), y_ref.map(|r| r.1)],
        })
    }

    pub(crate) fn layout_dimensions(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
    ) -> Vec<DimLayout> {
        let doc = &self.drawing.editor.doc;
        let mut out: Vec<DimLayout> = doc
            .constraints()
            .filter(|(id, c)| {
                c.is_dimension() && self.on_canvas(drift_core::AnnotationRef::Dimension(*id))
            })
            .filter_map(|(id, c)| {
                annotations::layout_dimension(painter, doc, Some(id), c, doc.placement(id), map)
            })
            .collect();
        // A picked opening's width and offset.
        if self.tool.kind() == ToolKind::Select {
            for o in self.drawing.editor.selected_openings() {
                for (c, at) in self.drawing.editor.opening_dimensions(o) {
                    out.extend(annotations::layout_dimension(
                        painter,
                        doc,
                        None,
                        &c,
                        Some(at),
                        map,
                    ));
                }
            }
        }
        // Walls' thickness dimensions (drawn as a dimension between the faces).
        if self.show_dimensions {
            for (q, d) in doc.property_dims() {
                if let Some((c, at)) = drift_edit::walls::thickness_dimension(doc, d) {
                    out.extend(
                        annotations::layout_dimension(painter, doc, None, &c, Some(at), map).map(
                            |mut l| {
                                l.property = Some(q);
                                l
                            },
                        ),
                    );
                }
            }
        }
        out
    }

    /// The dimensions and annotations in a selection box just made (as they are drawn).
    fn take_box_pick(
        &mut self,
        map: &ScreenMap,
        dims: &[DimLayout],
        notes: &[crate::notes::NoteLayout],
    ) {
        let Some(b) = self.drawing.editor.out.box_pick.take() else {
            return;
        };
        if !b.add {
            // (The tool cleared the editor's picks.)
            self.drawing.selected_constraint = None;
            self.drawing.selected_leader = None;
        }
        if !self.drawing.editor.prefs.pick_filter.annotations() {
            return;
        }
        let r = egui::Rect::from_two_pos(map.to_screen(b.rect.min), map.to_screen(b.rect.max));
        let found: Vec<drift_core::AnnotationRef> = dims
            .iter()
            .filter(|d| crate::picks::dimension_in_box(d, r, b.crossing))
            .filter_map(|d| d.id.map(drift_core::AnnotationRef::Dimension))
            .chain(
                notes
                    .iter()
                    .filter(|l| crate::picks::note_in_box(l, r, b.crossing))
                    .map(|l| drift_core::AnnotationRef::Annotation(l.id)),
            )
            .collect();
        for f in found {
            // The first becomes the primary pick, as with Shift+click.
            if !self.annotation_picked(f) {
                self.toggle_annotation_pick(f);
            }
        }
    }

    /// Hover, pick, drag and double-click dimensions and relation glyphs. Returns true if the
    /// pointer input was used here (so the tool shouldn't see it).
    pub(crate) fn annotation_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        dims: &[DimLayout],
        glyphs: &[annotations::GlyphLayout],
        pos: Option<egui::Pos2>,
    ) -> bool {
        let idle = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        // Dimensions can be picked and edited between dimensions too (the text is no use to
        // the dimension tool, which picks geometry).
        let annotating = idle || (self.tool.kind() == ToolKind::Dimension && !self.tool.is_busy());
        // The select tool's filter: dimensions with Annotations (or All), relations with the
        // geometry filters.
        let filter = self.drawing.editor.prefs.pick_filter;
        let select = self.tool.kind() == ToolKind::Select;
        let (dims_ok, glyphs_ok) = (
            !select || filter.annotations(),
            !select || filter.geometry(),
        );
        // Anywhere on a dimension's text, with a little to spare around it.
        let dim_at = |p: egui::Pos2| {
            dims.iter()
                .rev()
                .filter(|_| dims_ok)
                .find(|d| d.text_rect.expand(annotations::TEXT_REACH).contains(p))
        };
        let glyph_at = |p: egui::Pos2| {
            glyphs
                .iter()
                .rev()
                .filter(|_| glyphs_ok)
                .find(|g| g.rect.contains(p))
        };
        let hit = |p: egui::Pos2| {
            dim_at(p)
                .and_then(|d| d.id)
                .or_else(|| glyph_at(p).map(|g| g.id))
        };

        self.drawing.hovered_constraint = if annotating && self.drawing.label_drag.is_none() {
            pos.and_then(hit)
        } else {
            None
        };

        // The grips at the starts of the picked dimension's extension lines.
        let picked = self.drawing.selected_constraint;
        let grip_at = |p: egui::Pos2| -> Option<(ConstraintId, usize, DVec2, DVec2)> {
            let id = picked?;
            let dim = dims.iter().find(|d| d.id == Some(id))?;
            dim.shape
                .extension_grips()
                .into_iter()
                .find(|(_, at, ..)| at.distance(p) <= EXTENSION_GRIP_REACH)
                .map(|(end, _, from, out)| (id, end, from, out))
        };
        // Dragging one: the extension line starts where it is dropped (along its line).
        if idle
            && self.drawing.ext_drag.is_none()
            && self.drawing.label_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some((id, end, ..)) = grip_at(origin)
        {
            self.drawing.ext_drag = Some((id, end));
            self.drawing.editor.doc.begin();
        }
        if let Some((id, end)) = self.drawing.ext_drag {
            if let Some(p) = response.interact_pointer_pos()
                && let Some((_, _, from, out)) = dims
                    .iter()
                    .find(|d| d.id == Some(id))
                    .and_then(|d| d.shape.extension_grips().into_iter().find(|g| g.0 == end))
            {
                let t = (map.to_world(p) - from).dot(out);
                let _ = self
                    .drawing
                    .editor
                    .doc
                    .set_extension_start(id, end, Some(t));
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.drawing.ext_drag = None;
                self.drawing.editor.commit("Move extension line");
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return true;
        }
        if idle && pos.and_then(grip_at).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        // Dragging a dimension label.
        if idle
            && self.drawing.label_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(dim) = dim_at(origin)
            && let Some(id) = dim.id
        {
            // From where the number is drawn (it may have been centred, or moved outside a
            // tight dimension), so it doesn't jump as the drag begins.
            let label = map.to_world(dim.handle);
            self.drawing.label_drag = Some((id, map.to_world(origin) - label));
            self.drawing.selected_constraint = Some(id);
            self.drawing.editor.doc.begin();
        }
        if let Some((id, grab)) = self.drawing.label_drag {
            if let (Some(p), Some(c)) = (
                response.interact_pointer_pos(),
                self.drawing.editor.doc.constraint(id).copied(),
            ) && let Some(anchor) =
                drift_edit::labels::dimension_anchor(&self.drawing.editor.doc, &c)
            {
                let offset = map.to_world(p) - grab - anchor;
                // Level with another dimension's line when close (Ctrl places freely).
                let (offset, guide) = if ui.input(|i| i.modifiers.command) {
                    (offset, None)
                } else {
                    crate::app::dim_align::snap_label(
                        &self.drawing.editor.doc,
                        id,
                        &c,
                        offset,
                        map.camera.scale,
                    )
                };
                self.drawing.dim_guide = guide;
                self.drawing.editor.place_dimension(id, offset);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.drawing.label_drag = None;
                self.drawing.dim_guide = None;
                self.drawing.editor.commit("Move dimension");
            }
            return true;
        }
        if !annotating {
            return false;
        }
        let at = release_pos(ui, PointerButton::Primary).or(pos);
        // A double-click on a grip starts its extension line automatically again; a click on
        // one keeps the dimension picked.
        if idle && let Some((id, end, ..)) = at.and_then(grip_at) {
            if response.double_clicked_by(PointerButton::Primary) {
                self.drawing.editor.doc.begin();
                let _ = self.drawing.editor.doc.set_extension_start(id, end, None);
                self.drawing.editor.commit("Reset extension line");
                return true;
            }
            if response.clicked_by(PointerButton::Primary) {
                return true;
            }
        }
        if response.double_clicked_by(PointerButton::Primary)
            && let Some(id) = at.and_then(|p| dim_at(p).and_then(|d| d.id))
        {
            self.open_dimension_edit(id);
            return true;
        }
        // A wall's thickness dimension: typing into it sets the thickness.
        if response.double_clicked_by(PointerButton::Primary)
            && let Some(q) = at.and_then(|p| dim_at(p).and_then(|d| d.property))
        {
            self.open_property_edit(q);
            return true;
        }
        if response.clicked_by(PointerButton::Primary) {
            let shift = crate::app::visibility_ui::shift_click(ui);
            if let Some(id) = at.and_then(hit) {
                let is_dimension = self
                    .drawing
                    .editor
                    .doc
                    .constraint(id)
                    .is_some_and(|c| c.is_dimension());
                if shift && is_dimension {
                    // Added to what is picked, geometry and all.
                    self.toggle_annotation_pick(drift_core::AnnotationRef::Dimension(id));
                } else {
                    self.drawing.editor.annotation_picks.clear();
                    self.drawing.editor.selected_annotation = None;
                    self.drawing.editor.selected_texts.clear();
                    self.drawing.selected_constraint = Some(id);
                    self.drawing.editor.selection.clear();
                }
                return true;
            }
            // Shift or Ctrl adds geometry to the picked dimensions (the tool sees the click).
            if !shift && !ui.input(|i| i.modifiers.command) {
                self.drawing.selected_constraint = None;
                self.drawing.editor.annotation_picks.clear();
            }
        }
        false
    }

    /// Grips at the starts of a picked dimension's extension lines: hollow while the start is
    /// automatic, filled once it is set by hand.
    fn paint_extension_grips(
        &self,
        painter: &egui::Painter,
        dim: &DimLayout,
        id: ConstraintId,
        colors: &AnnotationColors,
    ) {
        let color = colors.selected;
        let manual = self.drawing.editor.doc.extension_starts(id);
        for (end, at, ..) in dim.shape.extension_grips() {
            let square = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
            let fill = if manual[end].is_some() {
                color
            } else {
                colors.text_background
            };
            painter.rect_filled(square, 1.0, fill);
            painter.rect_stroke(
                square,
                1.0,
                egui::Stroke::new(1.2, color),
                egui::StrokeKind::Middle,
            );
        }
    }

    pub(crate) fn open_dimension_edit(&mut self, id: ConstraintId) {
        self.open_dimension_editor(id, true);
    }

    /// Open the in-place value editor, with or without the text around the value.
    pub(crate) fn open_dimension_editor(&mut self, id: ConstraintId, extras: bool) {
        if self.drawing.editor.doc.is_driven(id) {
            self.drawing.editor.out.notice = Some(
                "A driven dimension follows the geometry: untick Driven in the constraint list \
                 to set its value"
                    .into(),
            );
            return;
        }
        if let Some(text) = self.drawing.editor.dimension_edit_text(id) {
            self.drawing.dimension_edit = Some(DimensionEdit {
                id,
                text,
                focus: true,
                extras,
            });
        }
    }

    /// Open the in-place editor of a property dimension (a wall's thickness).
    pub(crate) fn open_property_edit(&mut self, q: drift_core::PropertyDimId) {
        let Some(d) = self.drawing.editor.doc.property_dim(q) else {
            return;
        };
        let w = d.target.wall();
        let Some(wall) = self.drawing.editor.doc.wall(w) else {
            return;
        };
        let text = wall.thickness_expr.clone().unwrap_or_else(|| {
            self.drawing
                .editor
                .doc
                .units()
                .format_length(wall.thickness)
        });
        self.drawing.property_edit = Some((q, text, true));
    }

    /// The in-place editor of a property dimension: Enter sets the wall's thickness.
    pub(crate) fn property_editor(&mut self, ctx: &egui::Context, dims: &[DimLayout], now: f64) {
        let Some((q, mut text, focus)) = self.drawing.property_edit.take() else {
            return;
        };
        let Some(at) = dims
            .iter()
            .find(|d| d.property == Some(q))
            .map(|d| d.text_rect.center())
        else {
            return;
        };
        let field = egui::Id::new("property_edit_field");
        let area = egui::Area::new(egui::Id::new("property_edit"))
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
            let wall = self
                .drawing
                .editor
                .doc
                .property_dim(q)
                .map(|d| d.target.wall());
            if let Some(w) = wall
                && let Err(e) = self.drawing.editor.set_wall_thickness(&[w], &text)
            {
                self.set_status(format!("Can't use {text}: {e}"), true, now);
            }
            return;
        }
        if focus || !(escape || outside) {
            self.drawing.property_edit = Some((q, text, false));
        }
    }

    /// The in-place dimension value editor.
    pub(crate) fn dimension_editor(&mut self, ctx: &egui::Context, dims: &[DimLayout], now: f64) {
        let Some(mut edit) = self.drawing.dimension_edit.take() else {
            return;
        };
        let Some(at) = dims
            .iter()
            .find(|d| d.id == Some(edit.id))
            .map(|d| d.text_rect.center())
            .or_else(|| {
                self.drawing
                    .view
                    .pointer
                    .map(|_| self.drawing.view.canvas_rect.center())
            })
        else {
            return;
        };
        let field = egui::Id::new("dimension_edit_field");
        let first = edit.focus;
        // The text around the value is edited here too: before and after it, and the
        // common notes (TYP, EQ…) a click away.
        let mut note = self
            .drawing
            .editor
            .doc
            .dim_note(edit.id)
            .cloned()
            .unwrap_or_default();
        let was = note.clone();
        let area = egui::Area::new(egui::Id::new("dimension_edit"))
            .fixed_pos(at - egui::vec2(45.0, 11.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut edit.text)
                        .id(field)
                        .desired_width(90.0)
                        .horizontal_align(egui::Align::Center),
                );
                if edit.focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, field) {
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(edit.text.chars().count()),
                        );
                        state.cursor.set_char_range(Some(all));
                        state.store(ctx, field);
                    }
                    edit.focus = false;
                }
                if !edit.extras {
                    return;
                }
                ui.add_space(2.0);
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_max_width(250.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut note.prefix)
                                    .hint_text("before")
                                    .desired_width(70.0),
                            )
                            .on_hover_text("Text before the value (4×)");
                            ui.add(
                                egui::TextEdit::singleline(&mut note.suffix)
                                    .hint_text("after")
                                    .desired_width(110.0),
                            )
                            .on_hover_text("Text after the value (THRU)");
                        });
                        super::drafting_ui::dimension_chips(ui, &mut note);
                    });
            });
        // Enter applies the value and finishes; Esc, or a press anywhere else, finishes.
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
        let mut keep = first || !(enter || escape || outside);
        if !first
            && enter
            && Some(&edit.text) != self.drawing.editor.dimension_edit_text(edit.id).as_ref()
            && let Err(e) = self.drawing.editor.set_dimension_text(edit.id, &edit.text)
        {
            self.set_status(format!("Can't use {}: {e}", edit.text), true, now);
            keep = false;
        }
        if note != was {
            self.drawing.editor.doc.begin();
            self.drawing.editor.doc.set_dim_note(edit.id, Some(note));
            self.drawing.editor.commit("Dimension text");
        }
        if keep {
            self.drawing.dimension_edit = Some(edit);
        }
    }

    pub(crate) fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.drawing.view.canvas_rect = rect;
        self.drawing.editor.drawing_curves = self.tool.kind().draws_curves();
        // The Wall and Dimension tools snap to walls' centrelines: they show (faintly).
        self.drawing.editor.show_centrelines = self.show_wall_centrelines
            || matches!(self.tool.kind(), ToolKind::Wall | ToolKind::Dimension);
        let view_size = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
        self.drawing.editor.update_underlays();
        // A point the pointer snaps to lights up like a hovered one (as in Illustrator).
        self.drawing.editor.snapped_point =
            self.drawing.view.pointer.and_then(|p| p.snapped_point());

        if response.double_clicked_by(PointerButton::Middle) {
            self.drawing.view.fit_target = Some(self.fit_bounds());
        }
        self.pan_and_zoom(ui, &response, rect);
        if let Some(bounds) = self.drawing.view.fit_target {
            self.drawing.view.camera.fit(bounds, view_size, FIT_MARGIN);
        }

        let map = ScreenMap {
            rect,
            camera: self.drawing.view.camera,
        };
        let (mods, alt) = ui.input(|i| {
            (
                Modifiers {
                    shift: i.modifiers.shift,
                    command: i.modifiers.command,
                    alt: i.modifiers.alt,
                },
                i.modifiers.alt,
            )
        });
        let snap = !alt; // hold Alt to place freely
        self.drawing.view.keys_held = (mods.shift, mods.command);

        // Annotations are laid out first so they can take pointer input before the tool.
        let annotation_painter = ui.painter_at(rect);
        let dims = if self.show_dimensions {
            self.layout_dimensions(&annotation_painter, &map)
        } else {
            Vec::new()
        };
        let glyphs = if self.show_relations {
            let mut glyphs = annotations::layout_glyphs(&self.drawing.editor.doc, &map);
            // (Not on walls' centrelines unless the wall is picked.)
            if self.drawing.editor.doc.has_walls() {
                let hidden = self.drawing.editor.unpicked_centrelines();
                let doc = &self.drawing.editor.doc;
                glyphs.retain(|g| {
                    doc.constraint(g.id)
                        .is_none_or(|c| !c.entities().any(|e| hidden.contains(&e)))
                });
            }
            glyphs
        } else {
            Vec::new()
        };
        let notes = self.layout_notes(&annotation_painter, &map);
        let temps = self.layout_temp_dims(&annotation_painter, &map);

        // Feed pointer input to the active tool.
        let dragging = response.dragged_by(PointerButton::Primary);
        let pos = if dragging {
            response.interact_pointer_pos()
        } else {
            response.hover_pos()
        };
        // While a note's text is being typed, a click on the canvas only finishes it.
        let typing_note = self.drawing.note_edit.is_some()
            && (response.clicked() || response.drag_started() || response.dragged());
        let hatch = self.hatch_handles(&map);
        let consumed = typing_note
            || self.text_pointer(ui, &response, &map)
            || self.align_edge_input(ui, &response, &map, pos)
            || self.hatch_input(ui, &response, &map, hatch.as_ref(), snap)
            || self.note_input(ui, &response, &map, &notes, &dims, &glyphs, pos)
            || self.temp_dim_input(ui, &response, &map, &temps, pos)
            || self.annotation_input(ui, &response, &map, &dims, &glyphs, pos)
            || self.view_crop_input(ui, &response, &map, pos, snap);
        if consumed {
            self.drawing.view.pointer = pos.map(|pos| self.make_pointer(&map, pos, snap));
        } else if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let p = self.make_pointer(&map, origin, snap);
            self.tool
                .handle(Input::DragStart(p, mods), &mut self.drawing.editor);
        }
        if !consumed {
            self.drawing.view.pointer = pos.map(|pos| self.make_pointer(&map, pos, snap));
        }
        if self.drawing.view.pointer.is_some() {
            self.drawing.view.last_pointer = self.drawing.view.pointer;
        }
        if consumed {
            // The annotation took this frame's input.
        } else if let Some(p) = self.drawing.view.pointer {
            if dragging {
                self.tool.handle(Input::Drag(p), &mut self.drawing.editor);
            } else {
                self.tool.handle(Input::Hover(p), &mut self.drawing.editor);
            }
            // Use the exact release position: if a slow frame batched the release with later
            // pointer motion, the current position is no longer where the user clicked.
            let released = release_pos(ui, PointerButton::Primary)
                .map_or(p, |pos| self.make_pointer(&map, pos, snap));
            if response.drag_stopped_by(PointerButton::Primary) {
                self.tool
                    .handle(Input::DragEnd(released, mods), &mut self.drawing.editor);
            }
            if response.clicked_by(PointerButton::Primary) {
                self.tool
                    .handle(Input::Click(released, mods), &mut self.drawing.editor);
            }
            if response.double_clicked_by(PointerButton::Primary) {
                self.tool
                    .handle(Input::DoubleClick(p), &mut self.drawing.editor);
            }
        } else {
            self.drawing.editor.hovered = None;
            // A drag released off the canvas (e.g. over a side panel) still ends, where the
            // button came up, so a selection box or move doesn't stay stuck.
            if response.drag_stopped_by(PointerButton::Primary)
                && let Some(at) = release_pos(ui, PointerButton::Primary)
                    .or_else(|| ui.input(|i| i.pointer.latest_pos()))
            {
                let released = self.make_pointer(&map, at, snap);
                self.tool
                    .handle(Input::DragEnd(released, mods), &mut self.drawing.editor);
            }
        }
        self.take_box_pick(&map, &dims, &notes);
        // With the select tool, right-click opens a menu for what's under the pointer (picking
        // it first, as in Illustrator) or for the view.
        let menu_tool = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        if menu_tool
            && response.secondary_clicked()
            && let Some(h) = self.drawing.editor.hovered
            && !self.drawing.editor.selection.contains(&h)
        {
            self.drawing.editor.selection = [h].into_iter().collect();
        }
        // On a dimension or annotation: pick it (unless it already is), for its menu.
        if menu_tool && response.secondary_clicked() && self.drawing.editor.hovered.is_none() {
            let hovered = self
                .drawing
                .hovered_constraint
                .filter(|id| {
                    self.drawing
                        .editor
                        .doc
                        .constraint(*id)
                        .is_some_and(|c| c.is_dimension())
                })
                .map(drift_core::AnnotationRef::Dimension)
                .or(self
                    .drawing
                    .editor
                    .hovered_annotation
                    .map(drift_core::AnnotationRef::Annotation));
            if let Some(r) = hovered
                && !self.annotation_picked(r)
            {
                self.drawing.editor.annotation_picks.clear();
                self.drawing.editor.selection.clear();
                match r {
                    drift_core::AnnotationRef::Dimension(id) => {
                        self.drawing.selected_constraint = Some(id);
                        self.drawing.editor.selected_annotation = None;
                    }
                    drift_core::AnnotationRef::Annotation(id) => {
                        self.drawing.editor.selected_annotation = Some(id);
                        self.drawing.selected_constraint = None;
                    }
                }
            }
        }
        if menu_tool || response.context_menu_opened() {
            response.context_menu(|ui| self.canvas_menu(ui));
        }
        if response.clicked_by(PointerButton::Secondary) {
            if self.tool.is_modal() {
                // Right-click cancels a grab, as in Blender.
                self.tool.cancel(&mut self.drawing.editor);
            } else {
                self.tool.handle(Input::Finish, &mut self.drawing.editor);
            }
        }
        if !self.tool.is_busy() {
            self.drawing.editor.region_motion = None;
        }
        // Tool events may have changed the document (and the pointer's snap targets).
        self.drawing.editor.sync();
        // Just placed: only its value (the next pick of a chain may be under the rest).
        if let Some(id) = self.drawing.editor.out.pending_edit.take() {
            self.open_dimension_editor(id, false);
        }
        if let Some(id) = self.drawing.editor.out.pending_note_edit.take() {
            self.open_note_edit(id, true);
        }
        // A base point just picked: back to the select tool.
        if std::mem::take(&mut self.drawing.editor.out.base_point_done) {
            self.set_tool(ToolKind::Select);
        }
        // A view just drawn: picked, with the select tool, to name and reframe.
        // (Asked for from a sheet: it goes on that sheet.)
        if let Some(id) = self.drawing.editor.out.new_view.take()
            && !self.view_drawn(id)
        {
            self.set_tool(ToolKind::Select);
            self.pick_view_alone(id);
        }
        // Remember points the pointer snapped to, for inference lines.
        if let Some(snap) = self.drawing.view.pointer.and_then(|p| p.snap)
            && snap.kind.is_point()
            && !self.drawing.inference_points.contains(&snap.at)
        {
            self.drawing.inference_points.push(snap.at);
            if self.drawing.inference_points.len() > 4 {
                self.drawing.inference_points.remove(0);
            }
        }
        // Show what a hovered or picked relation/dimension applies to.
        if self.tool.kind() != ToolKind::Dimension {
            self.drawing.editor.highlight = self
                .drawing
                .hovered_constraint
                .or(self.drawing.selected_constraint)
                .and_then(|id| self.drawing.editor.doc.constraint(id))
                .map(|c| c.entities().collect())
                .unwrap_or_default();
            // A picked fill shows the curves that move with it.
            if self.drawing.editor.selection.is_empty()
                && !self.drawing.editor.selected_fills.is_empty()
                && !self.tool.is_busy()
                && let Some((curves, _)) = self.drawing.editor.selected_fill_curves()
            {
                self.drawing.editor.highlight.extend(curves);
            }
        }
        // Drawing and editing tools: Drift's own crosshair in place of the system cursor (whose
        // size and weight can't be set), drawn last.
        let crosshair_at = (self.tool.kind() != ToolKind::Select && response.hovered())
            .then(|| response.hover_pos())
            .flatten();
        if crosshair_at.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        } else if response.hovered()
            && !self.tool.is_modal()
            && let Some(icon) = self.transform_cursor().or_else(|| self.fillet_cursor())
        {
            ui.ctx().set_cursor_icon(icon);
        }

        // Fills and hatches under the geometry (GPU).
        self.hatch_patterns
            .update(self.drawing.editor.doc.custom_patterns());
        let view = {
            let (a, b) = (
                map.to_world(rect.left_top()),
                map.to_world(rect.right_bottom()),
            );
            Bounds {
                min: a.min(b),
                max: a.max(b),
            }
        };
        let dark = ui.visuals().dark_mode;
        let fills = drift_edit::fills::fill_graphics(
            &mut self.drawing.editor,
            &mut self.drawing.fill_meshes,
            &self.hatch_patterns,
            view,
            self.drawing.view.camera.scale,
            dark,
        );
        // Geometry (GPU): underlays under everything else.
        let underlays = self.drawing.editor.underlay_items(view, dark);
        let stacked = self.drawing.editor.doc.rules().stacked_fills;
        let pictures = self.drawing.editor.underlay_pictures();
        // Fills under every curve: their hatch lines go under the sketch too.
        let meshes = if stacked { Vec::new() } else { fills.meshes() };
        let hatch_lines = if stacked { Vec::new() } else { fills.lines() };
        let mut items = Vec::new();
        if self.show_dimensions {
            items.extend(crate::notes::centre_items(
                &self.drawing.editor.doc,
                self.drawing.editor.selected_annotation,
                self.drawing.editor.hovered_annotation,
                &|id| self.on_canvas(drift_core::AnnotationRef::Annotation(id)),
            ));
        }
        // Stacked: one draw group per step of the drawing's stack (the same order prints and
        // exports follow): a fill with the curves of its outline, placed pictures, or a run
        // of loose curves, each over every group before it. Points, text and copies' curves
        // go at the top of their layer. Lit-up curves, handles and previews go over
        // everything.
        let mut groups: Vec<DrawContent> = Vec::new();
        let mut images = Vec::new();
        self.drawing.editor.preview_hidden = self.tool.preview_hides(&self.drawing.editor);
        let text = self
            .drawing
            .editor
            .text_items_tagged(self.status_colors, dark);
        let stack = stacked.then(|| self.drawing.editor.stack());
        // The sketch's items, kept from frame to frame: one list per stack group (or one),
        // and what is lit up, over everything.
        let lit = self.drawing.sketch_items.prepare(
            &self.drawing.editor,
            self.status_colors,
            dark,
            stack.as_deref(),
        );
        let top = self.drawing.sketch_items.top();
        items.extend_from_slice(&self.drawing.sketch_items.lists()[top]);
        items.extend(lit);
        if let Some(stack) = &stack {
            let mut fill_groups: std::collections::HashMap<
                drift_edit::stack::FillRef,
                drift_edit::fills::FillGroup,
            > = fills.groups.into_iter().map(|g| (g.fill, g)).collect();
            let shown = |layer: Option<drift_core::LayerId>| {
                layer.is_none_or(|l| self.drawing.editor.doc.layer(l).is_none_or(|l| l.visible))
            };
            for g in &stack.groups {
                let mut content = DrawContent::default();
                if let Some(f) = g.fill.and_then(|f| fill_groups.remove(&f)) {
                    content.fills = f.meshes;
                    content.items = f.lines;
                }
                let placed = pictures
                    .iter()
                    .filter(|(u, _)| shown(g.layer) && g.pictures.contains(u))
                    .map(|(_, image)| image.clone());
                if g.layer.is_some() {
                    content.images.extend(placed);
                } else {
                    images.extend(placed);
                }
                groups.push(content);
            }
            // (Text isn't kept: it goes over the sketch's items in its group, as they would.)
            for t in text {
                let at = (t.item.emphasis == drift_core::Emphasis::Normal)
                    .then(|| stack.group_of(t.entity, t.layer))
                    .flatten()
                    .filter(|k| *k < groups.len());
                match at {
                    Some(k) => self.drawing.sketch_items.push(k, t.item),
                    None => items.push(t.item),
                }
            }
        } else {
            images.extend(pictures.into_iter().map(|(_, image)| image));
            items.extend(text.into_iter().map(|t| t.item));
        }
        // Bézier handles around picked curves, and their curvature combs.
        let handle_grey = if ui.visuals().dark_mode {
            [0x8a, 0x90, 0x9a]
        } else {
            [0x8a, 0x8e, 0x96]
        };
        items.extend(self.drawing.editor.handle_lines(handle_grey));
        if self.show_combs {
            let comb = if ui.visuals().dark_mode {
                [0xd9, 0x8c, 0xff]
            } else {
                [0x9b, 0x3c, 0xc8]
            };
            items.extend(
                self.drawing
                    .editor
                    .curvature_combs(28.0 / self.drawing.view.camera.scale, comb),
            );
        }
        let construction =
            self.drawing.editor.prefs.draw_construction && self.tool.kind().draws_curves();
        items.extend(
            self.tool
                .preview(self.drawing.view.pointer.as_ref())
                .into_iter()
                .map(|item| {
                    if construction && item.kind == drift_core::LineKind::Solid {
                        item.with_kind(drift_core::LineKind::Construction)
                    } else {
                        item
                    }
                }),
        );
        items.extend(self.tool.editor_preview(&self.drawing.editor));
        self.drawing.editor.preview_hidden.clear();
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let lists = self.drawing.sketch_items.lists();
        let group_refs: Vec<drift_render::DrawGroup<'_>> = groups
            .iter()
            .enumerate()
            .map(|(k, g)| drift_render::DrawGroup {
                fills: &g.fills,
                images: &g.images,
                items: &g.items,
                more: lists
                    .get(k)
                    .filter(|_| k + 1 < lists.len())
                    .map_or(&[], Vec::as_slice),
            })
            .collect();
        // Underlays, then hatch lines.
        let under: Vec<&[drift_core::SketchItem]> = underlays
            .iter()
            .map(|run| run.as_slice())
            .chain([hatch_lines.as_slice()])
            .collect();
        let callback = self.painter.callback(
            rect,
            ppp,
            crate::canvas::CanvasContent {
                under: &under,
                items: &items,
                fills: &meshes,
                images: &images,
                groups: &group_refs,
            },
            &self.drawing.view.camera,
            crate::canvas::CanvasOptions {
                dark_mode: ui.visuals().dark_mode,
                imperial_grid: self.imperial(),
                thin_lines: self.thin_lines,
                show_grid: self.show_grid,
                show_axes: self.show_axes,
            },
        );
        ui.painter().add(callback);
        self.drawing.sketch_items.restore();

        // Overlays (egui).
        let painter = ui.painter_at(rect);
        let colors = OverlayColors::new(ui.visuals().dark_mode);
        match self.tool.overlay() {
            Some(Overlay::SelectionBox { rect: r, crossing }) => {
                overlay::selection_box(&painter, &map, &r, crossing, &colors);
            }
            Some(Overlay::Move { base, to }) => {
                overlay::move_guide(&painter, &map, base, to, &colors);
            }
            Some(Overlay::Rotate {
                pivot,
                from,
                degrees,
            }) => {
                overlay::rotate_guide(&painter, &map, pivot, from, degrees, &colors);
            }
            Some(Overlay::Region { at }) => {
                let regions = self.drawing.editor.regions();
                if let Some(region) = drift_geom::regions::region_at(&regions, at) {
                    overlay::region_fill(&painter, &map, region, colors.snap.gamma_multiply(0.25));
                }
            }
            None => {}
        }
        // A block's base point, while it is edited or picked.
        if let Some(at) = self
            .drawing
            .editor
            .base_point_cross(self.tool.kind() == ToolKind::BasePoint)
        {
            overlay::base_point(&painter, &map, at, &colors);
        }
        // Picked and hovered fills.
        let shapes = self.drawing.editor.fill_shapes();
        for (id, alpha) in self
            .drawing
            .editor
            .selected_fills
            .iter()
            .map(|id| (*id, 0.3))
            .chain(self.drawing.editor.hovered_fill.map(|id| (id, 0.14)))
        {
            if let Some(shape) = shapes.get(&id) {
                let color = if shape.lost {
                    egui::Color32::from_rgb(0xff, 0x5d, 0x5d)
                } else {
                    colors.window
                };
                overlay::region_fill(&painter, &map, &shape.region, color.gamma_multiply(alpha));
            }
        }
        // Fills an edit divided, kept whole: marked at their seed until fitted, split or
        // kept.
        let dark = ui.visuals().dark_mode;
        for (id, fill) in self.drawing.editor.doc.fills() {
            let Some(shape) = shapes.get(&id).filter(|s| fill.divided && s.parts > 1) else {
                continue;
            };
            if self
                .drawing
                .editor
                .doc
                .layer(fill.layer)
                .is_some_and(|l| !l.visible)
            {
                continue;
            }
            let at = map.to_screen(fill.seed);
            if rect.expand(40.0).contains(at) {
                let bg = if dark {
                    egui::Color32::from_rgb(0x41, 0x24, 0x02)
                } else {
                    egui::Color32::from_rgb(0xfa, 0xee, 0xda)
                };
                let text = format!("Covers {} areas", shape.parts);
                overlay::badge(&painter, at, &text, theme::warning_color(dark), bg);
            }
        }
        // Drawn where the handles are now (a drag this frame has moved them).
        if let Some(h) = self.hatch_handles(&map) {
            self.paint_hatch_handles(&painter, &map, &h, &colors);
        }
        self.paint_ghosts(&painter, &map);
        self.paint_check_markers(&painter, &map);
        if self.pie.is_none() {
            self.paint_transform_box(&painter, &map, &colors, ui.visuals().dark_mode);
            self.paint_opening_arrows(&painter, &map, &colors);
        }
        if let Some(region) = self.drawing.editor.region() {
            overlay::region_fill(&painter, &map, &region, colors.window.gamma_multiply(0.22));
        }
        self.paint_annotations(&painter, &map, &dims, &glyphs, ui.visuals().dark_mode);
        self.paint_temp_dims(&painter, &map, &temps, ui.visuals().dark_mode);
        self.paint_view_crops(&painter, &map, ui.visuals().dark_mode);
        // A dragged dimension snapped level with another: a guide between them.
        if let Some([a, b]) = self.drawing.dim_guide {
            let (a, b) = (map.to_screen(a), map.to_screen(b));
            let along = (b - a).normalized() * 16.0;
            crate::overlay::dashed(
                &painter,
                &[a - along, b + along],
                egui::Stroke::new(1.0, colors.snap),
                4.0,
                3.0,
            );
        }
        if let Some(at) = crosshair_at {
            let color = ui.visuals().strong_text_color().gamma_multiply(0.9);
            self.crosshair.paint(&painter, at, rect, color);
        }
        self.paint_notes(&painter, &notes, ui.visuals().dark_mode);
        self.paint_note_grips(&painter, &notes, ui.visuals().dark_mode);
        self.paint_groups(&painter, &map, ui.visuals().dark_mode);
        self.paint_free_directions(&painter, &map);
        if let Some(status) = &self.status {
            let now = ui.input(|i| i.time);
            if now < status.until {
                let fade = ((status.until - now) / 0.4).clamp(0.0, 1.0) as f32;
                overlay::toast(
                    &painter,
                    rect,
                    &status.text,
                    status.error,
                    ui.visuals().dark_mode,
                    fade,
                );
                // Sleep until the fade starts, then animate it.
                let remaining = status.until - now;
                let wait = if remaining > 0.4 {
                    remaining - 0.4
                } else {
                    0.016
                };
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(wait));
            }
        }
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.dimension_editor(&ctx, &dims, now);
        self.property_editor(&ctx, &dims, now);
        self.temp_dim_editor(&ctx, &temps, now);
        // The notes as laid out now (a tool may have just placed one).
        let notes = self.layout_notes(&annotation_painter, &map);
        self.note_editor(&ctx, &notes);
        self.text_editor(ui, &map, &painter);
        self.paint_add_view_prompt(&painter, &map);
        if let (Some(p), Some(pos)) = (self.drawing.view.pointer, pos) {
            // The HUD: what was snapped to, the measurement, and the values being typed.
            // (What can be typed is in the status bar's prompt.)
            let mut info = overlay::CursorInfo::default();
            if let Some(snap) = p.snap {
                overlay::snap_marker(&painter, &map, &snap, colors.snap);
                let mut label = snap.kind.label().to_owned();
                // The relation the segment ending here gets too (#204).
                if let Some(relation) = p
                    .exact_snap()
                    .and(p.guide)
                    .and_then(|g| relation_name(g.kind))
                    .filter(|_| self.auto_relations)
                {
                    label = format!("{label} · {relation}");
                }
                info.snap = Some(label);
            } else if let Some(held) = p.overridden {
                // An axis lock (or an arc's circle) moved the point off what was hovered:
                // mark that, and where the click lands, in line with it (#200).
                overlay::snap_marker(&painter, &map, &held, colors.snap.gamma_multiply(0.55));
                overlay::landing_marker(&painter, &map, p.pos(), colors.snap);
                let how = if self.tool.axis_lock().is_some() {
                    "In line with"
                } else {
                    "Toward"
                };
                info.snap = Some(format!("{how} {}", held.kind.label().to_lowercase()));
            }
            if let Some(m) = self.tool.measurement(&p) {
                info.notes.push(m);
            }
            if let Some(t) = &self.drawing.typing {
                let results = t.evaluate(&self.drawing.editor);
                let units = self.drawing.editor.doc.units();
                for (i, field) in t.fields.iter().enumerate() {
                    let (note, error) = match &results[i] {
                        // Show what an expression comes to.
                        Ok(Some((v, _)))
                            if t.values[i]
                                .chars()
                                .any(|c| c.is_ascii_alphabetic() || "*/+^('\"".contains(c)) =>
                        {
                            (format!("= {}", units.format(field_kind(field), *v)), false)
                        }
                        Err(e) if !t.values[i].trim().is_empty() => (e.to_string(), true),
                        _ => (String::new(), false),
                    };
                    info.fields.push(overlay::HudField {
                        name: field.to_string(),
                        value: t.values[i].clone(),
                        note,
                        active: i == t.active,
                        error,
                    });
                }
            }
            overlay::cursor_hud(&painter, pos, &info, &colors);
        }
    }

    pub(crate) fn paint_annotations(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
        dims: &[DimLayout],
        glyphs: &[annotations::GlyphLayout],
        dark: bool,
    ) {
        let colors = AnnotationColors::new(dark);
        let dim_colors =
            AnnotationColors::drafting(dark, self.drawing.editor.doc.draft_style().color);
        // A dimension with a colour of its own is drawn in it.
        let colors_of = |id: ConstraintId| {
            let r = drift_core::AnnotationRef::Dimension(id);
            match self.drawing.editor.doc.style_override(r).color {
                Some(c) => AnnotationColors::drafting(dark, c),
                None => dim_colors,
            }
        };
        let conflicts = &self.drawing.editor.analysis().conflicts;
        let state = |id: ConstraintId| {
            if self.annotation_picked(drift_core::AnnotationRef::Dimension(id))
                || self.drawing.label_drag.is_some_and(|d| d.0 == id)
            {
                AnnotationState::Selected
            } else if self.drawing.hovered_constraint == Some(id) {
                AnnotationState::Hovered
            } else if conflicts.contains(&id) {
                AnnotationState::Conflict
            } else {
                AnnotationState::Normal
            }
        };
        for dim in dims {
            if let Some(id) = dim.id
                && self
                    .drawing
                    .dimension_edit
                    .as_ref()
                    .is_none_or(|e| e.id != id)
            {
                // Hidden on the canvas (drawn only while hidden ones are shown): faded.
                let shown = self
                    .drawing
                    .editor
                    .doc
                    .shown(drift_core::AnnotationRef::Dimension(id));
                let own = colors_of(id);
                let faded = own.faded();
                let look = if shown.canvas { &own } else { &faded };
                annotations::paint_dimension(painter, dim, state(id), look);
                if self.drawing.selected_constraint == Some(id)
                    && self.tool.kind() == ToolKind::Select
                {
                    self.paint_extension_grips(painter, dim, id, &colors);
                }
                if !shown.sheets {
                    Self::paint_off_sheets_badge(painter, dim.text_rect.right_top(), look.normal);
                }
                // Mark midpoints the dimension measures to (a line in a point slot).
                for (e, slot) in dim.constraint.slots() {
                    if slot == drift_core::Slot::PointOrMidpoint
                        && matches!(
                            self.drawing.editor.doc.get(e).map(|x| x.kind),
                            Some(drift_core::EntityKind::Line { .. })
                        )
                        && let Some(m) = self.drawing.editor.doc.ref_point(e)
                    {
                        let color = match state(id) {
                            AnnotationState::Normal => own.normal,
                            AnnotationState::Hovered => colors.hovered,
                            AnnotationState::Selected => colors.selected,
                            AnnotationState::Conflict => colors.conflict,
                            AnnotationState::Preview => colors.preview,
                        };
                        annotations::paint_midpoint_marker(painter, map.to_screen(m), color, false);
                    }
                }
            }
        }
        if let Some((c, offset)) = self
            .tool
            .pending_dimension(&self.drawing.editor, self.drawing.view.pointer.as_ref())
            && let Some(dim) = annotations::layout_dimension(
                painter,
                &self.drawing.editor.doc,
                None,
                &c,
                Some(offset),
                map,
            )
        {
            annotations::paint_dimension(painter, &dim, AnnotationState::Preview, &colors);
        }
        // Relation badges are quiet at rest, and at full strength on the picked geometry or
        // under the pointer, so they don't crowd the drawing.
        let quiet = AnnotationColors {
            normal: colors.normal.gamma_multiply(0.6),
            glyph_background: colors.glyph_background.gamma_multiply(0.55),
            ..colors
        };
        let selection = &self.drawing.editor.selection;
        for glyph in glyphs {
            let touches = self
                .drawing
                .editor
                .doc
                .constraint(glyph.id)
                .is_some_and(|c| c.entities().any(|e| selection.contains(&e)));
            let look = if touches { &colors } else { &quiet };
            annotations::paint_glyph(painter, glyph.rect, glyph.kind, state(glyph.id), look);
        }
        // Picked and hovered line midpoints.
        let ed = &self.drawing.editor;
        let midpoint_at = |line| match ed.index.shape(line) {
            Some(drift_core::Shape::Line { a, b }) => Some(map.to_screen((*a + *b) * 0.5)),
            _ => None,
        };
        for line in &ed.selected_midpoints {
            if let Some(at) = midpoint_at(*line) {
                annotations::paint_midpoint_marker(painter, at, colors.selected, true);
            }
        }
        if let Some(at) = ed.hovered_midpoint.and_then(midpoint_at) {
            annotations::paint_midpoint_marker(painter, at, colors.hovered, false);
        }
        // An axis lock: a full-width line through the segment's start, red for X, green for Y.
        if let Some((through, axis)) = self.tool.axis_lock() {
            let at = map.to_screen(through);
            let r = map.rect;
            let (ends, color) = match axis {
                Axis::X => (
                    [egui::pos2(r.left(), at.y), egui::pos2(r.right(), at.y)],
                    egui::Color32::from_rgb(0xe8, 0x4a, 0x5f),
                ),
                Axis::Y => (
                    [egui::pos2(at.x, r.top()), egui::pos2(at.x, r.bottom())],
                    egui::Color32::from_rgb(0x7c, 0xc4, 0x4e),
                ),
            };
            painter.line_segment(ends, egui::Stroke::new(1.5, color.gamma_multiply(0.85)));
        }
        // Inference guide lines and the automatic relation about to be added. On an exact
        // snap only the relation shows: the snap decides the position (#204).
        if let Some(p) = self.drawing.view.pointer
            && let Some(guide) = p.guide
        {
            let at = map.to_screen(guide.at);
            let stroke = egui::Stroke::new(1.0, colors.preview.gamma_multiply(0.8));
            if p.exact_snap().is_none() {
                for from in guide.from.iter().flatten() {
                    crate::overlay::dashed(painter, &[map.to_screen(*from), at], stroke, 4.0, 3.0);
                }
            }
            let kind = match guide.kind {
                GuideKind::Horizontal => Some(GlyphKind::Horizontal),
                GuideKind::Vertical => Some(GlyphKind::Vertical),
                GuideKind::Perpendicular(_) => Some(GlyphKind::Perpendicular),
                GuideKind::Tangent(_) => Some(GlyphKind::Tangent),
                GuideKind::Aligned => None,
            };
            if let Some(kind) = kind.filter(|_| self.auto_relations) {
                annotations::paint_auto_relation(
                    painter,
                    at + egui::vec2(-14.0, -16.0),
                    kind,
                    &colors,
                );
            }
        }
    }

    /// The transform box, when the select tool has one to show (not while a modal move or
    /// turn is under way): a thin frame with square handles.
    fn transform_box_shown(&mut self) -> Option<drift_edit::transform::TransformBox> {
        (self.tool.kind() == ToolKind::Select && !self.tool.is_modal())
            .then(|| self.drawing.editor.transform_box())
            .flatten()
    }

    fn paint_transform_box(
        &mut self,
        painter: &egui::Painter,
        map: &ScreenMap,
        colors: &OverlayColors,
        dark: bool,
    ) {
        let Some(frame) = self.transform_box_shown() else {
            return;
        };
        let pad = PICK_RADIUS / self.drawing.view.camera.scale;
        let b = frame.padded(pad);
        let (a, c) = (map.to_screen(b.min), map.to_screen(b.max));
        let stroke = egui::Stroke::new(1.0, colors.window);
        painter.rect_stroke(
            egui::Rect::from_two_pos(a, c),
            0.0,
            stroke,
            egui::StrokeKind::Middle,
        );
        let fill = if dark {
            egui::Color32::from_rgb(0x1b, 0x1d, 0x21)
        } else {
            egui::Color32::WHITE
        };
        for (_, at) in frame.scale_handles(pad) {
            let square = egui::Rect::from_center_size(map.to_screen(at), egui::vec2(7.0, 7.0));
            painter.rect_filled(square, 0.0, fill);
            painter.rect_stroke(square, 0.0, stroke, egui::StrokeKind::Middle);
        }
        // Over a turn handle: a curved arrow by the pointer.
        if let Some(p) = self.drawing.view.pointer
            && let Some(drift_edit::transform::Handle::Turn { .. }) = frame.handle_at(
                p.raw,
                drift_edit::transform::handle_reach(p.tolerance),
                p.tolerance,
            )
        {
            let at = map.to_screen(p.raw) + egui::vec2(14.0, -14.0);
            overlay::turn_glyph(painter, at, colors.window);
        }
    }

    /// The cursor over a transform handle, or inside a picked area a drag would move.
    fn transform_cursor(&mut self) -> Option<egui::CursorIcon> {
        use drift_edit::transform::Handle;
        let p = self.drawing.view.pointer?;
        if let Some(frame) = self.transform_box_shown() {
            match frame.handle_at(
                p.raw,
                drift_edit::transform::handle_reach(p.tolerance),
                p.tolerance,
            ) {
                // Screen y is down: a handle on the same side along both axes is on the
                // rising diagonal.
                Some(Handle::Scale { x, y }) => {
                    return Some(match (x, y) {
                        (0, _) => egui::CursorIcon::ResizeVertical,
                        (_, 0) => egui::CursorIcon::ResizeHorizontal,
                        _ if x == y => egui::CursorIcon::ResizeNeSw,
                        _ => egui::CursorIcon::ResizeNwSe,
                    });
                }
                Some(Handle::Turn { .. }) => return Some(egui::CursorIcon::Default),
                None => {}
            }
        }
        if self.tool.kind() == ToolKind::Select
            && !self.tool.is_busy()
            && self.drawing.editor.hovered.is_none()
            && self
                .drawing
                .editor
                .region()
                .is_some_and(|r| r.contains(p.raw))
        {
            return Some(egui::CursorIcon::Move);
        }
        None
    }

    /// Over a fillet's arc that a drag would re-size (rather than move): a resize cursor
    /// along the way it grows, towards its corner.
    fn fillet_cursor(&self) -> Option<egui::CursorIcon> {
        let ed = &self.drawing.editor;
        let arc = ed.hovered?;
        let alone = ed.selection.len() == 1 || !ed.selection.contains(&arc);
        let f = ed.fillet_at(arc).filter(|_| alone)?;
        let Some(drift_core::Shape::Arc { center, .. }) = ed.doc.shape(arc) else {
            return None;
        };
        let v = f.corner - center;
        // Screen directions: y is down.
        let eighth = (v.y.atan2(v.x) / std::f64::consts::FRAC_PI_4).round() as i64;
        Some(match eighth.rem_euclid(4) {
            0 => egui::CursorIcon::ResizeHorizontal,
            1 => egui::CursorIcon::ResizeNeSw,
            2 => egui::CursorIcon::ResizeVertical,
            _ => egui::CursorIcon::ResizeNwSe,
        })
    }

    pub(crate) fn pan_and_zoom(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
    ) {
        let to_view = |pos: egui::Pos2| {
            let v = pos - rect.center();
            DVec2::new(f64::from(v.x), f64::from(v.y))
        };
        // Pan with the middle or right button. Track positions from the press origin rather
        // than per-frame deltas, which include the pointer jump of a touch or pen press.
        let panning = response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary);
        if response.drag_started_by(PointerButton::Middle)
            || response.drag_started_by(PointerButton::Secondary)
        {
            self.drawing.view.pan_anchor = ui.input(|i| i.pointer.press_origin());
        }
        if panning
            && let (Some(from), Some(to)) = (
                self.drawing.view.pan_anchor,
                response.interact_pointer_pos(),
            )
        {
            let delta = to - from;
            if delta != egui::Vec2::ZERO {
                self.drawing
                    .view
                    .camera
                    .pan(DVec2::new(f64::from(delta.x), f64::from(delta.y)));
                self.drawing.view.fit_target = None;
            }
            self.drawing.view.pan_anchor = Some(to);
        }
        if !panning {
            self.drawing.view.pan_anchor = None;
        }

        if let Some(pos) = response.hover_pos() {
            let (pan, factor) = self.wheel.read(ui);
            if pan != egui::Vec2::ZERO {
                self.drawing
                    .view
                    .camera
                    .pan(DVec2::new(f64::from(pan.x), f64::from(pan.y)));
                self.drawing.view.fit_target = None;
            }
            if factor != 1.0 {
                self.drawing.view.camera.zoom_at(to_view(pos), factor);
                self.drawing.view.fit_target = None;
            }
        }
    }
}

/// What the scroll wheel, or two fingers on a trackpad, do over the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelMode {
    /// A mouse wheel zooms and a trackpad pans, as far as the two can be told apart.
    #[default]
    Auto,
    Zoom,
    /// Scrolling pans; Ctrl + scroll (and a pinch) zooms.
    Pan,
}

impl WheelMode {
    pub const ALL: [WheelMode; 3] = [WheelMode::Auto, WheelMode::Zoom, WheelMode::Pan];

    pub fn label(self) -> &'static str {
        match self {
            WheelMode::Auto => "Automatic",
            WheelMode::Zoom => "Zoom",
            WheelMode::Pan => "Pan",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            WheelMode::Auto => "auto",
            WheelMode::Zoom => "zoom",
            WheelMode::Pan => "pan",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Scroll input for a view that pans and zooms (the canvas, the sheet preview).
#[derive(Clone, Copy, Debug, Default)]
pub struct Wheel {
    pub mode: WheelMode,
    /// The scroll gesture in progress looks like a trackpad's.
    trackpad: bool,
    /// When the last scroll event arrived (events close together are one gesture).
    last: f64,
}

/// Whether one scroll event looks like a trackpad's rather than a wheel's notch. Trackpads
/// report in points on macOS, and as small, fractional or sideways amounts elsewhere; a
/// wheel reports whole notches straight up or down. (A free-spinning wheel or a touch mouse
/// can look like a trackpad, hence the preference.)
fn looks_like_trackpad(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> bool {
    match unit {
        egui::MouseWheelUnit::Point => {
            // Browsers report wheels in points too, a notch being 100 or so.
            !cfg!(target_arch = "wasm32") || delta.x != 0.0 || delta.y.abs() < 50.0
        }
        egui::MouseWheelUnit::Line => delta.x != 0.0 || (delta.y - delta.y.round()).abs() > 1e-3,
        egui::MouseWheelUnit::Page => false,
    }
}

impl Wheel {
    /// This frame's pan (in points, the way the content moves) and zoom factor.
    pub fn read(&mut self, ui: &egui::Ui) -> (egui::Vec2, f64) {
        ui.input(|i| {
            for event in &i.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = event {
                    let looks = looks_like_trackpad(*unit, *delta);
                    // A gesture that has looked like a trackpad's stays one: its fast
                    // middle can report whole numbers.
                    self.trackpad = looks || (self.trackpad && i.time - self.last < 0.3);
                    self.last = i.time;
                }
            }
            // Ctrl + scroll and pinches arrive as a zoom factor, with no scroll left over.
            let pinch = f64::from(i.zoom_delta());
            let scroll = i.smooth_scroll_delta;
            let pans = match self.mode {
                WheelMode::Auto => self.trackpad,
                WheelMode::Zoom => false,
                WheelMode::Pan => true,
            };
            if pans {
                (scroll, pinch)
            } else {
                (
                    egui::Vec2::ZERO,
                    (f64::from(scroll.y) * SCROLL_ZOOM_SPEED).exp() * pinch,
                )
            }
        })
    }
}

/// What one draw group of the canvas holds (see [`drift_render::DrawGroup`]).
#[derive(Default)]
struct DrawContent {
    fills: Vec<drift_core::FillMesh>,
    images: Vec<drift_core::CanvasImage>,
    items: Vec<drift_core::SketchItem>,
}

/// The automatic relation a guide adds, as the HUD names it.
fn relation_name(kind: GuideKind) -> Option<&'static str> {
    match kind {
        GuideKind::Horizontal => Some("Horizontal"),
        GuideKind::Vertical => Some("Vertical"),
        GuideKind::Perpendicular(_) => Some("Perpendicular"),
        GuideKind::Tangent(_) => Some("Tangent"),
        GuideKind::Aligned => None,
    }
}
