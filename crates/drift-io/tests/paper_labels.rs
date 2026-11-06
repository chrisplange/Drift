//! Dimension label offsets are kept in paper mm (44.2): read back from `paper_label` when the
//! world `label` beside it agrees, else from `label` at the scale the sheets print it at, so a
//! drawing from before (or one an older build has since edited) looks as it did on its
//! sheets.

use drift_core::sheets::{Sheet, View, Viewport};
use drift_core::{Bounds, Constraint, ConstraintId, DVec2, Document, DraftStyle};
use drift_io::{Metadata, load, save};

/// A 4 m line, a dimension of it, and the drawing at 1 : `n`.
fn drawing(n: f64) -> (Document, ConstraintId) {
    let mut doc = Document::new();
    let a = doc.add_point(DVec2::new(0.0, 0.0));
    let b = doc.add_point(DVec2::new(4000.0, 0.0));
    doc.add_line(a, b).unwrap();
    let id = doc
        .add_constraint(Constraint::Distance {
            a,
            b,
            value: 4000.0,
        })
        .unwrap();
    doc.set_draft_style(DraftStyle {
        scale: n,
        ..doc.draft_style()
    });
    (doc, id)
}

/// A sheet showing a view of `crop` at 1 : `scale`; returns the view.
fn sheet_with_view(doc: &mut Document, crop: Bounds, scale: f64) -> drift_core::ViewId {
    let view = doc.add_view(View::new("Plan", crop, scale));
    let mut sheet = Sheet::new("A-101");
    sheet
        .viewports
        .push(Viewport::new(view, DVec2::new(200.0, 150.0)));
    doc.add_sheet(sheet);
    view
}

fn around_the_line() -> Bounds {
    Bounds {
        min: DVec2::new(-500.0, -1000.0),
        max: DVec2::new(4500.0, 1000.0),
    }
}

/// The file as a build from before 44.2 writes it: the dimension's `label` in world mm, no
/// `paper_label`.
fn as_older_build(doc: &Document, id: ConstraintId, world: DVec2) -> String {
    let text = save(doc, &Metadata::default());
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    for c in json["constraints"].as_array_mut().unwrap() {
        if c["id"] == id.to_string() {
            let r = c.as_object_mut().unwrap();
            r.remove("paper_label");
            r.insert("label".into(), serde_json::json!([world.x, world.y]));
        }
    }
    serde_json::to_string_pretty(&json).unwrap()
}

fn near(a: DVec2, b: DVec2) -> bool {
    (a - b).abs().max_element() < 1e-9
}

#[test]
fn a_saved_label_reads_back_on_paper_and_in_the_model() {
    let (mut doc, id) = drawing(50.0);
    doc.set_placement(id, DVec2::new(0.0, -8.0)).unwrap();
    let text = save(&doc, &Metadata::default());
    // Both: the world offset older builds read, and the paper one.
    assert!(
        text.contains(r#""label":[0.0,-400.0],"paper_label":[0.0,-8.0]"#),
        "{text}"
    );
    let (back, _) = load(&text).unwrap();
    assert_eq!(back.placement(id), Some(DVec2::new(0.0, -8.0)));
    assert_eq!(back.model_placement(id), Some(DVec2::new(0.0, -400.0)));
}

#[test]
fn an_old_label_is_read_at_the_scale_its_sheet_prints_it() {
    // The drawing left at 1:1 (as most are) and shown on a 1:50 sheet, its label set there
    // 500 mm off in the model: 10 mm on that sheet, as before.
    let (mut doc, id) = drawing(1.0);
    sheet_with_view(&mut doc, around_the_line(), 50.0);
    let (back, _) = load(&as_older_build(&doc, id, DVec2::new(0.0, -500.0))).unwrap();
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -10.0)));
}

#[test]
fn an_old_label_with_no_sheet_is_read_at_the_drawing_scale() {
    let (doc, id) = drawing(20.0);
    let (back, _) = load(&as_older_build(&doc, id, DVec2::new(0.0, -500.0))).unwrap();
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -25.0)));
    // A view that doesn't show it (its crop is elsewhere) doesn't count either.
    let (mut doc, id) = drawing(20.0);
    let elsewhere = Bounds {
        min: DVec2::new(10_000.0, 10_000.0),
        max: DVec2::new(12_000.0, 12_000.0),
    };
    sheet_with_view(&mut doc, elsewhere, 100.0);
    let (back, _) = load(&as_older_build(&doc, id, DVec2::new(0.0, -500.0))).unwrap();
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -25.0)));
}

#[test]
fn an_old_label_on_sheets_at_two_scales_takes_the_first_sheets() {
    let (mut doc, id) = drawing(1.0);
    sheet_with_view(&mut doc, around_the_line(), 25.0);
    sheet_with_view(&mut doc, around_the_line(), 100.0);
    let (back, _) = load(&as_older_build(&doc, id, DVec2::new(0.0, -500.0))).unwrap();
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -20.0)));
    // A view that leaves dimensions out doesn't print it: the next one does.
    let (mut doc, id) = drawing(1.0);
    let first = sheet_with_view(&mut doc, around_the_line(), 25.0);
    let mut v = doc.view(first).unwrap().clone();
    v.dimensions = false;
    doc.set_view(first, v);
    sheet_with_view(&mut doc, around_the_line(), 100.0);
    let (back, _) = load(&as_older_build(&doc, id, DVec2::new(0.0, -500.0))).unwrap();
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -5.0)));
}

#[test]
fn a_label_an_older_build_moved_wins_over_the_stale_paper_one() {
    // Saved by this build (label and paper_label agree), then moved by an older build, which
    // rewrote `label` and carried `paper_label` along unchanged.
    let (mut doc, id) = drawing(1.0);
    sheet_with_view(&mut doc, around_the_line(), 50.0);
    doc.set_placement(id, DVec2::new(0.0, -8.0)).unwrap();
    let text = save(&doc, &Metadata::default());
    let moved = text.replace(
        r#""label":[0.0,-8.0],"paper_label":[0.0,-8.0]"#,
        r#""label":[0.0,-1000.0],"paper_label":[0.0,-8.0]"#,
    );
    assert_ne!(moved, text);
    let (back, _) = load(&moved).unwrap();
    // Read as an older label: at its sheet's 1:50.
    assert!(near(back.placement(id).unwrap(), DVec2::new(0.0, -20.0)));
    // Untouched, the paper offset is used as it is.
    let (back, _) = load(&text).unwrap();
    assert_eq!(back.placement(id), Some(DVec2::new(0.0, -8.0)));
}
