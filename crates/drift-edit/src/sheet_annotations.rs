//! Editing a sheet's own annotations (#432): what the Note and Dimension tools make while a
//! sheet is shown, and moving, changing and deleting them, each as one undoable step.

use drift_core::{
    Anchor, Constraint, DVec2, Leader, NoteStyle, PortId, SheetAnnotation, SheetAnnotationId,
    SheetId, SheetLeader, StyleOverride,
};

use crate::editor::{EditError, Editor};

/// Where the pointer is on a sheet, while the tools work there: set by the app before it
/// hands a tool its input, cleared when the model is shown. With it set, the Note and
/// Dimension tools make the sheet's own annotations instead of the model's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnSheet {
    pub sheet: SheetId,
    /// The viewport (its index on the sheet) the pointer's model position is seen through:
    /// the one under it, or the one a tool's first pick was made in.
    pub viewport: Option<usize>,
    /// The pointer on the paper (mm).
    pub paper: DVec2,
}

/// What a sheet note's leader points at, as the Note tool picked it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetTarget {
    /// The viewport (its index on the sheet) it was picked through; `None`: a paper point.
    pub viewport: Option<usize>,
    pub to: Anchor,
}

impl Editor {
    /// A step that only changes a sheet: `f` between `begin` and `commit`, rolled back (and
    /// said why) if it fails.
    fn sheet_step<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Self) -> Result<R, drift_core::DocumentError>,
    ) -> Result<R, EditError> {
        self.doc.begin();
        match f(self) {
            Ok(r) => {
                self.commit(label);
                Ok(r)
            }
            Err(e) => {
                self.doc.rollback();
                self.sync();
                Err(EditError::Invalid(e.to_string()))
            }
        }
    }

    /// The id sheet annotations name `sheet`'s viewport `index` by (given now if it has
    /// none), inside the open step.
    fn tied(&mut self, sheet: SheetId, index: usize) -> Result<PortId, drift_core::DocumentError> {
        self.doc
            .tie_viewport(sheet, index)
            .ok_or(drift_core::DocumentError::Degenerate(
                "sheet annotation: its view is not on the sheet",
            ))
    }

    /// Put a dimension of `measure` on `sheet`, measured through its viewport `viewport`,
    /// with its label `offset` from its anchor in the model (world mm, as the Dimension
    /// tool gives it). It measures and never constrains: it is not among the drawing's
    /// constraints. One undoable step.
    pub fn add_sheet_dimension(
        &mut self,
        sheet: SheetId,
        viewport: usize,
        measure: Constraint,
        offset: Option<DVec2>,
    ) -> Result<SheetAnnotationId, EditError> {
        let scale = self
            .doc
            .sheet(sheet)
            .and_then(|s| s.viewports.get(viewport))
            .and_then(|p| self.doc.view(p.view))
            .map(|v| v.scale)
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| EditError::Invalid("click inside a view to dimension it".into()))?;
        let id = self.sheet_step("Sheet dimension", |ed| {
            let port = ed.tied(sheet, viewport)?;
            // The label is kept in paper mm: the same distance whatever the view's scale.
            let label = offset.map(|o| o / scale);
            ed.doc
                .add_sheet_annotation(sheet, SheetAnnotation::dimension(port, measure, label))
        })?;
        self.out.new_sheet_annotation = Some(id);
        Ok(id)
    }

    /// Put a note on `sheet` with its text at `at` (paper mm) and a leader to each of
    /// `targets`. One undoable step; the app then lets its text be typed.
    pub fn add_sheet_note(
        &mut self,
        sheet: SheetId,
        targets: &[SheetTarget],
        at: DVec2,
    ) -> Result<SheetAnnotationId, EditError> {
        let id = self.sheet_step("Sheet note", |ed| {
            let mut leaders = Vec::new();
            for t in targets {
                let port = match t.viewport {
                    Some(i) => Some(ed.tied(sheet, i)?),
                    None => None,
                };
                leaders.push(SheetLeader {
                    leader: Leader::to(t.to),
                    port,
                });
            }
            ed.doc.add_sheet_annotation(
                sheet,
                SheetAnnotation::Note {
                    text: String::new(),
                    at,
                    leaders,
                    style: NoteStyle::default(),
                    look: StyleOverride::default(),
                },
            )
        })?;
        self.out.new_sheet_annotation = Some(id);
        self.out.pending_sheet_note_edit = Some(id);
        Ok(id)
    }

    /// Replace a sheet annotation, as one undoable step named `label` (nothing happens, and
    /// no step is made, if it is the same or isn't there).
    pub fn set_sheet_annotation(
        &mut self,
        sheet: SheetId,
        id: SheetAnnotationId,
        a: SheetAnnotation,
        label: &str,
    ) -> Result<(), EditError> {
        if self
            .doc
            .sheet_annotation(sheet, id)
            .is_none_or(|was| *was == a)
        {
            return Ok(());
        }
        self.sheet_step(label, |ed| ed.doc.set_sheet_annotation(sheet, id, a))
    }

    /// Delete a sheet annotation, as one undoable step. Returns whether there was one.
    pub fn delete_sheet_annotation(&mut self, sheet: SheetId, id: SheetAnnotationId) -> bool {
        let Some(name) = self.doc.sheet_annotation(sheet, id).map(|a| a.type_name()) else {
            return false;
        };
        let label = format!("Delete {}", name.to_lowercase());
        self.sheet_step(&label, |ed| {
            ed.doc.remove_sheet_annotation(sheet, id);
            Ok(())
        })
        .is_ok()
    }
}

