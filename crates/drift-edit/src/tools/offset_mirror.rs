//! Offset (click a curve, then the side and distance) and Mirror (the selection about a line, or
//! about a temporary axis through two clicked points).

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
        match ed.offset(id, d, side) {
            // The offset is picked: its Inspector shows the distance.
            Ok(x) => {
                ed.selection = ed.offset_pick_set(x);
                ed.selected_fills.clear();
                ed.selected_region = None;
            }
            Err(e) => ed.out.notice = Some(format!("Offset: {e}")),
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

/// Mirror the selection: click a line to mirror about it (the copies stay symmetric to the
/// originals), or click two points anywhere else for a temporary axis, drawn nowhere and
/// previewed while the second point is picked.
#[derive(Default)]
pub struct MirrorTool {
    /// The temporary axis's first point, once clicked.
    first: Option<DVec2>,
    /// The mirrored selection about the axis to the pointer, and that axis.
    shapes: Vec<Shape>,
    axis: Option<(DVec2, DVec2)>,
}

/// The line under the pointer that a click mirrors about. A click on a line (anywhere on it,
/// its ends and midpoint too) always uses the line, as before; a two-point axis starts from a
/// click anywhere off a line, and its second point may snap anywhere.
fn axis_line_under(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    curve_under(ed, p).filter(|id| {
        matches!(
            ed.doc.get(*id).map(|e| e.kind),
            Some(EntityKind::Line { .. })
        )
    })
}

impl MirrorTool {
    /// What is mirrored: the selection, its block copies and the text and notes picked with
    /// it; or, with nothing to mirror, an explanation.
    fn picks(
        ed: &mut Editor,
        axis: Option<EntityId>,
    ) -> Option<(
        Vec<EntityId>,
        Vec<drift_core::InstanceId>,
        crate::picks::Riders,
    )> {
        let ids: Vec<EntityId> = ed.selection.iter().copied().collect();
        let instances: Vec<drift_core::InstanceId> =
            ed.selected_instances.iter().copied().collect();
        let riders = ed.riders();
        if ids.iter().all(|id| Some(*id) == axis) && instances.is_empty() && riders.is_empty() {
            ed.out.notice = Some(
                "Mirror: select the geometry to mirror first (Select tool), then pick Mirror \
                 and click a line or two points"
                    .into(),
            );
            return None;
        }
        Some((ids, instances, riders))
    }

    fn mirror(ed: &mut Editor, axis: crate::modify::MirrorAxis) {
        let line = match axis {
            crate::modify::MirrorAxis::Line(id) => Some(id),
            crate::modify::MirrorAxis::Points(..) => None,
        };
        let Some((ids, instances, riders)) = Self::picks(ed, line) else {
            return;
        };
        match ed.mirror_about(&ids, &instances, axis, &riders) {
            Ok((copies, instances, (texts, notes))) => {
                ed.selection = copies.into_iter().collect();
                ed.pick_made_copies(instances);
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

    /// Preview the selection mirrored about the axis from the first point to `to`.
    fn update(&mut self, ed: &Editor, to: DVec2) {
        let Some(from) = self.first else {
            return;
        };
        self.shapes.clear();
        self.axis = None;
        if (to - from).length() < 1e-9 {
            return;
        }
        self.axis = Some((from, to));
        self.shapes = ed
            .selection
            .iter()
            .filter(|id| ed.doc.get(**id).is_some_and(|e| !e.kind.is_point()))
            .filter_map(|id| ed.doc.shape(*id))
            .map(|s| crate::modify::reflect_shape(from, to, s))
            .collect();
    }

    fn reset(&mut self) {
        self.first = None;
        self.shapes.clear();
        self.axis = None;
    }
}

impl Tool for MirrorTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Mirror
    }

    fn hint(&self) -> String {
        match self.first {
            None => "Click a line to mirror about it, or click off any line to start a \
                     two-point axis · select the geometry first with Select"
                .into(),
            Some(_) => "Click the axis's second point · Esc cancels".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                if self.first.is_some() {
                    ed.hovered = None;
                    self.update(ed, p.pos());
                } else {
                    ed.hovered = axis_line_under(ed, &p);
                }
            }
            Input::Click(p, _) => match self.first {
                Some(from) => {
                    let to = p.pos();
                    if (to - from).length() <= p.tolerance * 1e-3 {
                        return; // the same point again: keep picking
                    }
                    self.reset();
                    Self::mirror(ed, crate::modify::MirrorAxis::Points(from, to));
                }
                None => {
                    if let Some(line) = axis_line_under(ed, &p) {
                        Self::mirror(ed, crate::modify::MirrorAxis::Line(line));
                    } else if Self::picks(ed, None).is_some() {
                        self.first = Some(p.pos());
                        ed.hovered = None;
                        self.update(ed, p.pos());
                    }
                }
            },
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.first.is_some();
        self.reset();
        had
    }

    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn snap_from(&self) -> Option<DVec2> {
        self.first
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let mut items: Vec<SketchItem> = self.shapes.iter().map(|s| preview(*s)).collect();
        if let Some((a, b)) = self.axis {
            items.push(preview(Shape::Line { a, b }).with_kind(drift_core::LineKind::Center));
        }
        if let Some(a) = self.first {
            items.push(preview_vertex(a));
        }
        items
    }
}
