//! The Layers panel's rows as in creative apps: a layer's stroke and fill swatches open the
//! colour picker at once, its line cell opens weights and line types, several rows picked
//! change together, and the filter is always there.

use drift_core::{LayerId, LineWeight, Rgb};
use eframe::egui::accesskit::Toggled;
use egui_kittest::kittest::NodeT as _;

use super::*;

/// Three layers, A-WALL, A-DOOR and A-GLAZ, over Layer 1; the Layers panel showing.
fn three(h: &mut Harness<'_, DriftApp>) -> [LayerId; 3] {
    let ids = {
        let ed = &mut h.state_mut().drawing.editor;
        ["A-WALL", "A-DOOR", "A-GLAZ"].map(|n| ed.add_layer(n))
    };
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    h.run();
    ids
}

/// The centre of the control labelled `label` (each names its layer).
fn control(h: &Harness<'_, DriftApp>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

/// The control labelled `label` on the row of the layer named `name`.
fn on_row(h: &Harness<'_, DriftApp>, name: &str, label: &str) -> Pos2 {
    let row = h.get_by_label(name).rect();
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .find(|r| (r.center().y - row.center().y).abs() < 6.0)
        .unwrap_or_else(|| panic!("{label} on {name}"))
        .center()
}

fn click(h: &mut Harness<'_, DriftApp>, at: Pos2, modifiers: EguiModifiers) {
    h.hover_at(at);
    h.run();
    h.event(Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
        h.run();
    }
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
}

fn layer(h: &Harness<'_, DriftApp>, id: LayerId) -> drift_core::Layer {
    h.state().drawing.editor.doc.layer(id).cloned().unwrap()
}

#[test]
fn a_layers_swatch_opens_the_picker_and_one_click_sets_its_colour() {
    let Some(mut h) = harness() else { return };
    let [wall, ..] = three(&mut h);
    assert_eq!(layer(&h, wall).color, None, "Ink to start");
    // One click on the stroke swatch: the picker, with Ink first and the palette.
    let at = control(&h, "A-WALL stroke color");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(
        h.query_all_by_label("Ink").next().is_some(),
        "the Ink swatch"
    );
    save_png("drift_ui_layer_colour_picker.png", &mut h);
    h.get_by_label("#0090FF").click();
    h.run();
    h.run();
    assert_eq!(layer(&h, wall).color, Some(Rgb([0x00, 0x90, 0xff])));
    assert!(
        h.query_all_by_label("Ink").next().is_none(),
        "a swatch click closes the picker"
    );
    // The fill follows the stroke until it is given its own.
    assert_eq!(layer(&h, wall).fill_color, None);
    let at = control(&h, "A-WALL fill color");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(h.query_all_by_label("Match stroke").next().is_some());
    // The colour just picked is offered again among the recent ones (and the drawing's).
    assert!(h.query_all_by_label("#0090FF").count() >= 1);
    h.get_by_label("#FFC53D").click();
    h.run();
    h.run();
    assert_eq!(layer(&h, wall).fill_color, Some(Rgb([0xff, 0xc5, 0x3d])));
    // Back to Ink, from the stroke's picker.
    let at = control(&h, "A-WALL stroke color");
    click(&mut h, at, EguiModifiers::NONE);
    h.get_all_by_label("Ink").next().unwrap().click();
    h.run();
    h.run();
    assert_eq!(layer(&h, wall).color, None);
}

#[test]
fn several_layers_picked_take_a_new_weight_together_in_one_step() {
    let Some(mut h) = harness() else { return };
    let [wall, door, glaz] = three(&mut h);
    let before = h.state().drawing.editor.history.undo_depth();
    // A-WALL clicked (current), then Ctrl-click A-GLAZ: both picked, A-DOOR not.
    let at = layer_row_gap(&h, "A-WALL");
    click(&mut h, at, EguiModifiers::NONE);
    assert_eq!(h.state().drawing.editor.doc.current_layer(), wall);
    let at = layer_row_gap(&h, "A-GLAZ");
    click(&mut h, at, EguiModifiers::COMMAND);
    let picked = &h.state().drawing.layer_list.picked;
    assert!(picked.contains(&wall) && picked.contains(&glaz) && !picked.contains(&door));
    assert_eq!(
        h.state().drawing.editor.doc.current_layer(),
        wall,
        "Ctrl-click doesn't change the current layer"
    );
    let depth = h.state().drawing.editor.history.undo_depth();
    // A-GLAZ's line cell: 0.50 mm. Both picked layers take it, A-DOOR doesn't.
    let at = control(&h, "A-GLAZ line weight and type");
    click(&mut h, at, EguiModifiers::NONE);
    save_png("drift_ui_layer_line_menu.png", &mut h);
    h.get_by_label("0.50 mm").click();
    h.run();
    h.run();
    let half = LineWeight(50);
    assert_eq!(layer(&h, wall).weight, half);
    assert_eq!(layer(&h, glaz).weight, half);
    assert_ne!(layer(&h, door).weight, half);
    assert_eq!(
        h.state().drawing.editor.history.undo_depth(),
        depth + 1,
        "one undo step"
    );
    assert!(depth >= before);
    // A plain click lets the picked rows go.
    let at = layer_row_gap(&h, "A-DOOR");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(h.state().drawing.layer_list.picked.is_empty());
    assert_eq!(h.state().drawing.editor.doc.current_layer(), door);
    // Shift-click picks the run of rows from the last one clicked (top down: A-GLAZ,
    // A-DOOR, A-WALL).
    let at = layer_row_gap(&h, "A-GLAZ");
    click(&mut h, at, EguiModifiers::SHIFT);
    let picked = &h.state().drawing.layer_list.picked;
    assert_eq!(picked.len(), 2);
    assert!(picked.contains(&door) && picked.contains(&glaz));
}

#[test]
fn printing_is_a_switch_on_the_row_and_the_filter_is_always_there() {
    let Some(mut h) = harness() else { return };
    let [wall, ..] = three(&mut h);
    // Four layers: the filter shows all the same.
    let filter = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .count();
    assert!(filter >= 1, "the filter field");
    let at = on_row(&h, "A-WALL", PRINT);
    // The switch says whether it is on.
    let pressed = |h: &Harness<'_, DriftApp>| {
        let row = h.get_by_label("A-WALL").rect();
        h.query_all_by_label(PRINT)
            .find(|n| (n.rect().center().y - row.center().y).abs() < 6.0)
            .expect("the print switch")
            .accesskit_node()
            .toggled()
    };
    assert_eq!(pressed(&h), Some(Toggled::True), "printed: said as on");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(!layer(&h, wall).printable);
    assert_eq!(
        pressed(&h),
        Some(Toggled::False),
        "not printed: said as off"
    );
    click(&mut h, at, EguiModifiers::NONE);
    assert!(layer(&h, wall).printable);
    assert_eq!(pressed(&h), Some(Toggled::True));
}

