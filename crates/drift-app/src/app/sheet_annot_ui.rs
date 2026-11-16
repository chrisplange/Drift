//! A sheet's own annotations on the sheet canvas (#432): the Note and Dimension tools
//! working there, and picking, dragging, editing and deleting what they made.

use drift_core::{
    ArrowStyle, DVec2, LeaderEnd, NoteFrame, PlacedView, Sheet, SheetAnnotation, SheetAnnotationId,
    SheetId, TextAlign,
};
use drift_edit::sheet_annotations::{OnSheet, SheetPort, ToSheet, moved};
use eframe::egui::{self, PointerButton};

use crate::annotations::{AnnotationColors, AnnotationState, DimOwn};
use crate::app::sheet_editor::SheetPick;
use crate::overlay::ScreenMap;
use crate::sheet_notes::{self, SheetLayout, SheetShape};
use crate::*;

/// A sheet annotation being dragged: a note's text, or a dimension's label.
#[derive(Clone, Debug)]
pub(crate) struct AnnoDrag {
    pub sheet: SheetId,
    pub id: SheetAnnotationId,
    /// As it was when the drag began.
    pub was: SheetAnnotation,
    /// Where the drag began (screen).
    pub from: egui::Pos2,
    /// A dimension's label offset when the drag began (paper mm), placed or not.
    pub label: Option<DVec2>,
}

impl DriftApp {
    /// Whether the tool in hand works on a sheet: the Note tool (Leader is the same tool)
    /// and the Dimension tool. Any other is the Select tool there.
    pub(crate) fn sheet_tool(&self) -> bool {
        matches!(self.tool.kind(), ToolKind::Note | ToolKind::Dimension)
    }

    /// The sheet shown as a map: world is paper mm.
    pub(crate) fn paper_map(&self, rect: egui::Rect) -> ScreenMap {
        ScreenMap {
            rect,
            camera: Camera {
                center: self.drawing.view.preview.center,
                scale: f64::from(self.drawing.view.preview.zoom),
            },
        }
    }

    /// The annotation picked on the sheet shown, if it is still there.
    pub(crate) fn picked_sheet_annotation(&self) -> Option<(SheetId, SheetAnnotationId)> {
        let sheet = self.current_sheet()?;
        let id = self
            .drawing
            .view
            .sheet_annotation
            .filter(|_| self.drawing.view.sheet_pick == SheetPick::Annotation)?;
        self.drawing
            .editor
            .doc
            .sheet_annotation(sheet, id)
            .map(|_| (sheet, id))
    }

    /// Pick an annotation of the sheet shown (or none: the paper).
    pub(crate) fn pick_sheet_annotation(&mut self, id: Option<SheetAnnotationId>) {
        self.drop_empty_sheet_note(id);
        self.drawing.view.sheet_annotation = id;
        self.drawing.view.sheet_note_typing = None;
        self.drawing.view.view_set = None;
        self.drawing.confirm_delete_set = false;
        self.drawing.view.sheet_pick = match id {
            Some(_) => SheetPick::Annotation,
            None => SheetPick::Paper,
        };
    }

    /// A note left without text or leaders shows nothing and can't be picked: when the pick
    /// moves off one, it goes (its own undo step).
    fn drop_empty_sheet_note(&mut self, next: Option<SheetAnnotationId>) {
        let Some((sheet, id)) = self
            .picked_sheet_annotation()
            .filter(|(_, id)| Some(*id) != next)
        else {
            return;
        };
        let empty = matches!(
            self.drawing.editor.doc.sheet_annotation(sheet, id),
            Some(SheetAnnotation::Note { text, leaders, .. })
                if text.trim().is_empty() && leaders.is_empty()
        );
        if !empty {
            return;
        }
        // (By the note the step made, not by the step's name alone: the last step may have
        // placed another note.)
        let placed_last = self.drawing.view.sheet_note_placed == Some(id);
        let ed = &mut self.drawing.editor;
        if placed_last && ed.history.undo_label() == Some("Sheet note") {
            // Just placed and never typed in: taken back in the step that placed it, so
            // no undo brings an empty note back.
            ed.doc.begin();
            ed.doc.remove_sheet_annotation(sheet, id);
            let tx = ed.doc.commit("Sheet note");
            ed.history.merge_into_last(tx);
            ed.sync();
        } else {
            ed.delete_sheet_annotation(sheet, id);
        }
    }

