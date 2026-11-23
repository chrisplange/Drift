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
    // (A scene about the message keeps it.)
    if !scene.starts_with("toast") {
        h.state_mut().status = None;
    }
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
        h.run();
        h.state_mut().show_newest_sheet();
        whole_view(h);
        h.event(Event::PointerGone);
        h.run();
    }),
    // More sheets than the top bar has room for: the rest go under "More sheets".
    ("many-sheets", |h| {
        open_example(h);
        for _ in 0..9 {
            h.state_mut().new_sheet_shown();
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
    // The rail as a list, with each tool's name and key.
    ("rail-names", |h| {
        open_example(h);
        h.key_press(Key::L);
        h.run();
        h.get_by_label("Show tool names").click();
        h.run();
        h.event(Event::PointerGone);
        h.run();
    }),
    // A family's flyout, open beside the rail.
    ("rail-flyout", |h| {
        open_example(h);
        let tile = rail_tile(h, "Rectangle");
        h.hover_at(tile.center());
        h.run();
        h.event(Event::PointerButton {
            pos: tile.center(),
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: EguiModifiers::NONE,
        });
        h.event(Event::PointerButton {
            pos: tile.center(),
            button: PointerButton::Secondary,
            pressed: false,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
        let polygon = h
            .get_all_by_label("Polygon")
            .map(|n| n.rect())
            .find(|r| r.left() > tile.right())
            .expect("Polygon in the flyout");
        h.hover_at(polygon.center());
        h.run();
    }),
    // A tile's tooltip, beside the rail: the pointer resting on the arcs' tile.
    ("rail-tooltip", |h| {
        open_example(h);
        h.key_press(Key::R);
        h.run();
        let tile = rail_tile(h, "3-point arc");
        h.hover_at(tile.center());
        for _ in 0..6 {
            h.step();
        }
    }),
    // The quick-constrain bar: relations and a dimension for two lines, a dimension alone
    // for a circle, and a message keeping clear of it.
    ("quick-bar-lines", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        h.run();
    }),
    ("quick-bar-circle", |h| {
        open_example(h);
        let round = first(h, |k| matches!(k, EntityKind::Circle { .. }));
        h.state_mut().drawing.editor.selection = round.into_iter().take(1).collect();
        h.run();
    }),
    ("quick-bar-hover", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        h.run();
        h.get_by_label("Constrain: Parallel").hover();
        // (Long enough for its tip.)
        for _ in 0..8 {
            h.step();
        }
    }),
    ("toast-quick-bar", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        h.run();
        let now = h.ctx.input(|i| i.time);
        h.state_mut()
            .set_status("Parallel: the lines already are".into(), true, now);
    }),
    ("toast", |h| {
        open_example(h);
        let now = h.ctx.input(|i| i.time);
        h.state_mut()
            .set_status("Saved as Mounting plate.drift".into(), false, now);
    }),
    ("start-recents", |h| {
        h.state_mut().recent = [
            "C:/Projects/Mill Lane/Ground floor plan.drift",
            "C:/Projects/Mill Lane/Details/Stair section with a long name that is cut short.drift",
            "C:/Projects/Brackets/Mounting plate.drift",
            "D:/Archive/2025/Workshop bench.drift",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
        h.state_mut().show_start = true;
    }),
    ("empty-hint", |h| {
        h.event(Event::PointerGone);
        h.run();
    }),
    ("pie", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(2).collect();
        h.run();
        let at = h.state().drawing.view.canvas_rect.center();
        h.hover_at(at);
        h.run();
        h.key_press(Key::Q);
        h.run();
        h.hover_at(at + egui::vec2(60.0, -60.0));
        h.run();
    }),
    // The pie pointing at a relation that fits and holds nothing back: the accent's look.
    ("pie-add", |h| {
        open_example(h);
        let lines = first(h, |k| matches!(k, EntityKind::Line { .. }));
        h.state_mut().drawing.editor.selection = lines.into_iter().take(1).collect();
        h.run();
        let at = h.state().drawing.view.canvas_rect.center();
        h.hover_at(at);
        h.run();
        h.key_press(Key::Q);
        h.run();
        h.hover_at(at + egui::vec2(-90.0, 60.0));
        h.run();
    }),
    // The import dialog: its checkboxes and radio buttons.
    ("import", |h| {
        open_example(h);
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="60mm" viewBox="0 0 100 60"><rect x="10" y="10" width="60" height="30" fill="#888"/><line x1="0" y1="0" x2="100" y2="60" stroke="black"/></svg>"##;
        h.state_mut().begin_import(
            "plan.svg",
            svg.as_bytes().to_vec(),
            Some(crate::app::import_ui::ImportAs::Geometry),
            0.0,
        );
        h.run_steps(3);
    }),
    // The angle-snap menu of the status bar.
    ("angle-snap", |h| {
        open_example(h);
        h.get_by_label("Angle snap").click();
        h.run();
    }),
    // The Wall tool's options, and the pointer over the canvas (its position in the status
    // bar).
    ("wall-tool", |h| {
        h.state_mut().set_tool(ToolKind::Wall);
        h.run();
        let at = h.state().drawing.view.canvas_rect.center();
        h.hover_at(at + egui::vec2(-120.0, 80.0));
        h.run();
    }),
    ("hud-length", |h| {
        h.key_press(Key::L);
        h.run();
        let at = h.state().drawing.view.canvas_rect.center();
        h.hover_at(at + egui::vec2(-180.0, 40.0));
        h.run();
        button(h, at + egui::vec2(-180.0, 40.0), true);
        h.run();
        button(h, at + egui::vec2(-180.0, 40.0), false);
        h.run();
        h.hover_at(at + egui::vec2(-20.0, -30.0));
        h.run();
        type_text(h, "120");
        h.hover_at(at + egui::vec2(-20.0, -30.0));
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
        h.state_mut().new_sheet_shown();
        h.state_mut().new_sheet_shown();
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
        h.state_mut().new_sheet_shown();
        whole_view(h);
        h.get_by_label("Add revision").click();
        h.event(Event::PointerGone);
        h.run();
    }),
    ("dock-240-sheet", |h| {
        open_example(h);
        h.state_mut().new_sheet_shown();
        whole_view(h);
        h.get_by_label("Add revision").click();
        narrow_dock(h);
    }),
];

/// The rail's tile named `label` (below the bars, at the left).
fn rail_tile(h: &Harness<'_, DriftApp>, label: &str) -> egui::Rect {
    h.get_all_by_label(label)
        .map(|n| n.rect())
        .find(|r| r.left() < 100.0 && r.top() > crate::theme::bar::TOP + crate::theme::bar::OPTIONS)
        .expect("on the rail")
}

/// [`shot`] as the scene stands: no extra frames (they would move a resting pointer's
/// tooltip on), and nothing cleared.
fn shot_now(scene: &str, name: &str, dark: bool, h: &mut Harness<'_, DriftApp>) {
    if !scene.starts_with("rail-tooltip") {
        return shot(scene, name, dark, h);
    }
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/fresh");
    std::fs::create_dir_all(&dir).expect("create target/tmp/fresh");
    let look = if dark { "dark" } else { "light" };
    let path = dir.join(format!("{scene}_{name}_{look}.png"));
    image.save(&path).expect("save png");
    eprintln!("wrote {}", path.display());
}

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
            shot_now(scene, "1280x800", dark, &mut h);
        }
        // A short window: the slim rail takes two columns; with names, shorter rows.
        if matches!(*scene, "selection" | "rail-names" | "rail-flyout" | "sheet") {
            let mut h = sized([1280.0, 700.0], 1.0, true);
            set_up(&mut h);
            shot_now(scene, "1280x700", true, &mut h);
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
