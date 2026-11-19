//! Screenshots for judging the look: the main surfaces of the interface at the sizes it has
//! to look right at, saved to `target/tmp/fresh/{scene}_{w}x{h}_{dark|light}.png`. Nothing
//! is asserted about the layout: it only renders. Ignored in normal runs:
//!
//! ```text
//! cargo test -p drift-app --lib fresh_screenshots -- --ignored --nocapture
//! ```

use super::*;
use drift_core::EntityId;

/// The app in a window of `points` at `scale` pixels per point, in the dark or light look.
fn sized(points: [f32; 2], scale: f32, dark: bool) -> Harness<'static, DriftApp> {
    let mut h = Harness::builder()
        .with_size(points)
        .with_pixels_per_point(scale)
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    h.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    h.run();
    h.run();
    h
}

/// Save the window as `{scene}_{name}_{dark|light}.png`, without a status message over the
/// canvas.
fn shot(scene: &str, name: &str, dark: bool, h: &mut Harness<'_, DriftApp>) {
    h.state_mut().status = None;
    h.run();
    h.run();
    // DRIFT_FRESH_WIDE=1264: what reaches past that x (what makes a pane wider than it is).
    if let Some(x) = std::env::var("DRIFT_FRESH_WIDE")
        .ok()
        .and_then(|x| x.parse::<f32>().ok())
    {
        for node in h.query_all_by(|n| n.raw_bounds().is_some_and(|b| b.x1 as f32 > x)) {
            let r = node.rect();
            if r.width() < 330.0 && r.left() > 900.0 {
                eprintln!("  wide: {node:?} {r:?}");
            }
        }
    }
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/fresh");
    std::fs::create_dir_all(&dir).expect("create target/tmp/fresh");
    let look = if dark { "dark" } else { "light" };
    let path = dir.join(format!("{scene}_{name}_{look}.png"));
    image.save(&path).expect("save png");
    eprintln!(
        "wrote {} (canvas to {})",
        path.display(),
        h.state().drawing.view.canvas_rect.right()
    );
}

fn open_example(h: &mut Harness<'_, DriftApp>) {
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    let bounds = h.state().fit_bounds();
    h.state_mut().drawing.view.fit_target = Some(bounds);
    // (The pointer off the window, so nothing shows as hovered.)
    h.event(Event::PointerGone);
    h.run();
    h.run();
}

/// The example's first curve of a kind.
fn first(h: &Harness<'_, DriftApp>, pred: impl Fn(&EntityKind) -> bool) -> Vec<EntityId> {
    h.state()
        .drawing
        .editor
        .doc
        .iter()
        .filter(|(_, e)| pred(&e.kind))
        .map(|(id, _)| id)
        .collect()
}

type Scene = (&'static str, fn(&mut Harness<'_, DriftApp>));

