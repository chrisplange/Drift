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
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/fresh");
    std::fs::create_dir_all(&dir).expect("create target/tmp/fresh");
    let look = if dark { "dark" } else { "light" };
    let path = dir.join(format!("{scene}_{name}_{look}.png"));
    image.save(&path).expect("save png");
    eprintln!("wrote {}", path.display());
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

#[test]
#[ignore = "writes screenshots for judging the look; run with --ignored"]
fn fresh_screenshots() {
    if harness().is_none() {
        return;
    }
    // One scene alone, when iterating on it: DRIFT_FRESH_SCENE=palette.
    let only = std::env::var("DRIFT_FRESH_SCENE").ok();
    for (scene, set_up) in SCENES {
        if only.as_deref().is_some_and(|o| o != *scene) {
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
