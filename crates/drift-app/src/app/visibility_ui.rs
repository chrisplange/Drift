//! Where dimensions and annotations show: on the canvas and on sheets, per dimension or
//! annotation, with a switch to bring the hidden ones back into view.
//!
//! Hiding is display only: a hidden dimension still holds its geometry.

use std::collections::BTreeSet;

use drift_core::{AnnotationRef, Shown, ViewId};

use crate::notes::NoteLayout;
use crate::*;

impl DriftApp {
    /// The dimensions and annotations picked now: the picked dimension and annotation, and
    /// any added with Shift.
    pub(crate) fn picked_annotations(&self) -> Vec<AnnotationRef> {
        let doc = &self.editor.doc;
        let mut out = Vec::new();
        if let Some(id) = self.selected_constraint
            && doc.constraint(id).is_some_and(|c| c.is_dimension())
        {
            out.push(AnnotationRef::Dimension(id));
        }
        if let Some(id) = self.editor.selected_annotation
            && doc.annotation(id).is_some()
        {
            out.push(AnnotationRef::Annotation(id));
        }
        for r in &self.editor.annotation_picks {
            if doc.annotation_ref_exists(*r) && !out.contains(r) {
                out.push(*r);
            }
        }
        out
    }

    /// Whether a dimension or annotation is picked (as well as, or instead of, the primary
    /// pick).
    pub(crate) fn annotation_picked(&self, r: AnnotationRef) -> bool {
        self.editor.annotation_picks.contains(&r)
            || match r {
                AnnotationRef::Dimension(id) => self.selected_constraint == Some(id),
                AnnotationRef::Annotation(id) => self.editor.selected_annotation == Some(id),
            }
    }

    /// Shift+click: add a dimension or annotation to what is picked, or take it out.
    pub(crate) fn toggle_annotation_pick(&mut self, r: AnnotationRef) {
        if let Some(i) = self.editor.annotation_picks.iter().position(|p| *p == r) {
            self.editor.annotation_picks.remove(i);
            return;
        }
        // The first one picked becomes the primary pick (the inspector shows it).
        let primary_free = match r {
            AnnotationRef::Dimension(_) => {
                self.selected_constraint.is_none() && self.editor.selected_annotation.is_none()
            }
            AnnotationRef::Annotation(_) => {
                self.editor.selected_annotation.is_none() && self.selected_constraint.is_none()
            }
        };
        if primary_free {
            match r {
                AnnotationRef::Dimension(id) => self.selected_constraint = Some(id),
                AnnotationRef::Annotation(id) => self.editor.selected_annotation = Some(id),
            }
        } else if !self.annotation_picked(r) {
            self.editor.annotation_picks.push(r);
        }
    }

    /// Set where dimensions and annotations show (`None` leaves that side as it is), as one
    /// undo step.
    pub(crate) fn set_annotations_shown(
        &mut self,
        refs: &[AnnotationRef],
        canvas: Option<bool>,
        sheets: Option<bool>,
        now: f64,
    ) {
        if refs.is_empty() {
            return;
        }
        self.editor.doc.begin();
        for r in refs {
            let mut s = self.editor.doc.shown(*r);
            if let Some(c) = canvas {
                s.canvas = c;
            }
            if let Some(p) = sheets {
                s.sheets = p;
            }
            self.editor.doc.set_shown(*r, s);
        }
        let label = match (canvas, sheets) {
            (Some(false), _) => "Hide on canvas",
            (Some(true), _) => "Show on canvas",
            (_, Some(false)) => "Leave off sheets",
            _ => "Show on sheets",
        };
        self.editor.commit(label);
        if canvas == Some(false) && !self.show_hidden_annotations {
            self.set_status(
                format!(
                    "Hidden on the canvas ({}). Alt+H (View → Hidden annotations) shows hidden \
                     ones faded, to pick and show again.",
                    refs.len()
                ),
                false,
                now,
            );
        }
    }

    /// The inspector's Show section for what is picked: on the canvas, on sheets.
    pub(crate) fn visibility_section(&mut self, ui: &mut egui::Ui) {
        let refs = self.picked_annotations();
        if refs.is_empty() {
            return;
        }
        let doc = &self.editor.doc;
        let count = |f: fn(Shown) -> bool| refs.iter().filter(|r| f(doc.shown(**r))).count();
        let (canvas, sheets) = (count(|s| s.canvas), count(|s| s.sheets));
        let now = ui.input(|i| i.time);
        theme::section(ui, "Show");
        if refs.len() > 1 {
            ui.label(
                egui::RichText::new(format!("For the {} picked", refs.len()))
                    .small()
                    .weak(),
            );
        }
        let mut set_canvas = None;
        let mut set_sheets = None;
        let mut on = canvas == refs.len();
        let mixed = canvas > 0 && !on;
        if ui
            .add(egui::Checkbox::new(&mut on, "On the canvas").indeterminate(mixed))
            .on_hover_text(
                "Off: not drawn on the canvas (Alt+H, or View → Hidden annotations, shows \
                 them faded). A hidden dimension still holds its geometry",
            )
            .changed()
        {
            set_canvas = Some(on);
        }
        let mut on = sheets == refs.len();
        let mixed = sheets > 0 && !on;
        if ui
            .add(
                egui::Checkbox::new(&mut on, "On sheets, in print and export").indeterminate(mixed),
            )
            .on_hover_text("Off: left off sheets, PDF, SVG, DXF and print; still on the canvas")
            .changed()
        {
            set_sheets = Some(on);
        }
        if set_canvas.is_some() || set_sheets.is_some() {
            self.set_annotations_shown(&refs, set_canvas, set_sheets, now);
        }
        self.views_row(ui, &refs);
    }

