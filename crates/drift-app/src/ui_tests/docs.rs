//! Screenshots for the user guide in `docs/guide/images/`. Each scene drives the real app,
//! as the other UI tests do, and saves what the window shows. Ignored in normal runs; after
//! a change to the interface, regenerate them with
//!
//! ```text
//! cargo test -p drift-app docs_screenshots -- --ignored
//! ```

use super::*;
use drift_core::{Constraint as C, EntityId, Kind};

/// Save the window as `docs/guide/images/<name>`, without a status message over the canvas.
fn shot(name: &str, h: &mut Harness<'_, DriftApp>) {
    h.state_mut().status = None;
    h.run();
    h.run();
    let image = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/images");
    std::fs::create_dir_all(&dir).expect("create docs/guide/images");
    let path = dir.join(name);
    image.save(&path).expect("save png");
    eprintln!("wrote {}", path.display());
}

fn open_example(h: &mut Harness<'_, DriftApp>) {
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
}

/// A rectangle drawn in code: its corners (anticlockwise from the lower left) and sides
/// (bottom, right, top, left), level and plumb when `square`.
fn rectangle(
    h: &mut Harness<'_, DriftApp>,
    min: DVec2,
    max: DVec2,
    square: bool,
) -> ([EntityId; 4], [EntityId; 4]) {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let p = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
        .map(|at| ed.doc.add_point(at));
    let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
    if square {
        for (i, line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                C::Horizontal { line: *line }
            } else {
                C::Vertical { line: *line }
            };
            ed.doc.add_constraint(c).unwrap();
        }
    }
    ed.commit("Rectangle");
    (p, l)
}

fn fit(h: &mut Harness<'_, DriftApp>) {
    let bounds = h.state().fit_bounds();
    h.state_mut().fit_target = Some(bounds);
    h.run();
    h.run();
}

fn drag(h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run();
    button(h, from, true);
    h.run();
    for k in 1..=5 {
        h.hover_at(from + (to - from) * (k as f32 / 5.0));
        h.run();
    }
    button(h, to, false);
    h.run();
}

#[test]
#[ignore = "writes the user guide's screenshots; run with --ignored"]
fn docs_screenshots() {
    if harness().is_none() {
        return;
    }
    getting_started();
    first_sketch();
    constraints();
    parameters();
    editing();
    temporary_dimensions();
    fills();
    blocks();
    layers();
    construction();
    annotation();
    sheets();
    underlay();
    checker();
}

fn getting_started() {
    let mut h = harness().unwrap();
    // (Tests start without the start panel.)
    h.state_mut().show_start = true;
    shot("start-screen.png", &mut h);

    open_example(&mut h);
    click_world(&mut h, DVec2::new(84.0, 40.0)); // the bore
    shot("window.png", &mut h);

    h.event(Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: EguiModifiers::COMMAND,
    });
    h.run();
    h.run();
    type_text(&mut h, "dim");
    shot("command-palette.png", &mut h);
    h.key_press(Key::Escape);
    h.run();

    h.state_mut().show_shortcuts = true;
    shot("keyboard-shortcuts.png", &mut h);
}

fn first_sketch() {
    let mut h = harness().unwrap();
    // A line locked to the X axis, with its length typed.
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-60.0, 45.0));
    h.hover_at(screen(&h, DVec2::new(-20.0, 52.0)));
    h.run();
    h.key_press(Key::X);
    h.run();
    h.hover_at(screen(&h, DVec2::new(-15.0, 53.0)));
    h.run();
    type_text(&mut h, "50");
    shot("first-line.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();

    // A rectangle: the first corner clicked, the size typed.
    h.key_press(Key::R);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -25.0));
    h.hover_at(screen(&h, DVec2::new(30.0, 20.0)));
    h.run();
    type_text(&mut h, "80");
    h.key_press(Key::Tab);
    h.run();
    type_text(&mut h, "50");
    shot("first-rectangle.png", &mut h);
    h.key_press(Key::Enter);
    h.run();

    // A circle on the origin, its diameter typed.
    h.key_press(Key::C);
    h.run();
    click_world(&mut h, DVec2::ZERO);
    h.hover_at(screen(&h, DVec2::new(12.0, 6.0)));
    h.run();
    type_text(&mut h, "20");
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    fit(&mut h);
    shot("first-sketch.png", &mut h);
}