/// The print switch's label (and tooltip).
const PRINT: &str = "Printed and exported (off: shown here, left out of prints and exports)";
/// The eye's, on a layer not isolated.
const EYE: &str = "Show or hide · Alt-click: show this layer alone";

/// Tooltips showing now.
fn tooltips(h: &Harness<'_, DriftApp>) -> usize {
    h.ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .iter()
            .filter(|l| l.order == egui::Order::Tooltip)
            .count()
    })
}

/// Hold the pointer still at `at` long enough for a tooltip.
fn hold(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    for _ in 0..30 {
        h.step();
    }
}

#[test]
fn each_control_on_a_layer_row_shows_one_tooltip_its_own() {
    let Some(mut h) = harness() else { return };
    three(&mut h);
    // On the row but on none of its controls: no tooltip (the row has none of its own).
    let gap = layer_row_gap(&h, "A-WALL");
    hold(&mut h, gap);
    assert_eq!(tooltips(&h), 0, "no tooltip of the row's own");
    let controls = [
        ("print", on_row(&h, "A-WALL", PRINT)),
        ("snap", on_row(&h, "A-WALL", super::layer_toggles::SNAP)),
        ("eye", on_row(&h, "A-WALL", EYE)),
        ("lock", on_row(&h, "A-WALL", super::layer_toggles::LOCK)),
        ("stroke", control(&h, "A-WALL stroke color")),
        ("fill", control(&h, "A-WALL fill color")),
        ("line", control(&h, "A-WALL line weight and type")),
    ];
    let switches = h.query_all_by_label(PRINT).count();
    for (what, at) in controls {
        hold(&mut h, at);
        assert_eq!(tooltips(&h), 1, "one tooltip over the {what} control");
    }
    // Over the print switch, the one tooltip is the switch's own.
    hold(&mut h, controls[0].1);
    assert_eq!(
        h.query_all_by_label(PRINT).count(),
        switches + 1,
        "the rows' switches and the one tooltip"
    );
}

