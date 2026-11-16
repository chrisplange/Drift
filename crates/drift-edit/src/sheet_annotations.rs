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
    /// The viewport the pointer's model position is seen through: the one under it, or the
    /// one a tool's first pick was made in.
    pub viewport: Option<SheetPort>,
    /// The pointer on the paper (mm).
    pub paper: DVec2,
}

/// A viewport of a sheet as it was when a tool picked through it: its place in the sheet's
/// list, the view it shows and where it sits. A pick is only as good as the viewport it was
/// made through, and a sheet can change between two clicks (an undo, a view taken off or
/// moved, another sheet shown), so what a tool holds is checked against the sheet again
/// before anything is made from it ([`SheetPort::index_on`]); an index alone would then
/// name whatever viewport is in that place now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetPort {
    pub index: usize,
    pub view: drift_core::ViewId,
    pub at: DVec2,
}

impl SheetPort {
    /// The viewport at `index` of `sheet`, as it is now.
    pub fn of(sheet: &drift_core::Sheet, index: usize) -> Option<Self> {
        sheet.viewports.get(index).map(|p| SheetPort {
            index,
            view: p.view,
            at: p.at,
        })
    }

    /// Its index on `sheet`, if the viewport there is still the one that was picked through
    /// (the same view in the same place).
    pub fn index_on(&self, sheet: &drift_core::Sheet) -> Option<usize> {
        (Self::of(sheet, self.index) == Some(*self)).then_some(self.index)
    }
}

/// What a sheet note's leader points at, as the Note tool picked it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetTarget {
    /// The sheet it was picked on.
    pub sheet: SheetId,
    /// The viewport it was picked through; `None`: a paper point.
    pub viewport: Option<SheetPort>,
    pub to: Anchor,
}

