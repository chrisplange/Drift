//! A visual audit: every surface of the interface in the dark and light themes, saved to
//! `target/tmp/audit/<theme>/`. Ignored in normal runs:
//!
//! ```text
//! cargo test -p drift-app audit_screenshots -- --ignored
//! ```

use super::*;
use drift_core::{Constraint as C, EntityId};

fn themed(dark: bool) -> Harness<'static, DriftApp> {
    let mut h = harness().unwrap();
    h.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    h.run();
    h.run();
    h
}

fn shot(dark: bool, name: &str, h: &mut Harness<'_, DriftApp>) {
    h.run();
    h.run();
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/audit")
        .join(if dark { "dark" } else { "light" });
    std::fs::create_dir_all(&dir).expect("create target/tmp/audit");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save png");
    eprintln!("wrote {}", path.display());
}

fn open_example(h: &mut Harness<'_, DriftApp>) {
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
}

fn fit(h: &mut Harness<'_, DriftApp>) {
    let bounds = h.state().fit_bounds();
    h.state_mut().fit_target = Some(bounds);
    h.run();
    h.run();
}

fn rectangle(h: &mut Harness<'_, DriftApp>, min: DVec2, max: DVec2) -> [EntityId; 4] {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
        .map(|at| ed.doc.add_point(at));
    for i in 0..4 {
        ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
    }
    ed.commit("Rectangle");
    p
}

#[test]
#[ignore = "writes the visual audit's screenshots; run with --ignored"]
fn audit_screenshots() {
    if harness().is_none() {
        return;
    }
    for dark in [true, false] {
        surfaces(dark);
        tools(dark);
        panels(dark);
        menus(dark);
        dialogs(dark);
        own_style(dark);
        canvas_states(dark);
    }
}

fn surfaces(dark: bool) {
    let mut h = themed(dark);
    shot(dark, "01-empty", &mut h);
    h.state_mut().show_start = true;
    shot(dark, "02-start", &mut h);

    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().status = None;
    shot(dark, "03-example", &mut h);
    click_world(&mut h, DVec2::new(84.0, 40.0)); // the bore
    shot(dark, "04-inspector-circle", &mut h);
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(60.0, 0.0));
    shot(dark, "05-inspector-line", &mut h);
}

fn tools(dark: bool) {
    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().status = None;
    for (key, name) in [
        (Key::L, "line"),
        (Key::R, "rectangle"),
        (Key::C, "circle"),
        (Key::D, "dimension"),
        (Key::T, "text"),
        (Key::F, "fillet"),
    ] {
        h.key_press(Key::Escape);
        h.run();
        h.key_press(key);
        h.run();
        h.hover_at(screen(&h, DVec2::new(30.0, 60.0)));
        h.run();
        shot(dark, &format!("10-tool-{name}"), &mut h);
    }
    for label in ["Fill / hatch", "Note", "Section mark"] {
        h.key_press(Key::Escape);
        h.run();
        h.get_by_label(label).click();
        h.run();
        let slug = label.to_lowercase().replace([' ', '/'], "");
        shot(dark, &format!("10-tool-{slug}"), &mut h);
    }
    // A line in progress, with a typed length.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, 70.0));
    h.hover_at(screen(&h, DVec2::new(0.0, 78.0)));
    h.run();
    type_text(&mut h, "50");
    shot(dark, "11-line-typing", &mut h);
}

fn panels(dark: bool) {
    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().status = None;
    for (panel, name) in [
        (Panel::Layers, "layers"),
        (Panel::Sheets, "sheets"),
        (Panel::Parameters, "parameters"),
        (Panel::Blocks, "blocks"),
    ] {
        h.state_mut().show_panel(panel);
        h.run();
        shot(dark, &format!("20-panel-{name}"), &mut h);
    }
    // Some layers and parameters, so the lists aren't empty.
    {
        let ed = &mut h.state_mut().editor;
        let walls = ed.add_layer("Walls");
        ed.add_layer("Furniture");
        ed.add_layer("Survey (reference)");
        let mut l = ed.doc.layer(walls).cloned().unwrap();
        l.color = Some(drift_core::Rgb([0x5e, 0x7f, 0x5c]));
        ed.update_layer(walls, l);
        ed.set_current_layer(walls);
        ed.add_variable("width", "100", drift_core::Kind::Length)
            .unwrap();
        ed.add_variable("gap", "width / 10", drift_core::Kind::Length)
            .unwrap();
        ed.save_variant("Standard");
    }
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    shot(dark, "21-panel-layers-full", &mut h);
    h.state_mut().show_panel(Panel::Parameters);
    h.run();
    shot(dark, "21-panel-parameters-full", &mut h);

    // Floating panels.
    for p in [Panel::Layers, Panel::Parameters] {
        h.state_mut().layout.move_to(p, Place::Float, None);
    }
    h.run();
    h.run();
    shot(dark, "22-floating", &mut h);
    // Both docks collapsed to their rails.
    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().status = None;
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Left), None);
    h.state_mut().layout.collapsed = [true, true];
    h.run();
    shot(dark, "23-collapsed", &mut h);
}

