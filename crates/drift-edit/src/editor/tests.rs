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
        assert_eq!(ed.out.pending_driven, Some((horizontal, placement)));
        let (constraint, placement) = ed.out.pending_driven.take().unwrap();
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

    /// A bare number added to a length variable is in the drawing's unit (#241), for
    /// dimensions and variables alike, and the stored text carries the unit.
    #[test]
    fn bare_numbers_in_a_sum_follow_the_drawing_unit() {
        let (mut ed, d, a, b) = dimensioned_line();
        ed.add_variable("w", "25", Kind::Length).unwrap();
        ed.set_dimension_text(d, "w + 10").unwrap();
        assert!((length(&ed, a, b) - 35.0).abs() < 1e-9);
        assert_eq!(
            ed.doc.expression(d),
            Some("w + 10 mm"),
            "unit made explicit"
        );
        assert_eq!(ed.dimension_edit_text(d).as_deref(), Some("w + 10 mm"));
        let h = ed.add_variable("h", "w + 5", Kind::Length).unwrap();
        assert_eq!(ed.doc.variable(h).unwrap().expr, "w + 5 mm");
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
        // (Within the solver's bar: a billionth of a quarter of the sketch's size.)
        assert!((length(&ed, a, b) - 25.0).abs() < 1e-7);
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
            prefs: EditPrefs {
                auto_relations: false,
                ..EditPrefs::default()
            },
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

/// #253: `joining` as a forest of indices gives exactly what relabelling every item on each
/// merge gave (kept here as it was), on random drawings with chains of relations already
/// there, several kinds of them, relations reaching outside the pick, and repeated ids.
#[cfg(test)]
mod joining_matches_the_old_way {
    use std::collections::HashMap;

    use drift_core::{Constraint, DVec2, Document};

    use super::*;

    /// `Editor::joining` as it was before #253's fix.
    fn old_joining(
        doc: &Document,
        items: &[EntityId],
        related: fn(&Constraint) -> Option<(EntityId, EntityId)>,
        make: impl Fn(EntityId, EntityId) -> Constraint,
    ) -> Vec<Constraint> {
        let mut group: HashMap<EntityId, usize> =
            items.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        fn merge(group: &mut HashMap<EntityId, usize>, a: usize, b: usize) {
            for g in group.values_mut() {
                if *g == b {
                    *g = a;
                }
            }
        }
        let kind = std::mem::discriminant(&make(items[0], items[1]));
        let mut out = Vec::new();
        for (_, c) in doc.constraints() {
            if let Some((a, b)) = related(c)
                && let (Some(&ga), Some(&gb)) = (group.get(&a), group.get(&b))
            {
                if std::mem::discriminant(c) == kind {
                    out.push(*c);
                }
                if ga != gb {
                    merge(&mut group, ga, gb);
                }
            }
        }
        let first = items[0];
        for id in &items[1..] {
            let (g0, g) = (group[&first], group[id]);
            if g0 != g {
                out.push(make(first, *id));
                merge(&mut group, g0, g);
            }
        }
        out
    }

    struct Rnd(u64);
    impl Rnd {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }

    /// A drawing of `n` loose lines with random relations among them and their points:
    /// chains (each to the one before), and pairs anywhere.
    fn drawing(rnd: &mut Rnd, n: usize) -> (Editor, Vec<EntityId>, Vec<EntityId>) {
        let mut doc = Document::new();
        let mut lines = Vec::new();
        let mut points = Vec::new();
        for k in 0..n {
            let x = k as f64 * 3.0;
            let a = doc.add_point(DVec2::new(x, 0.0));
            let b = doc.add_point(DVec2::new(x + 1.0, 2.0));
            points.extend([a, b]);
            lines.push(doc.add_line(a, b).unwrap());
        }
        for _ in 0..(n / 2 + rnd.below(n + 1)) {
            let (i, j) = if rnd.below(3) == 0 && n > 1 {
                let i = 1 + rnd.below(n - 1);
                (i, i - 1)
            } else {
                (rnd.below(n), rnd.below(n))
            };
            if i == j {
                continue;
            }
            let (la, lb) = (lines[i], lines[j]);
            let (pa, pb) = (points[2 * i + rnd.below(2)], points[2 * j + rnd.below(2)]);
            let c = match rnd.below(6) {
                0 => Constraint::Parallel { a: la, b: lb },
                1 => Constraint::Collinear { a: la, b: lb },
                2 => Constraint::Equal { a: la, b: lb },
                3 => Constraint::Coincident { a: pa, b: pb },
                4 => Constraint::HorizontalPoints { a: pa, b: pb },
                _ => Constraint::VerticalPoints { a: pa, b: pb },
            };
            let _ = doc.add_constraint(c);
        }
        (Editor::new(doc), lines, points)
    }

    /// Up to `max` of `all` (some repeated), at least two.
    fn pick(rnd: &mut Rnd, all: &[EntityId], max: usize) -> Vec<EntityId> {
        let n = 2 + rnd.below(max.min(all.len()).max(2) - 1);
        (0..n).map(|_| all[rnd.below(all.len())]).collect()
    }

    type Pair = fn(&Constraint) -> Option<(EntityId, EntityId)>;
    type Make = fn(EntityId, EntityId) -> Constraint;

    #[test]
    fn on_random_drawings() {
        let mut rnd = Rnd(7);
        let mut compared = 0;
        let mut joined_some = 0;
        for round in 0..300 {
            let n = 2 + rnd.below(if round % 10 == 0 { 120 } else { 16 });
            let (ed, lines, points) = drawing(&mut rnd, n);
            for _ in 0..4 {
                let lines_pick = pick(&mut rnd, &lines, n);
                let points_pick = pick(&mut rnd, &points, 2 * n);
                let line_kinds: [(Pair, Make); 2] = [
                    (Editor::parallel_pair, |a, b| Constraint::Parallel { a, b }),
                    (Editor::collinear_pair, |a, b| Constraint::Collinear {
                        a,
                        b,
                    }),
                ];
                let point_kinds: [(Pair, Make); 3] = [
                    (Editor::coincident_pair, |a, b| Constraint::Coincident {
                        a,
                        b,
                    }),
                    (Editor::horizontal_pair, |a, b| {
                        Constraint::HorizontalPoints { a, b }
                    }),
                    (Editor::vertical_pair, |a, b| Constraint::VerticalPoints {
                        a,
                        b,
                    }),
                ];
                for (items, kinds) in [
                    (&lines_pick, &line_kinds[..]),
                    (&points_pick, &point_kinds[..]),
                ] {
                    for (related, make) in kinds {
                        let new = ed.joining(items, *related, make);
                        let old = old_joining(&ed.doc, items, *related, make);
                        assert_eq!(new, old, "round {round}, items {items:?}");
                        compared += 1;
                        joined_some += usize::from(new.len() < items.len() - 1);
                    }
                }
            }
        }
        assert_eq!(compared, 300 * 4 * 5);
        // (Relations already there tied some picks together: not only the plain case.)
        assert!(joined_some > 500, "{joined_some}");
    }
}

mod circle_drag {
    use drift_core::{Constraint, DVec2};
    use drift_solver::Param;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A point on a circle whose centre is fixed, dragged across the centre in 20 frames, at
    /// several sizes and places: every frame that goes meets every relation and leaves the
    /// circle a radius above zero, every frame that doesn't leaves the drawing as the last one
    /// did, and nothing slides (the centre stays where it is fixed). The frame that would put
    /// the point on the centre is refused (a circle crushed to a point counts as turned,
    /// #280); further on, past the centre, the drag goes on with the point on the far side.
    /// (A guard: the drag must never leave a circle without a radius, whatever stops it.)
    #[test]
    fn a_point_dragged_across_its_circles_centre_never_leaves_it_without_a_radius() {
        for (scale, at) in [
            (1.0, DVec2::ZERO),
            (10.0, DVec2::ZERO),
            (0.1, DVec2::ZERO),
            (1.0, v(1e5, 1e5)),
        ] {
            let mut ed = Editor::default();
            ed.doc.begin();
            let center = ed.doc.add_point(at);
            let circle = ed.doc.add_circle(center, 5.0 * scale).unwrap();
            let p = ed.doc.add_point(at + v(5.0, 0.0) * scale);
            ed.doc
                .add_constraint(Constraint::PointOnCurve {
                    point: p,
                    curve: circle,
                })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Fix { point: center, at })
                .unwrap();
            assert!(ed.commit("Sketch"));
            ed.doc.begin();
            let dragged = Param::point(p).to_vec();
            let mut refused = Vec::new();
            let mut applied = DVec2::ZERO;
            for k in 1..=20 {
                let target = v(-10.0, 0.0) * scale * (f64::from(k) / 20.0);
                let before = (ed.doc.point(p).unwrap(), ed.doc.get(circle).unwrap().kind);
                if ed.drag(&[p], target - applied, &dragged) {
                    applied = target;
                    assert!(
                        drift_solver::unmet(&ed.doc, 1e-8).is_empty(),
                        "{scale}× about {at}, frame {k}: a relation unmet"
                    );
                } else {
                    refused.push(k);
                    assert_eq!(
                        (ed.doc.point(p).unwrap(), ed.doc.get(circle).unwrap().kind),
                        before,
                        "{scale}× about {at}, frame {k}: a refused frame moved something"
                    );
                }
                let EntityKind::Circle { radius, .. } = ed.doc.get(circle).unwrap().kind else {
                    unreachable!()
                };
                assert!(
                    radius > 0.0,
                    "{scale}× about {at}, frame {k}: radius {radius}"
                );
                assert_eq!(
                    ed.doc.point(center).unwrap(),
                    at,
                    "{scale}× about {at}: slid"
                );
            }
            ed.commit("Move");
            assert!(
                refused.contains(&10),
                "{scale}× about {at}: refused {refused:?}"
            );
        }
    }

    /// A point on a circle (or an arc) whose centre is fixed, dragged along a line through the
    /// centre from `from` to x = −5 (times `scale`, about `at`) in `frames` frames: the frames
    /// refused.
    fn refused_crossing(arc: bool, from: f64, frames: u32, scale: f64, at: DVec2) -> Vec<u32> {
        let mut ed = Editor::default();
        ed.doc.begin();
        let center = ed.doc.add_point(at);
        let curve = if arc {
            let start = ed.doc.add_point(at + v(0.0, -from) * scale);
            let end = ed.doc.add_point(at + v(0.0, from) * scale);
            ed.doc.add_arc(center, start, end).unwrap()
        } else {
            ed.doc.add_circle(center, from * scale).unwrap()
        };
        let p = ed.doc.add_point(at + v(from, 0.0) * scale);
        ed.doc
            .add_constraint(Constraint::PointOnCurve { point: p, curve })
            .unwrap();
        ed.doc
            .add_constraint(Constraint::Fix { point: center, at })
            .unwrap();
        assert!(ed.commit("Sketch"));
        ed.doc.begin();
        let dragged = Param::point(p).to_vec();
        let mut refused = Vec::new();
        let mut applied = DVec2::ZERO;
        for k in 1..=frames {
            let target = v(-5.0 - from, 0.0) * scale * (f64::from(k) / f64::from(frames));
            if ed.drag(&[p], target - applied, &dragged) {
                applied = target;
            } else {
                refused.push(k);
            }
        }
        ed.commit("Move");
        refused
    }

    /// The crossing goes straight through, the same at every size and place: only a frame
    /// whose target is on the centre itself (where the circle would be crushed, F1) is
    /// refused, and the next frame lands on the far side. (The solve's step limit counts a
    /// circle's radius as part of how far the drawing reaches; counting points only, a radius
    /// change bigger than the points' spread had every step cut short, so the frame was taken
    /// again in small steps along a path through the centre, and refused for a stretch past it.)
    /// A point on an arc is refused on the centre frame too: the arc would turn (F1), as before.
    #[test]
    fn a_point_dragged_across_its_circles_centre_is_refused_only_on_it() {
        let cases: [(bool, f64, u32, &[u32]); 7] = [
            (false, 5.0, 20, &[10]),
            (false, 5.0, 64, &[32]),
            (false, 5.0, 7, &[]),
            (false, 5.0, 21, &[]),
            (false, 0.5, 20, &[]),
            (false, 0.5, 64, &[]),
            (true, 5.0, 20, &[10]),
        ];
        for (arc, from, frames, want) in cases {
            for scale in [0.1, 1.0, 10.0] {
                for at in [DVec2::ZERO, v(1e5, 1e5), v(1e6, 1e6)] {
                    assert_eq!(
                        refused_crossing(arc, from, frames, scale, at),
                        want,
                        "{} from {from}, {frames} frames, {scale}× about {at}",
                        if arc { "arc" } else { "circle" }
                    );
                }
            }
        }
    }
}