fn constraints() {
    // Drawn with the rectangle tool: level and plumb, but free to move and stretch.
    let mut h = harness().unwrap();
    h.key_press(Key::R);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -25.0));
    click_world(&mut h, DVec2::new(40.0, 25.0));
    h.key_press(Key::Escape);
    h.run();
    shot("dims-under-defined.png", &mut h);

    // Smart Dimension: the bottom side, placed below, its value typed.
    h.key_press(Key::D);
    h.run();
    click_world(&mut h, DVec2::new(-20.0, -25.0));
    click_world(&mut h, DVec2::new(-20.0, -40.0));
    type_text(&mut h, "100");
    shot("dims-smart-dimension.png", &mut h);
    h.key_press(Key::Enter);
    h.run();
    // The left side, and the lower left corner fixed: nothing left to move.
    click_world(&mut h, DVec2::new(-50.0, -10.0));
    click_world(&mut h, DVec2::new(-65.0, -10.0));
    type_text(&mut h, "60");
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let corner = {
        let doc = &h.state().editor.doc;
        doc.iter()
            .filter(|(_, e)| e.kind.is_point())
            .min_by(|(a, _), (b, _)| {
                let at = |id: &EntityId| {
                    let p = doc.point(*id).unwrap();
                    p.x + p.y
                };
                at(a).total_cmp(&at(b))
            })
            .map(|(id, _)| id)
            .unwrap()
    };
    let at = h.state().editor.doc.point(corner).unwrap();
    h.state_mut()
        .editor
        .add_constraints("Fix", &[C::Fix { point: corner, at }])
        .unwrap();
    h.key_press(Key::Escape);
    h.run();
    fit(&mut h);
    shot("dims-fully-defined.png", &mut h);

    // A dimension the sketch already holds: offered as a driven one.
    let mut h = harness().unwrap();
    let (p, _) = rectangle(
        &mut h,
        DVec2::new(-40.0, -25.0),
        DVec2::new(40.0, 25.0),
        true,
    );
    {
        let ed = &mut h.state_mut().editor;
        ed.add_dimension(
            C::HorizontalDistance {
                a: p[0],
                b: p[1],
                value: 80.0,
            },
            Some(DVec2::new(0.0, -12.0)),
        )
        .unwrap();
        ed.add_dimension(
            C::VerticalDistance {
                a: p[1],
                b: p[2],
                value: 50.0,
            },
            Some(DVec2::new(12.0, 0.0)),
        )
        .unwrap();
        let refused = ed.add_dimension(
            C::Distance {
                a: p[3],
                b: p[2],
                value: 80.0,
            },
            Some(DVec2::new(0.0, 12.0)),
        );
        assert!(refused.is_err());
    }
    fit(&mut h);
    shot("dims-driven-prompt.png", &mut h);

    // The relations pie on two lines.
    let mut h = harness().unwrap();
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
    shot("relations-pie.png", &mut h);
    h.key_up(Key::Q);
    h.run();

    // Chain dimensions from wall to wall, with the overall.
    let mut h = harness().unwrap();
    {
        let ed = &mut h.state_mut().editor;
        ed.doc.begin();
        for x in [-30.0, -10.0, 15.0, 30.0] {
            let a = ed.doc.add_point(DVec2::new(x, 0.0));
            let b = ed.doc.add_point(DVec2::new(x, 20.0));
            ed.doc.add_line(a, b).unwrap();
        }
        ed.commit("Walls");
    }
    h.run();
    h.key_press(Key::D);
    h.run();
    h.get_by_label("Chain").click();
    h.run();
    h.get_by_label("Overall").click();
    h.run();
    click_world(&mut h, DVec2::new(-30.0, 7.0));
    click_world(&mut h, DVec2::new(-10.0, 9.0));
    click_world(&mut h, DVec2::new(-20.0, 26.0));
    h.run();
    click_world(&mut h, DVec2::new(15.0, 4.0));
    h.run();
    click_world(&mut h, DVec2::new(30.0, 13.0));
    h.run();
    click_world(&mut h, DVec2::new(0.0, -30.0));
    h.run();
    shot("chain-dimensions.png", &mut h);
}

