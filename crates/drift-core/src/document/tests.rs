use super::*;

#[cfg(test)]
mod editing {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A unit square drawn as a closed chain sharing its corners.
    fn square(doc: &mut Document) -> ([EntityId; 4], [EntityId; 4]) {
        let p = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)].map(|at| doc.add_point(at));
        let l = [0, 1, 2, 3].map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
        (p, l)
    }

    #[test]
    fn views_are_shared_by_sheets_and_follow_their_crop() {
        use crate::sheets::{View, Viewport};
        let mut doc = Document::new();
        let mut history = History::default();
        let crop = crate::Bounds {
            min: v(0.0, 0.0),
            max: v(1000.0, 500.0),
        };
        doc.begin();
        let plan = doc.add_view(View::new("Plan", crop, 50.0));
        let mut a = Sheet::new("A-101");
        a.viewports.push(Viewport::new(plan, v(200.0, 150.0)));
        let a = doc.add_sheet(a);
        let mut b = Sheet::new("A-102");
        b.viewports.push(Viewport::new(plan, v(100.0, 100.0)));
        let b = doc.add_sheet(b);
        history.push(doc.commit("Set up"));
        assert_eq!(doc.sheets_showing(plan), [a, b]);
        let placed = doc.placed_views(doc.sheet(a).unwrap());
        assert_eq!(placed[0].frame.size, v(20.0, 10.0), "the crop at 1:50");
        assert_eq!(doc.sheet_scale_text(doc.sheet(a).unwrap()), "1:50");

        // Cropping it on one sheet reframes the view: the other sheet follows.
        let mut wider = placed[0].clone();
        wider.frame.size = v(30.0, 10.0);
        wider.frame.at = v(205.0, 150.0);
        doc.begin();
        doc.set_placed_view(a, 0, &wider);
        history.push(doc.commit("Crop"));
        assert_eq!(doc.view(plan).unwrap().size, v(1500.0, 500.0));
        let on_b = doc.placed_views(doc.sheet(b).unwrap());
        assert_eq!(on_b[0].frame.size, v(30.0, 10.0));
        assert_eq!(
            on_b[0].frame.at,
            v(100.0, 100.0),
            "only its own sheet's place moved"
        );

        // Deleting the view takes it off both sheets; undo brings it all back.
        let before = doc.clone();
        doc.begin();
        doc.remove_view(plan);
        history.push(doc.commit("Delete view"));
        assert!(doc.view(plan).is_none());
        assert!(doc.sheets().all(|(_, s)| s.viewports.is_empty()));
        assert_eq!(doc.sheet_scale_text(doc.sheet(a).unwrap()), "-");
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.undo(&mut doc));
        assert_eq!(doc.view(plan).unwrap().size, v(1000.0, 500.0));
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut doc = Document::new();
        let mut history = History::default();
        let empty = doc.clone();

        doc.begin();
        square(&mut doc);
        history.push(doc.commit("Rectangle"));
        let drawn = doc.clone();
        assert_eq!(doc.len(), 8);

        assert!(history.undo(&mut doc));
        assert_eq!(doc, empty);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, drawn);
        assert_eq!(history.undo_label(), Some("Rectangle"));
    }

    #[test]
    fn a_dimension_sets_its_own_style_over_the_drawings() {
        use crate::{AnnotationRef, ArrowStyle, StyleOverride};
        let mut doc = Document::new();
        let mut history = History::default();
        let (p, _) = square(&mut doc);
        let dim = doc
            .add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 1.0,
            })
            .unwrap();
        let r = AnnotationRef::Dimension(dim);
        assert_eq!(doc.draft_style_for(r), doc.draft_style());
        doc.begin();
        doc.set_style_override(
            r,
            StyleOverride {
                text_height: Some(5.0),
                arrow: Some(ArrowStyle::Tick),
                ..Default::default()
            },
        );
        history.push(doc.commit("Annotation style"));
        let own = doc.draft_style_for(r);
        assert_eq!((own.text_height, own.arrow), (5.0, ArrowStyle::Tick));
        // Everything else, and what the drawing changes later, still comes from the drawing.
        let mut drawing = doc.draft_style();
        drawing.extension_gap = 2.5;
        drawing.text_height = 7.0;
        doc.set_draft_style(drawing);
        let own = doc.draft_style_for(r);
        assert_eq!((own.extension_gap, own.text_height), (2.5, 5.0));
        assert_eq!(doc.style_overrides().count(), 1);
        // Undone, it follows the drawing again; nothing overridden is not kept.
        assert!(history.undo(&mut doc));
        assert_eq!(doc.draft_style_for(r).text_height, 7.0);
        doc.set_style_override(r, StyleOverride::default());
        assert_eq!(doc.style_overrides().count(), 0);
    }

    #[test]
    fn translate_moves_shared_points_once_and_stretches_neighbours() {
        let mut doc = Document::new();
        let (p, l) = square(&mut doc);
        doc.translate([l[2]], v(0.0, 1.0)); // the top edge
        assert_eq!(doc.point(p[2]).unwrap(), v(1.0, 2.0));
        assert_eq!(doc.point(p[3]).unwrap(), v(0.0, 2.0));
        assert_eq!(doc.point(p[0]).unwrap(), v(0.0, 0.0));
    }

    #[test]
    fn deleting_a_line_keeps_shared_points_and_drops_orphans() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(2.0, 0.0));
        let l1 = doc.add_line(a, b).unwrap();
        let l2 = doc.add_line(b, c).unwrap();
        doc.delete([l1]);
        assert!(!doc.contains(a), "orphaned start point removed");
        assert!(doc.contains(b), "shared point kept");
        assert!(doc.contains(l2));
    }

    #[test]
    fn deleting_a_point_deletes_its_curves() {
        let mut doc = Document::new();
        let (p, _) = square(&mut doc);
        doc.delete([p[0]]);
        // Two lines used p0; the other two lines and their three points remain.
        assert_eq!(doc.iter().filter(|(_, e)| !e.kind.is_point()).count(), 2);
        assert_eq!(doc.iter().filter(|(_, e)| e.kind.is_point()).count(), 3);
    }

    #[test]
    fn arc_end_stays_on_circle() {
        let mut doc = Document::new();
        let c = doc.add_point(v(0.0, 0.0));
        let s = doc.add_point(v(2.0, 0.0));
        let e = doc.add_point(v(0.0, 5.0));
        let arc = doc.add_arc(c, s, e).unwrap();
        assert_eq!(doc.point(e).unwrap(), v(0.0, 2.0));

        // Moving the start changes the radius; the end follows.
        doc.translate([s], v(1.0, 0.0));
        let end = doc.point(e).unwrap();
        assert!((end.length() - 3.0).abs() < 1e-12);
        let Some(Shape::Arc { radius, sweep, .. }) = doc.shape(arc) else {
            panic!()
        };
        assert!((radius - 3.0).abs() < 1e-12);
        assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn merge_points_connects_lines_and_refuses_collapse() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(1.0, 0.0));
        let d = doc.add_point(v(1.0, 1.0));
        let l1 = doc.add_line(a, b).unwrap();
        let l2 = doc.add_line(c, d).unwrap();
        doc.merge_points(c, b).unwrap();
        assert!(!doc.contains(c));
        assert_eq!(doc.get(l2).unwrap().kind, EntityKind::Line { a: b, b: d });
        // Merging a line's two ends would collapse it.
        assert_eq!(
            doc.merge_points(a, b),
            Err(DocumentError::Degenerate("merge"))
        );
        assert!(doc.contains(l1) && doc.contains(a));
    }

    #[test]
    fn joining_a_point_onto_its_curves_end_drops_the_point_on_curve() {
        // Regression: the relation survived the join, was saved, and the file then refused
        // to open ("degenerate point on its own curve").
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let p = doc.add_point(v(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        doc.add_constraint(Constraint::PointOnCurve {
            point: p,
            curve: line,
        })
        .unwrap();
        doc.merge_points(p, b).unwrap();
        assert_eq!(doc.constraints().count(), 0);
        // And a document holding one anyway is opened without it.
        let entities: Vec<_> = doc.iter().map(|(id, e)| (id, *e)).collect();
        let bad = Constraint::PointOnCurve {
            point: b,
            curve: line,
        };
        assert!(Document::from_parts(entities.clone(), [(ConstraintId(4), bad)]).is_err());
        let (doc, dropped) =
            Document::from_parts_dropping_degenerate(entities, [(ConstraintId(4), bad)]).unwrap();
        assert_eq!(dropped, [(ConstraintId(4), "point on its own curve")]);
        assert_eq!(doc.constraints().count(), 0);
    }

    #[test]
    fn copies_stay_apart_through_edits_undo_and_merged_steps() {
        let mut doc = Document::new();
        let mut history = History::default();
        let (pts, lines) = square(&mut doc);
        doc.begin();
        doc.set_custom_patterns(
            "*A, a
0, 0,0, 0,1
"
            .into(),
        );
        history.push(doc.commit("Patterns"));
        let copy = doc.clone();
        assert_eq!(copy, doc);
        doc.begin();
        doc.translate([lines[0]], v(5.0, 5.0));
        history.push(doc.commit("Move"));
        // A second change folded into the same step.
        doc.begin();
        doc.set_point(pts[2], v(40.0, 40.0)).unwrap();
        let tx = doc.commit("Move");
        history.merge_into_last(tx);
        assert_ne!(copy, doc);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, copy, "one undo takes back both merged changes");
        assert!(history.redo(&mut doc));
        assert_eq!(doc.point(pts[2]).unwrap(), v(40.0, 40.0));
        // Undoing past the copy changes the document, never the copy.
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert_eq!(doc.custom_patterns(), "");
        assert!(copy.custom_patterns().starts_with("*A"));
    }

    #[test]
    fn structure_survives_moves_but_not_edits() {
        let mut doc = Document::new();
        let (p, l) = square(&mut doc);
        let c = doc.add_point(v(50.0, 50.0));
        let circle = doc.add_circle(c, 3.0).unwrap();
        let s = doc.structure();
        doc.begin();
        doc.translate([l[0]], v(5.0, 5.0));
        doc.set_point(p[2], v(1.0, 2.0)).unwrap();
        doc.set_radius(circle, 4.0).unwrap();
        doc.set_entity_kind(
            circle,
            EntityKind::Circle {
                center: c,
                radius: 5.0,
            },
        )
        .unwrap();
        assert_eq!(doc.structure(), s, "moving geometry keeps the structure");
        doc.set_style(l[1], LineKind::Construction);
        let restyled = doc.structure();
        assert_ne!(restyled, s);
        doc.delete([l[2]]);
        assert_ne!(doc.structure(), restyled);
        doc.rollback();
        assert_ne!(doc.structure(), s, "a rollback is a change too");
        assert_ne!(Document::new().structure(), Document::new().structure());
    }

    #[test]
    fn rollback_restores_everything() {
        let mut doc = Document::new();
        let (_, l) = square(&mut doc);
        let before = doc.clone();
        doc.begin();
        doc.translate([l[0]], v(5.0, 5.0));
        doc.delete([l[1]]);
        doc.rollback();
        assert_eq!(doc, before);
    }

    #[test]
    fn constraints_are_validated_undoable_and_cascade_on_delete() {
        let mut doc = Document::new();
        let mut history = History::default();
        let (p, l) = square(&mut doc);

        assert_eq!(
            doc.add_constraint(Constraint::Horizontal { line: p[0] }),
            Err(DocumentError::WrongKind {
                id: p[0],
                expected: "a line"
            })
        );
        assert_eq!(
            doc.add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: -1.0
            }),
            Err(DocumentError::InvalidValue)
        );

        doc.begin();
        let h = doc
            .add_constraint(Constraint::Horizontal { line: l[0] })
            .unwrap();
        let d = doc
            .add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 1.0,
            })
            .unwrap();
        history.push(doc.commit("Constrain"));
        assert_eq!(h, ConstraintId(1));
        assert_eq!(doc.constraints_on(l[0]).collect::<Vec<_>>(), vec![h]);

        // Deleting the line removes its relation; the distance on its (still used) points stays.
        doc.begin();
        doc.delete([l[0]]);
        history.push(doc.commit("Delete"));
        assert!(doc.constraint(h).is_none());
        assert!(doc.constraint(d).is_some());

        assert!(history.undo(&mut doc));
        assert!(doc.constraint(h).is_some());
        assert!(history.undo(&mut doc));
        assert_eq!(doc.constraint_count(), 0);
    }

    #[test]
    fn merging_points_redirects_constraints_and_drops_implied_ones() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(1.0, 0.0));
        let d = doc.add_point(v(1.0, 2.0));
        doc.add_line(a, b).unwrap();
        doc.add_line(c, d).unwrap();
        let coincident = doc
            .add_constraint(Constraint::Coincident { a: b, b: c })
            .unwrap();
        let dist = doc
            .add_constraint(Constraint::Distance {
                a: c,
                b: d,
                value: 2.0,
            })
            .unwrap();
        doc.merge_points(c, b).unwrap();
        assert!(doc.constraint(coincident).is_none());
        assert_eq!(
            doc.constraint(dist),
            Some(&Constraint::Distance {
                a: b,
                b: d,
                value: 2.0
            })
        );
    }

    #[test]
    fn rejects_bad_references_and_degenerate_geometry() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(0.0, 0.0));
        assert_eq!(doc.add_line(a, b), Err(DocumentError::Degenerate("line")));
        assert_eq!(
            doc.add_circle(a, 0.0),
            Err(DocumentError::Degenerate("circle"))
        );
        let line_like = EntityId(999);
        assert_eq!(
            doc.add_circle(line_like, 1.0),
            Err(DocumentError::Missing(line_like))
        );
    }
}