/// #342: an edit whose solve leaves geometry broken (rows no relation owns: an arc's end on
/// the circle through its start, a line's hidden midpoint, an ellipse's axes, a wall end on
/// the wall it meets) is undone, as one that makes a new conflict is; geometry already in a
/// failing part doesn't lock the drawing.
mod broken_geometry {
    use drift_core::{Constraint, ConstraintId, DVec2, Document};
    use drift_solver::{Param, SolveReport};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn kinds(ed: &Editor) -> Vec<(EntityId, EntityKind)> {
        ed.doc.iter().map(|(id, e)| (id, e.kind)).collect()
    }

    /// A point `b` held 10 from two fixed points 30 apart (a conflict), never solved: (b, the
    /// two distances).
    fn conflicting(doc: &mut Document) -> (EntityId, [ConstraintId; 2]) {
        let a = doc.add_point(v(0.0, 0.0));
        let f = doc.add_point(v(30.0, 0.0));
        let b = doc.add_point(v(12.0, 3.0));
        doc.add_constraint(Constraint::Fix {
            point: a,
            at: v(0.0, 0.0),
        })
        .unwrap();
        doc.add_constraint(Constraint::Fix {
            point: f,
            at: v(30.0, 0.0),
        })
        .unwrap();
        let d1 = doc
            .add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        let d2 = doc
            .add_constraint(Constraint::Distance {
                a: b,
                b: f,
                value: 10.0,
            })
            .unwrap();
        (b, [d1, d2])
    }

