//! Linear and circular arrays of the selection, in the properties panel. While the section is
//! in use the copies show on the canvas as a preview, and nothing enters the drawing or its
//! undo history until Apply (#455).

use std::rc::Rc;

use drift_core::{AnnotationId, EntityId, InstanceId, SketchItem, TextId};
use drift_edit::modify::ArrayRequest;

use crate::*;

/// Which array the section makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArrayMode {
    #[default]
    Linear,
    Circular,
}

/// Edit buffers for the array fields (text, so they take expressions and units), and the
/// preview.
pub(crate) struct ArrayUi {
    pub(crate) mode: ArrayMode,
    pub(crate) linear_count: usize,
    pub(crate) dx: String,
    pub(crate) dy: String,
    pub(crate) circular_count: usize,
    pub(crate) angle: String,
    pub(crate) cx: String,
    pub(crate) cy: String,
    /// Open the fold and bring it into view (Modify → Linear / Circular array).
    pub(crate) reveal: bool,
    /// The preview is on: from the Modify menu, the palette or the rail, or any change made
    /// in the section. Apply, Cancel and Esc end it.
    pub(crate) active: bool,
    /// The last request whose fields all worked out, previewed while one is being fixed.
    last_good: Option<ArrayRequest>,
    /// What the canvas draws, and the frame the section last drew it for.
    preview: Option<Preview>,
    /// The frame the section was last drawn in (with something picked).
    drawn: u64,
}

/// The preview's copies, and what they were worked out from.
struct Preview {
    frame: u64,
    key: PreviewKey,
    items: Rc<Vec<SketchItem>>,
}

/// What a preview depends on: the request, the picks and the drawing's revision.
#[derive(PartialEq)]
struct PreviewKey {
    request: ArrayRequest,
    ids: Vec<EntityId>,
    instances: Vec<InstanceId>,
    texts: Vec<TextId>,
    notes: Vec<AnnotationId>,
    revision: u64,
}

impl Default for ArrayUi {
    fn default() -> Self {
        Self {
            mode: ArrayMode::Linear,
            linear_count: 3,
            dx: "20".into(),
            dy: "0".into(),
            circular_count: 6,
            angle: "360".into(),
            cx: "0".into(),
            cy: "0".into(),
            reveal: false,
            active: false,
            last_good: None,
            preview: None,
            drawn: 0,
        }
    }
}

impl ArrayUi {
    /// Open the section on an array and start its preview (Modify → Linear / Circular
    /// array).
    pub(crate) fn open(&mut self, circular: bool) {
        let mode = if circular {
            ArrayMode::Circular
        } else {
            ArrayMode::Linear
        };
        if mode != self.mode {
            self.last_good = None;
        }
        self.mode = mode;
        self.reveal = true;
        self.active = true;
    }

    /// End the preview, leaving the drawing as it is. True if one was on.
    pub(crate) fn cancel(&mut self) -> bool {
        self.last_good = None;
        self.preview = None;
        std::mem::take(&mut self.active)
    }

    /// End a preview whose section is no longer drawn (the picks went, or the Inspector
    /// closed), so it doesn't come back by itself.
    pub(crate) fn drop_unseen(&mut self, frame: u64) {
        if self.active && !self.reveal && self.drawn + 1 < frame {
            self.cancel();
        }
    }

    /// The preview's copies, if the section drew them this frame or the last (the panels
    /// come before the canvas, but a floating one may not).
    pub(crate) fn preview_items(&self, frame: u64) -> Option<Rc<Vec<SketchItem>>> {
        self.preview
            .as_ref()
            .filter(|p| self.active && p.frame + 1 >= frame)
            .map(|p| p.items.clone())
    }
}

/// The picks an array copies: curves, block copies, and the texts and notes picked with them.
type Picks = (Vec<EntityId>, Vec<InstanceId>, drift_edit::picks::Riders);