#[cfg(test)]
mod sheet_order_tests {
    use super::*;

    #[test]
    fn sheets_move_between_ids_in_one_undoable_step() {
        let mut doc = Document::new();
        doc.begin();
        let ids: Vec<SheetId> = ["A", "B", "C"]
            .map(|n| doc.add_sheet(Sheet::new(n)))
            .to_vec();
        doc.commit("Setup");
        let names =
            |doc: &Document| -> Vec<String> { doc.sheets().map(|(_, s)| s.name.clone()).collect() };
        doc.begin();
        let went = doc.move_sheet(ids[2], 0);
        let mut history = History::default();
        history.push(doc.commit("Move sheet"));
        assert_eq!(names(&doc), ["C", "A", "B"]);
        assert_eq!(went[&ids[2]], ids[0], "C is now first");
        assert_eq!(went[&ids[0]], ids[1]);
        assert_eq!(doc.sheet(went[&ids[1]]).unwrap().name, "B");
        assert!(history.undo(&mut doc));
        assert_eq!(names(&doc), ["A", "B", "C"]);
        // Past the end goes last; an unknown sheet moves nothing.
        doc.move_sheet(ids[0], 9);
        assert_eq!(names(&doc), ["B", "C", "A"]);
        assert!(doc.move_sheet(SheetId(999), 0).is_empty());
    }
}

