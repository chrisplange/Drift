//! Offset (click a curve, then the side and distance) and Mirror (the selection about a line).

use super::curve_ops::curve_under;
use super::*;

/// The picked curve, its chain (curve, direction) and whether the chain closes.
type Chain = (EntityId, Vec<(EntityId, bool)>, bool);

/// Click a curve to pick its connected chain, then click on the side to offset to: the
/// distance follows the pointer, or type it.
#[derive(Default)]
pub struct OffsetTool {
    chain: Option<Chain>,
    typed: [Option<f64>; 2],
    /// Pointer and its distance from the chain, for the preview.
    side: Option<(DVec2, f64)>,
    shapes: Vec<Shape>,
}

impl OffsetTool {
    fn distance(&self) -> Option<f64> {
        self.typed[0]
            .filter(|d| *d > 0.0)
            .or(self.side.map(|(_, d)| d))
    }

    fn update(&mut self, ed: &Editor, at: DVec2) {
        let Some((_, chain, closed)) = &self.chain else {
            return;
        };
        let d = chain
            .iter()
            .filter_map(|(id, _)| ed.doc.shape(*id))
            .map(|s| drift_geom::distance(&s, at))
            .fold(f64::INFINITY, f64::min);
        self.side = Some((at, d));
        let signed = ed.offset_side(chain, at) * self.distance().unwrap_or(d);
        self.shapes = ed
            .offset_shapes(chain, *closed, signed)
            .map(|pieces| pieces.into_iter().map(|p| p.shape).collect())
            .unwrap_or_default();
    }

    fn place(&mut self, ed: &mut Editor) {
        let (Some((id, ..)), Some((side, _)), Some(d)) =
            (self.chain.clone(), self.side, self.distance())
        else {
            return;
        };
        if let Err(e) = ed.offset(id, d, side) {
            ed.out.notice = Some(format!("Offset: {e}"));
        }
        self.reset(ed);
    }

    fn reset(&mut self, ed: &mut Editor) {
        self.chain = None;
        self.side = None;
        self.shapes.clear();
        self.typed = [None; 2];
        ed.highlight.clear();
    }
}

impl Tool for OffsetTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Offset
    }

    fn hint(&self) -> String {
        match self.chain {
            None => "Click a curve to offset it (with the curves connected to it)".into(),
            Some(_) => "Click the side to offset to · type the distance".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                if self.chain.is_some() {
                    self.update(ed, p.raw);
                } else {
                    ed.hovered = curve_under(ed, &p);
                }
            }
            Input::Click(p, _) => {
                if self.chain.is_some() {
                    self.update(ed, p.raw);
                    self.place(ed);
                } else if let Some(id) = curve_under(ed, &p) {
                    let (chain, closed) = ed.chain(id);
                    ed.highlight = chain.iter().map(|(c, _)| *c).collect();
                    self.chain = Some((id, chain, closed));
                    self.update(ed, p.raw);
                }
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        let had = self.chain.is_some();
        self.reset(ed);
        had
    }

    fn is_busy(&self) -> bool {
        self.chain.is_some()
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.chain.is_some() {
            &["Distance"]
        } else {
            &[]
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, _pointer: &Pointer, ed: &mut Editor) {
        if self.side.is_some() {
            self.place(ed);
        }
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.shapes.iter().map(|s| preview(*s)).collect()
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        self.distance()
            .filter(|_| self.chain.is_some())
            .map(|d| format!("Offset {}", p.units.format_readout(d)))
    }
}

/// Mirror the selection about a line: click the line.
#[derive(Default)]
pub struct MirrorTool;

impl Tool for MirrorTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Mirror
    }

    fn hint(&self) -> String {
        "Click the line to mirror the selection about (select geometry first with Select)".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                ed.hovered = curve_under(ed, &p).filter(|id| {
                    matches!(
                        ed.doc.get(*id).map(|e| e.kind),
                        Some(EntityKind::Line { .. })
                    )
                });
            }
            Input::Click(p, _) => {
                let Some(axis) = curve_under(ed, &p) else {
                    return;
                };
                let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
                let instances: Vec<drift_core::InstanceId> =
                    ed.selected_instances.iter().copied().collect();
                let riders = ed.riders();
                if ids.iter().all(|id| *id == axis) && instances.is_empty() && riders.is_empty() {
                    ed.out.notice = Some(
                        "Mirror: select the geometry to mirror first (Select tool), then pick \
                         Mirror and click the line"
                            .into(),
                    );
                    return;
                }
                match ed.mirror_with(&ids, &instances, axis, &riders) {
                    Ok((copies, instances, (texts, notes))) => {
                        ed.selection = copies.into_iter().collect();
                        ed.selected_instances = instances.into_iter().collect();
                        ed.selected_texts = texts.into_iter().collect();
                        ed.selected_annotation = None;
                        ed.annotation_picks = notes
                            .into_iter()
                            .map(drift_core::AnnotationRef::Annotation)
                            .collect();
                    }
                    Err(e) => ed.out.notice = Some(format!("Mirror: {e}")),
                }
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

    fn wants_snap(&self) -> bool {
        false
    }
}