fn parameters() {
    let mut h = harness().unwrap();
    let (p, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(100.0, 50.0), true);
    let (width, height) = {
        let ed = &mut h.state_mut().editor;
        ed.add_constraints(
            "Fix",
            &[C::Fix {
                point: p[0],
                at: DVec2::ZERO,
            }],
        )
        .unwrap();
        let w = ed
            .add_dimension(
                C::HorizontalDistance {
                    a: p[0],
                    b: p[1],
                    value: 100.0,
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
                Some(DVec2::new(14.0, 0.0)),
            )
            .unwrap();
        (w, ht)
    };
    {
        let ed = &mut h.state_mut().editor;
        let var = ed.add_variable("width", "100", Kind::Length).unwrap();
        ed.set_dimension_text(width, "width").unwrap();
        ed.set_dimension_text(height, "width / 2").unwrap();
        ed.save_variant("Wide");
        ed.set_variable(var, "60").unwrap();
        ed.save_variant("Narrow");
    }
    h.state_mut().params.show_ghosts = true;
    h.state_mut().show_panel(Panel::Parameters);
    // Room for the wide variant's ghost.
    h.state_mut().fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-20.0, -25.0),
        max: DVec2::new(125.0, 60.0),
    });
    h.run();
    h.run();
    shot("parameters.png", &mut h);
}

fn editing() {
    // Transform handles round a picked rectangle.
    let mut h = harness().unwrap();
    let (_, l) = rectangle(
        &mut h,
        DVec2::new(-20.0, -12.0),
        DVec2::new(20.0, 12.0),
        true,
    );
    h.state_mut().editor.selection = l.iter().copied().collect();
    h.run();
    shot("transform-handles.png", &mut h);

    // R turns a group of holes about its middle.
    let mut h = harness().unwrap();
    open_example(&mut h);
    let holes: std::collections::BTreeSet<EntityId> = {
        let doc = &h.state().editor.doc;
        doc.iter()
            .filter(|(id, e)| {
                matches!(e.kind, EntityKind::Circle { .. })
                    && drift_solver::radius_of(doc, *id).is_some_and(|r| (r - 3.2).abs() < 1e-9)
            })
            .map(|(id, _)| id)
            .collect()
    };
    h.state_mut().editor.selection = holes;
    h.run();
    h.hover_at(screen(&h, DVec2::new(110.0, 40.0)));
    h.run();
    h.key_press(Key::R);
    h.run();
    h.hover_at(screen(
        &h,
        DVec2::new(70.0, 40.0) + DVec2::from_angle(0.5) * 40.0,
    ));
    h.run();
    shot("rotate.png", &mut h);
    h.key_press(Key::Escape);
    h.run();

    // A fillet being sized on a corner.
    let mut h = harness().unwrap();
    h.state_mut().auto_relations = false;
    rectangle(
        &mut h,
        DVec2::new(-40.0, -25.0),
        DVec2::new(40.0, 25.0),
        false,
    );
    fit(&mut h);
    h.key_press(Key::F);
    h.run();
    click_world(&mut h, DVec2::new(40.0, 25.0));
    h.hover_at(screen(&h, DVec2::new(35.0, 20.0)));
    h.run();
    shot("fillet.png", &mut h);
}

