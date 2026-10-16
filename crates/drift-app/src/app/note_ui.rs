//! Drafting annotations in the app: picking, dragging and editing notes and callouts on the
//! canvas, the note text editor, and the inspector for a picked annotation.

use drift_core::{
    Anchor, Annotation, AnnotationId, Leader, LeaderEnd, LeaderShape, NoteFrame, OrdinateAxis,
    TextAlign,
};

use crate::notes::{self, NoteLayout};
use crate::*;

/// A note whose text is being typed on the canvas.
pub(crate) struct NoteEdit {
    pub id: AnnotationId,
    pub text: String,
    pub focus: bool,
    /// The note was just created: an empty result removes it again.
    pub new: bool,
}

/// A grip on a picked note: a leader's tip (to re-attach it), a bent leader's bend, or a "+"
/// beside the text (drag from it to add a leader).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LeaderGrip {
    Tip(usize),
    Bend(usize),
    New(egui::Pos2),
}

/// How far a "+" grip sits from the side of a note's text (points).
const PLUS_OUT: f32 = 14.0;

/// A mark's label and the sheet it refers to.
fn mark_fields(ui: &mut egui::Ui, label: &mut String, sheet: &mut String, example: &str) {
    theme::row(ui, "Label", |ui| {
        ui.add(
            egui::TextEdit::singleline(label)
                .hint_text(example)
                .desired_width(60.0),
        );
    });
    theme::row(ui, "On sheet", |ui| {
        ui.add(
            egui::TextEdit::singleline(sheet)
                .hint_text("A-201, 02…")
                .desired_width(ui.available_width() - 8.0),
        )
        .on_hover_text("The sheet the view is drawn on: written in the lower half of the flag");
    });
}

impl DriftApp {
    pub(crate) fn layout_notes(&self, painter: &egui::Painter, map: &ScreenMap) -> Vec<NoteLayout> {
        if !self.show_dimensions {
            return Vec::new();
        }
        notes::layout_annotations(painter, &self.editor.doc, map)
            .into_iter()
            .filter(|l| self.on_canvas(drift_core::AnnotationRef::Annotation(l.id)))
            .collect()
    }