    /// Delete the picked sheet annotation: one undo step. Returns whether one was picked.
    pub(crate) fn delete_picked_sheet_annotation(&mut self) -> bool {
        let Some((sheet, id)) = self.picked_sheet_annotation() else {
            return false;
        };
        self.drawing.editor.delete_sheet_annotation(sheet, id);
        self.drawing.view.sheet_annotation = None;
        self.drawing.view.sheet_pick = SheetPick::Paper;
        true
    }

    // ----- The model's annotations, onto the sheet (#432 part 2) ----------------------------

    /// The picked view of the sheet shown, as a viewport a tool can go through.
    fn picked_port(&self) -> Option<(SheetId, SheetPort)> {
        let id = self.current_sheet()?;
        let sheet = self.drawing.editor.doc.sheet(id)?;
        if self.drawing.view.sheet_pick != SheetPick::View || sheet.viewports.is_empty() {
            return None;
        }
        Some((id, SheetPort::of(sheet, self.current_view(sheet))?))
    }

    /// How many of the model's notes and dimensions the picked view shows: what **Move** and
    /// **Copy onto sheet** would bring.
    pub(crate) fn view_annotation_count(&self) -> usize {
        self.picked_port().map_or(0, |(_, port)| {
            self.drawing.editor.view_shows(port.view).len()
        })
    }

    /// Bring onto the sheet everything of the model's that the picked view shows: each note
    /// and dimension becomes the sheet's own, where it is on the paper. One undo step.
    pub(crate) fn bring_view_annotations(&mut self, how: ToSheet) {
        let Some((sheet, port)) = self.picked_port() else {
            return;
        };
        let items = self.drawing.editor.view_shows(port.view);
        self.bring_onto_sheet(sheet, port, &items, how);
    }

    /// Bring `items` of the model's onto `sheet` through `port`, and say what happened.
    pub(crate) fn bring_onto_sheet(
        &mut self,
        sheet: SheetId,
        port: SheetPort,
        items: &[drift_core::AnnotationRef],
        how: ToSheet,
    ) {
        if items.is_empty() {
            self.status_next =
                Some("This view shows no notes or dimensions of the model's".to_owned());
            return;
        }
        match self.drawing.editor.bring_to_sheet(sheet, port, items, how) {
            Ok(brought) => {
                // What arrived is picked, if it is one thing.
                if let [one] = brought.made[..] {
                    self.pick_sheet_annotation(Some(one));
                    self.show_panel(crate::dock::Panel::Inspector);
                }
                self.status_next = Some(brought.summary(how));
            }
            Err(e) => self.status_next = Some(format!("Not brought onto the sheet: {e}")),
        }
    }

    /// A right-click on the sheet at `pos`: the model's note or dimension under it in the
    /// view there, remembered for the menu.
    pub(crate) fn note_model_annotation_at(
        &mut self,
        painter: &egui::Painter,
        rect: egui::Rect,
        sheet: &Sheet,
        pos: egui::Pos2,
    ) {
        let map = self.paper_map(rect);
        let doc = &self.drawing.editor.doc;
        self.drawing.view.sheet_model_hit = sheet_notes::port_at(doc, sheet, map.to_world(pos))
            .and_then(|(index, view)| {
                let shown = self.drawing.editor.view_shows(view.id);
                let hit = sheet_notes::model_annotation_at(painter, doc, &map, &view, &shown, pos)?;
                Some((SheetPort::of(sheet, index)?, hit))
            });
    }