/// Said when what a tool picked through is no longer what is on the sheet.
const STALE: &str = "the view changed since it was picked: pick again";

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

    /// The id sheet annotations name `sheet`'s viewport `port` by (given now if it has
    /// none), inside the open step. Refused if the sheet's viewport there is no longer the
    /// one that was picked through.
    fn tied(
        &mut self,
        sheet: SheetId,
        port: SheetPort,
    ) -> Result<PortId, drift_core::DocumentError> {
        let index = self
            .doc
            .sheet(sheet)
            .and_then(|s| port.index_on(s))
            .ok_or(drift_core::DocumentError::Degenerate(STALE))?;
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
        viewport: SheetPort,
        measure: Constraint,
        offset: Option<DVec2>,
    ) -> Result<SheetAnnotationId, EditError> {
        let scale = self
            .doc
            .sheet(sheet)
            .and_then(|s| viewport.index_on(s))
            .and_then(|_| self.doc.view(viewport.view))
            .map(|v| v.scale)
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| EditError::Invalid(STALE.into()))?;
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
        // Each leader was picked on this sheet (a pick doesn't carry to another).
        if targets.iter().any(|t| t.sheet != sheet) {
            return Err(EditError::Invalid(STALE.into()));
        }
        let id = self.sheet_step("Sheet note", |ed| {
            let mut leaders = Vec::new();
            for t in targets {
                let port = match t.viewport {
                    Some(port) => Some(ed.tied(sheet, port)?),
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

/// Whether a model annotation brought onto a sheet stays in the model too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToSheet {
    /// The model keeps it as it is.
    Copy,
    /// It leaves the model: a note or a reference dimension is taken out. A dimension that
    /// drives the geometry stays (taking it out would let the geometry change) but no longer
    /// prints in the view it was brought through.
    Move,
}

/// What [`Editor::bring_to_sheet`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brought {
    /// The sheet's new annotations, in the order asked for.
    pub made: Vec<SheetAnnotationId>,
    /// Driving dimensions a move left in the model (they hold the geometry), now off that
    /// view's prints.
    pub kept: usize,
    /// What a sheet can't hold (centre marks, callouts, ordinates, section and elevation
    /// marks) or that wasn't there: left where it was.
    pub left: usize,
}

impl Brought {
    /// What happened, in words for the status line.
    pub fn summary(&self, how: ToSheet) -> String {
        let n = self.made.len();
        let mut text = format!(
            "{} {n} annotation{} onto the sheet",
            match how {
                ToSheet::Copy => "Copied",
                ToSheet::Move => "Moved",
            },
            if n == 1 { "" } else { "s" }
        );
        if self.kept > 0 {
            text.push_str(&format!(
                " · {} driving dimension{} stay{} in the model, off this view's prints",
                self.kept,
                if self.kept == 1 { "" } else { "s" },
                if self.kept == 1 { "s" } else { "" },
            ));
        }
        if self.left > 0 {
            text.push_str(&format!(
                " · {} left in the model (marks, callouts and ordinates stay there)",
                self.left
            ));
        }
        text
    }
}

impl Editor {
    /// The model's notes and dimensions that `view` shows where it is placed on a sheet: by
    /// the rule the page prints by (the view's Dimensions and Notes switches, each one's Show
    /// and In views settings, the layers the view hides) and inside its crop. These are what
    /// [`Self::bring_to_sheet`] can bring onto a sheet through a viewport of the view.
    pub fn view_shows(&self, view: drift_core::ViewId) -> Vec<drift_core::AnnotationRef> {
        use drift_core::{Annotation, AnnotationRef};
        let doc = &self.doc;
        let Some(v) = doc.view(view) else {
            return Vec::new();
        };
        let crop = v.crop();
        let inside = |p: DVec2| p.cmpge(crop.min).all() && p.cmple(crop.max).all();
        let mut out: Vec<AnnotationRef> = doc
            .constraints()
            .filter(|(id, c)| c.is_dimension() && doc.view_draws_dimension(view, *id))
            .map(|(id, _)| AnnotationRef::Dimension(id))
            .collect();
        for (id, a) in doc.annotations() {
            let Annotation::Note { at, leaders, .. } = a else {
                continue;
            };
            let r = AnnotationRef::Annotation(id);
            let entities = a.entities();
            let hidden = !v.hidden_layers.is_empty()
                && !entities.is_empty()
                && entities.iter().all(|e| {
                    doc.get(*e)
                        .is_some_and(|x| v.hidden_layers.contains(&x.layer))
                });
            let seen = doc.note_text_pos(*at, leaders).is_some_and(inside)
                || leaders
                    .iter()
                    .filter_map(|l| doc.anchor_pos(&l.to))
                    .any(inside);
            if v.notes && doc.shown(r).sheets && doc.in_view(r, view) && !hidden && seen {
                out.push(r);
            }
        }
        out
    }

    /// The model annotation `r` as the sheet's own, through `view` (placed) and tied by
    /// `port`: at the same place on the paper, with the same text and settings. `None` for a
    /// kind a sheet can't hold, or one that isn't there.
    fn as_sheet_annotation(
        &self,
        r: drift_core::AnnotationRef,
        view: &drift_core::PlacedView,
        port: PortId,
    ) -> Option<SheetAnnotation> {
        use drift_core::{Annotation, AnnotationRef, LeaderShape};
        let doc = &self.doc;
        let look = doc.style_override(r);
        match r {
            AnnotationRef::Dimension(id) => {
                let c = doc.constraint(id).filter(|c| c.is_dimension())?;
                Some(SheetAnnotation::Dimension {
                    port,
                    measure: *c,
                    // (A model dimension's label offset is kept in paper mm too.)
                    label: doc.placement(id),
                    note: doc.dim_note(id).cloned().unwrap_or_default(),
                    look,
                })
            }
            AnnotationRef::Annotation(id) => {
                let Annotation::Note {
                    text,
                    at,
                    leaders,
                    style,
                } = doc.annotation(id)?
                else {
                    return None;
                };
                let scale = view.scale;
                let leaders = leaders
                    .iter()
                    .map(|l| SheetLeader {
                        leader: Leader {
                            // A bend is given from the tip: model mm there, paper mm here.
                            shape: match l.shape {
                                LeaderShape::Bent(b) => LeaderShape::Bent(b / scale),
                                other => other,
                            },
                            ..*l
                        },
                        port: Some(port),
                    })
                    .collect();
                Some(SheetAnnotation::Note {
                    text: text.clone(),
                    at: view.to_paper(doc.note_text_pos(*at, leaders_of(doc, id))?),
                    leaders,
                    style: *style,
                    look,
                })
            }
        }
    }

    /// Bring the model's notes and dimensions `items` onto `sheet` through its viewport
    /// `port`: each becomes the sheet's own annotation, tied to the same geometry through
    /// that view and at the same place on the paper. [`ToSheet::Move`] then takes notes and
    /// reference dimensions out of the model; a driving dimension stays there and is taken
    /// off that view's prints instead. One undoable step for all of them. Nothing is made,
    /// and no step, if none of `items` can be brought.
    pub fn bring_to_sheet(
        &mut self,
        sheet: SheetId,
        port: SheetPort,
        items: &[drift_core::AnnotationRef],
        how: ToSheet,
    ) -> Result<Brought, EditError> {
        use drift_core::AnnotationRef;
        let placed = self
            .doc
            .sheet(sheet)
            .and_then(|s| port.index_on(s).map(|i| (s, i)))
            .and_then(|(s, i)| self.doc.placed_views(s).into_iter().nth(i))
            .filter(|v| v.id == port.view && v.scale.is_finite() && v.scale > 0.0)
            .ok_or_else(|| EditError::Invalid(STALE.into()))?;
        let label = match (how, items.len()) {
            (ToSheet::Copy, 1) => "Copy annotation onto sheet",
            (ToSheet::Copy, _) => "Copy annotations onto sheet",
            (ToSheet::Move, 1) => "Move annotation onto sheet",
            (ToSheet::Move, _) => "Move annotations onto sheet",
        };
        let mut brought = Brought::default();
        self.doc.begin();
        let done: Result<(), drift_core::DocumentError> = (|| {
            let id = self.tied(sheet, port)?;
            for r in items {
                let Some(a) = self.as_sheet_annotation(*r, &placed, id) else {
                    brought.left += 1;
                    continue;
                };
                brought.made.push(self.doc.add_sheet_annotation(sheet, a)?);
                if how == ToSheet::Copy {
                    continue;
                }
                match *r {
                    AnnotationRef::Annotation(n) => self.doc.remove_annotation(n),
                    AnnotationRef::Dimension(c) if self.doc.is_driven(c) => {
                        self.doc.remove_constraint(c);
                    }
                    // It holds the geometry: it stays, and stops printing in this view.
                    AnnotationRef::Dimension(_) => {
                        brought.kept += 1;
                        let others: std::collections::BTreeSet<drift_core::ViewId> = self
                            .doc
                            .views()
                            .map(|(v, _)| v)
                            .filter(|v| *v != port.view && self.doc.in_view(*r, *v))
                            .collect();
                        if others.is_empty() {
                            let mut shown = self.doc.shown(*r);
                            shown.sheets = false;
                            self.doc.set_shown(*r, shown);
                        } else {
                            self.doc.set_only_in(*r, others);
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = done {
            self.doc.rollback();
            self.sync();
            return Err(EditError::Invalid(e.to_string()));
        }
        if brought.made.is_empty() {
            self.doc.rollback();
            self.sync();
            return Ok(brought);
        }
        self.commit(label);
        // What left the model is no longer picked there.
        if self
            .selected_annotation
            .is_some_and(|n| self.doc.annotation(n).is_none())
        {
            self.selected_annotation = None;
        }
        let doc = &self.doc;
        self.annotation_picks
            .retain(|r| doc.annotation_ref_exists(*r));
        Ok(brought)
    }
}

/// A model note's leaders (empty for anything else).
fn leaders_of(doc: &drift_core::Document, id: drift_core::AnnotationId) -> &[Leader] {
    match doc.annotation(id) {
        Some(drift_core::Annotation::Note { leaders, .. }) => leaders,
        _ => &[],
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
            let viewport = viewport.and_then(|i| SheetPort::of(sheet, i));
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
    fn a_pick_is_not_finished_on_another_sheet_or_through_a_changed_viewport() {
        let mut p = plan();
        // A second sheet with an unrelated view of its own, at another scale.
        let elsewhere = p.ed.doc.add_view(View::new(
            "Elsewhere",
            Bounds {
                min: V(5000.0, 5000.0),
                max: V(5400.0, 5300.0),
            },
            2.0,
        ));
        let mut s = Sheet::new("A-102");
        s.viewports.push(Viewport::new(elsewhere, V(200.0, 150.0)));
        let other = p.ed.doc.add_sheet(s);
        let on_other = |p: &Plan, paper: DVec2| OnSheet {
            sheet: other,
            viewport: SheetPort::of(p.ed.doc.sheet(other).unwrap(), 0),
            paper,
        };
        let count =
            |p: &Plan| -> usize { p.ed.doc.sheets().map(|(_, s)| s.annotations.len()).sum() };

        // The Dimension tool: the edge picked on the first sheet, then a click on the other.
        let mut tool = DimensionTool::default();
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        assert!(tool.is_busy());
        p.ed.on_sheet = Some(on_other(&p, V(200.0, 135.0)));
        let at = Pointer::new(V(5200.0, 5120.0), TOL);
        tool.handle(Input::Click(at, Modifiers::default()), &mut p.ed);
        assert_eq!(count(&p), 0, "nothing is made from a pick on another sheet");
        assert!(!tool.is_busy(), "the pick is let go");
        assert!(p.ed.out.notice.take().is_some(), "and it says so");

        // The Note tool: a leader started on the first sheet, then the text on the other.
        // The note is placed there without it: leaders don't carry between sheets.
        let mut tool = NoteTool::default();
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        assert_eq!(tool.sheet_targets().len(), 1);
        p.ed.on_sheet = Some(OnSheet {
            viewport: None,
            ..on_other(&p, V(60.0, 60.0))
        });
        let at = Pointer::new(V(60.0, 60.0), TOL);
        tool.handle(Input::Click(at, Modifiers::default()), &mut p.ed);
        let made: Vec<_> =
            p.ed.doc
                .sheet(other)
                .unwrap()
                .annotations
                .values()
                .cloned()
                .collect();
        assert!(
            matches!(&made[..], [SheetAnnotation::Note { leaders, at, .. }]
                if leaders.is_empty() && *at == V(60.0, 60.0)),
            "{made:?}"
        );
        assert!(p.annotations().is_empty());
        assert!(p.ed.undo());

        // The viewport a leader was picked through goes (an undo of placing it, a view
        // taken off) before the text is clicked: the other viewport is now first in the
        // list, and the leader must not be tied through it.
        let mut tool = NoteTool::default();
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        let targets = tool.sheet_targets();
        p.ed.doc.begin();
        let mut s = p.ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports.remove(0);
        p.ed.doc.set_sheet(p.sheet, s);
        p.ed.commit("Remove view from sheet");
        let depth = p.ed.history.undo_depth();
        assert!(
            p.ed.add_sheet_note(p.sheet, &targets, V(300.0, 250.0))
                .is_err()
        );
        p.click(&mut tool, None, V(300.0, 250.0));
        assert!(p.annotations().is_empty(), "{:?}", p.annotations());
        assert_eq!(
            p.ed.history.undo_depth(),
            depth,
            "a refused note makes no step"
        );
        assert!(p.ed.out.notice.take().is_some());
        // The same for a dimension: its viewport moved on the sheet since the pick.
        let port = SheetPort::of(p.ed.doc.sheet(p.sheet).unwrap(), 0).unwrap();
        p.ed.doc.begin();
        let mut s = p.ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports[0].at = V(10.0, 10.0);
        p.ed.doc.set_sheet(p.sheet, s);
        p.ed.commit("Move view on sheet");
        let c = Constraint::Distance {
            a: p.points[0],
            b: p.points[1],
            value: 0.0,
        };
        assert!(p.ed.add_sheet_dimension(p.sheet, port, c, None).is_err());
        assert!(p.annotations().is_empty());
        // Picked through it as it is now, it is made.
        let port = SheetPort::of(p.ed.doc.sheet(p.sheet).unwrap(), 0).unwrap();
        assert!(p.ed.add_sheet_dimension(p.sheet, port, c, None).is_ok());
    }

    #[test]
    fn making_a_block_of_tied_geometry_takes_what_was_tied_to_it() {
        let mut p = plan();
        // A dimension of the bottom edge and a note with a leader to it, through the view.
        let mut tool = DimensionTool::default();
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        p.click(&mut tool, Some(0), V(200.0, 80.0));
        let mut tool = NoteTool::default();
        p.click(&mut tool, Some(0), V(180.0, 125.0));
        p.click(&mut tool, None, V(300.0, 250.0));
        assert_eq!(p.annotations().len(), 2);
        let before = p.ed.doc.clone();
        // The rectangle made into a block: its curves leave the sketch for the block, so
        // (as when they are deleted) the dimension goes and the note's leader lets go, in
        // the block's own step.
        p.ed.selection = p.lines.iter().chain(&p.points).copied().collect();
        let depth = p.ed.history.undo_depth();
        p.ed.make_block("Room").unwrap();
        assert_eq!(p.ed.history.undo_depth(), depth + 1);
        let left = p.annotations();
        assert!(
            matches!(&left[..], [(_, SheetAnnotation::Note { leaders, at, .. })]
                if leaders.is_empty() && *at == V(300.0, 250.0)),
            "{left:?}"
        );
        // One undo brings the geometry back with both, as they were; redo takes them again.
        assert!(p.ed.undo());
        assert_eq!(p.ed.doc, before);
        assert!(p.ed.redo());
        assert_eq!(p.annotations().len(), 1);
    }

    /// The plan with the model's own annotations: a driving dimension of the bottom edge
    /// (label 8 mm of paper below it), a reference dimension of the right edge, a note with
    /// a bent leader to the top edge, and an ordinate (a kind a sheet can't hold).
    struct Annotated {
        p: Plan,
        driving: drift_core::ConstraintId,
        reference: drift_core::ConstraintId,
        note: drift_core::AnnotationId,
        ordinate: drift_core::AnnotationId,
        view: drift_core::ViewId,
    }

    fn annotated() -> Annotated {
        use drift_core::{Annotation, LeaderShape, OrdinateAxis};
        let mut p = plan();
        // (Views print at 1:10 here; label offsets are kept in paper mm.)
        let driving =
            p.ed.add_dimension(
                Constraint::Distance {
                    a: p.points[0],
                    b: p.points[1],
                    value: 1000.0,
                },
                None,
            )
            .unwrap();
        p.ed.doc.set_placement(driving, V(0.0, -8.0)).unwrap();
        let reference =
            p.ed.add_driven_dimension(
                Constraint::Distance {
                    a: p.points[1],
                    b: p.points[2],
                    value: 0.0,
                },
                None,
            )
            .unwrap();
        let mut leader = Leader::to(Anchor::Curve {
            curve: p.lines[2],
            t: 0.25,
        });
        leader.shape = LeaderShape::Bent(V(50.0, 80.0));
        let note =
            p.ed.doc
                .add_annotation(Annotation::Note {
                    text: "LINTEL".into(),
                    // (From the leader's tip: the text is 100 right and 150 above it.)
                    at: V(100.0, 150.0),
                    leaders: vec![leader],
                    style: NoteStyle {
                        bold: true,
                        ..NoteStyle::default()
                    },
                })
                .unwrap();
        let ordinate =
            p.ed.doc
                .add_annotation(Annotation::Ordinate {
                    origin: p.points[0],
                    point: p.points[1],
                    axis: OrdinateAxis::X,
                    at: -100.0,
                })
                .unwrap();
        p.ed.doc.set_style_override(
            drift_core::AnnotationRef::Dimension(driving),
            StyleOverride {
                text_height: Some(5.0),
                ..StyleOverride::default()
            },
        );
        p.ed.doc.set_dim_note(
            driving,
            Some(drift_core::DimNote {
                suffix: "CLEAR".into(),
                ..Default::default()
            }),
        );
        p.ed.commit("Annotate");
        let view = p.ed.doc.sheet(p.sheet).unwrap().viewports[0].view;
        Annotated {
            p,
            driving,
            reference,
            note,
            ordinate,
            view,
        }
    }

    #[test]
    fn a_views_model_annotations_are_copied_onto_the_sheet_as_its_own() {
        use drift_core::{AnnotationRef, LeaderShape};
        let mut a = annotated();
        let shows = a.p.ed.view_shows(a.view);
        // The two dimensions and the note; not the ordinate (a sheet can't hold one).
        assert_eq!(
            shows,
            [
                AnnotationRef::Dimension(a.driving),
                AnnotationRef::Dimension(a.reference),
                AnnotationRef::Annotation(a.note),
            ]
        );
        let before = a.p.ed.doc.clone();
        let depth = a.p.ed.history.undo_depth();
        let port = SheetPort::of(a.p.ed.doc.sheet(a.p.sheet).unwrap(), 0).unwrap();
        let brought =
            a.p.ed
                .bring_to_sheet(a.p.sheet, port, &shows, ToSheet::Copy)
                .unwrap();
        assert_eq!((brought.made.len(), brought.kept, brought.left), (3, 0, 0));
        assert_eq!(
            a.p.ed.history.undo_depth(),
            depth + 1,
            "one step for all three"
        );
        // The model is as it was: the same relations and annotations.
        let doc = &a.p.ed.doc;
        assert_eq!(doc.constraints().count(), before.constraints().count());
        assert_eq!(doc.annotations().count(), before.annotations().count());
        assert!(doc.view_prints_dimension(a.view, a.driving));
        // The sheet has its own of each, tied through the viewport.
        let sheet = doc.sheet(a.p.sheet).unwrap();
        let tied = sheet.viewports[0].id.expect("the viewport got its id");
        let made = a.p.annotations();
        let SheetAnnotation::Dimension {
            port: through,
            measure,
            label,
            note,
            look,
        } = &made[0].1
        else {
            panic!("a dimension: {made:?}");
        };
        assert_eq!(*through, tied);
        assert_eq!(
            measure.entities().collect::<Vec<_>>(),
            [a.p.points[0], a.p.points[1]]
        );
        assert_eq!(*label, Some(V(0.0, -8.0)), "the same paper offset");
        assert_eq!(note.suffix, "CLEAR");
        assert_eq!(look.text_height, Some(5.0));
        assert!(matches!(
            &made[1].1,
            SheetAnnotation::Dimension { label: None, .. }
        ));
        let SheetAnnotation::Note {
            text,
            at,
            leaders,
            style,
            ..
        } = &made[2].1
        else {
            panic!("a note: {made:?}");
        };
        assert_eq!(text, "LINTEL");
        assert!(style.bold);
        // Its text is where the model's printed through the 1:10 view: the tip is the top
        // edge a quarter along from (1000, 500), at (750, 500); the text 100 right and 150
        // above it, at (850, 650) in the model.
        let view = doc.port_view(sheet, tied).unwrap();
        assert!(at.distance(view.to_paper(V(850.0, 650.0))) < 1e-9, "{at}");
        assert_eq!(leaders.len(), 1);
        assert_eq!(leaders[0].port, Some(tied));
        assert_eq!(leaders[0].leader.to.entity(), Some(a.p.lines[2]));
        let tip = doc.sheet_leader_tip(sheet, &leaders[0]).unwrap();
        assert!(tip.distance(view.to_paper(V(750.0, 500.0))) < 1e-9, "{tip}");
        // Its bend, 50 by 80 of model, is 5 by 8 of paper.
        assert_eq!(leaders[0].leader.shape, LeaderShape::Bent(V(5.0, 8.0)));
        // One undo takes all three off the sheet (and the viewport's id with them).
        assert!(a.p.ed.undo());
        assert_eq!(a.p.ed.doc, before);
    }

    #[test]
    fn a_move_takes_notes_and_reference_dimensions_out_of_the_model_and_keeps_what_drives() {
        use drift_core::AnnotationRef;
        let mut a = annotated();
        let before = a.p.ed.doc.clone();
        let depth = a.p.ed.history.undo_depth();
        let port = SheetPort::of(a.p.ed.doc.sheet(a.p.sheet).unwrap(), 0).unwrap();
        let items = [
            AnnotationRef::Dimension(a.driving),
            AnnotationRef::Dimension(a.reference),
            AnnotationRef::Annotation(a.note),
            AnnotationRef::Annotation(a.ordinate),
        ];
        a.p.ed.selected_annotation = Some(a.note);
        let brought =
            a.p.ed
                .bring_to_sheet(a.p.sheet, port, &items, ToSheet::Move)
                .unwrap();
        assert_eq!((brought.made.len(), brought.kept, brought.left), (3, 1, 1));
        assert_eq!(a.p.ed.history.undo_depth(), depth + 1);
        let doc = &a.p.ed.doc;
        // The note and the reference dimension left the model; the ordinate stays.
        assert!(doc.annotation(a.note).is_none());
        assert!(doc.constraint(a.reference).is_none());
        assert!(doc.annotation(a.ordinate).is_some());
        assert_eq!(
            a.p.ed.selected_annotation, None,
            "what left is no longer picked"
        );
        // The driving dimension still holds the edge, and no longer prints in the view (the
        // sheet has its own now), though it still shows on the canvas.
        assert!(doc.constraint(a.driving).is_some() && !doc.is_driven(a.driving));
        assert!(!doc.view_prints_dimension(a.view, a.driving));
        assert!(doc.shown(AnnotationRef::Dimension(a.driving)).canvas);
        assert_eq!(a.p.annotations().len(), 3);
        assert!(
            a.p.ed.view_shows(a.view).is_empty(),
            "nothing of the model's is left to bring"
        );
        // The edge is still held at 1000: a drag of its end is pulled back.
        assert_eq!(
            drift_solver::measure(doc, doc.constraint(a.driving).unwrap()),
            Some(1000.0)
        );
        // One undo puts all of it back; redo moves it again.
        assert!(a.p.ed.undo());
        assert_eq!(a.p.ed.doc, before);
        assert!(a.p.ed.redo());
        assert!(a.p.ed.doc.annotation(a.note).is_none());
        assert_eq!(a.p.annotations().len(), 3);
    }

    #[test]
    fn a_driving_dimension_moved_from_one_of_two_views_still_prints_in_the_other() {
        use drift_core::AnnotationRef;
        let mut a = annotated();
        // A second view of the same model, on the sheet beside the first.
        let v = a.p.ed.doc.view(a.view).unwrap().clone();
        let detail = a.p.ed.doc.add_view(v);
        let mut s = a.p.ed.doc.sheet(a.p.sheet).unwrap().clone();
        s.viewports.push(Viewport::new(detail, V(200.0, 40.0)));
        a.p.ed.doc.set_sheet(a.p.sheet, s);
        a.p.ed.commit("Second view");
        let port = SheetPort::of(a.p.ed.doc.sheet(a.p.sheet).unwrap(), 0).unwrap();
        let r = AnnotationRef::Dimension(a.driving);
        a.p.ed
            .bring_to_sheet(a.p.sheet, port, &[r], ToSheet::Move)
            .unwrap();
        let doc = &a.p.ed.doc;
        assert!(!doc.view_prints_dimension(a.view, a.driving));
        assert!(doc.view_prints_dimension(detail, a.driving));
        assert!(doc.shown(r).sheets);
    }

    #[test]
    fn nothing_is_brought_through_a_viewport_that_changed_or_of_what_is_not_there() {
        use drift_core::AnnotationRef;
        let mut a = annotated();
        let port = SheetPort::of(a.p.ed.doc.sheet(a.p.sheet).unwrap(), 0).unwrap();
        // The viewport moved since: refused, and nothing changes.
        let mut s = a.p.ed.doc.sheet(a.p.sheet).unwrap().clone();
        s.viewports[0].at = V(10.0, 10.0);
        a.p.ed.doc.set_sheet(a.p.sheet, s);
        a.p.ed.commit("Move view on sheet");
        let before = a.p.ed.doc.clone();
        let depth = a.p.ed.history.undo_depth();
        let items = [AnnotationRef::Annotation(a.note)];
        assert!(
            a.p.ed
                .bring_to_sheet(a.p.sheet, port, &items, ToSheet::Move)
                .is_err()
        );
        assert_eq!(a.p.ed.doc, before);
        // Only what a sheet can't hold, or isn't there: nothing made, no step, the model and
        // the viewport untouched.
        let port = SheetPort::of(a.p.ed.doc.sheet(a.p.sheet).unwrap(), 0).unwrap();
        let none = [
            AnnotationRef::Annotation(a.ordinate),
            AnnotationRef::Annotation(drift_core::AnnotationId(9999)),
            AnnotationRef::Dimension(drift_core::ConstraintId(9999)),
        ];
        let brought =
            a.p.ed
                .bring_to_sheet(a.p.sheet, port, &none, ToSheet::Move)
                .unwrap();
        assert_eq!((brought.made.len(), brought.left), (0, 3));
        assert_eq!(a.p.ed.doc, before);
        assert_eq!(a.p.ed.history.undo_depth(), depth);
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

#[cfg(test)]
mod wall_tests {
    use drift_core::{Bounds, EntityId, EntityKind, Owner, Sheet, View, Viewport};
    use drift_geom::SnapOptions;

    use super::*;
    use crate::editor::Pointer;
    use crate::tools::{Input, Modifiers, ToolKind};

    const V: fn(f64, f64) -> DVec2 = DVec2::new;

    fn run(ed: &mut Editor, pts: &[DVec2]) {
        let mut tool = ToolKind::Wall.create();
        for at in pts {
            let mut p = Pointer::new(*at, 0.5);
            p.snap = ed.index.snap(
                *at,
                SnapOptions {
                    tolerance: 0.5,
                    grid: None,
                    from: None,
                },
                |_| false,
            );
            tool.handle(Input::Hover(p), ed);
            tool.handle(Input::Click(p, Modifiers::default()), ed);
        }
        tool.handle(Input::Finish, ed);
    }

    /// The wall's face lines at least `min` long.
    fn long_faces(ed: &Editor, min: f64) -> Vec<EntityId> {
        ed.doc
            .iter()
            .filter(|(id, e)| {
                matches!(e.kind, EntityKind::Line { .. })
                    && matches!(ed.wall_owners().get(*id), Some(Owner::Wall(_)))
                    && matches!(ed.doc.shape(*id), Some(drift_core::Shape::Line { a, b }) if a.distance(b) >= min)
            })
            .map(|(id, _)| id)
            .collect()
    }

    #[test]
    fn a_dimension_across_a_wall_follows_its_thickness_and_survives_a_join() {
        let mut ed = Editor::default();
        ed.prefs.auto_relations = false;
        ed.prefs.wall_thickness = 100.0;
        run(&mut ed, &[V(0.0, 0.0), V(2000.0, 0.0)]);
        let faces = long_faces(&ed, 1500.0);
        assert_eq!(faces.len(), 2);
        let view = ed.doc.add_view(View::new(
            "Plan",
            Bounds {
                min: V(-500.0, -1500.0),
                max: V(2500.0, 1500.0),
            },
            20.0,
        ));
        let mut s = Sheet::new("A-101");
        s.viewports.push(Viewport::new(view, V(200.0, 150.0)));
        let sheet = ed.doc.add_sheet(s);
        // Face to face, as the Dimension tool measures two lines alongside each other.
        let across = Constraint::PointLineDistance {
            point: faces[0],
            line: faces[1],
            value: 0.0,
        };
        let port = SheetPort::of(ed.doc.sheet(sheet).unwrap(), 0).unwrap();
        let id = ed.add_sheet_dimension(sheet, port, across, None).unwrap();
        let read = |ed: &Editor| {
            ed.doc.sheet_annotation(sheet, id).and_then(|a| match a {
                SheetAnnotation::Dimension { measure, .. } => {
                    drift_solver::measure(&ed.doc, measure)
                }
                SheetAnnotation::Note { .. } => None,
            })
        };
        let near = |v: Option<f64>, want: f64| v.is_some_and(|v| (v - want).abs() < 1e-6);
        assert!(near(read(&ed), 100.0), "{:?}", read(&ed));
        // The wall made thicker (it is rebuilt): the dimension is still there and reads it.
        let walls: Vec<_> = ed.doc.walls().map(|(w, _)| w).collect();
        ed.set_wall_thickness(&walls, "250 mm").unwrap();
        assert!(near(read(&ed), 250.0), "{:?}", read(&ed));
        // Another wall run into it (its face is cut for the join): still there.
        run(&mut ed, &[V(1000.0, -1000.0), V(1000.0, -125.0)]);
        assert!(ed.doc.walls().count() >= 2);
        assert!(near(read(&ed), 250.0), "{:?}", read(&ed));
        // It added no constraint to the walls' sketch.
        assert!(ed.doc.constraints().all(|(_, c)| !c.is_dimension()));
    }
}