impl DriftApp {
    fn array_picks(&self) -> Picks {
        let ed = &self.drawing.editor;
        let instances = ed
            .selected_instances
            .iter()
            .copied()
            .filter(|i| ed.doc.instance(*i).is_some())
            .collect();
        let ids = ed
            .selection
            .iter()
            .copied()
            .filter(|id| ed.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        (ids, instances, ed.riders())
    }

    /// The fields worked out as a request, or what's wrong with the first that isn't.
    fn array_request(&self) -> Result<ArrayRequest, String> {
        let ed = &self.drawing.editor;
        let eval = |text: &str, kind| {
            ed.evaluate_text(text, kind)
                .map(|e| e.value)
                .map_err(|e| format!("\"{text}\": {e}"))
        };
        let a = &self.arrays;
        Ok(match a.mode {
            ArrayMode::Linear => ArrayRequest::Linear {
                count: a.linear_count,
                step: DVec2::new(eval(&a.dx, Kind::Length)?, eval(&a.dy, Kind::Length)?),
            },
            ArrayMode::Circular => ArrayRequest::Circular {
                count: a.circular_count,
                center: DVec2::new(eval(&a.cx, Kind::Length)?, eval(&a.cy, Kind::Length)?),
                degrees: eval(&a.angle, Kind::Angle)?,
            },
        })
    }

    /// Whether the preview is on the canvas now, with its picks still picked (so Enter
    /// applies it and Esc ends it).
    pub(crate) fn array_previewing(&self, ctx: &egui::Context) -> bool {
        self.arrays
            .preview_items(ctx.cumulative_frame_nr())
            .is_some()
            && {
                let (ids, instances, riders) = self.array_picks();
                !(ids.is_empty() && instances.is_empty() && riders.is_empty())
            }
    }

    /// Make the array, as one undo step, and end the preview.
    pub(crate) fn apply_array(&mut self, now: f64) {
        let (ids, instances, riders) = self.array_picks();
        let outcome = self.array_request().and_then(|request| {
            self.drawing
                .editor
                .array_by(&ids, &instances, &riders, &request)
                .map_err(|e| e.to_string())
        });
        match outcome {
            Ok((created, copies, (texts, notes))) => {
                self.arrays.cancel();
                let parts: Vec<String> = [
                    (created.len(), "curve", "curves"),
                    (copies.len(), "block copy", "block copies"),
                    (texts.len(), "text", "texts"),
                    (notes.len(), "note", "notes"),
                ]
                .into_iter()
                .filter(|(n, _, _)| *n > 0)
                .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
                .collect();
                let added = match parts.as_slice() {
                    [] => "nothing".to_owned(),
                    [one] => one.clone(),
                    [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                };
                self.set_status(format!("Array: added {added}"), false, now);
            }
            Err(e) => self.set_status(format!("Array: {e}"), true, now),
        }
    }

    /// The Array section, for the picked curves and block copies (#311), and the texts and
    /// notes picked with them (#327).
    pub(crate) fn array_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let (ids, instances, riders) = self.array_picks();
        if ids.is_empty() && instances.is_empty() && riders.is_empty() {
            self.arrays.cancel();
            return;
        }
        self.arrays.drawn = ui.ctx().cumulative_frame_nr();
        let reveal = std::mem::take(&mut self.arrays.reveal);
        if reveal {
            // The id `theme::fold` keeps its open state under.
            let id = ui.make_persistent_id(("fold", "array"));
            ui.data_mut(|d| d.insert_persisted(id, true));
        }
        let dark = ui.visuals().dark_mode;
        // Which fields don't work out, for marking them.
        let ed = &self.drawing.editor;
        let bad = |text: &str, kind| ed.evaluate_text(text, kind).err().map(|e| e.to_string());
        let mut apply = false;
        let mut cancel = false;
        let mut changed = false;
        let shown = crate::theme::fold(ui, "array", "Array", false, |ui| {
            if reveal {
                ui.scroll_to_cursor(Some(egui::Align::Min));
            }
            let a = &mut self.arrays;
            let modes = [
                (ArrayMode::Linear, "Linear"),
                (ArrayMode::Circular, "Circular"),
            ];
            if let Some(mode) = crate::theme::segmented(ui, a.mode, &modes) {
                if mode != a.mode {
                    a.last_good = None;
                }
                a.mode = mode;
                changed = true;
            }
            ui.add_space(6.0);
            // A labelled text field that takes an expression: red, with the reason on hover,
            // when it doesn't work out. Enter applies the array; Esc ends the preview. True
            // if it changed.
            let mut field = |ui: &mut egui::Ui, label: &str, text: &mut String, kind: Kind| {
                let label = ui.label(label);
                let error = bad(text, kind);
                let red = ui.visuals().error_fg_color;
                let response = crate::theme::field(
                    ui,
                    egui::TextEdit::singleline(text)
                        .desired_width(70.0)
                        .text_color_opt(error.is_some().then_some(red)),
                )
                .labelled_by(label.id);
                let response = match &error {
                    Some(e) => response.on_hover_text(e),
                    None => response,
                };
                if response.lost_focus() {
                    let (enter, esc) =
                        ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    apply |= enter;
                    cancel |= esc;
                }
                response.changed()
            };
            egui::Grid::new("array_fields")
                .num_columns(4)
                .spacing([8.0, 6.0])
                .show(ui, |ui| match a.mode {
                    ArrayMode::Linear => {
                        let label = ui.label("Count");
                        changed |= theme::drag_sized(
                            ui,
                            70.0,
                            egui::DragValue::new(&mut a.linear_count).range(2..=500),
                        )
                        .labelled_by(label.id)
                        .changed();
                        ui.end_row();
                        changed |= field(ui, "Spacing X", &mut a.dx, Kind::Length);
                        changed |= field(ui, "Y", &mut a.dy, Kind::Length);
                        ui.end_row();
                    }
                    ArrayMode::Circular => {
                        let label = ui.label("Count");
                        changed |= theme::drag_sized(
                            ui,
                            70.0,
                            egui::DragValue::new(&mut a.circular_count).range(2..=500),
                        )
                        .labelled_by(label.id)
                        .changed();
                        changed |= field(ui, "Angle", &mut a.angle, Kind::Angle);
                        ui.end_row();
                        changed |= field(ui, "Center X", &mut a.cx, Kind::Length);
                        changed |= field(ui, "Y", &mut a.cy, Kind::Length);
                        ui.end_row();
                    }
                });
            if a.mode == ArrayMode::Circular {
                ui.label(
                    egui::RichText::new("360° spreads the copies evenly round the full circle")
                        .small()
                        .weak(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding.x = 14.0;
                apply |= ui
                    .add(crate::theme::primary_button("Apply", dark))
                    .on_hover_text("Make the copies, as one step to undo (Enter)")
                    .clicked();
                if a.active {
                    cancel |= ui
                        .button("Cancel")
                        .on_hover_text("Leave the drawing as it was (Esc)")
                        .clicked();
                } else {
                    ui.label(egui::RichText::new("Change a value to preview").weak());
                }
            });
        });
        if shown.is_none() {
            // Folded: no preview.
            self.arrays.cancel();
            return;
        }
        if changed {
            self.arrays.active = true;
        }
        if cancel {
            self.arrays.cancel();
            return;
        }
        if apply {
            self.apply_array(now);
            return;
        }
        if !self.arrays.active {
            self.arrays.preview = None;
            return;
        }
        // The preview: the fields as they are, or as they last worked out.
        let request = match self.array_request() {
            Ok(r) if r.check().is_ok() => {
                self.arrays.last_good = Some(r);
                r
            }
            _ => match self.arrays.last_good {
                Some(r) => r,
                None => {
                    self.arrays.preview = None;
                    return;
                }
            },
        };
        let key = PreviewKey {
            request,
            ids,
            instances,
            texts: riders.texts.iter().map(|(id, _, _)| *id).collect(),
            notes: riders.notes.iter().map(|(id, _, _)| *id).collect(),
            revision: self.drawing.editor.doc.revision(),
        };
        let frame = ui.ctx().cumulative_frame_nr();
        match &mut self.arrays.preview {
            Some(p) if p.key == key => p.frame = frame,
            slot => {
                let items = self.drawing.editor.array_preview(
                    &key.ids,
                    &key.instances,
                    &riders,
                    &key.request,
                );
                *slot = Some(Preview {
                    frame,
                    key,
                    items: Rc::new(items),
                });
            }
        }
    }
}
