use super::*;

#[cfg(test)]
mod editing {
    use drift_core::{Constraint, DVec2};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// An editor with one slanted line; returns (editor, line, start, end).
    fn with_line() -> (Editor, EntityId, EntityId, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(10.0, 3.0));
        let line = ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        (ed, line, a, b)
    }

    fn length(ed: &Editor, a: EntityId, b: EntityId) -> f64 {
        ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap())
    }

    #[test]
    fn adding_a_relation_moves_geometry_and_undoes_in_one_step() {
        let (mut ed, line, a, b) = with_line();
        assert_eq!(ed.analysis().dof, Some(4));
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pa.y - pb.y).abs() < 1e-12);
        // Minimum change: both ends meet in the middle.
        assert!((pa.y - 1.5).abs() < 1e-9);
        assert_eq!(ed.analysis().dof, Some(3));
        assert!(ed.undo());
        assert_eq!(ed.doc.point(b).unwrap(), v(10.0, 3.0));
        assert_eq!(ed.doc.constraint_count(), 0);
    }

    #[test]
    fn conflicting_and_redundant_constraints_are_refused() {
        let (mut ed, line, ..) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let before = ed.doc.clone();
        assert_eq!(
            ed.add_constraints("Vertical", &[Constraint::Vertical { line }]),
            Err(EditError::Degenerate)
        );
        assert_eq!(
            ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }]),
            Err(EditError::Redundant)
        );
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn dimensions_drive_geometry() {
        let (mut ed, _, a, b) = with_line();
        let ids = ed
            .add_constraints(
                "Length",
                &[Constraint::Distance {
                    a,
                    b,
                    value: length(&ed, a, b),
                }],
            )
            .unwrap();
        ed.set_dimension(ids[0], 25.0).unwrap();
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
        // An impossible value is refused and changes nothing.
        ed.add_constraints(
            "Fix",
            &[
                Constraint::Fix {
                    point: a,
                    at: ed.doc.point(a).unwrap(),
                },
                Constraint::Fix {
                    point: b,
                    at: ed.doc.point(b).unwrap(),
                },
            ],
        )
        .unwrap(); // pins only what the length leaves free (#203)
        let pins: Vec<Constraint> = ed
            .doc
            .constraints()
            .filter(|(_, c)| c.is_point_fix())
            .map(|(_, c)| *c)
            .collect();
        assert!(
            matches!(
                pins.as_slice(),
                [Constraint::Fix { point, .. }, Constraint::FixX { point: other, .. } | Constraint::FixY { point: other, .. }]
                    if *point == a && *other == b
            ),
            "{pins:?}"
        );
        assert_eq!(ed.analysis().dof, Some(0));
        assert!(ed.set_dimension(ids[0], -3.0).is_err());
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn dragging_respects_constraints() {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Length", &[Constraint::Distance { a, b, value: 10.0 }])
            .unwrap();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        // Drag the end point 5 mm right and 4 mm up: it stays horizontal and 10 long, so the
        // whole line slides.
        ed.doc.begin();
        let dragged = Param::point(b).to_vec();
        assert!(ed.drag(&[b], v(5.0, 4.0), &dragged));
        ed.commit("Move");
        assert!((length(&ed, a, b) - 10.0).abs() < 1e-9);
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pa.y - pb.y).abs() < 1e-12);
    }

    #[test]
    fn fixed_geometry_does_not_move() {
        let (mut ed, line, a, b) = with_line();
        ed.selection = [line].into();
        let fix = ed
            .relation_options()
            .into_iter()
            .find(|o| o.label == "Fix")
            .unwrap();
        ed.add_constraints("Fix", &fix.constraints).unwrap();
        assert_eq!(ed.analysis().dof, Some(0));
        let before = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        ed.doc.begin();
        ed.drag(&[line], v(3.0, 3.0), &[]);
        ed.commit("Move");
        assert_eq!((ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()), before);
        // Fully defined geometry is reported as such.
        let items = ed.items();
        assert!(items.iter().all(|i| i.dof == DofState::Full));
    }

    #[test]
    fn options_fit_the_selection() {
        let (mut ed, line, a, b) = with_line();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 5.0));
        let d = ed.doc.add_point(v(4.0, 9.0));
        let other = ed.doc.add_line(c, d).unwrap();
        ed.commit("Line");
        let labels = |ed: &Editor| -> Vec<&'static str> {
            ed.relation_options()
                .iter()
                .chain(ed.dimension_options().iter())
                .map(|o| o.label)
                .collect()
        };
        ed.selection = [line, other].into();
        assert_eq!(
            labels(&ed),
            [
                "Horizontal",
                "Vertical",
                "Parallel",
                "Perpendicular",
                "Collinear",
                "Equal",
                "Fix",
                "Angle"
            ]
        );
        ed.selection = [a, b].into();
        assert_eq!(
            labels(&ed),
            [
                "Coincident",
                "Horizontal",
                "Vertical",
                "Fix",
                "Distance",
                "Horizontal distance",
                "Vertical distance"
            ]
        );
        // The dimension is created at the current value, so adding it changes nothing.
        let dim = &ed.dimension_options()[0].constraints[0];
        assert!((dim.value().unwrap() - length(&ed, a, b)).abs() < 1e-12);
    }

    /// A line, a circle beside it and a second circle: (editor, line, circle, circle2, centre).
    fn with_curves() -> (Editor, EntityId, EntityId, EntityId, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 0.0));
        let b = ed.doc.add_point(v(20.0, 0.0));
        let line = ed.doc.add_line(a, b).unwrap();
        let c = ed.doc.add_point(v(10.0, 7.0));
        let circle = ed.doc.add_circle(c, 4.0).unwrap();
        let c2 = ed.doc.add_point(v(30.0, 8.0));
        let circle2 = ed.doc.add_circle(c2, 2.0).unwrap();
        ed.commit("Setup");
        (ed, line, circle, circle2, c)
    }

    fn labels(ed: &Editor) -> Vec<&'static str> {
        ed.relation_options().iter().map(|o| o.label).collect()
    }

    fn apply(ed: &mut Editor, label: &str) {
        let option = ed
            .relation_options()
            .into_iter()
            .find(|o| o.label == label)
            .unwrap_or_else(|| panic!("no {label} option"));
        ed.add_constraints(label, &option.constraints).unwrap();
    }

    #[test]
    fn curve_relations_are_offered_and_applied() {
        let (mut ed, line, circle, circle2, c) = with_curves();
        ed.selection = [line, circle].into();
        assert_eq!(labels(&ed), ["Tangent", "Fix"]);
        apply(&mut ed, "Tangent");
        let centre = ed.doc.point(c).unwrap();
        let radius = drift_solver::radius_of(&ed.doc, circle).unwrap();
        let (a, b) = match ed.doc.get(line).unwrap().kind {
            EntityKind::Line { a, b } => (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()),
            _ => unreachable!(),
        };
        let distance = (b - a).perp_dot(centre - a).abs() / (b - a).length();
        assert!((distance - radius).abs() < 1e-9, "{distance} vs {radius}");
        // Still on the side it started.
        assert!(centre.y > a.y);

        ed.selection = [circle, circle2].into();
        assert_eq!(
            labels(&ed),
            ["Tangent", "Concentric", "Coradial", "Equal", "Fix"]
        );
        apply(&mut ed, "Coradial");
        let centres = |ed: &Editor| {
            [circle, circle2].map(|e| match ed.doc.get(e).unwrap().kind {
                EntityKind::Circle { center, radius } => (ed.doc.point(center).unwrap(), radius),
                _ => unreachable!(),
            })
        };
        let [(c1, r1), (c2, r2)] = centres(&ed);
        assert!(c1.distance(c2) < 1e-9 && (r1 - r2).abs() < 1e-9);
        // Coradial implies concentric.
        let concentric = Constraint::Concentric {
            a: circle,
            b: circle2,
        };
        assert_eq!(
            ed.add_constraints("Concentric", &[concentric]),
            Err(EditError::Redundant)
        );
    }

    #[test]
    fn point_relations_are_offered() {
        let (mut ed, line, circle, ..) = with_curves();
        ed.doc.begin();
        let p = ed.doc.add_point(v(5.0, 20.0));
        let q = ed.doc.add_point(v(8.0, -12.0));
        ed.commit("Points");
        ed.selection = [p, circle].into();
        assert_eq!(labels(&ed), ["On curve", "Fix"]);
        apply(&mut ed, "On curve");
        let (centre, radius) = match ed.doc.get(circle).unwrap().kind {
            EntityKind::Circle { center, radius } => (ed.doc.point(center).unwrap(), radius),
            _ => unreachable!(),
        };
        assert!((ed.doc.point(p).unwrap().distance(centre) - radius).abs() < 1e-9);

        ed.selection = [q, line].into();
        assert_eq!(labels(&ed), ["Midpoint", "On curve", "Fix"]);

        ed.selection = [p, q, line].into();
        assert_eq!(labels(&ed), ["On curve", "Symmetric", "Fix"]);
        apply(&mut ed, "Symmetric");
        // The line is the x axis (it may have moved, but stays horizontal-ish): q mirrors p.
        let (a, b) = match ed.doc.get(line).unwrap().kind {
            EntityKind::Line { a, b } => (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()),
            _ => unreachable!(),
        };
        let (pp, qq) = (ed.doc.point(p).unwrap(), ed.doc.point(q).unwrap());
        let u = (b - a).normalize();
        assert!((pp - qq).dot(u).abs() < 1e-9);
        assert!(((pp + qq) * 0.5 - a).perp_dot(u).abs() < 1e-9);
    }

    #[test]
    fn over_defining_dimensions_can_be_driven() {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let length = ed
            .add_dimension(Constraint::Distance { a, b, value: 12.0 }, None)
            .unwrap();
        // The horizontal distance is now implied: refused, with the driven option offered.
        let horizontal = Constraint::HorizontalDistance { a, b, value: 0.0 };
        let horizontal =
            horizontal.with_value(drift_solver::measure(&ed.doc, &horizontal).unwrap());
        let placement = Some(v(0.0, 4.0));
        assert_eq!(
            ed.add_dimension(horizontal, placement),
            Err(EditError::Redundant)
        );
        assert_eq!(ed.pending_driven, Some((horizontal, placement)));
        let (constraint, placement) = ed.pending_driven.take().unwrap();
        let driven = ed.add_driven_dimension(constraint, placement).unwrap();
        assert!(ed.doc.is_driven(driven));
        assert_eq!(ed.doc.placement(driven), placement);
        // It measures rather than imposes its value.
        let measured = |ed: &Editor| ed.doc.constraint(driven).unwrap().value().unwrap().abs();
        assert!((measured(&ed) - 12.0).abs() < 1e-9);
        assert!(ed.analysis().conflicts.is_empty());

        // Driven values follow changes to the geometry.
        ed.set_dimension(length, 20.0).unwrap();
        assert!((measured(&ed) - 20.0).abs() < 1e-9);
        assert!(matches!(
            ed.set_dimension(driven, 3.0),
            Err(EditError::Invalid(_))
        ));
        // Making it driving would over-define, until the length is gone.
        assert_eq!(ed.set_driven(driven, false), Err(EditError::Redundant));
        assert!(ed.doc.is_driven(driven));
        ed.remove_constraint(length);
        ed.set_driven(driven, false).unwrap();
        assert!(!ed.doc.is_driven(driven));
        ed.set_dimension(driven, 7.0).unwrap();
        assert!((ed.doc.point(b).unwrap().x - ed.doc.point(a).unwrap().x).abs() - 7.0 < 1e-9);
        // Each switch is one undo step.
        ed.undo();
        ed.undo();
        assert!(ed.doc.is_driven(driven));
    }

    /// A horizontal line with a length dimension: (editor, dimension, start, end).
    fn dimensioned_line() -> (Editor, ConstraintId, EntityId, EntityId) {
        let (mut ed, line, a, b) = with_line();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line }])
            .unwrap();
        let length = length(&ed, a, b);
        let d = ed
            .add_dimension(
                Constraint::Distance {
                    a,
                    b,
                    value: length,
                },
                None,
            )
            .unwrap();
        (ed, d, a, b)
    }

    #[test]
    fn variables_drive_linked_dimensions() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        assert_eq!(
            ed.doc.variable(w).unwrap().expr,
            "25 mm",
            "unit made explicit"
        );
        // A leading `=`, as typed while drawing, is taken too (and not stored).
        ed.set_dimension_text(d, "=w * 2").unwrap();
        assert_eq!(ed.doc.expression(d), Some("w * 2"));
        assert!((length(&ed, a, b) - 50.0).abs() < 1e-9);
        ed.set_variable(w, "30mm").unwrap();
        assert!((length(&ed, a, b) - 60.0).abs() < 1e-9);
        assert_eq!(ed.doc.constraint(d).unwrap().value(), Some(60.0));
        assert_eq!(ed.dimension_edit_text(d).as_deref(), Some("w * 2"));
        // Each edit is one undo step.
        ed.undo();
        assert!((length(&ed, a, b) - 50.0).abs() < 1e-9);
        // A constant unlinks it.
        ed.set_dimension_text(d, "12").unwrap();
        assert_eq!(ed.doc.expression(d), None);
        assert!((length(&ed, a, b) - 12.0).abs() < 1e-9);
        // Now the variable can be deleted.
        ed.remove_variable(w).unwrap();
    }

    #[test]
    fn bad_parameter_edits_are_refused_and_rolled_back() {
        let (mut ed, d, ..) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        let before = ed.doc.clone();
        let err = |r: Result<(), EditError>| match r {
            Err(EditError::Invalid(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(err(ed.set_variable(w, "0")).contains("isn't a valid value"));
        assert!(err(ed.set_variable(w, "5deg")).contains("expected a length"));
        assert!(err(ed.set_variable(w, "w + 1mm")).contains("itself"));
        let v = ed.add_variable("v", "w", Kind::Length).unwrap();
        assert!(err(ed.set_variable(w, "v")).contains("circular"));
        assert!(matches!(ed.remove_variable(w), Err(EditError::Invalid(_))));
        assert!(err(ed.set_dimension_text(d, "nope")).contains("unknown variable"));
        assert!(
            ed.add_variable("w", "1", Kind::Length).is_err(),
            "duplicate name"
        );
        ed.remove_variable(v).unwrap();
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn sliders_scrub_in_one_undo_step() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        assert!(ed.is_scrubbable(w));
        for value in [26.0, 30.0, 41.5] {
            assert!(ed.scrub_variable(w, value));
            assert!((length(&ed, a, b) - value).abs() < 1e-9);
        }
        // An impossible value is skipped, keeping the last good one.
        assert!(!ed.scrub_variable(w, 0.0));
        assert!((length(&ed, a, b) - 41.5).abs() < 1e-9);
        ed.end_scrub();
        assert_eq!(ed.doc.variable(w).unwrap().expr, "41.5 mm");
        ed.undo();
        assert_eq!(ed.doc.variable(w).unwrap().expr, "25 mm");
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn variants_store_and_apply_variable_sets() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        let small = ed.save_variant("Small");
        assert_eq!(ed.active_variant(), Some(small));
        ed.set_variable(w, "80").unwrap();
        assert_eq!(ed.active_variant(), None);
        let large = ed.save_variant("Large");
        ed.apply_variant(small).unwrap();
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
        assert_eq!(ed.active_variant(), Some(small));
        // Ghost geometry of the other variant.
        let ghost = ed.variant_geometry(large).unwrap();
        assert!(ghost.iter().any(|item| matches!(item.shape,
            Shape::Line { a, b } if (a.distance(b) - 80.0).abs() < 1e-9)));
        ed.undo();
        assert!((length(&ed, a, b) - 80.0).abs() < 1e-9);
    }

    #[test]
    fn whole_sketch_options_apply_as_an_undoable_step() {
        let (mut ed, d, a, b) = dimensioned_line();
        let original = length(&ed, a, b);
        let before = ed.save_option("Before", true);
        assert_eq!(ed.active_variant(), Some(before));
        ed.set_dimension(d, 40.0).unwrap();
        ed.add_variable("w", "1", Kind::Length).unwrap();
        assert_eq!(ed.active_variant(), None);
        let after = ed.save_option("After", true);
        ed.apply_variant(before).unwrap();
        assert_eq!(ed.history.undo_label(), Some("Apply Before"));
        assert_eq!(length(&ed, a, b), original);
        assert_eq!(ed.doc.variables().count(), 0);
        assert_eq!(ed.doc.variants().count(), 2, "applying keeps the options");
        assert_eq!(ed.active_variant(), Some(before));
        ed.undo();
        assert!((length(&ed, a, b) - 40.0).abs() < 1e-9);
        assert_eq!(ed.doc.variables().count(), 1);
        assert_eq!(ed.active_variant(), Some(after));
        // Update stores the sketch as it is now; rename and delete are steps too.
        ed.set_dimension(d, 45.0).unwrap();
        ed.update_variant(before);
        ed.apply_variant(after).unwrap();
        ed.apply_variant(before).unwrap();
        assert!((length(&ed, a, b) - 45.0).abs() < 1e-9);
        ed.rename_variant(before, "Long");
        assert_eq!(ed.doc.variant(before).unwrap().name, "Long");
        ed.remove_variant(after);
        assert_eq!(ed.doc.variants().count(), 1);
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.variants().count(), 2);
        assert_eq!(ed.doc.variant(before).unwrap().name, "Before");
    }

    #[test]
    fn values_options_apply_as_an_undoable_step() {
        let (mut ed, d, a, b) = dimensioned_line();
        let w = ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w").unwrap();
        let short = ed.save_option("Short", false);
        ed.set_variable(w, "60").unwrap();
        let long = ed.save_option("Long", false);
        assert!(!ed.doc.variant(long).unwrap().is_sketch());
        assert_eq!(ed.active_variant(), Some(long));
        ed.apply_variant(short).unwrap();
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-9);
        assert_eq!(ed.active_variant(), Some(short));
        ed.undo();
        assert!((length(&ed, a, b) - 60.0).abs() < 1e-9);
        assert_eq!(ed.active_variant(), Some(long));
    }

    #[test]
    fn units_change_display_only() {
        let (mut ed, d, a, b) = dimensioned_line();
        ed.set_dimension(d, 25.4 * 18.5).unwrap();
        let before = length(&ed, a, b);
        ed.set_units(Units::architectural());
        assert_eq!(length(&ed, a, b), before);
        assert_eq!(ed.dimension_edit_text(d).as_deref(), Some("1'-6 1/2\""));
        // Bare numbers are now inches.
        ed.set_dimension_text(d, "12").unwrap();
        assert!((length(&ed, a, b) - 12.0 * 25.4).abs() < 1e-9);
        ed.set_dimension_text(d, "2'-0\"").unwrap();
        assert!((length(&ed, a, b) - 24.0 * 25.4).abs() < 1e-9);
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.units(), Units::default());
    }

    #[test]
    fn pinned_measurements_follow_geometry() {
        let (mut ed, d, a, b) = dimensioned_line();
        let m = ed
            .pin_measurement(Constraint::Distance { a, b, value: 0.0 })
            .unwrap();
        ed.set_dimension(d, 33.0).unwrap();
        let (_, c) = ed.doc.measurements().find(|(id, _)| *id == m).unwrap();
        assert!((drift_solver::measure(&ed.doc, c).unwrap() - 33.0).abs() < 1e-9);
        // Deleting the geometry removes the measurement.
        ed.selection = ed.doc.iter().map(|(id, _)| id).collect();
        ed.delete_selection();
        assert_eq!(ed.doc.measurements().count(), 0);
        ed.undo();
        assert_eq!(ed.doc.measurements().count(), 1);
    }
}