    /// An arc held by fixes with its end off the circle through its start: the commit's solve
    /// fails on the arc's own row alone, naming no relation. It is undone with the notice
    /// (on main it was kept, the arc broken).
    #[test]
    fn a_commit_that_leaves_an_arc_off_its_circle_is_undone() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let s = ed.doc.add_point(v(10.0, 0.0));
        let e = ed.doc.add_point(v(0.0, 10.0));
        ed.doc.add_arc(c, s, e).unwrap();
        assert!(ed.commit("Arc"));
        let before = kinds(&ed);
        ed.doc.begin();
        ed.doc.set_point(e, v(0.0, 12.0)).unwrap();
        for (point, at) in [(c, v(0.0, 0.0)), (s, v(10.0, 0.0)), (e, v(0.0, 12.0))] {
            ed.doc
                .add_constraint(Constraint::Fix { point, at })
                .unwrap();
        }
        assert!(!ed.commit("Fix"));
        assert_eq!(
            ed.out.notice.as_deref(),
            Some("Fix undone: it conflicts with existing constraints")
        );
        assert_eq!(kinds(&ed), before);
        assert_eq!(ed.doc.constraints().count(), 0);
    }

    /// The mixed part: an arc drawn on a point of a part that already conflicts. The solve
    /// names only the old conflicts and puts the arc in their failed part, where it shares
    /// their misfit: the edit is undone (on main it was kept), even where the arc's own row
    /// happens to end within its tolerance. And an arc already drawn elsewhere joined to
    /// that point by a relation: undone (as on main: the relation is a new conflict).
    #[test]
    fn an_arc_joined_to_a_part_that_already_conflicts_is_undone() {
        let mut doc = Document::new();
        let (b, _) = conflicting(&mut doc);
        let c = doc.add_point(v(60.0, 0.0));
        let s = doc.add_point(v(65.0, 0.0));
        let e = doc.add_point(v(60.0, 5.0));
        let elsewhere = doc.add_arc(c, s, e).unwrap();
        let mut ed = Editor::new(doc);
        assert!(!ed.analysis().conflicts.is_empty());
        assert!(!ed.analysis().exposed.contains(&elsewhere));

        let before = kinds(&ed);
        ed.doc.begin();
        let at = ed.doc.point(b).unwrap();
        let s = ed.doc.add_point(at + v(5.0, 0.0));
        let e = ed.doc.add_point(at + v(0.0, 5.0));
        let arc = ed.doc.add_arc(b, s, e).unwrap();
        // Solved, the new arc's own row ends within its tolerance: it is refused for being in
        // the failed part, not for how far the solve left it off.
        let mut solved = ed.doc.clone();
        let report = drift_solver::solve(&mut solved, &drift_solver::SolveOptions::default());
        assert!(report.exposed.contains(&arc) && !report.broken.contains(&arc));
        assert!(!ed.commit("Arc"));
        assert_eq!(
            ed.out.notice.as_deref(),
            Some("Arc undone: it conflicts with existing constraints")
        );
        assert_eq!(kinds(&ed), before);

        ed.doc.begin();
        ed.doc
            .add_constraint(Constraint::Coincident { a: c, b })
            .unwrap();
        assert!(!ed.commit("Coincident"));
        assert_eq!(kinds(&ed), before);
    }

    /// A drawing that already conflicts, never solved, with an arc in the conflicting part
    /// exactly on its circle: the first solve shares the misfit with the
    /// arc, but the arc was already in a failing part, so that isn't the edit's doing. An
    /// unrelated edit is kept, so is a second, and so is a drag in that part. (Judged against
    /// what was broken before rather than what was exposed, every edit would be refused.)
    #[test]
    fn an_old_conflict_next_to_an_arc_does_not_lock_the_drawing() {
        let mut doc = Document::new();
        let (b, _) = conflicting(&mut doc);
        let at = doc.point(b).unwrap();
        let s = doc.add_point(at + v(5.0, 0.0));
        let e = doc.add_point(at + v(0.0, 5.0));
        let arc = doc.add_arc(b, s, e).unwrap();
        // (Not `Editor::new`, which solves the drawing it opens.)
        let mut ed = Editor {
            doc,
            ..Default::default()
        };
        ed.sync();
        assert_eq!(ed.analysis().exposed, vec![arc]);
        assert!(
            ed.analysis().broken.is_empty(),
            "stored exactly on its circle"
        );

        for k in 0..2 {
            ed.doc.begin();
            let p = ed.doc.add_point(v(100.0 + f64::from(k) * 20.0, 50.0));
            let q = ed.doc.add_point(v(110.0 + f64::from(k) * 20.0, 50.0));
            ed.doc.add_line(p, q).unwrap();
            assert!(ed.commit("Line"), "unrelated edit {k} refused");
        }
        ed.doc.begin();
        assert!(
            ed.drag(&[s], v(1.0, 1.0), &Param::point(s)),
            "a drag in the conflicting part refused"
        );
        assert!(ed.commit("Move"));
    }

    /// Both predicates, clause by clause, on reports against a drawing whose analysis has a
    /// conflict and an exposed arc: a newly exposed entity, or nothing written, refuses an
    /// edit and a drag step even when every conflict named is an old one; an entity exposed
    /// before doesn't.
    #[test]
    fn each_clause_refuses_alone() {
        let mut doc = Document::new();
        let (b, distances) = conflicting(&mut doc);
        let at = doc.point(b).unwrap();
        let s = doc.add_point(at + v(5.0, 0.0));
        let e = doc.add_point(at + v(0.0, 5.0));
        let old = doc.add_arc(b, s, e).unwrap();
        let c = doc.add_point(v(60.0, 0.0));
        let s2 = doc.add_point(v(65.0, 0.0));
        let e2 = doc.add_point(v(60.0, 5.0));
        let new = doc.add_arc(c, s2, e2).unwrap();
        let ed = Editor::new(doc);
        let report = |exposed: Vec<EntityId>, unwritten: bool| SolveReport {
            converged: false,
            conflicts: distances.to_vec(),
            broken: exposed.clone(),
            exposed,
            unwritten,
            ..Default::default()
        };
        // Only what was there before: kept.
        assert!(!ed.new_conflicts(&report(vec![old], false)));
        assert!(!ed.unfinished(&report(vec![old], false)));
        // A newly exposed arc.
        assert!(ed.new_conflicts(&report(vec![old, new], false)));
        assert!(ed.unfinished(&report(vec![old, new], false)));
        // Nothing written.
        assert!(ed.new_conflicts(&report(vec![], true)));
        assert!(ed.unfinished(&report(vec![], true)));
    }

    /// Cases A and B of drag PR 5, through `set_dimension` at several sizes and places: a
    /// dimension retyped so that a tangency's form, as the sketch was, would need a radius
    /// below zero. The edit ends consistent with the tangency's form re-chosen, as on main.
    /// (A guard: #342 must not make them worse; it passes on main too.)
    #[test]
    fn a_dimension_past_a_tangency_still_ends_consistent() {
        for (scale, at) in [
            (1.0, DVec2::ZERO),
            (10.0, DVec2::ZERO),
            (0.1, DVec2::ZERO),
            (1.0, v(1e5, 1e5)),
        ] {
            // A: a circle tangent inside a bigger one, the bigger radius 10 → 5.
            let mut ed = Editor::default();
            ed.doc.begin();
            let c1 = ed.doc.add_point(at);
            let big = ed.doc.add_circle(c1, 10.0 * scale).unwrap();
            let c2 = ed.doc.add_point(at + v(7.0, 0.0) * scale);
            let small = ed.doc.add_circle(c2, 3.0 * scale).unwrap();
            ed.doc
                .add_constraint(Constraint::Fix { point: c1, at })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Fix {
                    point: c2,
                    at: at + v(7.0, 0.0) * scale,
                })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Tangent { a: big, b: small })
                .unwrap();
            let radius = ed
                .doc
                .add_constraint(Constraint::Radius {
                    curve: big,
                    value: 10.0 * scale,
                })
                .unwrap();
            assert!(ed.commit("Sketch"));
            assert!(
                ed.set_dimension(radius, 5.0 * scale).is_ok(),
                "A {scale}× {at}"
            );
            let r = |id| drift_solver::radius_of(&ed.doc, id).unwrap();
            assert!(
                (r(big) - 5.0 * scale).abs() <= 1e-9 * scale,
                "A {scale}× {at}"
            );
            assert!(
                (r(small) - 2.0 * scale).abs() <= 1e-9 * scale,
                "A {scale}× {at}"
            );
            assert!(ed.analysis().conflicts.is_empty(), "A {scale}× {at}");

            // B: a level line tangent to a circle, its height 15 → 8 (2 past the centre).
            let mut ed = Editor::default();
            ed.doc.begin();
            let c = ed.doc.add_point(at);
            let circle = ed.doc.add_circle(c, 5.0 * scale).unwrap();
            let q = ed.doc.add_point(at + v(0.0, -10.0) * scale);
            let a = ed.doc.add_point(at + v(-20.0, 5.0) * scale);
            let b = ed.doc.add_point(at + v(20.0, 5.0) * scale);
            let line = ed.doc.add_line(a, b).unwrap();
            ed.doc
                .add_constraint(Constraint::Fix { point: c, at })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Fix {
                    point: q,
                    at: at + v(0.0, -10.0) * scale,
                })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Horizontal { line })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Tangent { a: line, b: circle })
                .unwrap();
            let height = ed
                .doc
                .add_constraint(Constraint::VerticalDistance {
                    a: q,
                    b: a,
                    value: 15.0 * scale,
                })
                .unwrap();
            assert!(ed.commit("Sketch"));
            assert!(
                ed.set_dimension(height, 8.0 * scale).is_ok(),
                "B {scale}× {at}"
            );
            let y = ed.doc.point(a).unwrap().y - at.y;
            assert!(
                (y + 2.0 * scale).abs() <= 1e-9 * scale.max(1.0),
                "B {scale}× {at}: {y}"
            );
            assert!(
                (drift_solver::radius_of(&ed.doc, circle).unwrap() - 2.0 * scale).abs()
                    <= 1e-9 * scale,
                "B {scale}× {at}"
            );
            assert!(ed.analysis().conflicts.is_empty(), "B {scale}× {at}");
        }
    }
}

