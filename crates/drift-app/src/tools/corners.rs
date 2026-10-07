//! Fillet and Chamfer: click a corner where two lines meet. The radius / distances can be
//! typed at any time and are kept for the next corner.

use super::*;

/// The corner point under the pointer, if two lines meet there.
fn corner_under(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    let point = p.snapped_point().or_else(|| {
        ed.index.pick(p.raw, p.tolerance, |id| {
            ed.doc.get(id).is_some_and(|e| e.kind.is_point())
        })
    })?;
    ed.corner(point).ok().map(|_| point)
}

fn default_size(ed: &Editor) -> f64 {
    // A round number in the document's units.
    if ed.doc.units().length.is_metric() {
        5.0
    } else {
        0.25 * drift_core::units::MM_PER_INCH
    }
}

#[derive(Default)]
pub struct FilletTool {
    radius: Option<f64>,
    typed: [Option<f64>; 2],
    hover: Option<EntityId>,
}

impl FilletTool {
    fn radius(&self, ed: &Editor) -> f64 {
        self.typed[0]
            .or(self.radius)
            .unwrap_or_else(|| default_size(ed))
    }
}

impl Tool for FilletTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Fillet
    }

    fn hint(&self) -> String {
        "Click a corner where two lines meet to round it · type the radius".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => self.hover = corner_under(ed, &p),
            Input::Click(p, _) => {
                if let Some(corner) = corner_under(ed, &p) {
                    let r = self.radius(ed);
                    if let Err(e) = ed.fillet(corner, r) {
                        ed.notice = Some(format!("Fillet: {e}"));
                    }
                } else {
                    ed.notice = Some("Fillet: click a corner where two lines meet".into());
                }
                self.hover = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        &["Radius"]
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, _pointer: &Pointer, _ed: &mut Editor) {
        if let Some(r) = self.typed[0].filter(|r| *r > 0.0) {
            self.radius = Some(r);
        }
        self.typed = [None; 2];
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        Some(format!(
            "R {}",
            p.units
                .format_readout(self.typed[0].or(self.radius).unwrap_or(f64::NAN))
                .replace("NaN", "default")
        ))
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        self.hover
            .and_then(|c| ed.fillet_preview(c, self.radius(ed)))
            .map(preview)
            .into_iter()
            .collect()
    }
}

#[derive(Default)]
pub struct ChamferTool {
    distances: Option<(f64, f64)>,
    typed: [Option<f64>; 2],
    hover: Option<EntityId>,
}

impl ChamferTool {
    fn distances(&self, ed: &Editor) -> (f64, f64) {
        let (d1, d2) = self
            .distances
            .unwrap_or_else(|| (default_size(ed), default_size(ed)));
        let d1 = self.typed[0].unwrap_or(d1);
        // A single typed distance is used for both.
        let d2 = self.typed[1].or(self.typed[0]).unwrap_or(d2);
        (d1, d2)
    }
}

impl Tool for ChamferTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Chamfer
    }

    fn hint(&self) -> String {
        "Click a corner where two lines meet to cut it · type the distance (Tab: second distance)"
            .into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => self.hover = corner_under(ed, &p),
            Input::Click(p, _) => {
                if let Some(corner) = corner_under(ed, &p) {
                    let (d1, d2) = self.distances(ed);
                    if let Err(e) = ed.chamfer(corner, d1, d2) {
                        ed.notice = Some(format!("Chamfer: {e}"));
                    }
                } else {
                    ed.notice = Some("Chamfer: click a corner where two lines meet".into());
                }
                self.hover = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        &["Distance", "Distance 2"]
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, _pointer: &Pointer, ed: &mut Editor) {
        if self.typed.iter().flatten().all(|d| *d > 0.0) && any_typed(self.typed) {
            self.distances = Some(self.distances(ed));
        }
        self.typed = [None; 2];
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let (d1, d2) = self.distances(ed);
        self.hover
            .and_then(|c| ed.chamfer_preview(c, d1, d2))
            .map(preview)
            .into_iter()
            .collect()
    }
}
