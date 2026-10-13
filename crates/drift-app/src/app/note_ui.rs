//! Drafting annotations in the app: picking, dragging and editing notes and callouts on the
//! canvas, the note text editor, and the inspector for a picked annotation.

use drift_core::{Annotation, AnnotationId, LeaderEnd, OrdinateAxis};

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
                self.editor.selected_annotation = Some(id);
                self.editor.selection.clear();
                self.editor.selected_fills.clear();
                self.editor.selected_region = None;
                self.selected_constraint = None;
                return true;
            }
            self.editor.selected_annotation = None;
        }
        false
    }

    pub(crate) fn paint_notes(&self, painter: &egui::Painter, layouts: &[NoteLayout], dark: bool) {
        let colors = AnnotationColors::new(dark);
        let style = annotations::DimStyle::of(&self.editor.doc.draft_style());
        for l in layouts {
            if self.note_edit.as_ref().is_some_and(|e| e.id == l.id) {
                continue;
            }
            let state = if self.editor.selected_annotation == Some(l.id)
                || self.note_drag.is_some_and(|d| d.0 == l.id)
            {
                AnnotationState::Selected
            } else if self.editor.hovered_annotation == Some(l.id) {
                AnnotationState::Hovered
            } else {
                AnnotationState::Normal
            };
            notes::paint_annotation(painter, l, state, &colors, &style);
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
        let Some(Annotation::Note { at, leader, .. }) =
            self.editor.doc.annotation(edit.id).cloned()
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
        let _ = self
            .editor
            .doc
            .set_annotation(edit.id, Annotation::Note { text, at, leader });
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
            Annotation::Note { text, leader, .. } => {
                theme::section(ui, "Text");
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(3)
                        .desired_width(ui.available_width() - 8.0),
                );
                theme::section(ui, "Leader");
                match leader {
                    Some(l) => {
                        theme::row(ui, "End", |ui| {
                            egui::ComboBox::from_id_salt("leader_end")
                                .selected_text(l.end.label())
                                .show_ui(ui, |ui| {
                                    for e in LeaderEnd::ALL {
                                        ui.selectable_value(&mut l.end, e, e.label());
                                    }
                                });
                        });
                        if ui
                            .button("Remove leader")
                            .on_hover_text("Keep the text where it is, without the leader")
                            .clicked()
                            && let Some(label) = notes::label_world(&self.editor.doc, &a)
                        {
                            edited = Annotation::Note {
                                text: text.clone(),
                                at: label,
                                leader: None,
                            };
                        }
                    }
                    None => {
                        ui.label(
                            egui::RichText::new(
                                "A free note. Use the Note tool and start on geometry for a \
                                 leader.",
                            )
                            .small()
                            .weak(),
                        );
                    }
                }
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