/// #342 with walls (their relations lowered, the drawing analysed whole): a teed wall's end
/// is held on the wall it meets by a relation no stored constraint owns (a lowering's extra,
/// owned by the end's point). Fixing the end off that wall breaks it: the commit is undone.
/// Only the verdict is new; what walls solve against is unchanged (#313). And an old
/// conflict next to an arc doesn't lock a drawing with walls either.
mod broken_geometry_with_walls {
    use drift_core::{Constraint, DVec2, Document, WallId};
    use drift_solver::Param;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A straight wall along `pts`, in the open transaction: (wall, its centreline points).
    fn wall(doc: &mut Document, a: DVec2, b: DVec2) -> (WallId, [EntityId; 2]) {
        let (p, q) = (doc.add_point(a), doc.add_point(b));
        let line = doc.add_line(p, q).unwrap();
        doc.set_style(line, drift_core::LineKind::Construction);
        let path = doc
            .add_path(drift_core::Path {
                curves: vec![line],
                closed: false,
            })
            .unwrap();
        let w = doc
            .add_wall(drift_core::Wall::new(
                path,
                200.0,
                drift_core::LayerId::DEFAULT,
            ))
            .unwrap();
        (w, [p, q])
    }

    #[test]
    fn a_teed_wall_end_fixed_off_its_host_is_undone() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let (host, [h0, h1]) = wall(&mut ed.doc, v(0.0, 0.0), v(4000.0, 0.0));
        let (branch, [b0, _]) = wall(&mut ed.doc, v(2000.0, 600.0), v(2000.0, 3000.0));
        assert!(ed.commit("Walls"));
        assert!(ed.join_walls(&[host, branch]));
        assert_eq!(
            ed.doc.wall(branch).unwrap().ends[0].tee.map(|t| t.0),
            Some(host)
        );
        // The host held where it is (so its end is held on it), then the branch's end fixed
        // 300 above its centreline.
        ed.doc.begin();
        for p in [h0, h1] {
            let at = ed.doc.point(p).unwrap();
            ed.doc
                .add_constraint(Constraint::Fix { point: p, at })
                .unwrap();
        }
        assert!(ed.commit("Fix host"));
        let before: Vec<_> = ed.doc.iter().map(|(id, e)| (id, e.kind)).collect();
        ed.doc.begin();
        ed.doc
            .add_constraint(Constraint::Fix {
                point: b0,
                at: v(2000.0, 300.0),
            })
            .unwrap();
        assert!(!ed.commit("Fix end"));
        assert_eq!(
            ed.out.notice.as_deref(),
            Some("Fix end undone: it conflicts with existing constraints")
        );
        assert_eq!(
            ed.doc
                .iter()
                .map(|(id, e)| (id, e.kind))
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn an_old_conflict_next_to_an_arc_does_not_lock_a_drawing_with_walls() {
        let mut doc = Document::new();
        doc.begin();
        wall(&mut doc, v(5000.0, 0.0), v(9000.0, 0.0));
        let a = doc.add_point(v(0.0, 0.0));
        let f = doc.add_point(v(30.0, 0.0));
        let b = doc.add_point(v(12.0, 3.0));
        for (point, at) in [(a, v(0.0, 0.0)), (f, v(30.0, 0.0))] {
            doc.add_constraint(Constraint::Fix { point, at }).unwrap();
        }
        doc.add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        doc.add_constraint(Constraint::Distance {
            a: b,
            b: f,
            value: 10.0,
        })
        .unwrap();
        let s = doc.add_point(v(17.0, 3.0));
        let e = doc.add_point(v(12.0, 8.0));
        let arc = doc.add_arc(b, s, e).unwrap();
        doc.commit("Drawing");
        let mut ed = Editor {
            doc,
            ..Default::default()
        };
        ed.sync();
        assert!(ed.doc.has_walls());
        assert_eq!(ed.analysis().exposed, vec![arc]);
        ed.doc.begin();
        let p = ed.doc.add_point(v(100.0, 50.0));
        let q = ed.doc.add_point(v(110.0, 50.0));
        ed.doc.add_line(p, q).unwrap();
        assert!(ed.commit("Line"), "an unrelated edit refused");
        ed.doc.begin();
        assert!(
            ed.drag(&[s], v(1.0, 1.0), &Param::point(s)),
            "a drag refused"
        );
        assert!(ed.commit("Move"));
    }
}

/// #342's census: a relation whose solve stops at the iteration cap short of meeting the
/// rows of the arcs it moves (named nothing, but newly put them in a failed part). One more
/// solve from where it stopped meets everything, so the relation is kept, met, with every
/// arc's end on its circle, as it was on main (where the commit's own solve finished it).
mod a_stalled_relation {
    use drift_core::{Constraint, DVec2};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// `points`, the `arcs` (centre, start, end) and `lines` through them at `scale` about
    /// `at`, committed; then `relation` (given the curves) added.
    fn kept(
        points: &[DVec2],
        arcs: &[[usize; 3]],
        lines: &[[usize; 2]],
        relation: impl Fn(&[EntityId], &[EntityId]) -> Constraint,
    ) {
        for (scale, at) in [
            (1.0, DVec2::ZERO),
            (10.0, DVec2::ZERO),
            (0.1, DVec2::ZERO),
            (1.0, v(1e5, 1e5)),
        ] {
            let mut ed = Editor::default();
            ed.doc.begin();
            let p: Vec<EntityId> = points
                .iter()
                .map(|q| ed.doc.add_point(at + *q * scale))
                .collect();
            let a: Vec<EntityId> = arcs
                .iter()
                .map(|&[c, s, e]| ed.doc.add_arc(p[c], p[s], p[e]).unwrap())
                .collect();
            let l: Vec<EntityId> = lines
                .iter()
                .map(|&[x, y]| ed.doc.add_line(p[x], p[y]).unwrap())
                .collect();
            assert!(ed.commit("Sketch"));
            let c = relation(&p, &l);
            assert!(
                ed.add_constraints("Relation", &[c]).is_ok(),
                "{scale}× about {at}: refused"
            );
            assert!(ed.analysis().conflicts.is_empty() && ed.analysis().converged);
            assert!(drift_solver::unmet(&ed.doc, 1e-9).is_empty());
            for &arc in &a {
                let EntityKind::Arc { center, start, end } = ed.doc.get(arc).unwrap().kind else {
                    unreachable!()
                };
                let pos = |q| ed.doc.point(q).unwrap();
                let (r0, r1) = (
                    pos(center).distance(pos(start)),
                    pos(center).distance(pos(end)),
                );
                // (Within the solver's bar: a billionth of a quarter of the part's size, so
                // within a billionth of its largest coordinate.)
                let size = [center, start, end]
                    .iter()
                    .map(|&q| pos(q).abs().max_element())
                    .fold(1.0, f64::max);
                assert!(
                    (r0 - r1).abs() <= 1e-9 * size,
                    "{scale}× about {at}: an arc's ends {r0} and {r1} from its centre"
                );
            }
        }
    }