fn temporary_dimensions() {
    let mut h = harness().unwrap();
    open_example(&mut h);
    let mut doc = drift_core::Document::new();
    let mut wall = |y: f64, x0: f64, x1: f64| {
        let a = doc.add_point(DVec2::new(x0, y));
        let b = doc.add_point(DVec2::new(x1, y));
        doc.add_line(a, b).unwrap()
    };
    wall(0.0, 0.0, 400.0);
    wall(100.0, 0.0, 400.0);
    wall(250.0, 100.0, 300.0);
    h.state_mut()
        .replace_document(doc, "walls".into(), None, Default::default(), true);
    h.run();
    h.run();
    click_world(&mut h, DVec2::new(120.0, 100.0));
    shot("temp-dims.png", &mut h);
    let below = h.state().temp_dims.values[2].1;
    h.hover_at(below);
    h.run();
    button(&h, below, true);
    h.run();
    button(&h, below, false);
    h.run();
    h.run();
    h.event(egui::Event::Text("120".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    shot("temp-dims-moved.png", &mut h);
}

fn fills() {
    let mut h = harness().unwrap();
    open_example(&mut h);
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    h.query_all_by_label("Solid").next().unwrap().click();
    h.run();
    click_world(&mut h, DVec2::new(75.0, 45.0));
    h.hover_at(screen(&h, DVec2::new(20.0, 60.0)));
    h.run();
    shot("fills.png", &mut h);

    // A picked hatch's handles: its origin moved onto a corner, the pattern turned.
    let mut h = harness().unwrap();
    h.get_by_label("Rectangle").click();
    h.run();
    click_world(&mut h, DVec2::new(10.0, 10.0));
    click_world(&mut h, DVec2::new(70.0, 50.0));
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(40.0, 30.0));
    h.run();
    let map = screen_map(&h);
    let handles = h.state_mut().hatch_handles(&map).expect("handles");
    let corner = screen(&h, DVec2::new(10.0, 10.0));
    drag(&mut h, handles.origin, corner + egui::vec2(2.0, -1.0));
    let map = screen_map(&h);
    let handles = h.state_mut().hatch_handles(&map).expect("handles");
    let to = handles.origin + egui::vec2(40.0, -22.0);
    drag(&mut h, handles.angle, to);
    shot("hatch-handles.png", &mut h);
}

fn blocks() {
    // A 12' × 10' bedroom furnished from the library: a door, a window, a queen bed with
    // nightstands, a desk with its chair, and a dresser.
    use crate::library::{self, INCH};
    let mut h = harness().unwrap();
    h.state_mut()
        .editor
        .set_units(drift_core::Units::architectural());
    let (w, d) = (144.0 * INCH, 120.0 * INCH);
    rectangle(&mut h, DVec2::ZERO, DVec2::new(w, d), true);
    fit(&mut h);
    h.state_mut().show_panel(Panel::Blocks);
    h.state_mut().library_category = Some(library::Category::Furniture);
    h.run();
    // The bed from its tile: it's added and placed against the head wall.
    h.get_by_label("Bed – Queen with Nightstands").click();
    h.run();
    click_world(&mut h, DVec2::new(w / 2.0, d));
    h.key_press(Key::Escape);
    h.run();
    let ed = &mut h.state_mut().editor;
    for (name, at, angle) in [
        (
            "Door – Single Swing – 32\"",
            DVec2::new(40.0 * INCH, 0.0),
            180.0,
        ),
        (
            "Window – Double Hung – 36\"",
            DVec2::new(0.0, 46.0 * INCH),
            90.0,
        ),
        ("Desk – 48\" × 24\"", DVec2::new(w, 54.0 * INCH), -90.0),
        (
            "Chair – Desk",
            DVec2::new(w - 50.0 * INCH, 54.0 * INCH),
            90.0,
        ),
        ("Dresser – 60\" × 18\"", DVec2::new(96.0 * INCH, 0.0), 180.0),
    ] {
        let id = ed.library_block(library::entry(name).expect(name));
        ed.place_instance(id, at, angle, false);
    }
    ed.selected_instances.clear();
    h.run();
    h.get_by_label("Select").click();
    h.run();
    click_world(&mut h, DVec2::new(w / 2.0 - 30.0 * INCH, d - 2.0 * INCH));
    shot("blocks.png", &mut h);
}

fn layers() {
    // A room's walls hatched on their own layer, a reference layer that isn't snapped to,
    // and the walls' layer current.
    let mut h = harness().unwrap();
    let (walls, hatch, reference) = {
        let ed = &mut h.state_mut().editor;
        let walls = ed.add_layer("Walls");
        let hatch = ed.add_layer("Hatch");
        let reference = ed.add_layer("Survey (reference)");
        for (id, rgb) in [
            (walls, [0x5e, 0x7f, 0x5c]),
            (hatch, [0xb0, 0x8a, 0x5a]),
            (reference, [0x8a, 0x90, 0x99]),
        ] {
            let mut l = ed.doc.layer(id).cloned().unwrap();
            l.color = Some(drift_core::Rgb(rgb));
            if id == reference {
                l.snappable = false;
            }
            ed.update_layer(id, l);
        }
        ed.set_current_layer(walls);
        (walls, hatch, reference)
    };
    let _ = reference;
    rectangle(&mut h, DVec2::ZERO, DVec2::new(4000.0, 3000.0), true);
    rectangle(
        &mut h,
        DVec2::new(200.0, 200.0),
        DVec2::new(3800.0, 2800.0),
        true,
    );
    {
        let ed = &mut h.state_mut().editor;
        ed.set_current_layer(hatch);
        ed.add_fill_at(DVec2::new(100.0, 1500.0)).unwrap();
        ed.selected_fills.clear();
        ed.set_current_layer(walls);
    }
    fit(&mut h);
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    shot("layers.png", &mut h);
}

fn construction() {
    let mut h = harness().unwrap();
    rectangle(
        &mut h,
        DVec2::new(-40.0, -25.0),
        DVec2::new(40.0, 25.0),
        true,
    );
    fit(&mut h);
    h.key_press(Key::L);
    h.run();
    h.key_press_modifiers(EguiModifiers::ALT, Key::C);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, -25.0));
    click_world(&mut h, DVec2::new(40.0, 25.0));
    h.key_press(Key::Escape);
    h.run();
    click_world(&mut h, DVec2::new(-40.0, 25.0));
    h.hover_at(screen(&h, DVec2::new(30.0, -18.0)));
    h.run();
    shot("construction.png", &mut h);
}

