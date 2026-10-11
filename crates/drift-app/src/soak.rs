//! A self-driving soak test of the real app: with `DRIFT_SOAK=<seconds>` set, Drift feeds
//! itself random input (pointer moves, clicks and drags on the canvas, tool keys, typed
//! values, zooming, window resizes and minimising) for that long, logging its memory once a
//! second to `<temp>/drift/soak.log`, then closes. It exercises what headless tests can't:
//! the real window, swapchain and event loop. The events are injected inside the app (never
//! through the operating system), so they can't land in another program.

use std::io::Write as _;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};

use crate::DriftApp;

/// Above this many entities the soak starts a new sketch, so memory growth means a leak
/// rather than a bigger drawing.
const MAX_ENTITIES: usize = 20_000;

pub(crate) struct Soak {
    started: Instant,
    length: Duration,
    seed: u64,
    frame: u64,
    reported: Instant,
    pointer: Pos2,
    down: bool,
    /// Frames left with the window minimised.
    minimised: u32,
    first_mb: u64,
    peak_mb: u64,
    log: Option<std::fs::File>,
}

impl Soak {
    pub(crate) fn from_env() -> Option<Self> {
        let seconds: u64 = std::env::var("DRIFT_SOAK").ok()?.trim().parse().ok()?;
        let dir = std::env::temp_dir().join("drift");
        let _ = std::fs::create_dir_all(&dir);
        let log = std::fs::File::create(dir.join("soak.log")).ok();
        Some(Self {
            started: Instant::now(),
            length: Duration::from_secs(seconds),
            seed: 0x9e37_79b9_7f4a_7c15,
            frame: 0,
            reported: Instant::now(),
            pointer: Pos2::new(600.0, 400.0),
            down: false,
            minimised: 0,
            first_mb: 0,
            peak_mb: 0,
            log,
        })
    }

    fn next(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn line(&mut self, text: &str) {
        if let Some(f) = &mut self.log {
            let _ = writeln!(f, "{text}");
            let _ = f.flush();
        }
        log::info!("{text}");
    }
}

const KEYS: [Key; 30] = [
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
    Key::S,
    Key::W,
    Key::I,
    Key::Tab,
    Key::Enter,
    Key::Escape,
    Key::Escape,
    Key::Delete,
    Key::Num2,
    Key::Num5,
    Key::Period,
];

impl DriftApp {
    /// Add this frame's soak events to the input (called from `raw_input_hook`).
    pub(crate) fn soak_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(mut soak) = self.soak.take() else {
            return;
        };
        ctx.request_repaint();
        soak.frame += 1;
        let entities = self.editor.doc.len();
        if soak.reported.elapsed() >= Duration::from_secs(1) {
            soak.reported = Instant::now();
            let mb = drift_platform::memory::used().unwrap_or(0) / (1024 * 1024);
            if soak.first_mb == 0 {
                soak.first_mb = mb;
            }
            soak.peak_mb = soak.peak_mb.max(mb);
            let t = soak.started.elapsed().as_secs();
            let frame = soak.frame;
            soak.line(&format!(
                "t={t:>4}s frame={frame:>7} memory={mb:>6} MB entities={entities:>6}"
            ));
        }
        if soak.started.elapsed() >= soak.length {
            let (first, peak) = (soak.first_mb, soak.peak_mb);
            let last = drift_platform::memory::used().unwrap_or(0) / (1024 * 1024);
            soak.line(&format!(
                "done: first {first} MB, peak {peak} MB, last {last} MB"
            ));
            // Close without asking about the scribbles.
            self.quit_confirmed = true;
            self.discard_session = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Leave a prompt alone (random keys would answer it).
        if self.pending.is_some() {
            self.soak = Some(soak);
            return;
        }
        if entities > MAX_ENTITIES {
            self.perform(crate::Pending::New);
        }
        if soak.minimised > 0 {
            soak.minimised -= 1;
            if soak.minimised == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            }
            self.soak = Some(soak);
            return;
        }
        // Now and then: resize the window, or minimise it for a while.
        match soak.frame % 900 {
            300 => {
                let r = soak.next();
                let size = egui::vec2(700.0 + (r % 1500) as f32, 500.0 + ((r >> 20) % 700) as f32);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            600 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                soak.minimised = 120;
            }
            _ => {}
        }
        let canvas = self.canvas_rect;
        if canvas.is_positive() {
            let at = |r: u64| {
                Pos2::new(
                    canvas.left() + (r % 10_000) as f32 / 10_000.0 * canvas.width(),
                    canvas.top() + ((r >> 24) % 10_000) as f32 / 10_000.0 * canvas.height(),
                )
            };
            let r = soak.next();
            match r % 16 {
                0..=5 => {
                    soak.pointer = at(soak.next());
                    raw.events.push(Event::PointerMoved(soak.pointer));
                }
                6..=8 => {
                    // A click, or one end of a drag.
                    soak.down = !soak.down;
                    raw.events.push(Event::PointerButton {
                        pos: soak.pointer,
                        button: PointerButton::Primary,
                        pressed: soak.down,
                        modifiers: Modifiers::NONE,
                    });
                }
                9..=11 => {
                    let key = KEYS[(soak.next() % KEYS.len() as u64) as usize];
                    raw.events.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    });
                }
                12 => raw.events.push(Event::Key {
                    key: Key::D,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::SHIFT,
                }),
                13 | 14 => {
                    // Zoom at the pointer.
                    let amount = (soak.next() % 400) as f32 - 200.0;
                    raw.events.push(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, amount),
                        phase: egui::TouchPhase::Move,
                        modifiers: Modifiers::NONE,
                    });
                }
                _ => {
                    // Pan with the middle button.
                    let to = at(soak.next());
                    for (pos, pressed) in [(soak.pointer, true), (to, false)] {
                        raw.events.push(Event::PointerMoved(pos));
                        raw.events.push(Event::PointerButton {
                            pos,
                            button: PointerButton::Middle,
                            pressed,
                            modifiers: Modifiers::NONE,
                        });
                    }
                    soak.pointer = to;
                }
            }
        }
        self.soak = Some(soak);
    }
}
