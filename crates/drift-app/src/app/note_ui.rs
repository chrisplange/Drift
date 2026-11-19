//! Drafting annotations in the app: picking, dragging and editing notes and callouts on the
//! canvas, the note text editor, and the inspector for a picked annotation.

use drift_core::{
    Anchor, Annotation, AnnotationId, Leader, LeaderEnd, LeaderShape, NoteFrame, NoteStyle,
    OrdinateAxis, TextAlign,
};

use crate::notes::{self, NoteLayout};
use crate::*;

/// Which note is being typed: one in the model, or a sheet's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoteTarget {
    Model(AnnotationId),
    Sheet(drift_core::SheetId, drift_core::SheetAnnotationId),
}

/// A note whose text is being typed on the canvas, straight into the note: the keyboard
/// goes to a field out of sight, and the caret and the selection are drawn on the note.
pub(crate) struct NoteEdit {
    pub target: NoteTarget,
    /// The note was just placed: left empty, it goes again in the step that placed it.
    pub new: bool,
    /// The next change to the words joins the last undo step: this visit's typing, or the
    /// step that placed a new note.
    merge: bool,
    /// The first frame: the field takes the keyboard.
    starting: bool,
    /// Where the click that began the typing was (screen): the caret starts there.
    caret_at: Option<egui::Pos2>,
    /// The caret and the other end of the selection (characters), as the field has them.
    cursor: Option<(usize, usize)>,
    /// A caret and selection to give the field.
    set_cursor: Option<(usize, usize)>,
    /// The primary button went down in the text and is still down (selecting).
    grip: bool,
    /// The pointer was used in the text this frame (taking the keyboard from the field).
    touched: bool,
    /// When the caret last moved (it blinks after a while still).
    blink: f64,
}

impl NoteEdit {
    /// The caret and the other end of the selection, in characters.
    #[cfg(test)]
    pub(crate) fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    fn new(target: NoteTarget, new: bool, merge: bool, caret_at: Option<egui::Pos2>) -> Self {
        Self {
            target,
            new,
            merge,
            starting: true,
            caret_at,
            cursor: None,
            set_cursor: None,
            grip: false,
            touched: false,
            blink: 0.0,
        }
    }
}

/// The range of the line (between Returns) the caret `i` is on, in characters.
pub(crate) fn paragraph_range(text: &str, i: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let i = i.min(chars.len());
    let start = chars[..i]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |k| k + 1);
    let end = chars[i..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(chars.len(), |k| i + k);
    (start, end)
}

/// A grip on a picked note: a leader's tip (to re-attach it), a bent leader's bend, or a "+"
/// beside the text (drag from it to add a leader).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LeaderGrip {
    Tip(usize),
    Bend(usize),
    /// Where a leader meets its shoulder: sets the shoulder's length.
    Shoulder(usize),
    /// A curved leader's middle: sets how far it bows.
    Curve(usize),
    New(egui::Pos2),
}

/// How far a "+" grip sits from the side of a note's text (points).
const PLUS_OUT: f32 = 14.0;

/// A mark's label and the sheet it refers to.
/// A section or elevation mark's label, and the view it points at or the sheet it names.
fn mark_fields(
    ui: &mut egui::Ui,
    doc: &drift_core::Document,
    label: &mut String,
    sheet: &mut String,
    view: &mut Option<drift_core::ViewId>,
    example: &str,
) {
    theme::row(ui, "Label", |ui| {
        ui.add(
            egui::TextEdit::singleline(label)
                .hint_text(example)
                .desired_width(60.0),
        );
    });
    theme::row(ui, "View", |ui| {
        let title = |v: drift_core::ViewId| doc.view(v).map_or("None", |v| v.title());
        egui::ComboBox::from_id_salt("mark_view")
            .selected_text(view.map_or("None", title))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(view, None, "None");
                for (id, v) in doc.views() {
                    ui.selectable_value(view, Some(id), v.title());
                }
            })
            .response
            .on_hover_text(
                "The view the mark points at: its flag reads the view's number and sheet",
            );
    });
    match *view {
        Some(v) => {
            let (number, on) = doc
                .view_reference(v)
                .unwrap_or_else(|| ("–".to_owned(), "–".to_owned()));
            theme::row(ui, "Reads", |ui| {
                let r = ui.label(format!("{number} / {on}"));
                if number == "–" {
                    r.on_hover_text("The view isn't on a sheet yet");
                } else {
                    r.on_hover_text("The view's number on its sheet, and the sheet's number");
                }
            });
        }
        None => {
            theme::row(ui, "On sheet", |ui| {
                ui.add(
                    egui::TextEdit::singleline(sheet)
                        .hint_text("e.g. A-201")
                        .desired_width(ui.available_width()),
                )
                .on_hover_text(
                    "The sheet the view is drawn on: written in the lower half of the flag",
                );
            });
        }
    }
}