/// `a` with what a drag moves put `by` further (paper mm): a note's text, or a dimension's
/// label (from where it is now, `label_now` paper mm from its anchor, if it hasn't been
/// placed yet).
pub fn moved(a: &SheetAnnotation, by: DVec2, label_now: Option<DVec2>) -> SheetAnnotation {
    let mut a = a.clone();
    match &mut a {
        SheetAnnotation::Note { at, .. } => *at += by,
        SheetAnnotation::Dimension { label, .. } => {
            if let Some(from) = label.or(label_now) {
                *label = Some(from + by);
            }
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use drift_core::{Bounds, Document, EntityId, Sheet, View, Viewport};

    use super::*;
    use crate::editor::Pointer;
    use crate::tools::{DimensionTool, Input, Modifiers, NoteTool, Tool};

    const V: fn(f64, f64) -> DVec2 = DVec2::new;
    const TOL: f64 = 2.0;

    /// A 1000 × 500 rectangle in a 1:10 view placed at (200, 150) on a sheet, with a second
    /// viewport of the same view at (350, 150).
    struct Plan {
        ed: Editor,
        points: Vec<EntityId>,
        lines: Vec<EntityId>,
        sheet: SheetId,
    }

    fn plan() -> Plan {
        let mut doc = Document::new();
        let points: Vec<EntityId> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0), (0.0, 500.0)]
            .into_iter()
            .map(|(x, y)| doc.add_point(V(x, y)))
            .collect();
        let lines = (0..4)
            .map(|i| doc.add_line(points[i], points[(i + 1) % 4]).unwrap())
            .collect();
        let view = doc.add_view(View::new(
            "Plan",
            Bounds {
                min: V(-250.0, -250.0),
                max: V(1250.0, 750.0),
            },
            10.0,
        ));
        let mut s = Sheet::new("A-101");
        s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
        s.viewports.push(Viewport::new(view, V(350.0, 150.0)));
        let sheet = doc.add_sheet(s);
        Plan {
            ed: Editor::new(doc),
            points,
            lines,
            sheet,
        }
    }

    impl Plan {
        /// The pointer at paper point `paper`, seen through viewport `viewport` (as the
        /// sheet canvas makes it), set on the editor; the tool then handles a click there.
        fn click(&mut self, tool: &mut dyn Tool, viewport: Option<usize>, paper: DVec2) {
            self.click_with(tool, viewport, paper, Modifiers::default());
        }

        fn click_with(
            &mut self,
            tool: &mut dyn Tool,
            viewport: Option<usize>,
            paper: DVec2,
            mods: Modifiers,
        ) {
            let sheet = self.ed.doc.sheet(self.sheet).unwrap();
            let views = self.ed.doc.placed_views(sheet);
            // (Outside every view the model position means nothing: the first view's.)
            let world = views[viewport.unwrap_or(0)].to_world(paper);
            self.ed.on_sheet = Some(OnSheet {
                sheet: self.sheet,
                viewport,
                paper,
            });
            let mut p = Pointer::new(world, TOL);
            p.alt = mods.alt;
            p.shift = mods.shift;
            tool.handle(Input::Hover(p), &mut self.ed);
            tool.handle(Input::Click(p, mods), &mut self.ed);
        }

        fn annotations(&self) -> Vec<(SheetAnnotationId, SheetAnnotation)> {
            self.ed
                .doc
                .sheet(self.sheet)
                .unwrap()
                .annotations
                .iter()
                .map(|(id, a)| (*id, a.clone()))
                .collect()
        }
    }

    #[test]
    fn the_dimension_tool_on_a_sheet_makes_the_sheets_own_reference_dimension() {
        let mut p = plan();
        // The model holds the bottom edge at 1000 already: a model dimension of it again
        // would be refused as redundant. The sheet's is not a constraint at all.
        p.ed.add_dimension(
            Constraint::Distance {
                a: p.points[0],
                b: p.points[1],
                value: 1000.0,
            },
            None,
        )
        .unwrap();
        let constraints = p.ed.doc.constraints().count();
        let depth = p.ed.history.undo_depth();
        let mut tool = DimensionTool::default();
        // Click the bottom edge in the first view (off its middle, which would pick its
        // midpoint; the edge is at y = 125 on the paper), then below it to place it.
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        assert!(tool.is_busy());
        p.click(&mut tool, Some(0), V(200.0, 80.0));
        assert!(!tool.is_busy());
        let made = p.annotations();
        assert_eq!(made.len(), 1);
        let SheetAnnotation::Dimension {
            port,
            measure,
            label,
            ..
        } = &made[0].1
        else {
            panic!("a dimension: {made:?}");
        };
        // Through the viewport it was clicked in, which got its id for it.
        let sheet = p.ed.doc.sheet(p.sheet).unwrap();
        assert_eq!(sheet.viewports[0].id, Some(*port));
        assert_eq!(sheet.viewports[1].id, None);
        assert!(matches!(
            measure,
            Constraint::HorizontalDistance { .. } | Constraint::Distance { .. }
        ));
        // Its label is where it was clicked, kept in paper mm: 45 below the edge.
        let label = label.expect("placed");
        assert!((label.y + 45.0).abs() < 1e-9, "{label}");
        // One undo step; nothing added to the model: no constraint, no annotation, and no
        // value to type.
        assert_eq!(p.ed.history.undo_depth(), depth + 1);
        assert_eq!(p.ed.doc.constraints().count(), constraints);
        assert_eq!(p.ed.doc.annotations().count(), 0);
        assert!(p.ed.out.pending_edit.is_none() && p.ed.out.pending_driven.is_none());
        assert_eq!(p.ed.out.new_sheet_annotation, Some(made[0].0));
        // It never drives: with the model's own dimension gone, the edge is free to change
        // and the sheet dimension only follows.
        let model_dim = p.ed.doc.constraints().next().unwrap().0;
        p.ed.doc.begin();
        p.ed.doc.remove_constraint(model_dim);
        p.ed.doc.set_point(p.points[1], V(1200.0, 0.0)).unwrap();
        assert!(p.ed.commit("Stretch"));
        assert_eq!(p.ed.doc.point(p.points[1]).unwrap(), V(1200.0, 0.0));
        assert_eq!(drift_solver::measure(&p.ed.doc, measure), Some(1200.0));
        // Undo, back through the stretch and the dimension.
        assert!(p.ed.undo() && p.ed.undo());
        assert!(p.annotations().is_empty());
        assert!(p.ed.redo());
        assert_eq!(p.annotations().len(), 1);
    }

    #[test]
    fn a_dimension_is_tied_to_the_view_it_was_clicked_in() {
        let mut p = plan();
        let mut tool = DimensionTool::default();
        // The same edge in the second viewport of the same view (its middle at (350, 125)).
        p.click(&mut tool, Some(1), V(330.0, 125.0));
        p.click(&mut tool, Some(1), V(350.0, 100.0));
        let made = p.annotations();
        let SheetAnnotation::Dimension { port, .. } = &made[0].1 else {
            panic!("a dimension");
        };
        let sheet = p.ed.doc.sheet(p.sheet).unwrap();
        assert_eq!(sheet.viewports[1].id, Some(*port));
        assert_eq!(sheet.viewports[0].id, None);
        // Outside every view there is nothing to dimension: nothing is made, and it says so.
        let mut tool = DimensionTool::default();
        p.click(&mut tool, None, V(180.0, 125.0));
        p.click(&mut tool, None, V(200.0, 100.0));
        assert_eq!(p.annotations().len(), 1);
        assert!(p.ed.out.notice.take().is_some());
    }

    #[test]
    fn the_note_tool_on_a_sheet_makes_the_sheets_own_note() {
        let mut p = plan();
        let mut tool = NoteTool::default();
        let depth = p.ed.history.undo_depth();
        // The bottom edge in the first view, then a spot on the paper for the text.
        p.click(&mut tool, Some(0), V(200.0, 125.0));
        assert!(tool.is_busy());
        assert_eq!(tool.sheet_targets().len(), 1);
        p.click(&mut tool, None, V(300.0, 250.0));
        assert!(!tool.is_busy());
        let made = p.annotations();
        assert_eq!(made.len(), 1);
        let SheetAnnotation::Note { at, leaders, .. } = &made[0].1 else {
            panic!("a note");
        };
        assert_eq!(*at, V(300.0, 250.0));
        assert_eq!(leaders.len(), 1);
        let sheet = p.ed.doc.sheet(p.sheet).unwrap();
        assert_eq!(leaders[0].port, sheet.viewports[0].id);
        assert_eq!(leaders[0].leader.to.entity(), Some(p.lines[0]));
        // Its tip is where it was clicked, through the view.
        let tip = p.ed.doc.sheet_leader_tip(sheet, &leaders[0]).unwrap();
        assert!(tip.distance(V(200.0, 125.0)) < 1e-9, "{tip}");
        // One step; its text is to be typed; the model has no note.
        assert_eq!(p.ed.history.undo_depth(), depth + 1);
        assert_eq!(p.ed.out.pending_sheet_note_edit, Some(made[0].0));
        assert!(p.ed.out.pending_note_edit.is_none());
        assert_eq!(p.ed.doc.annotations().count(), 0);

        // A click on bare paper: a note without a leader, there.
        p.click(&mut tool, None, V(40.0, 40.0));
        let made = p.annotations();
        assert!(matches!(
            &made[1].1,
            SheetAnnotation::Note { at, leaders, .. } if *at == V(40.0, 40.0) && leaders.is_empty()
        ));

        // Alt+click inside a view, on nothing: a leader to that spot of the model.
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        p.click_with(&mut tool, Some(0), V(210.0, 160.0), alt);
        assert!(tool.is_busy());
        p.click(&mut tool, None, V(60.0, 200.0));
        let made = p.annotations();
        let SheetAnnotation::Note { leaders, .. } = &made[2].1 else {
            panic!("a note");
        };
        // (210, 160) on the paper is the model's (600, 350) through the 1:10 view.
        assert_eq!(leaders[0].leader.to, Anchor::Free(V(600.0, 350.0)));
        assert!(leaders[0].port.is_some());

        // Esc with a leader started lets go of it and makes nothing.
        p.click(&mut tool, Some(0), V(200.0, 125.0));
        assert!(tool.cancel(&mut p.ed));
        assert!(!tool.is_busy());
        assert_eq!(p.annotations().len(), 3);
    }

    #[test]
    fn in_the_model_the_tools_make_what_they_always_did() {
        let mut p = plan();
        assert!(p.ed.on_sheet.is_none());
        let mut tool = NoteTool::default();
        let at = Pointer::new(V(2000.0, 2000.0), TOL);
        tool.handle(Input::Click(at, Modifiers::default()), &mut p.ed);
        assert_eq!(p.ed.doc.annotations().count(), 1);
        assert!(p.ed.out.pending_note_edit.is_some());
        let mut tool = DimensionTool::default();
        for at in [V(300.0, 0.0), V(500.0, -100.0)] {
            let at = Pointer::new(at, TOL);
            tool.handle(Input::Click(at, Modifiers::default()), &mut p.ed);
        }
        assert_eq!(p.ed.doc.constraints().count(), 1);
        assert!(p.annotations().is_empty());
        assert!(p.ed.out.new_sheet_annotation.is_none());
    }

    #[test]
    fn moving_changing_and_deleting_are_one_step_each() {
        let mut p = plan();
        let mut tool = NoteTool::default();
        p.click(&mut tool, None, V(40.0, 40.0));
        let (id, note) = p.annotations().remove(0);
        let depth = p.ed.history.undo_depth();
        // Moved 5 right, 3 up.
        let there = moved(&note, V(5.0, 3.0), None);
        p.ed.set_sheet_annotation(p.sheet, id, there.clone(), "Move sheet note")
            .unwrap();
        assert_eq!(p.ed.history.undo_depth(), depth + 1);
        assert!(
            matches!(&p.annotations()[0].1, SheetAnnotation::Note { at, .. } if *at == V(45.0, 43.0))
        );
        // Set to the same again: no step.
        p.ed.set_sheet_annotation(p.sheet, id, there.clone(), "Move sheet note")
            .unwrap();
        assert_eq!(p.ed.history.undo_depth(), depth + 1);
        // Its text and style changed.
        let mut edited = there.clone();
        if let SheetAnnotation::Note { text, style, .. } = &mut edited {
            *text = "SEE DETAIL 3".into();
            style.bold = true;
        }
        p.ed.set_sheet_annotation(p.sheet, id, edited.clone(), "Edit sheet note")
            .unwrap();
        assert_eq!(p.ed.history.undo_depth(), depth + 2);
        // A change that can't be kept is refused and makes no step.
        let mut bad = edited.clone();
        if let SheetAnnotation::Note { at, .. } = &mut bad {
            at.x = f64::INFINITY;
        }
        assert!(
            p.ed.set_sheet_annotation(p.sheet, id, bad, "Edit sheet note")
                .is_err()
        );
        assert_eq!(p.ed.history.undo_depth(), depth + 2);
        assert_eq!(p.annotations()[0].1, edited);
        // Deleted; and each step undoes on its own.
        assert!(p.ed.delete_sheet_annotation(p.sheet, id));
        assert!(!p.ed.delete_sheet_annotation(p.sheet, id));
        assert_eq!(p.ed.history.undo_depth(), depth + 3);
        assert!(p.annotations().is_empty());
        assert!(p.ed.undo());
        assert_eq!(p.annotations()[0].1, edited);
        assert!(p.ed.undo());
        assert_eq!(p.annotations()[0].1, there);
        assert!(p.ed.undo());
        assert_eq!(p.annotations()[0].1, note);
        // A dimension's label moves from where it is, placed or not.
        let d = SheetAnnotation::dimension(
            PortId(1),
            Constraint::Distance {
                a: p.points[0],
                b: p.points[1],
                value: 1.0,
            },
            None,
        );
        assert!(matches!(
            moved(&d, V(1.0, 2.0), Some(V(0.0, -7.0))),
            SheetAnnotation::Dimension { label: Some(l), .. } if l == V(1.0, -5.0)
        ));
    }
}