    /// The sheet's right-click menu entries for the model's annotations: the one under the
    /// pointer, and the picked view's.
    pub(crate) fn bring_menu(&mut self, ui: &mut egui::Ui) {
        let mut act = None;
        if let Some((port, r)) = self.drawing.view.sheet_model_hit {
            let what = match r {
                drift_core::AnnotationRef::Dimension(_) => "dimension",
                drift_core::AnnotationRef::Annotation(_) => "note",
            };
            for (how, verb) in [(ToSheet::Move, "Move"), (ToSheet::Copy, "Copy")] {
                if ui
                    .button(format!("{verb} this {what} onto the sheet"))
                    .clicked()
                {
                    act = Some((Some((port, r)), how));
                }
            }
            ui.separator();
        }
        let count = self.view_annotation_count();
        for (how, verb) in [(ToSheet::Move, "Move"), (ToSheet::Copy, "Copy")] {
            if ui
                .add_enabled(
                    count > 0,
                    egui::Button::new(format!(
                        "{verb} the view's notes and dimensions onto the sheet ({count})"
                    )),
                )
                .clicked()
            {
                act = Some((None, how));
            }
        }
        ui.separator();
        let Some((one, how)) = act else {
            return;
        };
        ui.close();
        match one {
            Some((port, r)) => {
                if let Some(sheet) = self.current_sheet() {
                    self.bring_onto_sheet(sheet, port, &[r], how);
                }
            }
            None => self.bring_view_annotations(how),
        }
        self.drawing.view.sheet_model_hit = None;
    }

    /// Give up a drag of a sheet annotation in progress: it goes back to where it was, and
    /// no undo step is made. Returns whether one was in progress.
    pub(crate) fn cancel_sheet_annotation_drag(&mut self) -> bool {
        if self.drawing.view.sheet_anno_drag.take().is_none() {
            return false;
        }
        self.drawing.editor.doc.rollback();
        self.drawing.editor.sync();
        true
    }

    /// The annotation of `layouts` under `pos`: the topmost (drawn last).
    fn sheet_annotation_at(layouts: &[SheetLayout], pos: egui::Pos2) -> Option<SheetAnnotationId> {
        layouts.iter().rev().find(|l| l.hit(pos)).map(|l| l.id)
    }