impl DriftApp {
    pub(crate) fn layout_notes(&self, painter: &egui::Painter, map: &ScreenMap) -> Vec<NoteLayout> {
        if !self.show_dimensions {
            return Vec::new();
        }
        self.anno_space(map)
            .layout_annotations(painter, &self.drawing.editor.doc)
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
        // (With the select tool's filter on Annotations or All.)
        let idle = self.tool.kind() == ToolKind::Select
            && !self.tool.is_busy()
            && self.drawing.editor.prefs.pick_filter.annotations();
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
        self.drawing.editor.hovered_annotation =
            if idle && self.drawing.note_drag.is_none() && self.drawing.label_drag.is_none() {
                pos.and_then(note_at)
            } else {
                None
            };
        if idle
            && self.drawing.note_drag.is_none()
            && self.drawing.label_drag.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(id) = note_at(origin)
            && let Some(a) = self.drawing.editor.doc.annotation(id)
            && let Some(label) = drift_edit::labels::label_world(&self.drawing.editor.doc, a)
        {
            self.drawing.note_drag = Some((id, map.to_world(origin) - label));
            self.drawing.editor.selected_annotation = Some(id);
            self.drawing.editor.doc.begin();
        }
        if let Some((id, grab)) = self.drawing.note_drag {
            let at = response.interact_pointer_pos().map(|p| map.to_world(p));
            let done = !response.dragged_by(PointerButton::Primary);
            let ok = self.guarded("while moving the note", |app| {
                if let (Some(at), Some(a)) = (at, app.drawing.editor.doc.annotation(id).cloned())
                    && let Some(moved) =
                        drift_edit::labels::with_label_at(&app.drawing.editor.doc, &a, at - grab)
                {
                    let _ = app.drawing.editor.doc.set_annotation(id, moved);
                    #[cfg(test)]
                    if crate::app::guard::PANIC_IN_NOTE_DRAG.get() {
                        panic!("a note drag's bug (injected)");
                    }
                }
                if done {
                    app.drawing.editor.commit("Move note");
                }
            });
            if done || ok.is_none() {
                self.drawing.note_drag = None;
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
            self.open_note_edit_at(id, false, at);
            return true;
        }
        if response.clicked_by(PointerButton::Primary) {
            if let Some(id) = at.and_then(note_at) {
                // On one of its leaders: that leader is picked too (Delete removes it).
                self.drawing.selected_leader = at.and_then(|p| {
                    let l = layouts.iter().find(|l| l.id == id)?;
                    let on_text = l.text.as_ref().is_some_and(|t| t.rect.contains(p));
                    let note = matches!(
                        self.drawing.editor.doc.annotation(id),
                        Some(Annotation::Note { .. })
                    );
                    (note && !on_text)
                        .then(|| l.leader_at(p))
                        .flatten()
                        .map(|k| (id, k))
                });
                let shift = crate::app::visibility_ui::shift_click(ui);
                if shift {
                    // Added to what is picked, geometry and all.
                    self.toggle_annotation_pick(drift_core::AnnotationRef::Annotation(id));
                } else if !ui.input(|i| i.modifiers.command)
                    && let Some(at) = at
                    && self.click_past_annotation(map, at)
                {
                    // Clicked again: the select tool picks what's under the note.
                    self.drawing.selected_leader = None;
                    return false;
                } else {
                    self.drawing.editor.annotation_picks.clear();
                    self.drawing.editor.selected_annotation = Some(id);
                    self.drawing.selected_constraint = None;
                    self.drawing.editor.selection.clear();
                    self.drawing.editor.selected_fills.clear();
                    self.drawing.editor.selected_instances.clear();
                    self.drawing.editor.selected_texts.clear();
                    self.drawing.editor.selected_region = None;
                }
                return true;
            }
            if !crate::app::visibility_ui::shift_click(ui) && !ui.input(|i| i.modifiers.command) {
                self.drawing.editor.selected_annotation = None;
            }
            self.drawing.selected_leader = None;
        }
        false
    }

    /// The grips of the picked note, on screen.
    fn note_grips(&self, layouts: &[NoteLayout]) -> Vec<(egui::Pos2, LeaderGrip)> {
        let Some(id) = self.drawing.editor.selected_annotation else {
            return Vec::new();
        };
        if !matches!(
            self.drawing.editor.doc.annotation(id),
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
            if let Some(at) = leader.shoulder {
                grips.push((at, LeaderGrip::Shoulder(k)));
            }
            if let Some((mid, _)) = leader.curve {
                grips.push((mid, LeaderGrip::Curve(k)));
            }
        }
        if let Some(t) = &l.text {
            // Off the text's top corners, clear of where leaders land.
            for x in [t.rect.left() - PLUS_OUT, t.rect.right() + PLUS_OUT] {
                let at = egui::pos2(x, t.rect.top() - PLUS_OUT * 0.6);
                grips.push((at, LeaderGrip::New(at)));
            }
        }
        grips
    }

    /// Where a leader dropped at `pos` points: a snapped point, a spot on a curve, or free.
    fn leader_anchor(&self, map: &ScreenMap, pos: egui::Pos2) -> Anchor {
        let raw = map.to_world(pos);
        let tolerance = PICK_RADIUS / self.drawing.view.camera.scale;
        let options = drift_geom::SnapOptions {
            tolerance,
            grid: self
                .grid_snap
                .then(|| drift_render::grid_info(&self.drawing.view.camera, self.imperial()).minor),
            from: None,
        };
        let mut p = Pointer::new(raw, tolerance);
        p.relations = true;
        p.snap = self.drawing.editor.snap(raw, options, |_| false);
        if let Some(point) = p.snapped_point() {
            return Anchor::Point(point);
        }
        // Any line or curve under the pointer, at the spot nearest it.
        let doc = &self.drawing.editor.doc;
        match drift_edit::tools::reference_at(&self.drawing.editor, &p) {
            Some(id) if doc.get(id).is_some_and(|e| e.kind.is_point()) => Anchor::Point(id),
            Some(curve) => {
                let at = doc
                    .shape(curve)
                    .map_or(raw, |s| drift_geom::nearest_point(&s, raw));
                drift_edit::labels::anchor_at(doc, None, Some(curve), at)
            }
            None => drift_edit::labels::anchor_at(doc, None, p.on_curve(), p.pos()),
        }
    }

    /// Change a note's leaders, keeping its text where it is (inside the caller's
    /// transaction).
    fn change_leaders(&mut self, id: AnnotationId, f: impl FnOnce(&mut Vec<Leader>)) {
        let doc = &self.drawing.editor.doc;
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
        let _ = self.drawing.editor.doc.set_annotation(
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
        self.drawing.editor.doc.begin();
        self.change_leaders(id, |leaders| {
            if k < leaders.len() {
                leaders.remove(k);
            }
        });
        self.drawing.editor.commit("Remove leader");
        self.drawing.selected_leader = None;
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
        if let Some(id) = self.drawing.adding_leader {
            if pos.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if response.clicked_by(PointerButton::Primary)
                && let Some(at) = released()
            {
                self.guarded("while adding the leader", |app| {
                    let to = app.leader_anchor(map, at);
                    app.drawing.editor.doc.begin();
                    app.change_leaders(id, |leaders| leaders.push(Leader::to(to)));
                    app.drawing.editor.commit("Add leader");
                });
                self.drawing.adding_leader = None;
                return true;
            }
        }
        let grips = self.note_grips(layouts);
        // The nearest grip within reach (they can sit close together).
        let grip_at = |p: egui::Pos2| {
            grips
                .iter()
                .filter(|(g, _)| g.distance(p) <= 7.0)
                .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
                .map(|(_, grip)| *grip)
        };
        if let Some(grip) = pos.and_then(grip_at) {
            ui.ctx().set_cursor_icon(match grip {
                LeaderGrip::New(_) => egui::CursorIcon::Crosshair,
                _ => egui::CursorIcon::Grab,
            });
        }
        if self.drawing.leader_grip.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(grip) = grip_at(origin)
            && let Some(id) = self.drawing.editor.selected_annotation
        {
            self.drawing.leader_grip = Some((id, grip));
            self.drawing.editor.doc.begin();
        }
        let Some((id, grip)) = self.drawing.leader_grip else {
            return false;
        };
        let done = !response.dragged_by(PointerButton::Primary);
        let ok = self.guarded("while moving the leader", |app| {
            if let Some(p) = response.interact_pointer_pos() {
                match grip {
                    LeaderGrip::Tip(k) => {
                        let to = app.leader_anchor(map, p);
                        app.change_leaders(id, |leaders| {
                            if let Some(l) = leaders.get_mut(k) {
                                l.to = to;
                            }
                        });
                    }
                    LeaderGrip::Bend(k) => {
                        let at = map.to_world(p);
                        let doc = &app.drawing.editor.doc;
                        let tip = match doc.annotation(id) {
                            Some(Annotation::Note { leaders, .. }) => {
                                leaders.get(k).and_then(|l| doc.anchor_pos(&l.to))
                            }
                            _ => None,
                        };
                        if let Some(tip) = tip {
                            app.change_leaders(id, |leaders| {
                                if let Some(l) = leaders.get_mut(k) {
                                    l.shape = LeaderShape::Bent(at - tip);
                                }
                            });
                        }
                    }
                    LeaderGrip::Shoulder(k) => {
                        // Along the text's line, out from where it meets the text; very short
                        // turns the shoulder off.
                        let landing = layouts
                            .iter()
                            .find(|l| l.id == id)
                            .and_then(|l| l.leaders.get(k))
                            .map(|l| l.landing);
                        if let Some(landing) = landing {
                            // On paper: screen points over the canvas's points per paper point.
                            let zoom = app.anno_space(map).zoom.map_or(1.0, |z| z.k);
                            let reach = (p.x - landing.x).abs() / zoom;
                            app.change_leaders(id, |leaders| {
                                if let Some(l) = leaders.get_mut(k) {
                                    l.shoulder = reach >= 2.0;
                                    if l.shoulder {
                                        l.shoulder_len =
                                            f64::from(reach / annotations::PT_PER_MM).min(100.0);
                                    }
                                }
                            });
                        }
                    }
                    LeaderGrip::Curve(k) => {
                        let chord = layouts
                            .iter()
                            .find(|l| l.id == id)
                            .and_then(|l| l.leaders.get(k))
                            .and_then(|l| l.curve)
                            .map(|(_, chord)| chord);
                        if let Some(chord) = chord {
                            let bow = notes::bow_through(p, chord);
                            app.change_leaders(id, |leaders| {
                                if let Some(l) = leaders.get_mut(k) {
                                    l.shape = LeaderShape::Curved(bow);
                                }
                            });
                        }
                    }
                    // Drawn by `paint_notes` until it's let go.
                    LeaderGrip::New(_) => {}
                }
            }
            if done {
                let label = match grip {
                    LeaderGrip::New(_) => {
                        if let Some(at) = released() {
                            let to = app.leader_anchor(map, at);
                            app.change_leaders(id, |leaders| leaders.push(Leader::to(to)));
                        }
                        "Add leader"
                    }
                    LeaderGrip::Tip(_) => "Move leader",
                    LeaderGrip::Bend(_) => "Bend leader",
                    LeaderGrip::Shoulder(_) => "Shoulder length",
                    LeaderGrip::Curve(_) => "Curve leader",
                };
                app.drawing.editor.commit(label);
            }
        });
        if done || ok.is_none() {
            self.drawing.leader_grip = None;
        }
        true
    }

    pub(crate) fn paint_notes(
        &self,
        painter: &egui::Painter,
        map: &ScreenMap,
        layouts: &[NoteLayout],
        dark: bool,
    ) {
        let space = self.anno_space(map);
        for l in layouts {
            let r = drift_core::AnnotationRef::Annotation(l.id);
            // Each in its own style: the drawing's, with its own settings in place.
            let draft = self.drawing.editor.doc.draft_style_for(r);
            let colors = AnnotationColors::drafting(dark, draft.color);
            let faded = colors.faded();
            let style = space.style(&draft);
            let state = if self.annotation_picked(r)
                || self.drawing.note_drag.is_some_and(|d| d.0 == l.id)
            {
                AnnotationState::Selected
            } else if self.drawing.editor.hovered_annotation == Some(l.id) {
                AnnotationState::Hovered
            } else {
                AnnotationState::Normal
            };
            let shown = self.drawing.editor.doc.shown(r);
            let look = if shown.canvas { &colors } else { &faded };
            space.paint_annotation(painter, l, state, look, &draft);
            if self
                .drawing
                .selected_leader
                .is_some_and(|(id, _)| id == l.id)
                && let Some(leader) = self
                    .drawing
                    .selected_leader
                    .and_then(|(_, k)| l.leaders.get(k))
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
                LeaderGrip::Bend(_) | LeaderGrip::Curve(_) => {
                    painter.circle(
                        at,
                        3.5,
                        colors.text_background,
                        egui::Stroke::new(1.2, accent),
                    );
                }
                LeaderGrip::Shoulder(_) => {
                    // A small diamond: it slides along the text's line.
                    let d = 4.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            at + egui::vec2(0.0, -d),
                            at + egui::vec2(d, 0.0),
                            at + egui::vec2(0.0, d),
                            at + egui::vec2(-d, 0.0),
                        ],
                        colors.text_background,
                        egui::Stroke::new(1.2, accent),
                    ));
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
        if let Some((_, LeaderGrip::New(from))) = self.drawing.leader_grip
            && let Some(to) = painter.ctx().input(|i| i.pointer.latest_pos())
        {
            crate::overlay::dashed(
                painter,
                &[from, to],
                egui::Stroke::new(1.0, accent),
                4.0,
                3.0,
            );
        }
    }

    /// Start typing a note's text on the canvas (other annotations open the inspector).
    /// `new`: the note was just placed (the step that placed it is the last undo step).
    pub(crate) fn open_note_edit(&mut self, id: AnnotationId, new: bool) {
        self.open_note_edit_at(id, new, None);
    }

    /// [`Self::open_note_edit`], the caret where `at` (on screen) is on the text.
    pub(crate) fn open_note_edit_at(
        &mut self,
        id: AnnotationId,
        new: bool,
        at: Option<egui::Pos2>,
    ) {
        match self.drawing.editor.doc.annotation(id) {
            Some(Annotation::Note { .. }) => {
                self.start_note_edit(NoteTarget::Model(id), new, at);
                self.drawing.editor.selected_annotation = Some(id);
            }
            Some(_) => {
                self.drawing.editor.selected_annotation = Some(id);
                self.show_panel(crate::dock::Panel::Inspector);
            }
            None => {}
        }
    }

    /// Start typing into a note (a sheet's or the model's): the caret goes where `at` is on
    /// its text, or at the end.
    pub(crate) fn start_note_edit(
        &mut self,
        target: NoteTarget,
        new: bool,
        at: Option<egui::Pos2>,
    ) {
        if self.drawing.note_edit.as_ref().map(|e| e.target) == Some(target) {
            return;
        }
        self.finish_note_edit();
        // A note just placed: its first words join the step that placed it.
        self.drawing.note_edit = Some(NoteEdit::new(target, new, new, at));
    }

    /// Whether a sheet's note is being typed.
    pub(crate) fn typing_sheet_note(&self) -> bool {
        matches!(
            self.drawing.note_edit.as_ref().map(|e| e.target),
            Some(NoteTarget::Sheet(..))
        )
    }

    /// The words and the style of a note.
    fn note_words(&self, target: NoteTarget) -> Option<(String, NoteStyle)> {
        let doc = &self.drawing.editor.doc;
        match target {
            NoteTarget::Model(id) => match doc.annotation(id)? {
                Annotation::Note { text, style, .. } => Some((text.clone(), *style)),
                _ => None,
            },
            NoteTarget::Sheet(sheet, id) => match doc.sheet_annotation(sheet, id)? {
                drift_core::SheetAnnotation::Note { text, style, .. } => {
                    Some((text.clone(), *style))
                }
                _ => None,
            },
        }
    }

    /// Write a note's words and style back, its text moved by `shift` (on screen, through
    /// `map`): one undo step named `label`, or joined to the last one (`merge`). True if
    /// anything changed.
    #[allow(clippy::too_many_arguments)]
    fn put_note(
        &mut self,
        target: NoteTarget,
        text: String,
        style: NoteStyle,
        shift: egui::Vec2,
        map: &ScreenMap,
        label: &str,
        merge: bool,
    ) -> bool {
        let by = {
            let o = map.rect.center();
            map.to_world(o + shift) - map.to_world(o)
        };
        let ed = &mut self.drawing.editor;
        let changed = match target {
            NoteTarget::Model(id) => {
                let Some(Annotation::Note {
                    text: was,
                    at,
                    leaders,
                    style: had,
                }) = ed.doc.annotation(id).cloned()
                else {
                    return false;
                };
                if was == text && had == style {
                    return false;
                }
                let at = ed
                    .doc
                    .note_text_pos(at, &leaders)
                    .and_then(|pos| ed.doc.note_at(pos + by, &leaders))
                    .unwrap_or(at);
                ed.doc.begin();
                ed.doc
                    .set_annotation(
                        id,
                        Annotation::Note {
                            text,
                            at,
                            leaders,
                            style,
                        },
                    )
                    .is_ok()
            }
            NoteTarget::Sheet(sheet, id) => {
                let Some(mut a) = ed.doc.sheet_annotation(sheet, id).cloned() else {
                    return false;
                };
                let drift_core::SheetAnnotation::Note {
                    text: was,
                    style: had,
                    at,
                    ..
                } = &mut a
                else {
                    return false;
                };
                if *was == text && *had == style {
                    return false;
                }
                *was = text;
                *had = style;
                *at += by;
                ed.doc.begin();
                ed.doc.set_sheet_annotation(sheet, id, a).is_ok()
            }
        };
        if !changed {
            ed.doc.rollback();
            return false;
        }
        let tx = ed.doc.commit(label);
        if merge {
            ed.history.merge_into_last(tx);
        } else {
            ed.history.push(tx);
        }
        ed.sync();
        true
    }

    /// How far a note's text moves (on screen) when it is set as `to` instead of as `from`:
    /// the side its lines align to and its top stay put, so the text grows away from where
    /// the typing started (as in any editor), not both ways from its middle.
    fn note_growth(
        painter: &egui::Painter,
        from: &notes::NoteText,
        to: &notes::NoteText,
    ) -> egui::Vec2 {
        let size = |t: &notes::NoteText| t.galley(painter, egui::Color32::WHITE).rect.size();
        let d = size(to) - size(from);
        let x = match from.align {
            egui::Align::Min => d.x / 2.0,
            egui::Align::Center => 0.0,
            egui::Align::Max => -d.x / 2.0,
        };
        egui::vec2(x, d.y / 2.0)
    }

    /// The model note being typed, on the canvas (see [`Self::live_note`]).
    pub(crate) fn note_editor(&mut self, ctx: &egui::Context, layouts: &[NoteLayout]) {
        let Some(target) = self.drawing.note_edit.as_ref().map(|e| e.target) else {
            return;
        };
        let NoteTarget::Model(id) = target else {
            // (A sheet's note is typed on its sheet.)
            if !self.drawing.view.sheet_preview {
                self.finish_note_edit();
            }
            return;
        };
        let Some(text) = layouts
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.text.clone())
        else {
            // Hidden (annotations off, its layer hidden) or gone: done with.
            self.finish_note_edit();
            return;
        };
        let canvas = self.drawing.view.canvas_rect;
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(canvas);
        let map = self.canvas_map();
        self.live_note(ctx, &painter, &map, &text, canvas, false);
    }