    /// The views the picked dimensions and annotations are drawn in on sheets: every view,
    /// or some (a note for the detail alone).
    fn views_row(&mut self, ui: &mut egui::Ui, refs: &[AnnotationRef]) {
        let doc = &self.editor.doc;
        let views: Vec<(ViewId, String)> = doc
            .views()
            .map(|(id, v)| (id, v.title().to_owned()))
            .collect();
        if views.is_empty() {
            return;
        }
        // How many of the picked are in each view.
        let count = |v: ViewId| refs.iter().filter(|r| doc.in_view(**r, v)).count();
        let all = refs.iter().all(|r| doc.only_in(*r).is_none());
        let summary = if all {
            "All views".to_owned()
        } else {
            let names: Vec<&str> = views
                .iter()
                .filter(|(id, _)| count(*id) == refs.len())
                .map(|(_, n)| n.as_str())
                .collect();
            match names.len() {
                0 => "Some views".to_owned(),
                1..=2 => format!("Only {}", names.join(", ")),
                n => format!("{n} views"),
            }
        };
        let mut change = None;
        theme::row(ui, "In views", |ui| {
            egui::ComboBox::from_id_salt("annotation_views")
                .selected_text(summary)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(all, "All views").clicked() {
                        change = Some(None);
                    }
                    ui.separator();
                    for (id, name) in &views {
                        let n = count(*id);
                        let mut on = n == refs.len();
                        let mixed = n > 0 && !on;
                        if ui
                            .add(egui::Checkbox::new(&mut on, name.as_str()).indeterminate(mixed))
                            .changed()
                        {
                            change = Some(Some((*id, on)));
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The views it is drawn in on sheets: every view, or only some (a note for \
                     the detail alone). It is always on the canvas",
                );
        });
        let Some(change) = change else { return };
        let every: BTreeSet<ViewId> = views.iter().map(|(id, _)| *id).collect();
        self.editor.doc.begin();
        for r in refs {
            let set = match change {
                None => BTreeSet::new(),
                Some((view, on)) => {
                    let mut set = self
                        .editor
                        .doc
                        .only_in(*r)
                        .cloned()
                        .unwrap_or_else(|| every.clone());
                    if on {
                        set.insert(view);
                    } else {
                        set.remove(&view);
                    }
                    // In none: keep it in that one (Off sheets leaves it out everywhere).
                    if set.is_empty() {
                        continue;
                    }
                    if set == every { BTreeSet::new() } else { set }
                }
            };
            self.editor.doc.set_only_in(*r, set);
        }
        self.editor.commit("Views it's in");
    }

    /// How many dimensions and annotations are hidden on the canvas.
    pub(crate) fn hidden_annotation_count(&self) -> usize {
        self.editor
            .doc
            .shown_settings()
            .filter(|(_, s)| !s.canvas)
            .count()
    }

    /// Whether a dimension or annotation is drawn on the canvas (shown, or hidden while hidden
    /// ones are shown).
    pub(crate) fn on_canvas(&self, r: AnnotationRef) -> bool {
        self.show_hidden_annotations || self.editor.doc.shown(r).canvas
    }

    /// A small "not on sheets" badge: a sheet of paper struck through, by `at`.
    pub(crate) fn paint_off_sheets_badge(
        painter: &egui::Painter,
        at: egui::Pos2,
        color: egui::Color32,
    ) {
        let r = egui::Rect::from_min_size(at + egui::vec2(3.0, -12.0), egui::vec2(8.0, 10.0));
        let stroke = egui::Stroke::new(1.0, color);
        painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
        painter.line_segment(
            [
                r.left_bottom() + egui::vec2(-2.0, 2.0),
                r.right_top() + egui::vec2(2.0, -2.0),
            ],
            egui::Stroke::new(1.4, color),
        );
    }
}

/// Whether this frame's click came with Shift held (as the button event says, or as the
/// keyboard is now).
pub(crate) fn shift_click(ui: &egui::Ui) -> bool {
    ui.input(|i| {
        i.modifiers.shift
            || i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        modifiers,
                        ..
                    } if modifiers.shift
                )
            })
    })
}

/// Where a note's badge goes: by its text, else by its first mark or line.
pub(crate) fn note_badge_at(l: &NoteLayout) -> Option<egui::Pos2> {
    if let Some(t) = &l.text {
        return Some(t.rect.right_top());
    }
    if let Some([_, b]) = l.segments.first() {
        return Some(*b);
    }
    l.leaders.first().and_then(|p| p.points.last().copied())
}