#[cfg(test)]
mod collinear_tests {
    use super::*;

    /// Line (0,0)–(10,0) and a second line from `c` (a new point, or the first line's end
    /// when `connected`) to (20, 4). Returns the editor, both lines and all end points.
    fn two_lines(connected: bool) -> (Editor, [EntityId; 2], [EntityId; 4]) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(10.0, 0.0));
        let l1 = ed.doc.add_line(a, b).unwrap();
        let c = if connected {
            b
        } else {
            ed.doc.add_point(DVec2::new(12.0, 1.0))
        };
        let d = ed.doc.add_point(DVec2::new(20.0, 4.0));
        let l2 = ed.doc.add_line(c, d).unwrap();
        ed.commit("Lines");
        (ed, [l1, l2], [a, b, c, d])
    }

    fn assert_collinear(ed: &Editor, pts: [EntityId; 4]) {
        let p = pts.map(|id| ed.doc.point(id).unwrap());
        let u = (p[1] - p[0]).normalize();
        for q in &p[2..] {
            assert!((*q - p[0]).perp_dot(u).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn collinear_works_for_separate_and_connected_lines() {
        for connected in [false, true] {
            let (mut ed, [l1, l2], pts) = two_lines(connected);
            let dof = ed.analysis().dof.unwrap();
            ed.add_constraints("Collinear", &[Constraint::Collinear { a: l1, b: l2 }])
                .unwrap_or_else(|e| panic!("connected = {connected}: {e}"));
            assert_collinear(&ed, pts);
            // Separate lines lose two freedoms (direction and offset); connected ones one.
            let lost = if connected { 1 } else { 2 };
            assert_eq!(
                ed.analysis().dof,
                Some(dof - lost),
                "connected = {connected}"
            );
            // Adding it again is redundant.
            assert_eq!(
                ed.add_constraints("Collinear", &[Constraint::Collinear { a: l2, b: l1 }]),
                Err(EditError::Redundant)
            );
        }
    }

    #[test]
    fn collinear_works_for_lines_joined_by_a_coincident_relation() {
        let (mut ed, [l1, l2], [a, b, c, d]) = two_lines(false);
        ed.add_constraints("Coincident", &[Constraint::Coincident { a: b, b: c }])
            .unwrap();
        ed.add_constraints("Collinear", &[Constraint::Collinear { a: l1, b: l2 }])
            .unwrap();
        assert_collinear(&ed, [a, b, c, d]);
    }
}

/// A drag of many separate pieces (#253): each part of what is dragged moves on its own.
#[cfg(test)]
mod drag_parts {
    use drift_core::DVec2;
    use drift_solver::Param;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// The parameters a move of `ids` drags: all their points (as the Select tool's).
    fn dragged(ed: &Editor, ids: &[EntityId]) -> Vec<Param> {
        ed.doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .flat_map(Param::point)
            .collect()
    }

    /// A line from `a` with an arc hanging from its end (a round to reshape when only the
    /// line moves): returns (line, arc).
    fn line_with_arc(ed: &mut Editor, a: DVec2) -> (EntityId, EntityId) {
        let p = ed.doc.add_point(a);
        let q = ed.doc.add_point(a + v(10.0, 0.0));
        let line = ed.doc.add_line(p, q).unwrap();
        let c = ed.doc.add_point(a + v(10.0, 5.0));
        let e = ed.doc.add_point(a + v(15.0, 5.0));
        let arc = ed.doc.add_arc(c, q, e).unwrap();
        (line, arc)
    }

    #[test]
    fn free_parts_are_not_solved_with_the_others() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let (line, _) = line_with_arc(&mut ed, v(0.0, 0.0));
        // Many free lines, none joined to the others (nothing to solve in them).
        let mut ids = vec![line];
        for k in 0..300 {
            let p = ed.doc.add_point(v(100.0 + 3.0 * k as f64, 0.0));
            let q = ed.doc.add_point(v(100.0 + 3.0 * k as f64, 20.0));
            ids.push(ed.doc.add_line(p, q).unwrap());
        }
        // A whole arc moved with its every point: it keeps its shape, nothing to solve.
        let c = ed.doc.add_point(v(0.0, 100.0));
        let s = ed.doc.add_point(v(5.0, 100.0));
        let e = ed.doc.add_point(v(0.0, 105.0));
        ids.push(ed.doc.add_arc(c, s, e).unwrap());
        assert!(ed.commit("Sketch"));
        let free: Vec<DVec2> = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Point(p) if p.x >= 100.0 => Some(p),
                _ => None,
            })
            .collect();

        ed.doc.begin();
        let dragged = dragged(&ed, &ids);
        assert!(ed.drag(&ids, v(2.0, 1.0), &dragged));
        // Only the line and its arc were solved, not the free lines with them.
        assert_eq!(ed.drag_solves, 1);
        assert!(
            ed.drag_solved <= 6,
            "{} entities solved for a part of 5",
            ed.drag_solved
        );
        // Everything dragged went the whole way.
        let moved: Vec<DVec2> = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Point(p) if p.x >= 100.0 => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(moved.len(), free.len());
        for (was, now) in free.iter().zip(&moved) {
            assert!(now.distance(*was + v(2.0, 1.0)) < 1e-9);
        }
        assert!(ed.doc.point(c).unwrap().distance(v(2.0, 101.0)) < 1e-9);
        assert!(ed.commit("Move"));
    }

    #[test]
    fn a_sliver_turned_by_a_move_does_not_split_its_steps() {
        // A line with a round on its end, and from the round's far end a sliver (a
        // thousandth of a millimetre, as imports bring in) to a line that stays: the round
        // reshapes as the line moves, and turns the sliver round, which a drag never minds
        // in a line too short to see.
        let mut ed = Editor::default();
        ed.doc.begin();
        let (line, arc) = line_with_arc(&mut ed, v(0.0, 0.0));
        let EntityKind::Arc { end, .. } = ed.doc.get(arc).unwrap().kind else {
            unreachable!()
        };
        let e = ed.doc.point(end).unwrap();
        let tip = ed.doc.add_point(e + v(0.0007, 0.0007));
        let sliver = ed.doc.add_line(end, tip).unwrap();
        let far = ed.doc.add_point(e + v(0.0007, 30.0));
        ed.doc.add_line(tip, far).unwrap();
        assert!(ed.commit("Sketch"));
        ed.doc.begin();
        let ids = [line];
        let dragged = dragged(&ed, &ids);
        for k in 1..=4 {
            assert!(ed.drag(&ids, v(2.0, 1.0), &dragged));
            assert_eq!(ed.drag_solves, 1, "step {k}: the sliver split the step");
        }
        let EntityKind::Line { a, b } = ed.doc.get(sliver).unwrap().kind else {
            unreachable!()
        };
        // (It did turn round.)
        let now = ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap();
        assert!(now.dot(v(1.0, 1.0)) < 0.0, "{now:?}");
        assert!(ed.commit("Move"));
    }
}