    /// Census case 292: a line from an arc's centre made upright.
    #[test]
    fn an_upright_line_from_an_arcs_centre_is_kept() {
        kept(
            &[
                v(0.0, 0.0),
                v(6.92820323027551, 3.9999999999999996),
                v(4.857650869455343, -3.727406610312547),
                v(-0.14234913054466158, -12.387660648156933),
                v(-0.14234913054466158, -20.38766064815693),
                v(-0.14234913054466306, -28.38766064815693),
                v(-14.285000205320912, 25.213203435596427),
            ],
            &[[1, 0, 2], [4, 3, 5]],
            &[[2, 3], [1, 6]],
            |_, l| Constraint::Vertical { line: l[1] },
        );
    }

    /// Census case 332: an arc's centre put on another arc's end.
    #[test]
    fn an_arcs_centre_put_on_anothers_end_is_kept() {
        kept(
            &[
                v(0.0, 0.0),
                v(-6.000000000000005, -10.392304845413262),
                v(-9.10582854123025, 1.198805070055558),
                v(-25.31851652578137, -15.568685747463679),
                v(-39.46065214951232, -1.4265501237327278),
            ],
            &[[1, 0, 2], [3, 1, 4]],
            &[],
            |p, _| Constraint::Coincident { a: p[3], b: p[2] },
        );
    }
}

mod drag_size {
    use drift_core::{Constraint, DVec2};
    use drift_solver::Param;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A 10 mm line kept to its length, one end held level (`FixY`), the other dragged 40
    /// frames away from the origin, from 16 to about 20 times its size out: across where
    /// #377's solves stopped being judged by the part's size (17.6 part-sizes; gone since PR
    /// 2 of the solver audit) every frame goes and lands where the same drag does at the
    /// origin, to 1e-9 of a millimetre. (A guard.)
    #[test]
    fn a_drag_across_the_hand_over_moves_on_continuously() {
        let run = |out: f64| {
            let mut ed = Editor::default();
            ed.doc.begin();
            let a = ed.doc.add_point(v(out, out));
            let b = ed.doc.add_point(v(out + 6.0, out + 8.0));
            ed.doc.add_line(a, b).unwrap();
            ed.doc
                .add_constraint(Constraint::Distance { a, b, value: 10.0 })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::FixY { point: a, y: out })
                .unwrap();
            assert!(ed.commit("Sketch"));
            ed.doc.begin();
            let dragged = Param::point(b).to_vec();
            (1..=40)
                .map(|k| {
                    assert!(
                        ed.drag(&[b], v(1.0, -0.05), &dragged),
                        "{out}: frame {k} refused"
                    );
                    [ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()].map(|p| p - v(out, out))
                })
                .collect::<Vec<_>>()
        };
        let (near, across) = (run(0.0), run(150.0));
        for (k, (p, q)) in near.iter().zip(&across).enumerate() {
            for (p, q) in p.iter().zip(q) {
                assert!(
                    p.distance(*q) <= 1e-9,
                    "frame {}: {p} at the origin, {q} out",
                    k + 1
                );
            }
        }
    }