const SCENES: &[Scene] = &[
    ("empty", |_| {}),
    ("start", |h| h.state_mut().show_start = true),
    ("example", open_example),
    ("selection", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        h.run();
    }),
    ("circle", |h| {
        open_example(h);
        let round = first(h, |k| {
            matches!(k, EntityKind::Circle { .. } | EntityKind::Arc { .. })
        });
        h.state_mut().drawing.editor.selection = round.into_iter().take(1).collect();
        h.run();
    }),
    ("line-tool", |h| {
        open_example(h);
        h.key_press(Key::L);
        h.run();
        let at = h.state().drawing.view.canvas_rect.center();
        h.hover_at(at + egui::vec2(-180.0, -140.0));
        h.run();
        button(h, at + egui::vec2(-180.0, -140.0), true);
        h.run();
        button(h, at + egui::vec2(-180.0, -140.0), false);
        h.run();
        h.hover_at(at + egui::vec2(-40.0, -170.0));
        h.run();
    }),
    ("file-menu", |h| {
        open_example(h);
        h.get_by_label("File").click();
        h.run();
    }),
    ("palette", |h| {
        open_example(h);
        h.key_press_modifiers(EguiModifiers::COMMAND, Key::K);
        h.run();
        h.run();
        type_text(h, "fil");
    }),
    ("preferences", |h| {
        open_example(h);
        h.state_mut().show_settings = true;
    }),
    ("sheet", |h| {
        open_example(h);
        h.state_mut().show_panel(Panel::Sheets);
        h.run();
        h.get_by_label("New sheet").click();
        whole_view(h);
        h.event(Event::PointerGone);
        h.run();
    }),
    // More sheets than the top bar has room for: the rest go under "More sheets".
    ("many-sheets", |h| {
        open_example(h);
        for _ in 0..9 {
            h.state_mut().new_sheet();
        }
        h.run();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("layers", |h| {
        open_example(h);
        h.state_mut().show_panel(Panel::Layers);
        h.run();
    }),
    // The panels: each tab with something in it, the Inspector's other kinds, and the dock
    // floating, collapsed and at its narrowest.
    ("parameters", |h| {
        open_example(h);
        parameters(h);
    }),
    ("blocks", |h| {
        open_example(h);
        let first = &drift_edit::library::all()[0];
        h.state_mut().drawing.editor.library_block(first);
        h.state_mut().show_panel(Panel::Blocks);
        h.run();
    }),
    ("sheets-panel", |h| {
        open_example(h);
        h.state_mut().new_sheet();
        h.state_mut().new_sheet();
        h.state_mut().show_panel(Panel::Sheets);
        h.run();
        h.get_by_label("Model").click();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("dimension", |h| {
        open_example(h);
        let app = h.state_mut();
        let dimension = app
            .drawing
            .editor
            .doc
            .constraints()
            .find(|(_, c)| c.is_dimension())
            .map(|(id, _)| id);
        app.drawing.selected_constraint = dimension;
        h.run();
    }),
    ("filled-shape", |h| {
        h.get_by_label("Rectangle").click();
        h.run();
        if !h.state().drawing.editor.prefs.fill_new_shapes {
            h.get_by_label("Fill").click();
            h.run();
        }
        click_world(h, DVec2::new(-30.0, -20.0));
        click_world(h, DVec2::new(30.0, 20.0));
        h.key_press(Key::Escape);
        h.run();
        h.get_by_label("Select").click();
        h.run();
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().collect();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("fill", |h| {
        h.get_by_label("Rectangle").click();
        h.run();
        if !h.state().drawing.editor.prefs.fill_new_shapes {
            h.get_by_label("Fill").click();
            h.run();
        }
        click_world(h, DVec2::new(-30.0, -20.0));
        click_world(h, DVec2::new(30.0, 20.0));
        h.key_press(Key::Escape);
        h.run();
        h.get_by_label("Select").click();
        h.run();
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection.clear();
        ed.selected_fills = ed.doc.fills().map(|(id, _)| id).collect();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("floating-layers", |h| {
        open_example(h);
        h.state_mut()
            .layout
            .move_to(Panel::Layers, Place::Float, None);
        h.run();
    }),
    ("dock-collapsed", |h| {
        open_example(h);
        h.state_mut().layout.collapsed = [true, true];
        h.run();
    }),
    ("dock-240", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(1).collect();
        narrow_dock(h);
    }),
    ("dock-240-many", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        narrow_dock(h);
    }),
    ("dock-240-layers", |h| {
        open_example(h);
        h.state_mut().show_panel(Panel::Layers);
        narrow_dock(h);
    }),
    ("dock-240-parameters", |h| {
        open_example(h);
        parameters(h);
        narrow_dock(h);
    }),
    ("layer-open", |h| {
        open_example(h);
        h.state_mut().show_panel(Panel::Layers);
        let app = h.state_mut();
        let layer = app.drawing.editor.doc.current_layer();
        app.drawing.expanded_layers.insert(layer);
        h.run();
    }),
    ("relations", |h| {
        open_example(h);
        h.get_by_label("Show all").click();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("block-copy", |h| {
        let ed = &mut h.state_mut().drawing.editor;
        let block = ed.library_block(&drift_edit::library::all()[0]);
        ed.place_instance(block, DVec2::ZERO, 0.0, false);
        ed.selected_instances = ed.doc.instances().map(|(id, _)| id).collect();
        h.run();
    }),
    ("text", |h| {
        h.state_mut().set_tool(ToolKind::Text);
        h.run();
        click_world(h, DVec2::new(-20.0, 10.0));
        type_text(h, "Drift");
        h.key_press(Key::Escape);
        h.run();
        h.key_press(Key::Escape);
        h.run();
        let ed = &mut h.state_mut().drawing.editor;
        ed.selected_texts = ed.doc.texts().map(|(id, _)| id).collect();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("wall", |h| {
        h.state_mut().set_tool(ToolKind::Wall);
        h.run();
        click_world(h, DVec2::new(-40.0, 0.0));
        click_world(h, DVec2::new(40.0, 0.0));
        h.key_press(Key::Escape);
        h.run();
        h.key_press(Key::Escape);
        h.run();
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().collect();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("note", note),
    ("dock-240-note", |h| {
        note(h);
        narrow_dock(h);
    }),
    ("dock-240-fill", |h| {
        open_example(h);
        h.get_by_label("Rectangle").click();
        h.run();
        if !h.state().drawing.editor.prefs.fill_new_shapes {
            h.get_by_label("Fill").click();
            h.run();
        }
        click_world(h, DVec2::new(20.0, 20.0));
        click_world(h, DVec2::new(40.0, 30.0));
        h.key_press(Key::Escape);
        h.run();
        h.get_by_label("Select").click();
        h.run();
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection.clear();
        ed.selected_fills = ed.doc.fills().map(|(id, _)| id).collect();
        narrow_dock(h);
    }),
    ("dock-240-dimension", |h| {
        open_example(h);
        let app = h.state_mut();
        let dimension = app
            .drawing
            .editor
            .doc
            .constraints()
            .find(|(_, c)| c.is_dimension())
            .map(|(id, _)| id);
        app.drawing.selected_constraint = dimension;
        narrow_dock(h);
    }),
    ("dock-240-blocks", |h| {
        open_example(h);
        let first = &drift_edit::library::all()[0];
        h.state_mut().drawing.editor.library_block(first);
        h.state_mut().show_panel(Panel::Blocks);
        narrow_dock(h);
    }),
    ("sheet-revision", |h| {
        open_example(h);
        h.state_mut().new_sheet();
        whole_view(h);
        h.get_by_label("Add revision").click();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("dock-240-sheet", |h| {
        open_example(h);
        h.state_mut().new_sheet();
        whole_view(h);
        h.get_by_label("Add revision").click();
        narrow_dock(h);
    }),
];

/// A note with a leader on the example, picked.
fn note(h: &mut Harness<'_, DriftApp>) {
    open_example(h);
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    click_world(h, DVec2::new(40.0, 80.0));
    click_world(h, DVec2::new(25.0, 95.0));
    type_text(h, "DEBURR ALL EDGES");
    h.key_press(Key::Escape);
    h.run();
    h.run();
    let app = h.state_mut();
    app.set_tool(ToolKind::Select);
    let id = app
        .drawing
        .editor
        .doc
        .annotations()
        .map(|(id, _)| id)
        .next();
    app.drawing.editor.selected_annotation = id;
    h.event(Event::PointerGone);
    h.run();
}

/// The Parameters tab with a couple of variables, an option and a pinned measurement.
fn parameters(h: &mut Harness<'_, DriftApp>) {
    let app = h.state_mut();
    let ed = &mut app.drawing.editor;
    let _ = ed.add_variable("width", "120", drift_core::Kind::Length);
    let _ = ed.add_variable("half", "width/2", drift_core::Kind::Length);
    ed.save_option("As drawn", false);
    ed.save_option("Wide", true);
    let dimension = ed
        .doc
        .constraints()
        .find(|(_, c)| c.is_dimension())
        .map(|(_, c)| *c);
    if let Some(c) = dimension {
        let _ = ed.pin_measurement(c);
    }
    app.show_panel(Panel::Parameters);
    h.run();
}

/// Drag the right dock's edge until it is as narrow as it goes (240).
fn narrow_dock(h: &mut Harness<'_, DriftApp>) {
    h.run();
    let canvas = h.state().drawing.view.canvas_rect;
    let from = egui::pos2(canvas.right() + 2.0, canvas.center().y);
    drag_steps(h, from, from + egui::vec2(120.0, 0.0), 4, None);
    h.event(Event::PointerGone);
    h.run();
}

#[test]
#[ignore = "writes screenshots for judging the look; run with --ignored"]
fn fresh_screenshots() {
    if harness().is_none() {
        return;
    }
    // Some scenes alone, when iterating on them: DRIFT_FRESH_SCENE=palette,layers.
    let only = std::env::var("DRIFT_FRESH_SCENE").ok();
    for (scene, set_up) in SCENES {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|o| o == *scene))
        {
            continue;
        }
        for dark in [true, false] {
            let mut h = sized([1280.0, 800.0], 1.0, dark);
            set_up(&mut h);
            shot(scene, "1280x800", dark, &mut h);
        }
        // A small window: the search field is a button, the tabs take what is left.
        if matches!(*scene, "selection" | "many-sheets" | "sheet") {
            let mut h = sized([900.0, 700.0], 1.0, true);
            set_up(&mut h);
            shot(scene, "900x700", true, &mut h);
        }
        if *scene != "selection" {
            continue;
        }
        // The larger windows: the chrome keeps its size in points and the canvas takes the
        // room. The last is a 4K screen at 150 %: 2560 × 1440 points, 3840 × 2160 pixels.
        for (points, scale, name) in [
            ([1920.0, 1080.0], 1.0, "1920x1080"),
            ([3440.0, 1440.0], 1.0, "3440x1440"),
            ([2560.0, 1440.0], 1.5, "3840x2160"),
        ] {
            let mut h = sized(points, scale, true);
            set_up(&mut h);
            shot(scene, name, true, &mut h);
        }
    }
}