    /// Hover, pick, drag and double-click annotations. Dimensions and relation glyphs under
    /// the pointer take precedence. Returns true if the input was used.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn note_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        layouts: &[NoteLayout],
        dims: &[DimLayout],
        glyphs: &[annotations::GlyphLayout],
        pos: Option<egui::Pos2>,
    ) -> bool {
        let idle = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        if idle && self.leader_input(ui, response, map, layouts, pos) {
            return true;
        }
        let blocked = |p: egui::Pos2| {
            dims.iter().any(|d| d.text_rect.contains(p))
                || glyphs.iter().any(|g| g.rect.contains(p))
        };
        let note_at = |p: egui::Pos2| {
            if blocked(p) {
                None
            } else {
                layouts.iter().rev().find(|l| l.hit(p)).map(|l| l.id)
            }
        };
        self.editor.hovered_annotation =
            if idle && self.note_drag.is_none() && self.label_drag.is_none() {
                pos.and_then(note_at)
            } else {
                None
            };
        if idle
            && self.note_drag.is_none()
            && self.label_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(id) = note_at(origin)
            && let Some(a) = self.editor.doc.annotation(id)
            && let Some(label) = notes::label_world(&self.editor.doc, a)
        {
            self.note_drag = Some((id, map.to_world(origin) - label));
            self.editor.selected_annotation = Some(id);
            self.editor.doc.begin();
        }
        if let Some((id, grab)) = self.note_drag {
            if let (Some(p), Some(a)) = (
                response.interact_pointer_pos(),
                self.editor.doc.annotation(id).cloned(),
            ) && let Some(moved) =
                notes::with_label_at(&self.editor.doc, &a, map.to_world(p) - grab)
            {
                let _ = self.editor.doc.set_annotation(id, moved);
            }
            if !response.dragged_by(PointerButton::Primary) {
                self.note_drag = None;
                self.editor.commit("Move note");
            }
            return true;
        }
        if !idle {
            return false;
        }
        let at = release_pos(ui, PointerButton::Primary).or(pos);
        if response.double_clicked_by(PointerButton::Primary)
            && let Some(id) = at.and_then(note_at)
        {
            self.open_note_edit(id, false);
            return true;
        }
        if response.clicked_by(PointerButton::Primary) {
            if let Some(id) = at.and_then(note_at) {
                // On one of its leaders: that leader is picked too (Delete removes it).
                self.selected_leader = at.and_then(|p| {
                    let l = layouts.iter().find(|l| l.id == id)?;
                    let on_text = l.text.as_ref().is_some_and(|t| t.rect.contains(p));
                    let note = matches!(
                        self.editor.doc.annotation(id),
                        Some(Annotation::Note { .. })
                    );
                    (note && !on_text)
                        .then(|| l.leader_at(p))
                        .flatten()
                        .map(|k| (id, k))
                });
                if crate::app::visibility_ui::shift_click(ui) {
                    self.toggle_annotation_pick(drift_core::AnnotationRef::Annotation(id));
                } else {
                    self.annotation_picks.clear();
                    self.editor.selected_annotation = Some(id);
                    self.selected_constraint = None;
                }
                self.editor.selection.clear();
                self.editor.selected_fills.clear();
                self.editor.selected_region = None;
                return true;
            }
            self.editor.selected_annotation = None;
            self.selected_leader = None;
        }
        false
    }

    /// The grips of the picked note, on screen.
    fn note_grips(&self, layouts: &[NoteLayout]) -> Vec<(egui::Pos2, LeaderGrip)> {
        let Some(id) = self.editor.selected_annotation else {
            return Vec::new();
        };
        if !matches!(
            self.editor.doc.annotation(id),
            Some(Annotation::Note { .. })
        ) {
            return Vec::new();
        }
        let Some(l) = layouts.iter().find(|l| l.id == id) else {
            return Vec::new();
        };
        let mut grips = Vec::new();
        for (k, leader) in l.leaders.iter().enumerate() {
            if let Some(tip) = leader.points.first() {
                grips.push((*tip, LeaderGrip::Tip(k)));
            }
            if let Some(bend) = leader.bend {
                grips.push((bend, LeaderGrip::Bend(k)));
            }
        }
        if let Some(t) = &l.text {
            for x in [t.rect.left() - PLUS_OUT, t.rect.right() + PLUS_OUT] {
                let at = egui::pos2(x, t.rect.center().y);
                grips.push((at, LeaderGrip::New(at)));
            }
        }
        grips
    }

    /// Where a leader dropped at `pos` points: a snapped point, a spot on a curve, or free.
    fn leader_anchor(&self, map: &ScreenMap, pos: egui::Pos2) -> Anchor {
        let raw = map.to_world(pos);
        let tolerance = PICK_RADIUS / self.camera.scale;
        let options = drift_geom::SnapOptions {
            tolerance,
            grid: self
                .grid_snap
                .then(|| drift_render::grid_info(&self.camera, self.imperial()).minor),
        };
        let mut p = Pointer::new(raw, tolerance);
        p.relations = true;
        p.snap = self.editor.snap(raw, options, |_| false);
        if let Some(point) = p.snapped_point() {
            return Anchor::Point(point);
        }
        // Any line or curve under the pointer, at the spot nearest it.
        let doc = &self.editor.doc;
        match crate::tools::reference_at(&self.editor, &p) {
            Some(id) if doc.get(id).is_some_and(|e| e.kind.is_point()) => Anchor::Point(id),
            Some(curve) => {
                let at = doc
                    .shape(curve)
                    .map_or(raw, |s| drift_geom::nearest_point(&s, raw));
                notes::anchor_at(doc, None, Some(curve), at)
            }
            None => notes::anchor_at(doc, None, p.on_curve(), p.pos()),
        }
    }

    /// Change a note's leaders, keeping its text where it is (inside the caller's
    /// transaction).
    fn change_leaders(&mut self, id: AnnotationId, f: impl FnOnce(&mut Vec<Leader>)) {
        let doc = &self.editor.doc;
        let Some(Annotation::Note {
            text,
            at,
            mut leaders,
            style,
        }) = doc.annotation(id).cloned()
        else {
            return;
        };
        let Some(pos) = doc.note_text_pos(at, &leaders) else {
            return;
        };
        f(&mut leaders);
        let Some(at) = doc.note_at(pos, &leaders) else {
            return;
        };
        let _ = self.editor.doc.set_annotation(
            id,
            Annotation::Note {
                text,
                at,
                leaders,
                style,
            },
        );
    }

    /// Remove a note's leader (one undo step); the text stays where it is.
    pub(crate) fn remove_leader(&mut self, id: AnnotationId, k: usize) {
        self.editor.doc.begin();
        self.change_leaders(id, |leaders| {
            if k < leaders.len() {
                leaders.remove(k);
            }
        });
        self.editor.commit("Remove leader");
        self.selected_leader = None;
    }

    /// The picked note's grips, and a click that adds a leader after Add leader: returns
    /// true if it used the input.
    fn leader_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
        layouts: &[NoteLayout],
        pos: Option<egui::Pos2>,
    ) -> bool {
        let released = || release_pos(ui, PointerButton::Primary).or(pos);
        // After Add leader: the next click says what it points to.
        if let Some(id) = self.adding_leader {
            if pos.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if response.clicked_by(PointerButton::Primary)
                && let Some(at) = released()
            {
                let to = self.leader_anchor(map, at);
                self.editor.doc.begin();
                self.change_leaders(id, |leaders| leaders.push(Leader::to(to)));
                self.editor.commit("Add leader");
                self.adding_leader = None;
                return true;
            }
        }
        let grips = self.note_grips(layouts);
        let grip_at = |p: egui::Pos2| {
            grips
                .iter()
                .find(|(g, _)| g.distance(p) <= 7.0)
                .map(|(_, grip)| *grip)
        };
        if let Some(grip) = pos.and_then(grip_at) {
            ui.ctx().set_cursor_icon(match grip {
                LeaderGrip::New(_) => egui::CursorIcon::Crosshair,
                _ => egui::CursorIcon::Grab,
            });
        }
        if self.leader_grip.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(grip) = grip_at(origin)
            && let Some(id) = self.editor.selected_annotation
        {
            self.leader_grip = Some((id, grip));
            self.editor.doc.begin();
        }
        let Some((id, grip)) = self.leader_grip else {
            return false;
        };
        if let Some(p) = response.interact_pointer_pos() {
            match grip {
                LeaderGrip::Tip(k) => {
                    let to = self.leader_anchor(map, p);
                    self.change_leaders(id, |leaders| {
                        if let Some(l) = leaders.get_mut(k) {
                            l.to = to;
                        }
                    });
                }
                LeaderGrip::Bend(k) => {
                    let at = map.to_world(p);
                    let doc = &self.editor.doc;
                    let tip = match doc.annotation(id) {
                        Some(Annotation::Note { leaders, .. }) => {
                            leaders.get(k).and_then(|l| doc.anchor_pos(&l.to))
                        }
                        _ => None,
                    };
                    if let Some(tip) = tip {
                        self.change_leaders(id, |leaders| {
                            if let Some(l) = leaders.get_mut(k) {
                                l.shape = LeaderShape::Bent(at - tip);
                            }
                        });
                    }
                }
                // Drawn by `paint_notes` until it's let go.
                LeaderGrip::New(_) => {}
            }
        }
        if !response.dragged_by(PointerButton::Primary) {
            let label = match grip {
                LeaderGrip::New(_) => {
                    if let Some(at) = released() {
                        let to = self.leader_anchor(map, at);
                        self.change_leaders(id, |leaders| leaders.push(Leader::to(to)));
                    }
                    "Add leader"
                }
                LeaderGrip::Tip(_) => "Move leader",
                LeaderGrip::Bend(_) => "Bend leader",
            };
            self.leader_grip = None;
            self.editor.commit(label);
        }
        true
    }

    pub(crate) fn paint_notes(&self, painter: &egui::Painter, layouts: &[NoteLayout], dark: bool) {
        let draft = self.editor.doc.draft_style();
        let colors = AnnotationColors::drafting(dark, draft.color);
        let faded = colors.faded();
        let style = annotations::DimStyle::of(&draft);
        for l in layouts {
            if self.note_edit.as_ref().is_some_and(|e| e.id == l.id) {
                continue;
            }
            let r = drift_core::AnnotationRef::Annotation(l.id);
            let state = if self.annotation_picked(r) || self.note_drag.is_some_and(|d| d.0 == l.id)
            {
                AnnotationState::Selected
            } else if self.editor.hovered_annotation == Some(l.id) {
                AnnotationState::Hovered
            } else {
                AnnotationState::Normal
            };
            let shown = self.editor.doc.shown(r);
            let look = if shown.canvas { &colors } else { &faded };
            notes::paint_annotation(painter, l, state, look, &style);
            if self.selected_leader.is_some_and(|(id, _)| id == l.id)
                && let Some(leader) = self.selected_leader.and_then(|(_, k)| l.leaders.get(k))
            {
                let picked = egui::Stroke::new(2.5, look.selected);
                notes::paint_leader(painter, leader, picked, look.selected, &style);
            }
            if !shown.sheets
                && let Some(at) = crate::app::visibility_ui::note_badge_at(l)
            {
                Self::paint_off_sheets_badge(painter, at, look.normal);
            }
        }
    }

    /// The picked note's grips, and a leader being drawn from a "+" grip.
    pub(crate) fn paint_note_grips(
        &self,
        painter: &egui::Painter,
        layouts: &[NoteLayout],
        dark: bool,
    ) {
        let colors = annotations::AnnotationColors::new(dark);
        let accent = theme::accent(dark);
        for (at, grip) in self.note_grips(layouts) {
            match grip {
                LeaderGrip::Tip(_) => {
                    let r = egui::Rect::from_center_size(at, egui::Vec2::splat(7.0));
                    painter.rect_filled(r, 1.0, colors.text_background);
                    painter.rect_stroke(
                        r,
                        1.0,
                        egui::Stroke::new(1.2, accent),
                        egui::StrokeKind::Inside,
                    );
                }
                LeaderGrip::Bend(_) => {
                    painter.circle(
                        at,
                        3.5,
                        colors.text_background,
                        egui::Stroke::new(1.2, accent),
                    );
                }
                LeaderGrip::New(_) => {
                    painter.circle(
                        at,
                        6.0,
                        colors.text_background,
                        egui::Stroke::new(1.2, accent),
                    );
                    let s = egui::Stroke::new(1.4, accent);
                    painter.line_segment([at - egui::vec2(3.0, 0.0), at + egui::vec2(3.0, 0.0)], s);
                    painter.line_segment([at - egui::vec2(0.0, 3.0), at + egui::vec2(0.0, 3.0)], s);
                }
            }
        }
        if let Some((_, LeaderGrip::New(from))) = self.leader_grip
            && let Some(to) = painter.ctx().input(|i| i.pointer.latest_pos())
        {
            painter.extend(egui::Shape::dashed_line(
                &[from, to],
                egui::Stroke::new(1.0, accent),
                4.0,
                3.0,
            ));
        }
    }

    /// Start typing a note's text on the canvas (other annotations open the inspector).
    pub(crate) fn open_note_edit(&mut self, id: AnnotationId, new: bool) {
        match self.editor.doc.annotation(id) {
            Some(Annotation::Note { text, .. }) => {
                self.note_edit = Some(NoteEdit {
                    id,
                    text: text.clone(),
                    focus: true,
                    new,
                });
                self.editor.selected_annotation = Some(id);
            }
            Some(_) => {
                self.editor.selected_annotation = Some(id);
                self.show_panel(crate::dock::Panel::Inspector);
            }
            None => {}
        }
    }

    /// The in-place note editor: Return adds a line, clicking away keeps the text, Esc
    /// cancels (removing a note that was just placed).
    pub(crate) fn note_editor(&mut self, ctx: &egui::Context, layouts: &[NoteLayout]) {
        let Some(mut edit) = self.note_edit.take() else {
            return;
        };
        let Some(rect) = layouts
            .iter()
            .find(|l| l.id == edit.id)
            .and_then(NoteLayout::text_rect)
        else {
            return;
        };
        let field = egui::Id::new("note_edit_field");
        let font = notes::text_size(&self.editor.doc);
        let mut keep = true;
        egui::Area::new(egui::Id::new("note_edit"))
            .fixed_pos(rect.left_top() - egui::vec2(6.0, 4.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut edit.text)
                            .id(field)
                            .font(egui::FontId::proportional(font))
                            .hint_text("Note")
                            .return_key(egui::KeyboardShortcut::new(
                                egui::Modifiers::SHIFT,
                                Key::Enter,
                            ))
                            .desired_rows(1)
                            .desired_width(rect.width().max(160.0)),
                    );
                    let enter = response.has_focus()
                        && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
                    if edit.focus {
                        response.request_focus();
                        edit.focus = false;
                    } else if enter {
                        keep = false;
                        response.surrender_focus();
                        self.finish_note_edit(&edit, false);
                    } else if response.lost_focus() {
                        keep = false;
                        let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
                        self.finish_note_edit(&edit, cancelled);
                    }
                    ui.label(
                        egui::RichText::new("Return to finish · Shift+Return for a new line")
                            .small()
                            .weak(),
                    );
                });
            });
        if keep {
            self.note_edit = Some(edit);
        }
    }

    fn finish_note_edit(&mut self, edit: &NoteEdit, cancelled: bool) {
        let text = edit.text.trim_end().to_owned();
        let Some(Annotation::Note {
            at, leaders, style, ..
        }) = self.editor.doc.annotation(edit.id).cloned()
        else {
            return;
        };
        if (cancelled || text.trim().is_empty()) && edit.new {
            // A note placed but never written goes away (part of the same undo step).
            self.editor.doc.begin();
            self.editor.doc.remove_annotation(edit.id);
            self.editor.commit("Note");
            self.editor.selected_annotation = None;
            return;
        }
        if cancelled || text.trim().is_empty() {
            return;
        }
        self.editor.doc.begin();
        let _ = self.editor.doc.set_annotation(
            edit.id,
            Annotation::Note {
                text,
                at,
                leaders,
                style,
            },
        );
        self.editor.commit("Note text");
    }

    /// "Add centre marks" for selected circles and arcs that don't have one yet.
    pub(crate) fn centre_mark_button(&mut self, ui: &mut egui::Ui) {
        let doc = &self.editor.doc;
        let unmarked: Vec<drift_core::EntityId> = self
            .editor
            .selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    doc.get(*id).map(|e| e.kind),
                    Some(
                        drift_core::EntityKind::Circle { .. } | drift_core::EntityKind::Arc { .. }
                    )
                ) && !doc
                    .annotations()
                    .any(|(_, a)| matches!(a, Annotation::CentreMark { curve } if curve == id))
            })
            .collect();
        if unmarked.is_empty() {
            return;
        }
        ui.add_space(6.0);
        let label = if unmarked.len() == 1 {
            "Add centre mark".to_owned()
        } else {
            format!("Add {} centre marks", unmarked.len())
        };
        if ui
            .button(label)
            .on_hover_text("Centre lines across the selected circles and arcs")
            .clicked()
        {
            self.editor.doc.begin();
            for curve in unmarked {
                let _ = self
                    .editor
                    .doc
                    .add_annotation(Annotation::CentreMark { curve });
            }
            self.editor.commit("Centre marks");
        }
    }

    /// The inspector for a picked annotation. Returns true if one is picked.
    pub(crate) fn annotation_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(id) = self.editor.selected_annotation else {
            return false;
        };
        let Some(a) = self.editor.doc.annotation(id).cloned() else {
            self.editor.selected_annotation = None;
            return false;
        };
        crate::app::panels::title(ui, Some(ToolKind::Note), a.type_name(), "");
        let mut edited = a.clone();
        match &mut edited {
            Annotation::Note {
                text,
                at,
                leaders,
                style,
            } => {
                theme::section(ui, "Text");
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(3)
                        .desired_width(ui.available_width() - 8.0),
                );
                theme::section(ui, "Style");
                theme::row(ui, "Size", |ui| {
                    let mut own = style.height.is_some();
                    if ui
                        .checkbox(&mut own, "")
                        .on_hover_text("Off: the drafting style's text height")
                        .changed()
                    {
                        style.height = own.then(|| self.editor.doc.draft_style().text_height);
                    }
                    if let Some(h) = &mut style.height {
                        ui.add(
                            egui::DragValue::new(h)
                                .speed(0.05)
                                .range(0.5..=50.0)
                                .suffix(" mm"),
                        );
                    } else {
                        ui.label(egui::RichText::new("Drafting style").weak());
                    }
                });
                theme::row(ui, "Type", |ui| {
                    ui.toggle_value(&mut style.bold, egui::RichText::new("B").strong())
                        .on_hover_text("Bold");
                    ui.toggle_value(&mut style.italic, egui::RichText::new("I").italics())
                        .on_hover_text("Italic");
                });
                theme::row(ui, "Align", |ui| {
                    if let Some(a) =
                        theme::segmented(ui, style.align, &TextAlign::ALL.map(|a| (a, a.label())))
                    {
                        style.align = a;
                    }
                });
                theme::row(ui, "Wrap", |ui| {
                    let mut wraps = style.wrap.is_some();
                    if ui
                        .checkbox(&mut wraps, "")
                        .on_hover_text("Wrap the text to a width")
                        .changed()
                    {
                        style.wrap = wraps.then_some(40.0);
                    }
                    if let Some(w) = &mut style.wrap {
                        ui.add(
                            egui::DragValue::new(w)
                                .speed(0.5)
                                .range(5.0..=1000.0)
                                .suffix(" mm"),
                        );
                    }
                });
                theme::row(ui, "Frame", |ui| {
                    if let Some(f) =
                        theme::segmented(ui, style.frame, &NoteFrame::ALL.map(|f| (f, f.label())))
                    {
                        style.frame = f;
                    }
                });
                ui.checkbox(&mut style.mask, "Mask what's behind it")
                    .on_hover_text("Paper behind the text, so it reads over hatches and lines");
                theme::section(ui, &format!("Leaders · {}", leaders.len()));
                let doc = &self.editor.doc;
                let pos = doc.note_text_pos(*at, leaders);
                let mut remove = None;
                for (k, l) in leaders.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}", k + 1)).weak());
                        egui::ComboBox::from_id_salt(("leader_end", k))
                            .width(96.0)
                            .selected_text(l.end.label())
                            .show_ui(ui, |ui| {
                                for e in LeaderEnd::ALL {
                                    ui.selectable_value(&mut l.end, e, e.label());
                                }
                            });
                        egui::ComboBox::from_id_salt(("leader_shape", k))
                            .width(82.0)
                            .selected_text(l.shape.label())
                            .show_ui(ui, |ui| {
                                let tip = doc.anchor_pos(&l.to);
                                // A new bend sits halfway to the text, a little to the side.
                                let bend = match (tip, pos) {
                                    (Some(t), Some(p)) => (p - t) * 0.5 + (p - t).perp() * 0.15,
                                    _ => drift_core::DVec2::new(5.0, 5.0),
                                };
                                for shape in [
                                    LeaderShape::Straight,
                                    LeaderShape::Bent(bend),
                                    LeaderShape::Curved(0.25),
                                ] {
                                    let same = std::mem::discriminant(&l.shape)
                                        == std::mem::discriminant(&shape);
                                    if ui.selectable_label(same, shape.label()).clicked() && !same {
                                        l.shape = shape;
                                    }
                                }
                            });
                        ui.checkbox(&mut l.shoulder, "")
                            .on_hover_text("A short level shoulder into the text");
                        if ui
                            .small_button("×")
                            .on_hover_text("Remove this leader (the text stays)")
                            .clicked()
                        {
                            remove = Some(k);
                        }
                    });
                    if let LeaderShape::Curved(bow) = &mut l.shape {
                        theme::row(ui, "Curve", |ui| {
                            ui.add(egui::Slider::new(bow, -0.6..=0.6).show_value(false));
                        });
                    }
                }
                if let Some(k) = remove {
                    leaders.remove(k);
                    if let Some(p) = pos
                        && let Some(new_at) = doc.note_at(p, leaders)
                    {
                        *at = new_at;
                    }
                    self.selected_leader = None;
                }
                if ui
                    .button("Add leader")
                    .on_hover_text(
                        "Then click what it points to. Or drag from a + beside the picked \
                         note's text",
                    )
                    .clicked()
                {
                    self.adding_leader = Some(id);
                    let now = ui.input(|i| i.time);
                    self.set_status(
                        "Click what the new leader points to (it snaps) · Esc to stop".into(),
                        false,
                        now,
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "On the canvas: drag a + beside the text to add one, a leader's square \
                         end to re-attach it, a bend's circle to move it; click a leader and \
                         press Delete to remove it.",
                    )
                    .small()
                    .weak(),
                );
            }
            Annotation::HoleCallout { curve, suffix, .. } => {
                if let Some(t) = notes::hole_callout_text(&self.editor.doc, *curve, suffix) {
                    ui.label(t);
                }
                theme::section(ui, "Text");
                theme::row(ui, "Suffix", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(suffix)
                            .hint_text("THRU, ↧ 10…")
                            .desired_width(ui.available_width() - 8.0),
                    );
                });
                ui.horizontal(|ui| {
                    for s in ["THRU", "TAP", "CBORE", "CSK"] {
                        if ui.small_button(s).clicked() {
                            *suffix = s.to_owned();
                        }
                    }
                });
            }
            Annotation::ChamferCallout { line, .. } => {
                if let Some(t) = notes::chamfer_text(&self.editor.doc, *line) {
                    ui.label(t);
                }
            }
            Annotation::Ordinate {
                origin,
                point,
                axis,
                ..
            } => {
                if let Some(v) = notes::ordinate_value(&self.editor.doc, *origin, *point, *axis) {
                    let units = self.editor.doc.units();
                    ui.label(self.editor.doc.draft_style().length(&units, v));
                }
                theme::section(ui, "Measures");
                if let Some(x) =
                    theme::segmented(ui, *axis, &[(OrdinateAxis::X, "X"), (OrdinateAxis::Y, "Y")])
                {
                    *axis = x;
                }
            }
            Annotation::CentreMark { .. } | Annotation::Centreline { .. } => {
                ui.label(
                    egui::RichText::new("Follows its geometry; drawn as a centre line.")
                        .small()
                        .weak(),
                );
            }
            Annotation::Section {
                a: from,
                b: to,
                label,
                sheet,
                flip,
            } => {
                theme::section(ui, "Mark");
                mark_fields(ui, label, sheet, "A");
                theme::row(ui, "Looks", |ui| {
                    if ui
                        .button("Flip")
                        .on_hover_text("Look the other way across the cutting line")
                        .clicked()
                    {
                        *flip = !*flip;
                    }
                    if ui
                        .button("Swap ends")
                        .on_hover_text("Run the cutting line the other way (the flags stay)")
                        .clicked()
                    {
                        std::mem::swap(from, to);
                        *flip = !*flip;
                    }
                });
            }
            Annotation::Elevation {
                angle,
                label,
                sheet,
                ..
            } => {
                theme::section(ui, "Mark");
                mark_fields(ui, label, sheet, "1");
                theme::row(ui, "Looks", |ui| {
                    ui.add(
                        egui::DragValue::new(angle)
                            .speed(1.0)
                            .range(-360.0..=360.0)
                            .suffix("°"),
                    );
                    for (text, degrees) in [("→", 0.0), ("↑", 90.0), ("←", 180.0), ("↓", 270.0)]
                    {
                        if ui.small_button(text).clicked() {
                            *angle = degrees;
                        }
                    }
                });
            }
        }
        if edited != a {
            self.editor.doc.begin();
            let _ = self.editor.doc.set_annotation(id, edited);
            self.editor.commit("Edit note");
        }
        self.visibility_section(ui);
        ui.add_space(10.0);
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::new(
                    egui::RichText::new("Delete").color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                ),
            )
            .clicked()
        {
            self.editor.delete_selection();
        }
        true
    }
}
