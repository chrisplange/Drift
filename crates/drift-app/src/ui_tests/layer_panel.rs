//! The Layers panel's rows as in creative apps: a layer's stroke and fill swatches open the
//! colour picker at once, its line cell opens weights and line types, several rows picked
//! change together, and the filter is always there.

use drift_core::{LayerId, LineWeight, Rgb};

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
    let at = on_row(&h, "A-WALL", "Stroke color");
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
    let at = on_row(&h, "A-WALL", "Fill color");
    click(&mut h, at, EguiModifiers::NONE);
    assert!(h.query_all_by_label("Match stroke").next().is_some());
    // The colour just picked is offered again among the recent ones (and the drawing's).
    assert!(h.query_all_by_label("#0090FF").count() >= 1);
    h.get_by_label("#FFC53D").click();
    h.run();
    h.run();
    assert_eq!(layer(&h, wall).fill_color, Some(Rgb([0xff, 0xc5, 0x3d])));
    // Back to Ink, from the stroke's picker.
    let at = on_row(&h, "A-WALL", "Stroke color");
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
    let at = on_row(&h, "A-GLAZ", "Line weight and type");
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
fn printing_is_a_toggle_on_the_row_and_the_filter_is_always_there() {
    let Some(mut h) = harness() else { return };
    let [wall, ..] = three(&mut h);
    // Four layers: the filter shows all the same.
    let filter = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .count();
    assert!(filter >= 1, "the filter field");
    let tip = "Printed and exported (off: shown here, left out of prints and exports)";
    let at = on_row(&h, "A-WALL", tip);
    click(&mut h, at, EguiModifiers::NONE);
    assert!(!layer(&h, wall).printable);
    click(&mut h, at, EguiModifiers::NONE);
    assert!(layer(&h, wall).printable);
}