fn menus(dark: bool) {
    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().status = None;
    for menu in ["File", "Edit", "View", "Help"] {
        h.get_by_label(menu).click();
        h.run();
        shot(dark, &format!("30-menu-{}", menu.to_lowercase()), &mut h);
        h.key_press(Key::Escape);
        h.run();
    }
    // The context menu on a picked line.
    let at = screen(&h, DVec2::new(60.0, 0.0));
    click_world(&mut h, DVec2::new(60.0, 0.0));
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: EguiModifiers::NONE,
    });
    h.run();
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: EguiModifiers::NONE,
    });
    h.run();
    shot(dark, "31-context-menu", &mut h);
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();

    // The command palette, empty and filtered.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::K);
    h.run();
    h.run();
    shot(dark, "32-palette", &mut h);
    type_text(&mut h, "dim");
    shot(dark, "33-palette-filtered", &mut h);
    h.key_press(Key::Escape);
    h.run();

    // The relations pie on two lines.
    let mut h = themed(dark);
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [
        DVec2::new(-40.0, -10.0),
        DVec2::new(20.0, -8.0),
        DVec2::new(-40.0, 15.0),
        DVec2::new(25.0, 20.0),
    ]
    .map(|at| ed.doc.add_point(at));
    let a = ed.doc.add_line(p[0], p[1]).unwrap();
    let b = ed.doc.add_line(p[2], p[3]).unwrap();
    ed.commit("Lines");
    h.run();
    let at = h.state().canvas_rect.center();
    h.hover_at(at);
    h.run();
    h.state_mut().editor.selection = [a, b].into();
    h.run();
    h.key_down(Key::Q);
    h.run();
    h.hover_at(at + egui::vec2(60.0, -60.0));
    h.run();
    shot(dark, "34-pie", &mut h);
    h.key_up(Key::Q);
    h.run();
}

fn dialogs(dark: bool) {
    let mut h = themed(dark);
    h.state_mut().show_settings = true;
    shot(dark, "40-preferences", &mut h);
    h.get_by_label("Drawing aids").click();
    shot(dark, "40-preferences-drawing", &mut h);
    h.get_by_label("General").click();
    h.run();
    h.state_mut().show_settings = false;
    h.state_mut().show_about = true;
    shot(dark, "41-about", &mut h);
    h.state_mut().show_about = false;
    h.state_mut().show_shortcuts = true;
    shot(dark, "42-shortcuts", &mut h);
    h.state_mut().show_shortcuts = false;
    h.run();

    let svg = r##"<svg version="1.1" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 720 360">
<g id="Walls"><rect x="72" y="72" width="288" height="144" fill="none" stroke="#000"/></g>
</svg>"##;
    h.state_mut()
        .begin_import("plan.svg", svg.as_bytes().to_vec(), None, 0.0);
    h.run_steps(3);
    shot(dark, "43-import", &mut h);
    h.state_mut().pending_import = None;
    h.run();

    h.state_mut().import_report = Some(crate::app::sheets_ui::ImportSummary {
        title: "site-plan.dwg".into(),
        summary: "Imported 1,204 curves, 38 lines of text, 12 hatches and fills from site-plan.dwg"
            .into(),
        skipped: vec!["4 × 3DSOLID".into(), "2 × OLE2FRAME".into()],
        notes: vec![
            "1 dimension was read as lines and text.".into(),
            "Objects drawn on layouts came in on their own sheets.".into(),
        ],
        far: Vec::new(),
    });
    shot(dark, "44-import-report", &mut h);
    h.state_mut().import_report = None;
    h.run();

    // The unsaved-changes prompt.
    rectangle(&mut h, DVec2::new(-20.0, -10.0), DVec2::new(20.0, 10.0));
    h.state_mut().pending = Some(crate::Pending::New);
    shot(dark, "45-unsaved", &mut h);
    h.state_mut().pending = None;
    h.run();

    // The driven-dimension prompt.
    let mut h = themed(dark);
    let p = rectangle(&mut h, DVec2::new(-40.0, -25.0), DVec2::new(40.0, 25.0));
    {
        let ed = &mut h.state_mut().editor;
        for (i, line) in ed
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
        {
            let c = if i % 2 == 0 {
                C::Horizontal { line }
            } else {
                C::Vertical { line }
            };
            ed.add_constraints("Square", &[c]).unwrap();
        }
        ed.add_dimension(
            C::HorizontalDistance {
                a: p[0],
                b: p[1],
                value: 80.0,
            },
            Some(DVec2::new(0.0, -12.0)),
        )
        .unwrap();
        let _ = ed.add_dimension(
            C::Distance {
                a: p[3],
                b: p[2],
                value: 80.0,
            },
            Some(DVec2::new(0.0, 12.0)),
        );
    }
    fit(&mut h);
    shot(dark, "46-driven", &mut h);

    // Check sketch.
    let mut h = themed(dark);
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        let p = [
            DVec2::new(-20.0, -15.0),
            DVec2::new(20.0, -15.0),
            DVec2::new(20.0, 15.0),
            DVec2::new(-20.0, 15.0),
            DVec2::new(-20.0, -14.7),
        ]
        .map(|at| ed.doc.add_point(at));
        for i in 0..4 {
            ed.doc.add_line(p[i], p[i + 1]).unwrap();
        }
        ed.commit("Outline");
    }
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.get_by_label("Check sketch…").click();
    h.run();
    shot(dark, "47-check-sketch", &mut h);
}

