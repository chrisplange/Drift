//! Memory torture tests: drive the real app through the things most likely to allocate
//! without bound (extreme zoom, huge text, dense hatches, big sketches, long sessions) and
//! report the process's peak and retained memory for each. Opt-in:
//!
//! `cargo test --release -p drift-app mem_ -- --ignored --nocapture --test-threads=1`
//!
//! A watchdog aborts the test process if it grows by more than [`ABORT_MB`], so a runaway
//! allocation can't take the machine down with it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use drift_core::underlays::{RefPrim, Underlay};
use drift_core::{DVec2, FillKind, Hatch, SketchText};
use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::Harness;

use crate::DriftApp;
use crate::dock::Panel;
use crate::editor::Editor;

/// Growth over the baseline at which the watchdog aborts.
const ABORT_MB: usize = 6_000;
/// Peak growth a scenario may reach before it fails.
const PEAK_LIMIT_MB: usize = 1_500;

fn used_mb() -> usize {
    drift_platform::memory::used().map_or(0, |b| (b / (1024 * 1024)) as usize)
}

/// Samples the process's memory in the background, keeping the peak.
struct Meter {
    peak: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    start: usize,
}

impl Meter {
    fn start() -> Self {
        let start = used_mb();
        let peak = Arc::new(AtomicUsize::new(start));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let now = used_mb();
                p.fetch_max(now, Ordering::Relaxed);
                if now > start + ABORT_MB {
                    eprintln!("\nmemory watchdog: grew by {} MB; aborting", now - start);
                    std::process::abort();
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self {
            peak,
            stop,
            thread: Some(thread),
            start,
        }
    }

    /// (peak growth, retained growth) in MB.
    fn finish(mut self) -> (usize, usize) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let end = used_mb();
        (
            self.peak.load(Ordering::Relaxed).max(end) - self.start,
            end.saturating_sub(self.start),
        )
    }
}

type App<'a> = Harness<'a, DriftApp>;

fn harness() -> Option<App<'static>> {
    let instance = eframe::egui_wgpu::wgpu::Instance::default();
    if pollster::block_on(
        instance.request_adapter(&eframe::egui_wgpu::wgpu::RequestAdapterOptions::default()),
    )
    .is_err()
    {
        eprintln!("no GPU adapter; skipping");
        return None;
    }
    let mut h = Harness::builder()
        .with_size([1920.0, 1080.0])
        .wgpu()
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    Some(h)
}

/// A frame through layout and the GPU.
fn frame(h: &mut App<'_>) {
    h.step();
    let _ = h.render();
}

fn example(h: &mut App<'_>) {
    h.state_mut().editor = Editor::new(drift_core::demo::mounting_plate());
    let fit = h.state().fit_bounds();
    h.state_mut().fit_target = Some(fit);
    frame(h);
    h.state_mut().fit_target = None;
}