    /// "Stop, as now" at any size: a fully held triangle (three lengths, a corner fixed, a
    /// side level) can't follow a drag of its free corner: no frame moves anything, and the
    /// frames go or are refused alike at 0.1×, 1× and 10× and shifted near the origin. A solve
    /// whose rows can't hold is never read as one that landed. (A guard.)
    #[test]
    fn a_held_shape_refuses_a_drag_alike_at_every_size() {
        let mut patterns = Vec::new();
        for (scale, at) in [
            (1.0, DVec2::ZERO),
            (0.1, DVec2::ZERO),
            (10.0, DVec2::ZERO),
            (1.0, v(40.0, -40.0)),
        ] {
            let mut ed = Editor::default();
            ed.doc.begin();
            let p = [v(0.0, 0.0), v(30.0, 0.0), v(12.0, 20.0)]
                .map(|q| ed.doc.add_point(at + q * scale));
            let ab = ed.doc.add_line(p[0], p[1]).unwrap();
            ed.doc.add_line(p[1], p[2]).unwrap();
            ed.doc.add_line(p[2], p[0]).unwrap();
            for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                let value = ed
                    .doc
                    .point(p[i])
                    .unwrap()
                    .distance(ed.doc.point(p[j]).unwrap());
                ed.doc
                    .add_constraint(Constraint::Distance {
                        a: p[i],
                        b: p[j],
                        value,
                    })
                    .unwrap();
            }
            ed.doc
                .add_constraint(Constraint::Fix { point: p[0], at })
                .unwrap();
            ed.doc
                .add_constraint(Constraint::Horizontal { line: ab })
                .unwrap();
            assert!(ed.commit("Sketch"));
            ed.doc.begin();
            let dragged = Param::point(p[2]).to_vec();
            let before: Vec<DVec2> = p.iter().map(|q| ed.doc.point(*q).unwrap()).collect();
            let mut went = Vec::new();
            for k in 1..=5 {
                went.push(ed.drag(&[p[2]], v(3.0, 2.0) * scale, &dragged));
                let now: Vec<DVec2> = p.iter().map(|q| ed.doc.point(*q).unwrap()).collect();
                for (q, r) in now.iter().zip(&before) {
                    assert!(
                        q.distance(*r) <= 1e-9 * scale,
                        "{scale}× about {at}, frame {k} moved"
                    );
                }
            }
            patterns.push(went);
        }
        assert!(patterns.iter().all(|w| *w == patterns[0]), "{patterns:?}");
    }
}