#[cfg(test)]
mod reserved_ids {
    use super::*;
    use crate::Group;

    /// Ids something outside the drawing names (a newer Drift's data, carried through) are
    /// never handed out again: each kind of id steps over its own, and the drawing is equal to
    /// what it was.
    #[test]
    fn new_ids_step_over_reserved_ones_of_their_kind() {
        let mut doc = Document::new();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::X);
        let before = doc.clone();
        doc.reserve_ids([
            "e7",
            "c4",
            "l9",
            "g12",
            "x3",
            "e",
            "e1x",
            "e+9",
            "e4294967295",
        ]);
        assert_eq!(doc, before, "reserving isn't part of the drawing");
        assert_eq!(doc.add_point(DVec2::Y), EntityId(8));
        assert!(doc.holds_id("e8") && !doc.holds_id("e7") && !doc.holds_id("x8"));
        let line = doc.add_line(a, b).unwrap();
        assert_eq!(line, EntityId(9));
        let c = doc.add_constraint(Constraint::Horizontal { line }).unwrap();
        assert_eq!(c, ConstraintId(5));
        assert_eq!(doc.add_layer(crate::Layer::new("A")), LayerId(10));
        let group = Group {
            name: "G".into(),
            members: [a].into_iter().collect(),
        };
        assert_eq!(doc.add_group(group).unwrap(), crate::GroupId(13));
        assert!(doc.holds_id("g13") && doc.holds_id("l10") && doc.holds_id("c5"));
        // Lower ids than the counters already hand out change nothing.
        doc.reserve_ids(["e2"]);
        assert_eq!(doc.add_point(DVec2::Y), EntityId(10));
    }
}