#[test]
fn a_pick_that_matches_the_clicked_row_still_goes_to_every_picked_row() {
    let Some(mut h) = harness() else { return };
    let [wall, _, glaz] = three(&mut h);
    // A-GLAZ red at 0.50 mm; A-WALL Ink at its own weight.
    {
        let ed = &mut h.state_mut().drawing.editor;
        let mut l = ed.doc.layer(glaz).cloned().unwrap();
        l.color = Some(Rgb([0xe5, 0x48, 0x4d]));
        l.weight = LineWeight(50);
        ed.update_layer(glaz, l);
    }
    h.run();
    let weight = layer(&h, wall).weight;
    assert_ne!(weight, LineWeight(50));
    assert_eq!(layer(&h, wall).color, None);
    let at = layer_row_gap(&h, "A-WALL");
    click(&mut h, at, EguiModifiers::NONE);
    let at = layer_row_gap(&h, "A-GLAZ");
    click(&mut h, at, EguiModifiers::COMMAND);
    // Ink, picked from A-WALL's swatch (A-WALL is Ink already): A-GLAZ takes it.
    let at = control(&h, "A-WALL stroke color");
    click(&mut h, at, EguiModifiers::NONE);
    h.get_all_by_label("Ink").next().unwrap().click();
    h.run();
    h.run();
    assert_eq!(layer(&h, glaz).color, None);
    // A-WALL's own weight from its line menu: A-GLAZ takes it, in one undo step.
    let depth = h.state().drawing.editor.history.undo_depth();
    let at = control(&h, "A-WALL line weight and type");
    click(&mut h, at, EguiModifiers::NONE);
    h.get_by_label(&weight.label()).click();
    h.run();
    h.run();
    assert_eq!(layer(&h, glaz).weight, weight);
    assert_eq!(layer(&h, wall).weight, weight);
    assert_eq!(h.state().drawing.editor.history.undo_depth(), depth + 1);
}