/// Overlapping curves found again after an edit only where it changed things (#253).
#[cfg(test)]
mod overlaps {
    use drift_core::DVec2;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// What a fresh look over the whole drawing finds.
    fn fresh(ed: &Editor) -> Vec<EntityId> {
        *ed.overlap_cache.borrow_mut() = None;
        ed.overlapping().to_vec()
    }

    #[test]
    fn found_after_edits_as_a_fresh_look_finds_them() {
        let mut seed = 7u64;
        let mut rnd = move |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        // Lines on a coarse grid, so many lie on (or along) others.
        let mut ed = Editor::default();
        ed.doc.begin();
        for _ in 0..150 {
            let a = ed
                .doc
                .add_point(v(rnd(6) as f64 * 10.0, rnd(6) as f64 * 10.0));
            let b = ed
                .doc
                .add_point(v(rnd(6) as f64 * 10.0, rnd(6) as f64 * 10.0));
            let _ = ed.doc.add_line(a, b);
        }
        assert!(ed.commit("Lines"));
        ed.sync();
        assert!(!ed.overlapping().is_empty());
        for step in 0..120 {
            let curves: Vec<EntityId> = ed
                .doc
                .iter()
                .filter(|(_, e)| !e.kind.is_point())
                .map(|(id, _)| id)
                .collect();
            match rnd(5) {
                0 if !curves.is_empty() => {
                    ed.selection = [curves[rnd(curves.len())]].into_iter().collect();
                    let d = v(rnd(3) as f64 * 10.0 - 10.0, rnd(3) as f64 * 10.0 - 10.0);
                    let _ = ed.nudge(d);
                }
                1 if !curves.is_empty() => {
                    ed.selection = [curves[rnd(curves.len())]].into_iter().collect();
                    ed.delete_selection();
                }
                2 => {
                    ed.doc.begin();
                    let a = ed
                        .doc
                        .add_point(v(rnd(6) as f64 * 10.0, rnd(6) as f64 * 10.0));
                    let b = ed
                        .doc
                        .add_point(v(rnd(6) as f64 * 10.0, rnd(6) as f64 * 10.0));
                    let _ = ed.doc.add_line(a, b);
                    ed.commit("Line");
                }
                3 => {
                    ed.undo();
                }
                _ => {
                    ed.redo();
                }
            }
            ed.selection.clear();
            ed.sync();
            let kept = ed.overlapping().to_vec();
            assert_eq!(kept, fresh(&ed), "step {step}");
        }
    }
}

