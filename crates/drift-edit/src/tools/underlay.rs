//! Calibrate and move underlays: pick two points on the underlay, then type the real distance
//! between them (calibrate), or pick a point on it and where it should go (move).

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnderlayMode {
    #[default]
    Calibrate,
    Move,
    /// Calibrate what the last import brought in (scaling it, not an underlay).
    CalibrateImport,
}

#[derive(Default)]
pub struct UnderlayTool {
    pub(super) mode: UnderlayMode,
    pub(super) first: Option<DVec2>,
    pub(super) second: Option<DVec2>,
    pub(super) typed: Option<f64>,
}

impl UnderlayTool {
    pub fn new(mode: UnderlayMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    fn reset(&mut self) {
        self.first = None;
        self.second = None;
        self.typed = None;
    }

    fn calibrate(&mut self, b: DVec2, distance: f64, ed: &mut Editor) {
        if self.mode == UnderlayMode::CalibrateImport {
            if let Some(a) = self.first {
                ed.out.notice = Some(match ed.calibrate_import(a, b, distance) {
                    Ok(k) => format!(
                        "Calibrated: that distance is now {} (scaled × {})",
                        ed.doc.units().format_readout(distance),
                        trim_factor(k)
                    ),
                    Err(e) => e,
                });
            }
            self.reset();
            return;
        }
        let (Some(id), Some(a)) = (ed.active_underlay(), self.first) else {
            return;
        };
        match ed.calibrate_underlay(id, a, b, distance) {
            Ok(()) => {
                let units = ed.doc.units();
                let scale = ed
                    .doc
                    .underlay(id)
                    .map(|u| (u.scale, u.source.contains(", page")));
                let ratio = match scale {
                    // A PDF drawn at 1 : n on paper: n mm in the sketch per mm on the page.
                    Some((s, true)) => {
                        format!(
                            ", a 1 : {:.1} drawing",
                            s / drift_core::underlays::MM_PER_POINT
                        )
                    }
                    Some((s, false)) => format!(", scale × {s:.4}"),
                    None => String::new(),
                };
                ed.out.notice = Some(format!(
                    "Calibrated: that distance is now {}{ratio}",
                    units.format_readout(distance)
                ));
            }
            Err(e) => ed.out.notice = Some(e),
        }
        self.reset();
    }
}

impl Tool for UnderlayTool {
    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn kind(&self) -> ToolKind {
        match self.mode {
            UnderlayMode::Calibrate | UnderlayMode::CalibrateImport => ToolKind::Calibrate,
            UnderlayMode::Move => ToolKind::MoveUnderlay,
        }
    }

    fn hint(&self) -> String {
        match (self.mode, self.first, self.second) {
            (UnderlayMode::Calibrate, None, _) => {
                "Calibrate: click a point of known distance on the underlay (e.g. a wall end)"
                    .into()
            }
            (UnderlayMode::CalibrateImport, None, _) => {
                "Calibrate the import: click a point of known distance on it (e.g. a wall end)"
                    .into()
            }
            (UnderlayMode::Calibrate | UnderlayMode::CalibrateImport, Some(_), None) => {
                "Click the other end, or type the real distance and press Enter".into()
            }
            (UnderlayMode::Calibrate | UnderlayMode::CalibrateImport, Some(_), Some(_)) => {
                "Type the real distance between the two points and press Enter".into()
            }
            (UnderlayMode::Move, None, _) => "Move underlay: click a point on it".into(),
            (UnderlayMode::Move, Some(_), _) => "Click where that point goes".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let Input::Click(p, _) = input else {
            if matches!(input, Input::Finish) {
                self.reset();
            }
            return;
        };
        if self.mode == UnderlayMode::CalibrateImport {
            let at = p.pos();
            match self.first {
                None => self.first = Some(at),
                Some(a) if a.distance(at) > 1e-9 => self.second = Some(at),
                Some(_) => {}
            }
            return;
        }
        let Some(id) = ed.active_underlay() else {
            ed.out.notice = Some("Pick an underlay in the Layers tab first".into());
            return;
        };
        if ed.doc.underlay(id).is_some_and(|u| u.locked) {
            ed.out.notice = Some("The underlay is locked: unlock it in the Layers tab".into());
            return;
        }
        let at = p.pos();
        match (self.mode, self.first) {
            (_, None) => self.first = Some(at),
            (UnderlayMode::Calibrate | UnderlayMode::CalibrateImport, Some(a)) => {
                if a.distance(at) > 1e-9 {
                    self.second = Some(at);
                }
            }
            (UnderlayMode::Move, Some(from)) => {
                if let Err(e) = ed.move_underlay(id, from, at) {
                    ed.out.notice = Some(e);
                }
                self.reset();
            }
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let busy = self.first.is_some();
        self.reset();
        busy
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let Some(a) = self.first else {
            return Vec::new();
        };
        let Some(b) = self.second.or_else(|| pointer.map(Pointer::pos)) else {
            return vec![preview_vertex(a)];
        };
        vec![
            preview(Shape::Line { a, b }),
            preview_vertex(a),
            preview_vertex(b),
        ]
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let a = self.first?;
        let b = self.second.unwrap_or_else(|| p.pos());
        Some(match self.mode {
            UnderlayMode::Calibrate | UnderlayMode::CalibrateImport => {
                format!("Measured {}", p.units.format_readout(a.distance(b)))
            }
            UnderlayMode::Move => format!("Move {}", p.units.format_readout(a.distance(b))),
        })
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.mode != UnderlayMode::Move && self.first.is_some() {
            &["Distance"]
        } else {
            &[]
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values[0];
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let Some(distance) = self.typed else {
            return;
        };
        let b = self.second.unwrap_or_else(|| p.pos());
        self.calibrate(b, distance, ed);
    }
}

/// A scale factor, without trailing zeros.
fn trim_factor(k: f64) -> String {
    let s = format!("{k:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