#[test]
fn rows_the_filter_hides_are_let_go_and_take_no_change() {
    let Some(mut h) = harness() else { return };
    let [wall, door, glaz] = three(&mut h);
    let at = layer_row_gap(&h, "A-WALL");
    click(&mut h, at, EguiModifiers::NONE);
    let at = layer_row_gap(&h, "A-DOOR");
    click(&mut h, at, EguiModifiers::COMMAND);
    assert!(h.state().drawing.layer_list.picked.contains(&door));
    // Filtered to A-GLAZ: A-DOOR is out of sight and no longer picked; A-WALL, current,
    // stays in sight above the list.
    h.state_mut().drawing.layer_list.filter = "GLAZ".into();
    h.run();
    h.run();
    assert!(!h.state().drawing.layer_list.picked.contains(&door));
    let at = control(&h, "A-WALL stroke color");
    click(&mut h, at, EguiModifiers::NONE);
    h.get_by_label("#0090FF").click();
    h.run();
    h.run();
    assert_eq!(layer(&h, wall).color, Some(Rgb([0x00, 0x90, 0xff])));
    assert_eq!(
        layer(&h, door).color,
        None,
        "the hidden row is left as it was"
    );
    // Shift-click from a row the filter hides (A-DOOR, the last clicked) picks from the row
    // clicked.
    let at = layer_row_gap(&h, "A-GLAZ");
    click(&mut h, at, EguiModifiers::SHIFT);
    let picked = &h.state().drawing.layer_list.picked;
    assert_eq!(picked.iter().copied().collect::<Vec<_>>(), vec![glaz]);
}

#[test]
fn every_colour_picker_offers_the_open_drawings_colours() {
    let Some(mut h) = harness() else { return };
    let id = h.state_mut().drawing.editor.add_layer("A-WALL");
    // With no picker open, edits (a drag's every frame) make no pass over the drawing for
    // its colours, the Layers panel shown or not.
    if h.state().layout.is_shown(Panel::Layers) {
        h.state_mut().toggle_panel(Panel::Layers);
    }
    h.run();
    let scans = h.state().drawing.document_colours.scans;
    for g in 1..=5u8 {
        let ed = &mut h.state_mut().drawing.editor;
        let mut l = ed.doc.layer(id).cloned().unwrap();
        l.color = Some(Rgb([0x12, g, 0x56]));
        ed.update_layer(id, l);
        h.run();
    }
    assert_eq!(h.state().drawing.document_colours.scans, scans);
    let offered = |h: &Harness<'_, DriftApp>| -> crate::colour::Offered {
        h.ctx
            .data(|d| d.get_temp(crate::colour::document_colours_id()))
            .unwrap_or_default()
    };
    assert!(!offered(&h).fresh, "marked stale for a picker to ask");
    // A picker opened: the drawing's colours as they are now, worked out once.
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    h.run();
    let at = control(&h, "A-WALL stroke color");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(offered(&h).fresh);
    assert!(h.query_all_by_label("#120556").next().is_some(), "offered");
    assert_eq!(h.state().drawing.document_colours.scans, scans + 1);
    // A drag on the canvas with the picker still open (a drag is not a click, so it stays):
    // geometry moving can't change a colour, so no pass over the drawing and no pass of the
    // app run twice, frame after frame.
    let p = h
        .state_mut()
        .drawing
        .editor
        .doc
        .add_point(DVec2::new(0.0, 0.0));
    h.run();
    h.run();
    let scans = h.state().drawing.document_colours.scans;
    let reruns =
        |h: &Harness<'_, DriftApp>| h.ctx.cumulative_pass_nr() - h.ctx.cumulative_frame_nr();
    let before = reruns(&h);
    let revision = h.state().drawing.editor.doc.revision();
    h.state_mut().drawing.editor.doc.begin();
    for i in 1..=5 {
        let doc = &mut h.state_mut().drawing.editor.doc;
        doc.set_point(p, DVec2::new(f64::from(i), 0.0)).unwrap();
        h.step();
    }
    assert_ne!(
        h.state().drawing.editor.doc.revision(),
        revision,
        "it moved"
    );
    assert!(
        h.query_all_by_label("#120556").next().is_some(),
        "still open"
    );
    assert_eq!(h.state().drawing.document_colours.scans, scans);
    assert_eq!(reruns(&h), before, "no pass run again");
    h.state_mut().drawing.editor.doc.rollback();
    // Changed by undo while the picker is open, the list follows.
    h.state_mut().undo();
    h.run();
    h.run();
    assert!(h.query_all_by_label("#120556").next().is_none());
    assert!(h.query_all_by_label("#120456").next().is_some());
}