fn canvas_states(dark: bool) {
    // Hover and snap: the line tool over a corner.
    let mut h = themed(dark);
    rectangle(&mut h, DVec2::new(-40.0, -25.0), DVec2::new(40.0, 25.0));
    fit(&mut h);
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-60.0, 40.0));
    h.hover_at(screen(&h, DVec2::new(40.4, 25.3)));
    h.run();
    shot(dark, "50-snap-endpoint", &mut h);
    h.hover_at(screen(&h, DVec2::new(0.3, 25.2)));
    h.run();
    shot(dark, "51-snap-midpoint", &mut h);
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    // Hover with the select tool, then a picked set with its handles.
    h.hover_at(screen(&h, DVec2::new(40.0, 0.0)));
    h.run();
    shot(dark, "52-hover", &mut h);
    let all: std::collections::BTreeSet<EntityId> = h
        .state()
        .editor
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .collect();
    h.state_mut().editor.selection = all;
    h.run();
    shot(dark, "53-selection-handles", &mut h);

    // Dimensions, notes and marks on the example.
    let mut h = themed(dark);
    open_example(&mut h);
    h.get_by_label("Note").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 80.0));
    click_world(&mut h, DVec2::new(25.0, 95.0));
    h.event(Event::Text("DEBURR ALL EDGES".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    h.get_by_label("Centre mark").click();
    h.run();
    click_world(&mut h, DVec2::new(84.0, 40.0));
    h.get_by_label("Section mark").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 40.0));
    click_world(&mut h, DVec2::new(120.0, 40.0));
    h.get_by_label("Select").click();
    h.run();
    fit(&mut h);
    h.state_mut().status = None;
    shot(dark, "54-annotations", &mut h);

    // Sheets: the sheet and its print preview.
    let mut h = themed(dark);
    open_example(&mut h);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    h.state_mut().status = None;
    shot(dark, "55-sheet", &mut h);
    h.state_mut().sheet_preview = true;
    h.run();
    shot(dark, "56-sheet-preview", &mut h);
}

fn own_style(dark: bool) {
    // A dimension with settings of its own: open ticks and a colour.
    let mut h = themed(dark);
    let p = rectangle(&mut h, DVec2::new(-40.0, -25.0), DVec2::new(40.0, 25.0));
    let (width, height) = {
        let ed = &mut h.state_mut().editor;
        let w = ed
            .add_dimension(
                C::HorizontalDistance {
                    a: p[0],
                    b: p[1],
                    value: 80.0,
                },
                Some(DVec2::new(0.0, -12.0)),
            )
            .unwrap();
        let ht = ed
            .add_dimension(
                C::VerticalDistance {
                    a: p[1],
                    b: p[2],
                    value: 50.0,
                },
                Some(DVec2::new(12.0, 0.0)),
            )
            .unwrap();
        (w, ht)
    };
    let r = drift_core::AnnotationRef::Dimension(width);
    h.state_mut().editor.doc.set_style_override(
        r,
        drift_core::StyleOverride {
            arrow: Some(drift_core::ArrowStyle::Tick),
            text_height: Some(5.0),
            color: Some(Some(drift_core::Rgb([0xd0, 0x40, 0x40]))),
            ..Default::default()
        },
    );
    let _ = height;
    fit(&mut h);
    h.state_mut().selected_constraint = Some(width);
    h.run();
    h.get_by_label("Drawing style…").scroll_to_me();
    h.run();
    shot(dark, "48-dimension-style", &mut h);
    h.state_mut().selected_constraint = None;
    h.state_mut().show_drawing_settings = true;
    shot(dark, "49-drawing-settings", &mut h);
}