/// Run a scenario, print what it cost, and fail it if it peaked too high.
fn scenario(
    label: &str,
    h: &mut App<'_>,
    failures: &mut Vec<String>,
    f: impl FnOnce(&mut App<'_>),
) {
    frame(h);
    let meter = Meter::start();
    let t = std::time::Instant::now();
    f(h);
    frame(h);
    let secs = t.elapsed().as_secs_f64();
    let (peak, kept) = meter.finish();
    let flag = if peak > PEAK_LIMIT_MB {
        "  <-- TOO MUCH"
    } else {
        ""
    };
    println!("{label:<52} peak +{peak:>6} MB   kept +{kept:>6} MB   {secs:>7.1} s{flag}");
    if peak > PEAK_LIMIT_MB {
        failures.push(format!("{label}: peak +{peak} MB"));
    }
}

/// Zoom from far out to far in and back, drawing each step.
fn zoom_sweep(h: &mut App<'_>) {
    let centre = DVec2::new(900.0, 500.0);
    for factor in [0.5, 2.0, 2.0, 0.5] {
        for _ in 0..40 {
            h.state_mut().camera.zoom_at(centre, factor);
            frame(h);
        }
    }
}

/// Imperial units with grid snapping, and geometry at absurd sizes and distances (what a
/// mistyped value can make): drawn, zoomed, indexed, filled and put on a sheet.
#[test]
#[ignore = "memory torture test: run with --release --ignored --nocapture"]
fn mem_extremes() {
    use drift_core::LengthFormat;
    let Some(mut h) = harness() else { return };
    let h = &mut h;
    let mut failures = Vec::new();
    println!(
        "
baseline {} MB",
        used_mb()
    );
    example(h);
    scenario(
        "imperial, grid snap, grid hidden: zoom sweep",
        h,
        &mut failures,
        |h| {
            let mut units = h.state().editor.doc.units();
            units.length = LengthFormat::Architectural;
            h.state_mut().editor.set_units(units);
            h.state_mut().grid_snap = true;
            h.state_mut().show_grid = false;
            h.state_mut().status_colors = false;
            zoom_sweep(h);
            // Draw with the line tool while zoomed far out and far in.
            for factor in [1e-9, 1e9, 1e9, 1e-9] {
                h.state_mut()
                    .camera
                    .zoom_at(DVec2::new(900.0, 500.0), factor);
                h.key_press(Key::L);
                for k in 0..6 {
                    let pos =
                        eframe::egui::pos2(500.0 + 90.0 * k as f32, 300.0 + 40.0 * (k % 3) as f32);
                    h.hover_at(pos);
                    h.step();
                    for pressed in [true, false] {
                        h.event(Event::PointerButton {
                            pos,
                            button: eframe::egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        });
                        h.step();
                    }
                }
                h.key_press(Key::Escape);
                frame(h);
            }
            println!("    {} entities", h.state().editor.doc.len());
        },
    );
    for size in [1e6, 1e12, 1e18, 1e150, 1e300] {
        example(h);
        scenario(
            &format!("geometry out to {size:e}"),
            h,
            &mut failures,
            |h| {
                let ed = &mut h.state_mut().editor;
                ed.doc.begin();
                let c = [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)]
                    .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
                for n in 0..4 {
                    let _ = ed.doc.add_line(c[n], c[(n + 1) % 4]);
                }
                let centre = ed.doc.add_point(DVec2::new(size, size));
                let _ = ed.doc.add_circle(centre, size * 0.3);
                ed.commit("Huge");
                let regions = h.state_mut().editor.regions().len();
                let _ = h
                    .state_mut()
                    .editor
                    .add_fill_at(DVec2::new(size * 0.5, size * 0.1));
                println!(
                    "    {regions} regions, {} fills",
                    h.state().editor.doc.fills().count()
                );
                frame(h);
                let fit = h.state().fit_bounds();
                h.state_mut().fit_target = Some(fit);
                frame(h);
                h.state_mut().fit_target = None;
                zoom_sweep(h);
                h.state_mut().editor.select_all();
                for p in Panel::ALL {
                    h.state_mut().show_panel(p);
                    frame(h);
                }
                h.state_mut().add_sheet();
                h.state_mut().sheet_preview = true;
                h.state_mut().preview.zoom = 0.0;
                for _ in 0..5 {
                    frame(h);
                }
                h.state_mut().sheet_preview = false;
                let mut storage = crate::ui_tests::MemoryStorage::default();
                eframe::App::save(h.state_mut(), &mut storage);
            },
        );
    }
    println!("end {} MB", used_mb());
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The sheet preview draws the page's text with egui at the preview's zoom: zoom all the way
/// in and out, as the scroll wheel does.
#[test]
#[ignore = "memory torture test: run with --release --ignored --nocapture"]
fn mem_sheet_preview() {
    let Some(mut h) = harness() else { return };
    let h = &mut h;
    let mut failures = Vec::new();
    println!(
        "
baseline {} MB",
        used_mb()
    );
    example(h);
    h.state_mut().show_panel(Panel::Sheets);
    scenario("sheet preview: open", h, &mut failures, |h| {
        h.state_mut().add_sheet();
        h.state_mut().sheet_preview = true;
        h.state_mut().preview.zoom = 0.0;
        for _ in 0..10 {
            frame(h);
        }
    });
    for round in 0..3 {
        scenario(
            &format!("sheet preview: zoom 0.05 .. 200 and back ({round})"),
            h,
            &mut failures,
            |h| {
                let mut zoom = 0.05_f32;
                while zoom < 200.0 {
                    h.state_mut().preview.zoom = zoom;
                    frame(h);
                    zoom *= 1.07;
                }
                while zoom > 0.05 {
                    h.state_mut().preview.zoom = zoom;
                    frame(h);
                    zoom /= 1.13;
                }
            },
        );
    }
    println!("end {} MB", used_mb());
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "memory torture test: run with --release --ignored --nocapture"]
fn mem_torture() {
    let Some(mut h) = harness() else { return };
    let h = &mut h;
    let mut failures = Vec::new();
    println!("\nbaseline {} MB", used_mb());

    example(h);
    scenario("idle: 600 frames of the example", h, &mut failures, |h| {
        for _ in 0..600 {
            frame(h);
        }
    });
    scenario(
        "zoom sweep (2^-40 .. 2^40) on the example",
        h,
        &mut failures,
        zoom_sweep,
    );
    scenario("pan to 1e12 and back", h, &mut failures, |h| {
        for d in [1e3, 1e6, 1e9, 1e12, -1e12, -1e9, -1e6, -1e3] {
            h.state_mut().camera.pan(DVec2::new(d, d * 0.5));
            frame(h);
        }
    });

    // Text at absurd sizes, and lots of it.
    example(h);
    scenario(
        "text: heights 0.001 .. 1e6, then zoom",
        h,
        &mut failures,
        |h| {
            for (k, height) in [0.001, 0.1, 10.0, 1e3, 1e6].into_iter().enumerate() {
                let at = DVec2::new(0.0, -30.0 * k as f64);
                h.state_mut().editor.add_text(SketchText::new(
                    "Hamburgefonstiv 0123456789",
                    at,
                    height,
                ));
                frame(h);
            }
            zoom_sweep(h);
        },
    );
    scenario("text: 400 lines of 60 characters", h, &mut failures, |h| {
        let line = "The quick brown fox jumps over the lazy dog 0123456789 ABCDEF";
        for k in 0..400 {
            let at = DVec2::new(200.0, 6.0 * f64::from(k));
            h.state_mut()
                .editor
                .add_text(SketchText::new(line, at, 3.0));
        }
        for _ in 0..30 {
            frame(h);
        }
        zoom_sweep(h);
    });
    scenario(
        "text: typing 300 characters into one text",
        h,
        &mut failures,
        |h| {
            let id =
                h.state_mut()
                    .editor
                    .add_text(SketchText::new("", DVec2::new(-300.0, 0.0), 5.0));
            let mut words = String::new();
            for k in 0..300 {
                words.push(char::from(b'a' + (k % 26) as u8));
                h.state_mut().editor.type_text(id, &words, k > 0);
                frame(h);
            }
        },
    );

    // Hatches from coarse to absurdly dense, zoomed through.
    example(h);
    scenario(
        "hatch: scales 1 .. 1e-6, zoomed through",
        h,
        &mut failures,
        |h| {
            let fill = h
                .state_mut()
                .editor
                .add_fill_at(DVec2::new(100.0, 15.0))
                .expect("a region to fill");
            for scale in [1.0, 0.1, 1e-3, 1e-6] {
                let mut f = h.state().editor.doc.fill(fill).cloned().expect("fill");
                f.kind = FillKind::Hatch(Hatch {
                    pattern: "ANSI31".into(),
                    scale,
                    angle: 0.0,
                    origin: DVec2::ZERO,
                    model: true,
                    background: None,
                });
                h.state_mut().editor.update_fill(fill, f);
                for _ in 0..5 {
                    frame(h);
                }
                zoom_sweep(h);
            }
        },
    );

    // Every panel, the palette and the shortcuts sheet.
    example(h);
    scenario(
        "panels, palette and shortcuts: 60 frames each",
        h,
        &mut failures,
        |h| {
            h.state_mut().editor.select_all();
            for p in Panel::ALL {
                h.state_mut().show_panel(p);
                for _ in 0..60 {
                    frame(h);
                }
            }
            h.state_mut().open_palette();
            for _ in 0..60 {
                frame(h);
            }
            h.key_press(Key::Escape);
            h.state_mut().show_shortcuts = true;
            for _ in 0..60 {
                frame(h);
            }
            h.state_mut().show_shortcuts = false;
        },
    );

    // A long editing session: draw, duplicate, undo, redo.
    example(h);
    scenario(
        "session: 400 rectangles, undo all, redo all",
        h,
        &mut failures,
        |h| {
            for k in 0..400 {
                let o = DVec2::new(200.0 + 12.0 * f64::from(k % 40), 12.0 * f64::from(k / 40));
                let ed = &mut h.state_mut().editor;
                ed.doc.begin();
                let c = [(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)]
                    .map(|(x, y)| ed.doc.add_point(o + DVec2::new(x, y)));
                for n in 0..4 {
                    ed.doc.add_line(c[n], c[(n + 1) % 4]).expect("line");
                }
                ed.commit("Rectangle");
                if k % 20 == 0 {
                    frame(h);
                }
            }
            for _ in 0..400 {
                h.state_mut().undo();
            }
            frame(h);
            for _ in 0..400 {
                h.state_mut().redo();
            }
            frame(h);
        },
    );
    scenario(
        "session: duplicate everything in place x6",
        h,
        &mut failures,
        |h| {
            for _ in 0..6 {
                h.state_mut().editor.select_all();
                h.state_mut().editor.duplicate_selection();
                frame(h);
            }
            // Regions over coincident copies (fills and region picking ask for them).
            let n = h.state_mut().editor.regions().len();
            println!("    {} curves, {n} regions", h.state().editor.doc.len());
            frame(h);
        },
    );
    scenario("autosave x40", h, &mut failures, |h| {
        let mut storage = crate::ui_tests::MemoryStorage::default();
        for _ in 0..40 {
            eframe::App::save(h.state_mut(), &mut storage);
        }
    });

    // The 10,000-curve sketch: open, draw, select everything, drag it.
    scenario("10k curves: open and draw", h, &mut failures, |h| {
        let (doc, _) = crate::perf_tests::torture_sketch();
        h.state_mut().editor = Editor::new(doc);
        let fit = h.state().fit_bounds();
        h.state_mut().fit_target = Some(fit);
        frame(h);
        h.state_mut().fit_target = None;
        for _ in 0..30 {
            frame(h);
        }
    });
    scenario("10k curves: zoom sweep", h, &mut failures, zoom_sweep);
    scenario(
        "10k curves: select all, every panel",
        h,
        &mut failures,
        |h| {
            h.state_mut().editor.select_all();
            for p in Panel::ALL {
                h.state_mut().show_panel(p);
                for _ in 0..10 {
                    frame(h);
                }
            }
        },
    );
    scenario("10k curves: regions", h, &mut failures, |h| {
        let n = h.state_mut().editor.regions().len();
        println!("    {n} regions");
    });
    scenario("10k curves: duplicate in place x2", h, &mut failures, |h| {
        for _ in 0..2 {
            h.state_mut().editor.select_all();
            h.state_mut().editor.duplicate_selection();
            frame(h);
        }
        println!("    {} entities", h.state().editor.doc.len());
    });
    scenario("10k+ curves: autosave x10", h, &mut failures, |h| {
        let mut storage = crate::ui_tests::MemoryStorage::default();
        for _ in 0..10 {
            eframe::App::save(h.state_mut(), &mut storage);
        }
    });

    // A big underlay (a dense PDF page), zoomed through.
    example(h);
    scenario(
        "underlay: 300k segments, zoomed through",
        h,
        &mut failures,
        |h| {
            let prims: Vec<RefPrim> = (0..300_000)
                .map(|k| {
                    let (x, y) = (f64::from(k % 600), f64::from(k / 600));
                    RefPrim::Line(DVec2::new(x, y), DVec2::new(x + 0.8, y + 0.6))
                })
                .collect();
            h.state_mut()
                .editor
                .add_underlay(Underlay::new("plan", "plan.pdf", prims));
            for _ in 0..10 {
                frame(h);
            }
            zoom_sweep(h);
        },
    );

    // Keys in quick succession, as a cat on the keyboard would.
    example(h);
    scenario(
        "random keys and clicks: 3000 events",
        h,
        &mut failures,
        |h| {
            let keys = [
                Key::L,
                Key::R,
                Key::C,
                Key::A,
                Key::K,
                Key::U,
                Key::B,
                Key::P,
                Key::D,
                Key::N,
                Key::H,
                Key::T,
                Key::E,
                Key::O,
                Key::M,
                Key::F,
                Key::G,
                Key::X,
                Key::Y,
                Key::Tab,
                Key::Enter,
                Key::Escape,
                Key::Num1,
                Key::Num5,
                Key::Period,
                Key::Delete,
                Key::S,
            ];
            let mut seed = 0x2545_f491_4f6c_dd1d_u64;
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            };
            for _ in 0..3000 {
                let r = next();
                let pos =
                    eframe::egui::pos2(300.0 + (r % 1100) as f32, 120.0 + ((r >> 16) % 800) as f32);
                match r % 5 {
                    0 | 1 => h.key_press(keys[(r >> 8) as usize % keys.len()]),
                    2 => h.hover_at(pos),
                    3 => {
                        for pressed in [true, false] {
                            h.event(Event::PointerButton {
                                pos,
                                button: eframe::egui::PointerButton::Primary,
                                pressed,
                                modifiers: Modifiers::NONE,
                            });
                        }
                    }
                    _ => h.key_press_modifiers(Modifiers::SHIFT, Key::D),
                }
                h.step();
            }
            println!("    {} entities", h.state().editor.doc.len());
        },
    );

    println!("end {} MB", used_mb());
    assert!(failures.is_empty(), "{failures:#?}");
}