fn annotation() {
    // A note with a leader, a centre mark and a hole callout on the plate.
    let mut h = harness().unwrap();
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
    h.get_by_label("Callout").click();
    h.run();
    let hole = DVec2::new(70.0, 40.0) + DVec2::splat(26.0 / 2f64.sqrt());
    click_world(&mut h, hole + DVec2::new(3.2, 0.0));
    click_world(&mut h, DVec2::new(108.0, 92.0));
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Select").click();
    h.run();
    fit(&mut h);
    shot("annotate-notes.png", &mut h);

    // Section and elevation marks.
    let mut h = harness().unwrap();
    open_example(&mut h);
    h.get_by_label("Section mark").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 40.0));
    click_world(&mut h, DVec2::new(120.0, 40.0));
    h.get_by_label("Elevation mark").click();
    h.run();
    click_world(&mut h, DVec2::new(130.0, -10.0));
    click_world(&mut h, DVec2::new(110.0, 10.0));
    h.get_by_label("Select").click();
    h.run();
    fit(&mut h);
    shot("annotate-marks.png", &mut h);

    // A dimension's value editor, with the quick chips.
    let mut h = harness().unwrap();
    let (p, _) = rectangle(
        &mut h,
        DVec2::new(-20.0, -12.0),
        DVec2::new(20.0, 12.0),
        false,
    );
    let width = h
        .state_mut()
        .editor
        .add_dimension(
            C::HorizontalDistance {
                a: p[0],
                b: p[1],
                value: 40.0,
            },
            Some(DVec2::new(0.0, -10.0)),
        )
        .unwrap();
    h.run();
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().canvas_rect,
    );
    let at = h
        .state()
        .layout_dimensions(&painter, &screen_map(&h))
        .into_iter()
        .find(|d| d.id == Some(width))
        .unwrap()
        .text_rect
        .center();
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        button(&h, at, true);
        h.step();
        button(&h, at, false);
        h.step();
    }
    h.run();
    h.get_all_by_label("TYP")
        .min_by(|p, q| {
            let d = |n: &egui_kittest::Node<'_>| n.rect().center().distance(at);
            d(p).total_cmp(&d(q))
        })
        .unwrap()
        .click();
    h.run();
    shot("dimension-chips.png", &mut h);
}