/// Editor rules that were tested through the app's window (ui_tests) with no more than the
/// window as a host (R3 S1).
mod from_the_app {
    use drift_core::DVec2;

    use super::*;

    #[test]
    fn long_text_and_long_histories_are_bounded() {
        let mut ed = Editor::default();
        let id = ed.add_text(drift_core::SketchText::new("", DVec2::ZERO, 5.0));
        ed.type_text(id, &"x".repeat(50_000), false);
        assert_eq!(
            ed.doc.text(id).unwrap().text.chars().count(),
            crate::texts::MAX_TEXT_CHARS
        );
        // Far more edits than the history keeps: the oldest steps are forgotten.
        for k in 0..drift_core::History::MAX_STEPS + 50 {
            ed.update_text(id, |t| t.height = 5.0 + k as f64);
        }
        let mut undone = 0;
        while ed.undo() {
            undone += 1;
        }
        assert_eq!(undone, drift_core::History::MAX_STEPS);
    }

    #[test]
    fn sizes_typed_in_the_inspector_become_dimensions() {
        use drift_core::Constraint;
        let mut ed = Editor::default();
        // A line from (-30, 0) to (20, 0).
        ed.doc.begin();
        let (a, b) = (
            ed.doc.add_point(DVec2::new(-30.0, 0.0)),
            ed.doc.add_point(DVec2::new(20.0, 0.0)),
        );
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Line");
        let wanted = Constraint::Distance { a, b, value: 0.0 };
        // No dimension yet: typing a length adds one; typing again changes it.
        assert!(ed.size_dimension(&wanted).is_none());
        let id = ed.set_size(wanted, "80").unwrap();
        assert!((ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap()) - 80.0).abs() < 1e-6);
        assert_eq!(ed.set_size(wanted, "2in").unwrap(), id);
        assert!((ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap()) - 50.8).abs() < 1e-6);
        assert_eq!(
            ed.doc
                .constraints()
                .filter(|(_, c)| c.value().is_some())
                .count(),
            1
        );
        // A circle's radius typed where its diameter is dimensioned changes that dimension.
        ed.doc.begin();
        let centre = ed.doc.add_point(DVec2::new(0.0, 40.0));
        let circle = ed.doc.add_circle(centre, 10.0).unwrap();
        ed.commit("Circle");
        let d = ed
            .set_size(
                Constraint::Diameter {
                    curve: circle,
                    value: 0.0,
                },
                "30",
            )
            .unwrap();
        let r = ed
            .set_size(
                Constraint::Radius {
                    curve: circle,
                    value: 0.0,
                },
                "25",
            )
            .unwrap();
        assert_eq!(d, r);
        assert!((drift_solver::radius_of(&ed.doc, circle).unwrap() - 25.0).abs() < 1e-6);
    }

    #[test]
    fn a_radius_typed_for_a_fillet_keeps_its_lines() {
        let v = DVec2::new;
        // A plain corner, filleted with automatic relations off: nothing holds the lines.
        let mut ed = Editor {
            auto_relations: false,
            ..Editor::default()
        };
        ed.doc.begin();
        let corner = ed.doc.add_point(v(0.0, 0.0));
        let ends = [v(60.0, 10.0), v(-5.0, 40.0)].map(|at| ed.doc.add_point(at));
        ed.doc.add_line(corner, ends[0]).unwrap();
        ed.doc.add_line(corner, ends[1]).unwrap();
        ed.commit("Corner");
        let arc = ed.fillet(corner, 6.0).unwrap();
        assert_eq!(ed.doc.constraint_count(), 0);
        // The radius typed in the inspector becomes a dimension; the fillet takes that size
        // where it is.
        let wanted = drift_core::Constraint::Radius {
            curve: arc,
            value: 0.0,
        };
        let id = ed.set_size(wanted, "10").unwrap();
        let f = ed.fillet_at(arc).expect("still a fillet");
        assert!((f.radius - 10.0).abs() < 1e-7);
        assert!(
            f.corner.length() < 1e-7,
            "the lines still meet where they did"
        );
        assert!(kept(&f));
        // And editing that dimension afterwards, the same.
        ed.set_dimension(id, 4.0).unwrap();
        let f = ed.fillet_at(arc).expect("still a fillet");
        assert!((f.radius - 4.0).abs() < 1e-7 && f.corner.length() < 1e-7);
        assert!(kept(&f));
        /// The lines' far ends are where they were (in either order).
        fn kept(f: &crate::modify::FilletInfo) -> bool {
            [DVec2::new(60.0, 10.0), DVec2::new(-5.0, 40.0)]
                .iter()
                .all(|end| {
                    f.arms.iter().any(|a| {
                        let far = f.corner + a.dir(f.corner) * a.length;
                        far.distance(*end) < 1e-9
                    })
                })
        }
    }

    #[test]
    fn text_can_use_the_computers_fonts() {
        use drift_core::{SketchText, TextFont};
        let mut ed = Editor::default();
        let outline_of = |ed: &mut Editor, font: TextFont| {
            ed.doc.begin();
            let mut text = SketchText::new("Rag", DVec2::ZERO, 10.0);
            text.font = font;
            let id = ed.doc.add_text(text);
            ed.commit("Text");
            ed.text_outline(id).expect("an outline")
        };
        let sans = outline_of(&mut ed, TextFont::Sans);

        // A font that isn't installed: the text is drawn in the built-in sans, and says so.
        let missing = TextFont::System("No Such Typeface 123".into());
        let fallback = outline_of(&mut ed, missing.clone());
        assert_eq!(fallback.contours, sans.contours);
        assert_eq!(
            drift_draft::outline::font_known("No Such Typeface 123"),
            Some(false)
        );
        // It keeps its name in the file all the same.
        let saved = drift_io::save(&ed.doc, &Default::default());
        assert!(
            saved.contains(r#""font":"system:No Such Typeface 123""#),
            "{saved}"
        );
        let (loaded, _) = drift_io::load(&saved).unwrap();
        assert!(loaded.texts().any(|(_, t)| t.font == missing));
        assert_eq!(TextFont::from_name("system:"), None);
        assert_eq!(TextFont::from_name("serif"), None);

        // One that is: its own outlines. (Whatever this computer has; none in a bare CI image.)
        let families = drift_platform::fonts::families();
        let Some(family) = ["Arial", "Helvetica", "DejaVu Sans", "Liberation Sans"]
            .into_iter()
            .find(|f| families.iter().any(|x| x == f))
            .map(str::to_owned)
            .or_else(|| families.first().cloned())
        else {
            eprintln!("no system fonts here; skipping the rest");
            return;
        };
        assert!(
            families
                .windows(2)
                .all(|w| w[0].to_lowercase() <= w[1].to_lowercase())
        );
        let system = outline_of(&mut ed, TextFont::System(family.clone()));
        assert_eq!(
            drift_draft::outline::font_known(&family),
            Some(true),
            "{family}"
        );
        assert!(!system.contours.is_empty());
        assert_ne!(
            system.contours, sans.contours,
            "{family} is not the built-in sans"
        );
        // Capitals are as tall as asked, whatever the font.
        let height = system.bounds.max.y - system.bounds.min.y;
        assert!(height > 9.0 && height < 16.0, "{height}");
    }

    #[test]
    fn a_dragged_or_typed_tolerance_is_one_undo_step_and_clears_redo() {
        let mut ed = Editor::default();
        let start = ed.doc.rules().gap_tolerance;
        // A tick box, then a tolerance edited in three frames of one drag.
        ed.change_rules(|r| r.stacked_fills = !r.stacked_fills);
        ed.change_rules_merging(false, |r| r.gap_tolerance = 1.0);
        ed.change_rules_merging(true, |r| r.gap_tolerance = 1.5);
        ed.change_rules_merging(true, |r| r.gap_tolerance = 2.0);
        ed.undo();
        assert_eq!(ed.doc.rules().gap_tolerance, start, "the drag was one step");
        assert_ne!(
            ed.doc.rules().stacked_fills,
            drift_core::DrawingRules::default().stacked_fills,
            "the tick box was a step of its own"
        );
        // A new drag after an undo is its own step, and what was undone can't be redone over.
        ed.undo();
        assert!(ed.history.can_redo());
        ed.change_rules_merging(false, |r| r.gap_tolerance = 3.0);
        ed.change_rules_merging(true, |r| r.gap_tolerance = 4.0);
        assert!(!ed.history.can_redo());
        ed.undo();
        assert_eq!(ed.doc.rules(), drift_core::DrawingRules::default());
    }
}