    /// Picking and dragging the sheet's annotations with the Select tool. Returns true when
    /// the pointer is theirs this frame, so the views under it are left alone.
    pub(crate) fn sheet_annotation_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        id: SheetId,
        layouts: &[SheetLayout],
    ) -> bool {
        // A view being moved or a box being dragged goes on undisturbed.
        if self.drawing.view.view_drag.is_some() || self.drawing.view.view_box.is_some() {
            return false;
        }
        let zoom = f64::from(self.drawing.view.preview.zoom);
        if response.drag_started_by(PointerButton::Primary)
            && let Some(from) = ui.input(|i| i.pointer.press_origin())
            && let Some(hit) = Self::sheet_annotation_at(layouts, from)
            && let Some(was) = self.drawing.editor.doc.sheet_annotation(id, hit).cloned()
        {
            // A dimension's label, where it is now (placed, or where a new one's goes).
            let label = match (&was, layouts.iter().find(|l| l.id == hit)) {
                (SheetAnnotation::Dimension { label: Some(l), .. }, _) => Some(*l),
                (SheetAnnotation::Dimension { .. }, Some(l)) => match &l.shape {
                    SheetShape::Dimension(d) => Some(
                        l.space
                            .default_offset(&self.drawing.editor.doc, &d.constraint)
                            / l.space.world_per_paper,
                    ),
                    SheetShape::Note(_) => None,
                },
                _ => None,
            };
            self.pick_sheet_annotation(Some(hit));
            self.drawing.view.sheet_anno_drag = Some(AnnoDrag {
                sheet: id,
                id: hit,
                was,
                from,
                label,
            });
            self.drawing.editor.doc.begin();
        }
        if let Some(drag) = self.drawing.view.sheet_anno_drag.clone() {
            if let Some(pos) = response.interact_pointer_pos() {
                let d = pos - drag.from;
                let by = DVec2::new(f64::from(d.x), -f64::from(d.y)) / zoom;
                let to = moved(&drag.was, by, drag.label);
                // (Refused only if it isn't there any more: then it stays as it was.)
                let _ = self
                    .drawing
                    .editor
                    .doc
                    .set_sheet_annotation(drag.sheet, drag.id, to);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.drawing.view.sheet_anno_drag = None;
                let label = match drag.was {
                    SheetAnnotation::Note { .. } => "Move sheet note",
                    SheetAnnotation::Dimension { .. } => "Move sheet dimension",
                };
                self.drawing.editor.commit(label);
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return true;
        }
        let over = response
            .hover_pos()
            .and_then(|p| Self::sheet_annotation_at(layouts, p));
        self.drawing.view.sheet_anno_hover = over;
        let primary = response.clicked_by(PointerButton::Primary);
        let secondary = response.clicked_by(PointerButton::Secondary);
        if (primary || secondary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            match Self::sheet_annotation_at(layouts, pos) {
                Some(hit) => {
                    self.pick_sheet_annotation(Some(hit));
                    self.show_panel(crate::dock::Panel::Inspector);
                    // A double-click on a note: its text, to type.
                    if response.double_clicked_by(PointerButton::Primary) {
                        self.drawing.view.focus_sheet_note = true;
                    }
                    return true;
                }
                // Off every annotation: the pick goes to what the click is on (a view, or
                // the paper), which the views' own input decides.
                None if self.drawing.view.sheet_pick == SheetPick::Annotation => {
                    self.pick_sheet_annotation(None);
                }
                None => {}
            }
        }
        if over.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            return true;
        }
        false
    }

    /// The viewport the tool in hand sees the model through, for a pointer at `paper`: the
    /// one a Dimension tool's first pick was made in while it has one (its label may be
    /// placed outside the view's frame), else the one under the pointer.
    fn sheet_tool_viewport(
        &mut self,
        sheet: &Sheet,
        paper: DVec2,
    ) -> Option<(SheetPort, PlacedView)> {
        let held = self
            .drawing
            .view
            .sheet_tool_port
            .filter(|_| self.tool.is_busy() && self.tool.kind() == ToolKind::Dimension);
        // The viewport a pick was made through is gone, moved or shows another view (an
        // undo, the Inspector): the pick is let go rather than finished through another.
        if held.is_some_and(|port| port.index_on(sheet).is_none()) {
            self.tool_cancel();
            self.drawing.view.sheet_tool_port = None;
            self.status_next = Some("The view changed: pick again".to_owned());
        }
        let doc = &self.drawing.editor.doc;
        let placed = |i: usize| {
            let p = sheet.viewports.get(i)?;
            Some((
                SheetPort::of(sheet, i)?,
                PlacedView::new(p.view, doc.view(p.view)?, p),
            ))
        };
        let locked = self
            .drawing
            .view
            .sheet_tool_port
            .filter(|_| self.tool.is_busy() && self.tool.kind() == ToolKind::Dimension)
            .and_then(|port| placed(port.index_on(sheet)?));
        let found =
            locked.or_else(|| sheet_notes::port_at(doc, sheet, paper).and_then(|(i, _)| placed(i)));
        self.drawing.view.sheet_tool_port = found.as_ref().map(|(port, _)| *port);
        found
    }

    /// The Note or Dimension tool on the sheet: the pointer is mapped through the viewport
    /// under it and the tool picks in the model as it does there; what it makes is the
    /// sheet's.
    pub(crate) fn sheet_tool_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        id: SheetId,
        sheet: &Sheet,
    ) {
        let Some(pos) = response.hover_pos().or(response.interact_pointer_pos()) else {
            return;
        };
        let map = self.paper_map(rect);
        let paper = map.to_world(pos);
        let port = self.sheet_tool_viewport(sheet, paper);
        // Through a view: the model point there, picked within the same few pixels as in
        // the model. Off every view the model position means nothing; the paper's stands in.
        let (world, tolerance) = match &port {
            Some((_, v)) => (v.to_world(paper), PICK_RADIUS * v.scale / map.camera.scale),
            None => (paper, PICK_RADIUS / map.camera.scale),
        };
        let mut pointer = Pointer::new(world, tolerance);
        pointer.relations = false;
        pointer.units = self.drawing.editor.doc.units();
        let (shift, command, alt) =
            ui.input(|i| (i.modifiers.shift, i.modifiers.command, i.modifiers.alt));
        (pointer.shift, pointer.command, pointer.alt) = (shift, command, alt);
        if port.is_some() && !alt && self.tool.wants_snap() {
            let ed = &self.drawing.editor;
            let tool = &self.tool;
            pointer.snap = ed.index.snap(
                world,
                SnapOptions {
                    tolerance,
                    grid: None,
                    from: None,
                },
                |e| tool.snap_excludes(ed, e),
            );
        }
        // Set for the tool's own handling only, so nothing made in the model is ever
        // mistaken for the sheet's.
        let on = OnSheet {
            sheet: id,
            viewport: port.as_ref().map(|(port, _)| *port),
            paper,
        };
        let mods = Modifiers {
            shift,
            command,
            alt,
        };
        let primary = response.clicked_by(PointerButton::Primary);
        let secondary = response.clicked_by(PointerButton::Secondary);
        self.drawing.editor.on_sheet = Some(on);
        self.tool_input(Input::Hover(pointer));
        if primary {
            // A note just placed and left empty goes before this click makes anything, so
            // it is taken back in its own step and not in the next note's.
            if self.picked_sheet_annotation().is_some() {
                self.pick_sheet_annotation(None);
            }
            self.tool_input(Input::Click(pointer, mods));
        } else if secondary {
            self.with_tool(|tool, ed| tool.secondary_click(ed));
        }
        self.drawing.editor.on_sheet = None;
        self.drawing.view.pointer = Some(pointer);
        self.drawing.editor.sync();
        // What it just made is picked; a note's text is typed in the Inspector.
        if let Some(made) = self.drawing.editor.out.new_sheet_annotation.take() {
            self.pick_sheet_annotation(Some(made));
            self.show_panel(crate::dock::Panel::Inspector);
        }
        if let Some(note) = self.drawing.editor.out.pending_sheet_note_edit.take() {
            self.drawing.view.focus_sheet_note = true;
            self.drawing.view.sheet_note_placed = Some(note);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }

    /// Over the sheet as drawn: the picked and hovered annotations in the pick colours, and
    /// what the tool in hand is about to make.
    pub(crate) fn paint_sheet_annotations(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        sheet: &Sheet,
        layouts: &[SheetLayout],
        dark: bool,
    ) {
        let colors = AnnotationColors::new(dark);
        let picked = self.picked_sheet_annotation().map(|(_, id)| id);
        let hovered = self
            .drawing
            .view
            .sheet_anno_hover
            .filter(|_| !self.sheet_tool());
        for l in layouts {
            let state = if Some(l.id) == picked {
                AnnotationState::Selected
            } else if Some(l.id) == hovered {
                AnnotationState::Hovered
            } else {
                continue;
            };
            l.paint(painter, state, &colors);
        }
        if !self.sheet_tool() {
            return;
        }
        let doc = &self.drawing.editor.doc;
        let map = self.paper_map(rect);
        let accent = theme::accent(dark);
        let port = self.drawing.view.sheet_tool_port.and_then(|port| {
            let p = sheet.viewports.get(port.index_on(sheet)?)?;
            Some(PlacedView::new(p.view, doc.view(p.view)?, p))
        });
        // The view the tool is picking in.
        if let Some(v) = &port {
            let b = v.frame.bounds();
            let frame = egui::Rect::from_two_pos(map.to_screen(b.min), map.to_screen(b.max));
            painter.rect_stroke(
                frame,
                0.0,
                egui::Stroke::new(1.0, accent.gamma_multiply(0.6)),
                egui::StrokeKind::Outside,
            );
        }
        let Some(pointer) = self.drawing.view.pointer else {
            return;
        };
        // What the tool has picked, and what is under the pointer: drawn over the view.
        if let Some(v) = &port {
            let through = sheet_notes::port_map(&map, v);
            let ed = &self.drawing.editor;
            let lit = ed
                .highlight
                .iter()
                .copied()
                .chain(ed.hovered)
                .chain(ed.hovered_midpoint)
                .chain(pointer.snapped_point());
            for e in lit {
                paint_entity(painter, &through, doc, e, accent);
            }
            // A dimension being placed, as it will sit.
            if let Some((c, offset)) = self.tool.pending_dimension(ed, Some(&pointer)) {
                let own = DimOwn {
                    style: doc.draft_style(),
                    note: Default::default(),
                };
                let n = if v.scale.is_finite() && v.scale > 0.0 {
                    v.scale
                } else {
                    1.0
                };
                if let Some((layout, space)) = sheet_notes::layout_sheet_dimension(
                    painter,
                    doc,
                    &map,
                    v,
                    &c,
                    Some(offset / n),
                    &own,
                ) {
                    space.paint_dimension(painter, &layout, AnnotationState::Preview, &colors);
                }
            }
        }
        // A note's leaders so far, from their tips to the pointer.
        let to = match &port {
            Some(v) => map.to_screen(v.to_paper(pointer.raw)),
            None => map.to_screen(pointer.raw),
        };
        for t in self.tool.sheet_targets() {
            let tip = t
                .viewport
                .and_then(|port| {
                    let p = sheet.viewports.get(port.index_on(sheet)?)?;
                    let v = PlacedView::new(p.view, doc.view(p.view)?, p);
                    Some(v.to_paper(doc.anchor_pos(&t.to)?))
                })
                .map(|p| map.to_screen(p));
            if let Some(tip) = tip {
                painter.line_segment([tip, to], egui::Stroke::new(1.0, accent));
                painter.circle_filled(tip, 3.0, accent);
            }
        }
    }

    /// What the tool in hand does on the sheet, at the top of the canvas.
    pub(crate) fn paint_sheet_tool_hint(&self, painter: &egui::Painter, rect: egui::Rect) {
        if !self.sheet_tool() {
            return;
        }
        let dark = painter.ctx().theme() == egui::Theme::Dark;
        let n = theme::neutrals(dark);
        let busy = self.tool.is_busy();
        let text = match self.tool.kind() {
            ToolKind::Dimension if busy => {
                "Click where the dimension goes, or another point or line · Esc cancels"
            }
            ToolKind::Dimension => {
                "Sheet dimension: click a line, circle or arc inside a view, or two points · \
                 Esc for Select"
            }
            _ if busy => {
                "Click where the text goes · Shift+click more geometry for more leaders · \
                 Esc cancels"
            }
            _ => {
                "Sheet note: click the paper, or geometry inside a view first for a leader \
                 (Alt+click: to that spot) · Esc for Select"
            }
        };
        let galley = painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(theme::size::BODY),
            n.text,
        );
        // (Under the paper's own label at the canvas's top left.)
        let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 44.0);
        let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(12.0, 6.0));
        painter.rect_filled(back, theme::radius::MENU, n.raised);
        painter.rect_stroke(
            back,
            theme::radius::MENU,
            egui::Stroke::new(1.0, theme::accent(dark)),
            egui::StrokeKind::Inside,
        );
        painter.galley(at, galley, n.text);
    }

    /// Write a changed annotation back: one undo step, or (typing) merged into the step the
    /// same annotation's typing began.
    fn update_sheet_annotation(
        &mut self,
        sheet: SheetId,
        id: SheetAnnotationId,
        a: SheetAnnotation,
        typing: bool,
    ) {
        let doc = &self.drawing.editor.doc;
        if doc.sheet_annotation(sheet, id).is_none_or(|was| *was == a) {
            return;
        }
        let label = match (&a, typing) {
            (SheetAnnotation::Note { .. }, true) => "Edit sheet note text",
            (SheetAnnotation::Note { .. }, false) => "Edit sheet note",
            (SheetAnnotation::Dimension { .. }, true) => "Edit sheet dimension text",
            (SheetAnnotation::Dimension { .. }, false) => "Edit sheet dimension",
        };
        self.drawing.editor.doc.begin();
        if self
            .drawing
            .editor
            .doc
            .set_sheet_annotation(sheet, id, a)
            .is_err()
        {
            self.drawing.editor.doc.rollback();
            return;
        }
        let tx = self.drawing.editor.doc.commit(label);
        let same = self.drawing.view.sheet_note_typing == Some(id)
            && self.drawing.editor.history.undo_label() == Some(label);
        if typing && same {
            self.drawing.editor.history.merge_into_last(tx);
        } else {
            self.drawing.editor.history.push(tx);
        }
        self.drawing.view.sheet_note_typing = typing.then_some(id);
        self.drawing.editor.sync();
    }

    /// The Inspector for the picked sheet annotation: its text, how it is set, and Delete.
    pub(crate) fn sheet_annotation_inspector(&mut self, ui: &mut egui::Ui, label: &str) {
        let Some((sheet, id)) = self.picked_sheet_annotation() else {
            return;
        };
        let Some(original) = self.drawing.editor.doc.sheet_annotation(sheet, id).cloned() else {
            return;
        };
        let mut a = original.clone();
        let mut typing = false;
        let scale_text = |doc: &drift_core::Document, a: &SheetAnnotation| {
            let s = doc.sheet(sheet)?;
            let port = a.ports().first().copied()?;
            let v = doc.port_view(s, port)?;
            Some(format!(
                "{} · {}",
                v.title(),
                drift_core::sheets::scale_label(v.scale, !doc.units().length.is_metric())
            ))
        };
        let through = scale_text(&self.drawing.editor.doc, &a);
        let draft = self.drawing.editor.doc.draft_style();
        crate::app::panels::title(ui, None, a.type_name(), &format!("On {label}"));
        match &mut a {
            SheetAnnotation::Note {
                text,
                leaders,
                style,
                ..
            } => {
                let field = ui.add(
                    egui::TextEdit::multiline(text)
                        .id_salt("sheet-note-text")
                        .hint_text("Type the note")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.drawing.view.focus_sheet_note) {
                    field.request_focus();
                }
                typing |= field.changed();
                ui.add_space(6.0);
                egui::Grid::new("sheet-note-style")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Text height");
                        let mut h = style.height.unwrap_or(draft.text_height);
                        if ui
                            .add(
                                egui::DragValue::new(&mut h)
                                    .range(0.5..=100.0)
                                    .speed(0.1)
                                    .suffix(" mm"),
                            )
                            .on_hover_text("The height of its capitals on the paper")
                            .changed()
                        {
                            style.height = Some(h);
                        }
                        ui.end_row();
                        ui.label("Align");
                        ui.horizontal(|ui| {
                            for al in TextAlign::ALL {
                                ui.selectable_value(&mut style.align, al, al.label());
                            }
                        });
                        ui.end_row();
                        ui.label("Frame");
                        egui::ComboBox::from_id_salt("sheet-note-frame")
                            .selected_text(style.frame.label())
                            .show_ui(ui, |ui| {
                                for f in NoteFrame::ALL {
                                    ui.selectable_value(&mut style.frame, f, f.label());
                                }
                            });
                        ui.end_row();
                        ui.label("Style");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut style.bold, "Bold");
                            ui.checkbox(&mut style.italic, "Italic");
                        });
                        ui.end_row();
                        ui.label("Mask");
                        ui.checkbox(&mut style.mask, "Paper behind the text")
                            .on_hover_text("So it reads over lines and hatches");
                        ui.end_row();
                        if !leaders.is_empty() {
                            ui.label("Leader end");
                            let mut end = leaders[0].leader.end;
                            egui::ComboBox::from_id_salt("sheet-note-end")
                                .selected_text(end.label())
                                .show_ui(ui, |ui| {
                                    for e in LeaderEnd::ALL {
                                        ui.selectable_value(&mut end, e, e.label());
                                    }
                                });
                            for l in leaders.iter_mut() {
                                l.leader.end = end;
                            }
                            ui.end_row();
                        }
                    });
                if !leaders.is_empty() {
                    let tied = leaders.iter().filter(|l| l.port.is_some()).count();
                    let what = match (leaders.len(), tied) {
                        (1, 1) => "Its leader is tied to the model through a view: it follows \
                                   the geometry, the view and its scale."
                            .to_owned(),
                        (n, 0) => format!("{n} leader{} to the paper.", plural(n)),
                        (n, t) => format!("{n} leader{}, {t} tied to the model.", plural(n)),
                    };
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(what).small().weak());
                    if ui
                        .button("Remove leaders")
                        .on_hover_text("The note keeps its text where it is")
                        .clicked()
                    {
                        leaders.clear();
                    }
                }
            }
            SheetAnnotation::Dimension { note, look, .. } => {
                if let Some(through) = &through {
                    ui.label(egui::RichText::new(format!("Measures through {through}")).weak());
                }
                ui.label(
                    egui::RichText::new(
                        "A reference dimension: it reads the model's true size and never \
                         holds it.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                egui::Grid::new("sheet-dim-text")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        for (name, field, hint) in [
                            ("Prefix", &mut note.prefix, "4×"),
                            ("Suffix", &mut note.suffix, "TYP"),
                            ("Below", &mut note.below, "TO FACE OF STUD"),
                            ("Text", &mut note.text, "instead of the value"),
                        ] {
                            ui.label(name);
                            typing |= ui
                                .add(
                                    egui::TextEdit::singleline(field)
                                        .hint_text(hint)
                                        .desired_width(150.0),
                                )
                                .changed();
                            ui.end_row();
                        }
                        ui.label("Text height");
                        let mut h = look.text_height.unwrap_or(draft.text_height);
                        if ui
                            .add(
                                egui::DragValue::new(&mut h)
                                    .range(0.5..=100.0)
                                    .speed(0.1)
                                    .suffix(" mm"),
                            )
                            .changed()
                        {
                            look.text_height = Some(h);
                        }
                        ui.end_row();
                        ui.label("Arrows");
                        let mut arrow = look.arrow.unwrap_or(draft.arrow);
                        egui::ComboBox::from_id_salt("sheet-dim-arrow")
                            .selected_text(arrow.label())
                            .show_ui(ui, |ui| {
                                for s in ArrowStyle::ALL {
                                    ui.selectable_value(&mut arrow, s, s.label());
                                }
                            });
                        if arrow != look.arrow.unwrap_or(draft.arrow) {
                            look.arrow = Some(arrow);
                        }
                        ui.end_row();
                    });
                if !look.is_empty()
                    && ui
                        .button("Follow the drawing's style")
                        .on_hover_text("Drop this dimension's own text height and arrows")
                        .clicked()
                {
                    *look = Default::default();
                }
            }
        }
        ui.add_space(16.0);
        let delete = theme::danger_button(ui, "Delete")
            .on_hover_text("Delete it from this sheet (Delete)")
            .clicked();
        if delete {
            self.delete_picked_sheet_annotation();
        } else if a != original {
            self.update_sheet_annotation(sheet, id, a, typing);
        }
        ui.add_space(10.0);
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A model entity drawn through a view's map on the sheet, to show what a tool picked or is
/// over there.
fn paint_entity(
    painter: &egui::Painter,
    map: &ScreenMap,
    doc: &drift_core::Document,
    e: drift_core::EntityId,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(2.0, color);
    let s = |p: DVec2| map.to_screen(p);
    match doc.shape(e) {
        Some(Shape::Point(p)) => {
            painter.circle_filled(s(p), 4.0, color);
        }
        Some(Shape::Line { a, b }) => {
            painter.line_segment([s(a), s(b)], stroke);
        }
        Some(shape) => {
            // Any curve, as a run of short lines (within half a point of it on screen).
            let points: Vec<egui::Pos2> = shape
                .flatten(0.5 / map.camera.scale)
                .into_iter()
                .map(s)
                .collect();
            if points.len() > 1 {
                painter.add(egui::Shape::line(points, stroke));
            }
        }
        None => {}
    }
}