    /// The sheet note being typed, on the sheet drawn in `rect` (see [`Self::live_note`]).
    pub(crate) fn sheet_note_editor(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: egui::Rect,
        layouts: &[crate::sheet_notes::SheetLayout],
    ) {
        let Some(NoteTarget::Sheet(sheet, id)) = self.drawing.note_edit.as_ref().map(|e| e.target)
        else {
            return;
        };
        let text = layouts
            .iter()
            .filter(|_| self.current_sheet() == Some(sheet))
            .find(|l| l.id == id)
            .and_then(|l| match &l.shape {
                crate::sheet_notes::SheetShape::Note(n) => n.text.clone(),
                crate::sheet_notes::SheetShape::Dimension(_) => None,
            });
        let Some(text) = text else {
            self.finish_note_edit();
            return;
        };
        let map = self.paper_map(rect);
        self.live_note(ctx, &painter.with_clip_rect(rect), &map, &text, rect, true);
    }

    /// Typing a note in place, as a text is typed (`text_editor`): the keyboard goes to a
    /// field out of sight, for all a field knows (the clipboard, composed characters, the
    /// platform's keys), and every change goes straight into the note, so its frame and
    /// leaders follow. The caret and the selection are drawn on the note as it is set
    /// (`text`, on `painter`, mapped by `map`). A click in the text puts the caret there, a
    /// drag selects, a double click selects a word and a triple click a line; Up, Down,
    /// Home and End go by the lines as set; Return adds a line; Ctrl+B and Ctrl+I switch
    /// the note's bold and italic. Esc or a press anywhere else on `canvas` finishes,
    /// keeping the text. `paper`: the note is on a sheet's white paper, whatever the theme.
    fn live_note(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        map: &ScreenMap,
        text: &notes::NoteText,
        canvas: egui::Rect,
        paper: bool,
    ) {
        let Some(mut edit) = self.drawing.note_edit.take() else {
            return;
        };
        let Some((words, style)) = self.note_words(edit.target) else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let field = egui::Id::new("note_edit_field");
        let count = words.chars().count();
        let caret = text.caret_map(painter);

        // The pointer: in the text it moves the caret or selects; anywhere else on the
        // canvas it finishes.
        let (pressed, down, at, shift, double, triple) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
                i.modifiers.shift,
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_triple_clicked(PointerButton::Primary),
            )
        });
        let from = edit.cursor.map(|c| c.1);
        if !edit.starting {
            let on_canvas = |p: egui::Pos2| {
                canvas.contains(p) && ctx.layer_id_at(p) == Some(painter.layer_id())
            };
            if pressed && let Some(p) = at.filter(|p| on_canvas(*p)) {
                if !text.rect.expand(6.0).contains(p) {
                    self.drawing.note_edit = Some(edit);
                    self.finish_note_edit();
                    return;
                }
                let to = caret.index_at(p);
                edit.set_cursor = Some((to, from.filter(|_| shift).unwrap_or(to)));
                edit.grip = true;
                edit.touched = true;
            } else if edit.grip {
                edit.touched = true;
                if down {
                    if let Some(p) = at {
                        let to = caret.index_at(p);
                        edit.set_cursor = Some((to, from.unwrap_or(to)));
                    }
                } else {
                    edit.grip = false;
                    let now_at = edit.cursor.map_or(0, |c| c.0);
                    if triple {
                        let (start, end) = paragraph_range(&words, now_at);
                        edit.set_cursor = Some((end, start));
                    } else if double {
                        let (start, end) = drift_draft::outline::word_range(&words, now_at);
                        edit.set_cursor = Some((end, start));
                    }
                }
            }
        }

        // Where the typing starts: at the click that began it, or at the end.
        if edit.starting && edit.set_cursor.is_none() {
            let to = edit.caret_at.take().map_or(count, |p| caret.index_at(p));
            edit.set_cursor = Some((to, to));
        }
        // Up, Down, Home and End go by the lines as they are set on the canvas, which the
        // field knows nothing of; Ctrl+B and Ctrl+I set the whole note bold or italic.
        let mut restyled = style;
        if !edit.starting
            && ctx.memory(|m| m.has_focus(field))
            && let Some((now_at, from)) = edit.cursor
        {
            for key in [
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::Home,
                egui::Key::End,
            ] {
                let pressed = ctx.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::SHIFT, key) {
                        Some(true)
                    } else {
                        i.consume_key(egui::Modifiers::NONE, key).then_some(false)
                    }
                });
                if let Some(extend) = pressed {
                    let to = match key {
                        egui::Key::ArrowUp => caret.line_step(now_at, -1),
                        egui::Key::ArrowDown => caret.line_step(now_at, 1),
                        egui::Key::Home => caret.line_ends(now_at).0,
                        _ => caret.line_ends(now_at).1,
                    };
                    edit.set_cursor = Some((to, if extend { from } else { to }));
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
                restyled.bold = !restyled.bold;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
                restyled.italic = !restyled.italic;
            }
        }
        if restyled != style {
            let mut to = text.clone();
            to.bold = restyled.bold;
            to.italic = restyled.italic;
            let shift = Self::note_growth(painter, text, &to);
            let label = match edit.target {
                NoteTarget::Model(_) => "Edit note",
                NoteTarget::Sheet(..) => "Edit sheet note",
            };
            self.put_note(
                edit.target,
                words.clone(),
                restyled,
                shift,
                map,
                label,
                false,
            );
            // Typing after it is a step of its own.
            edit.merge = false;
            edit.blink = now;
        }
        if let Some((to, from)) = edit.set_cursor.take() {
            let mut state = egui::TextEdit::load_state(ctx, field).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(from.min(count)),
                    egui::text::CCursor::new(to.min(count)),
                )));
            state.store(ctx, field);
            edit.blink = now;
        }

        // The field, out of sight by the caret (where a window for composing characters
        // opens) and click-through.
        let [foot, head] = caret.caret(edit.cursor.map_or(count, |c| c.0));
        let mut typed = words.clone();
        let response = egui::Area::new(egui::Id::new("note_edit"))
            .fixed_pos(head)
            .constrain(false)
            .interactable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_clip_rect(egui::Rect::from_min_size(
                    head - egui::vec2(16.0, 16.0),
                    egui::Vec2::ZERO,
                ));
                ui.add(
                    egui::TextEdit::multiline(&mut typed)
                        .id(field)
                        .desired_rows(1)
                        .desired_width(260.0),
                )
            })
            .inner;
        if response.changed() && typed != words {
            let mut to = text.clone();
            to.text = if typed.is_empty() {
                " ".to_owned()
            } else {
                typed.clone()
            };
            let shift = Self::note_growth(painter, text, &to);
            let label = match edit.target {
                NoteTarget::Model(_) => "Note text",
                NoteTarget::Sheet(..) => "Sheet note text",
            };
            if self.put_note(edit.target, typed, restyled, shift, map, label, edit.merge) {
                // Typing is one undo step per visit to the note.
                edit.merge = true;
            }
            edit.blink = now;
        }
        if edit.starting {
            response.request_focus();
            edit.starting = false;
        } else if std::mem::take(&mut edit.touched) {
            // A press or a click in the text took the keyboard from the field: give it back.
            response.request_focus();
        } else if !response.has_focus() {
            // Esc, or a click elsewhere: done, the text kept.
            self.drawing.note_edit = Some(edit);
            self.finish_note_edit();
            return;
        }
        let cursor = egui::TextEdit::load_state(ctx, field)
            .and_then(|state| state.cursor.char_range())
            .map(|r| (r.primary.index.0, r.secondary.index.0));
        if cursor != edit.cursor {
            edit.blink = now;
        }
        edit.cursor = cursor;
        let (blink, grip) = (edit.blink, edit.grip);
        self.drawing.note_edit = Some(edit);

        // The caret and the selection, on the note as it was laid out this frame (a change
        // just typed shows on the next, asked for now).
        if response.changed() {
            ctx.request_repaint();
        }
        let dark = ctx.style_of(ctx.theme()).visuals.dark_mode;
        let colors = OverlayColors::new(dark);
        let (to, from) = cursor.unwrap_or((count, count));
        for r in caret.selection(from, to) {
            painter.rect_filled(r, 0.0, colors.window.gamma_multiply(0.35));
        }
        // Steady for half a second after it moves, then half a second off and on.
        let since = (now - blink).max(0.0);
        if since % 1.0 < 0.5 {
            let ink = if dark && !paper {
                egui::Color32::WHITE
            } else {
                egui::Color32::BLACK
            };
            painter.line_segment([foot, head], egui::Stroke::new(1.5, ink));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 - since % 0.5));
        if words.is_empty() {
            painter.text(
                foot + egui::vec2(8.0, 0.0),
                egui::Align2::LEFT_BOTTOM,
                "Type the note",
                egui::FontId::proportional(12.0),
                colors.window.gamma_multiply(0.7),
            );
        }
        // Composed characters (accents, Chinese, Japanese…) open their window at the caret.
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: canvas,
                cursor_rect: egui::Rect::from_two_pos(foot, head),
                should_interrupt_composition: false,
            });
        });
        // An I-beam over the text.
        if !grip
            && let Some(p) = ctx.input(|i| i.pointer.hover_pos())
            && canvas.contains(p)
            && text.rect.expand(6.0).contains(p)
            && ctx.layer_id_at(p) == Some(painter.layer_id())
        {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    /// Stop typing the note being typed, keeping its text. Trailing spaces and lines go
    /// (with the typing, in its step). A note left empty goes: a new one in the step that
    /// placed it, so no undo brings a blank note back.
    pub(crate) fn finish_note_edit(&mut self) {
        let Some(edit) = self.drawing.note_edit.take() else {
            return;
        };
        let Some((words, style)) = self.note_words(edit.target) else {
            return;
        };
        // (One with leaders still shows them.)
        let bare = match edit.target {
            NoteTarget::Model(id) => matches!(
                self.drawing.editor.doc.annotation(id),
                Some(Annotation::Note { leaders, .. }) if leaders.is_empty()
            ),
            NoteTarget::Sheet(sheet, id) => matches!(
                self.drawing.editor.doc.sheet_annotation(sheet, id),
                Some(drift_core::SheetAnnotation::Note { leaders, .. }) if leaders.is_empty()
            ),
        };
        if words.trim().is_empty() && (edit.new || bare) {
            let ed = &mut self.drawing.editor;
            if let NoteTarget::Model(id) = edit.target
                && ed.selected_annotation == Some(id)
            {
                ed.selected_annotation = None;
            }
            if edit.new && edit.merge {
                // Placed and never kept: the step that placed it (with any typing) is taken
                // back and forgotten, so no undo brings a blank note back.
                ed.history.discard_last(1, &mut ed.doc);
            } else {
                ed.doc.begin();
                let label = match edit.target {
                    NoteTarget::Model(id) => {
                        ed.doc.remove_annotation(id);
                        "Note"
                    }
                    NoteTarget::Sheet(sheet, id) => {
                        ed.doc.remove_sheet_annotation(sheet, id);
                        "Sheet note"
                    }
                };
                let tx = ed.doc.commit(label);
                // (With the typing that emptied it.)
                if edit.merge {
                    ed.history.merge_into_last(tx);
                } else {
                    ed.history.push(tx);
                }
            }
            ed.sync();
            if let NoteTarget::Sheet(_, id) = edit.target
                && self.drawing.view.sheet_annotation == Some(id)
            {
                self.pick_sheet_annotation(None);
            }
            return;
        }
        let trimmed = words.trim_end().to_owned();
        if trimmed != words {
            let map = self.canvas_map();
            self.put_note(
                edit.target,
                trimmed,
                style,
                egui::Vec2::ZERO,
                &map,
                "Note text",
                edit.merge,
            );
        }
    }

    /// "Add centre marks" for selected circles and arcs that don't have one yet.
    pub(crate) fn centre_mark_button(&mut self, ui: &mut egui::Ui) {
        let doc = &self.drawing.editor.doc;
        let unmarked: Vec<drift_core::EntityId> = self
            .drawing
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
            self.drawing.editor.doc.begin();
            for curve in unmarked {
                let _ = self
                    .drawing
                    .editor
                    .doc
                    .add_annotation(Annotation::CentreMark { curve });
            }
            self.drawing.editor.commit("Center marks");
        }
    }

    /// The inspector for a picked annotation. Returns true if one is picked.
    pub(crate) fn annotation_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(id) = self.drawing.editor.selected_annotation else {
            return false;
        };
        let Some(a) = self.drawing.editor.doc.annotation(id).cloned() else {
            self.drawing.editor.selected_annotation = None;
            return false;
        };
        // The icon of the tool that makes it.
        let tool = match a {
            Annotation::Note { .. } => ToolKind::Note,
            Annotation::CentreMark { .. } | Annotation::Centreline { .. } => ToolKind::CentreMark,
            Annotation::HoleCallout { .. } | Annotation::ChamferCallout { .. } => ToolKind::Callout,
            Annotation::Ordinate { .. } => ToolKind::Ordinate,
            Annotation::Section { .. } => ToolKind::Section,
            Annotation::Elevation { .. } => ToolKind::Elevation,
        };
        crate::app::panels::title(ui, Some(tool), a.type_name(), "");
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
                        .desired_width(ui.available_width()),
                );
                theme::section(ui, "Style");
                theme::row(ui, "Size", |ui| {
                    let mut own = style.height.is_some();
                    if ui
                        .checkbox(&mut own, "")
                        .on_hover_text("Off: the drafting style's text height")
                        .changed()
                    {
                        style.height =
                            own.then(|| self.drawing.editor.doc.draft_style().text_height);
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
                let doc = &self.drawing.editor.doc;
                let pos = doc.note_text_pos(*at, leaders);
                let mut remove = None;
                for (k, l) in leaders.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(format!("{}", k + 1)).weak());
                        // The two menus share what the shoulder's box and the delete button
                        // leave (in a narrow pane those two go on a line of their own).
                        let gap = ui.spacing().item_spacing.x;
                        let tail = ui.spacing().interact_size.x + 20.0 + 3.0 * gap;
                        let width = ((ui.available_width() - tail - gap) / 2.0)
                            .floor()
                            .max(88.0);
                        egui::ComboBox::from_id_salt(("leader_end", k))
                            .width(width)
                            .selected_text(l.end.label())
                            .show_ui(ui, |ui| {
                                for e in LeaderEnd::ALL {
                                    ui.selectable_value(&mut l.end, e, e.label());
                                }
                            });
                        egui::ComboBox::from_id_salt(("leader_shape", k))
                            .width(width)
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
                        let tip = "Remove this leader (the text stays)";
                        if theme::delete_button(ui, "×", tip).clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if let Some(k) = remove {
                    leaders.remove(k);
                    if let Some(p) = pos
                        && let Some(new_at) = doc.note_at(p, leaders)
                    {
                        *at = new_at;
                    }
                    self.drawing.selected_leader = None;
                }
                if ui
                    .button("Add leader")
                    .on_hover_text(
                        "Then click what it points to. Or drag from a + beside the picked \
                         note's text",
                    )
                    .clicked()
                {
                    self.drawing.adding_leader = Some(id);
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
                         end to re-attach it, a bend's or a curve's circle to shape it, the \
                         diamond to lengthen its shoulder; click a leader and press Delete to \
                         remove it.",
                    )
                    .small()
                    .weak(),
                );
            }
            Annotation::HoleCallout { curve, suffix, .. } => {
                if let Some(t) = notes::hole_callout_text(&self.drawing.editor.doc, *curve, suffix)
                {
                    ui.label(t);
                }
                theme::section(ui, "Text");
                theme::row(ui, "Suffix", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(suffix)
                            .hint_text("THRU, ↧ 10…")
                            .desired_width(ui.available_width()),
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
                if let Some(t) = notes::chamfer_text(&self.drawing.editor.doc, *line) {
                    ui.label(t);
                }
            }
            Annotation::Ordinate {
                origin,
                point,
                axis,
                ..
            } => {
                if let Some(v) =
                    notes::ordinate_value(&self.drawing.editor.doc, *origin, *point, *axis)
                {
                    let units = self.drawing.editor.doc.units();
                    ui.label(self.drawing.editor.doc.draft_style().length(&units, v));
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
                view,
            } => {
                theme::section(ui, "Mark");
                mark_fields(ui, &self.drawing.editor.doc, label, sheet, view, "A");
                theme::row(ui, "Direction", |ui| {
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
                view,
                ..
            } => {
                theme::section(ui, "Mark");
                mark_fields(ui, &self.drawing.editor.doc, label, sheet, view, "1");
                theme::row(ui, "Facing", |ui| {
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
            self.drawing.editor.doc.begin();
            let _ = self.drawing.editor.doc.set_annotation(id, edited);
            self.drawing.editor.commit("Edit note");
        }
        self.visibility_section(ui);
        self.style_section(ui);
        ui.add_space(10.0);
        if theme::danger_button(ui, "Delete").clicked() {
            self.drawing.editor.delete_selection();
        }
        true
    }
}