#[cfg(test)]
mod change_log {
    use super::*;

    /// A copy has a lineage of its own, so it starts an empty log (copying the entries made
    /// every copy of a big drawing cost a megabyte), yet still answers for changes after it.
    #[test]
    fn a_copy_starts_its_own_log_and_still_answers_for_later_changes() {
        let mut doc = Document::new();
        let points: Vec<EntityId> = (0..100)
            .map(|i| doc.add_point(DVec2::new(i as f64, 0.0)))
            .collect();
        assert_eq!(doc.changes.entries.len(), 100);
        let mut copy = doc.clone();
        assert!(copy.changes.entries.is_empty());
        assert_ne!(copy.lineage(), doc.lineage());
        let at = copy.revision();
        assert_eq!(
            copy.changes_since(copy.lineage(), at),
            Some(Changes::default())
        );
        assert_eq!(copy.changes_since(doc.lineage(), at), None);
        copy.set_point(points[7], DVec2::new(7.0, 1.0)).unwrap();
        let changes = copy.changes_since(copy.lineage(), at).unwrap();
        assert_eq!(changes.entities, vec![points[7]]);
        // The original keeps its own log.
        assert_eq!(
            doc.changes_since(doc.lineage(), 0).unwrap().entities.len(),
            100
        );
        // Every change is noted before the revision moves on (an edit committed, one rolled
        // back), so a copy's floor is never past its revision.
        doc.begin();
        doc.set_point(points[3], DVec2::new(3.0, 2.0)).unwrap();
        doc.commit("Move");
        doc.begin();
        doc.set_point(points[4], DVec2::new(4.0, 2.0)).unwrap();
        doc.rollback();
        let copy = doc.clone();
        assert!(copy.changes.floor <= copy.revision());
        let at = copy.revision();
        assert_eq!(
            copy.changes_since(copy.lineage(), at),
            Some(Changes::default())
        );
    }
}