fn sheets() {
    let mut h = harness().unwrap();
    open_example(&mut h);
    h.get_by_label("Fill / hatch").click();
    h.run();
    click_world(&mut h, DVec2::new(100.0, 15.0));
    h.get_by_label("Select").click();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    shot("sheet.png", &mut h);

    // A second, framed view: a detail at 2:1.
    h.get_by_label("Add view").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    let mut s = h.state().editor.doc.sheet(id).cloned().unwrap();
    s.views[1].scale = 0.5;
    s.views[1].name = "Detail A".into();
    s.views[1].frame = Some(drift_core::ViewFrame {
        at: DVec2::new(330.0, 200.0),
        size: DVec2::new(100.0, 80.0),
    });
    s.views[1].center = DVec2::new(84.0, 40.0);
    s.views[1].border = true;
    h.state_mut().editor.doc.begin();
    h.state_mut().editor.doc.set_sheet(id, s);
    h.state_mut().editor.commit("Detail");
    shot("sheet-views.png", &mut h);
}

fn underlay() {
    let mut h = harness().unwrap();
    let v = DVec2::new;
    // A 1 : 50 plan on A3: a 5 × 3 m room with a door swing.
    let mut d = drift_draft::page::Drawing::new(v(420.0, 297.0));
    let pen = drift_draft::page::Pen::new(0.35);
    let corners = [
        v(50.0, 50.0),
        v(150.0, 50.0),
        v(150.0, 110.0),
        v(50.0, 110.0),
    ];
    for i in 0..4 {
        d.push(
            "0",
            pen,
            drift_draft::page::Prim::Line(corners[i], corners[(i + 1) % 4]),
        );
    }
    d.push(
        "0",
        pen,
        drift_draft::page::Prim::Arc {
            center: v(70.0, 50.0),
            radius: 16.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        },
    );
    let bytes = drift_io::pdf::write(&[d], "Plan");
    h.state_mut().import_underlay_file("plan.pdf", bytes, 0.0);
    h.run();
    h.get_by_label("Calibrate…").click();
    h.run();
    click_world(&mut h, v(50.2, 49.9));
    click_world(&mut h, v(149.8, 50.1));
    type_text(&mut h, "5000");
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::F);
    h.run();
    h.run();
    let u = h.state().editor.doc.underlays().next().unwrap().1.clone();
    let Some(drift_core::Shape::Line { a, b }) = u.shapes().first().copied() else {
        panic!("the bottom wall")
    };
    h.key_press(Key::L);
    h.run();
    let near = |p: DVec2| p + v(30.0, -20.0);
    click_world(&mut h, near(a));
    click_world(&mut h, near(b));
    click_world(&mut h, near(b + v(0.0, 3000.0)));
    h.key_press(Key::Escape);
    h.run();
    h.hover_at(screen(&h, v(2500.0, 1500.0)));
    h.run();
    shot("underlay.png", &mut h);
}

fn checker() {
    let mut h = harness().unwrap();
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
        let (a, b) = (
            ed.doc.add_point(DVec2::new(40.0, 0.0)),
            ed.doc.add_point(DVec2::new(20.2, 0.0)),
        );
        ed.doc.add_line(a, b).unwrap();
        ed.commit("Outline");
    }
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    h.get_by_label("Check sketch…").click();
    h.run();
    shot("check-sketch.png", &mut h);
}
