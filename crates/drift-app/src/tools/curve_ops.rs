//! Tools that act on the curve under the pointer: Trim (click or drag across pieces), Extend
//! (to the next curve) and Split (at a point).

use super::*;
use crate::editor::EditError;
use crate::modify::CuttingSet;

/// The line, arc or circle under the pointer.
pub(super) fn curve_under(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    ed.index.pick(p.raw, p.tolerance, |id| {
        ed.doc.get(id).is_some_and(|e| !e.kind.is_point()) && !ed.is_locked(id)
    })
}

fn report(ed: &mut Editor, what: &str, result: Result<(), crate::editor::EditError>) {
    if let Err(e) = result {
        ed.notice = Some(format!("{what}: {e}"));
    }
}

/// The curves picked when a Trim or Extend tool starts are the only ones it cuts against or
/// reaches (AutoCAD's cutting edges); with no curves picked, every curve counts. Read at the
/// tool's first input, and let go of with Esc.
#[derive(Default)]
struct Edges {
    read: bool,
    against: Option<CuttingSet>,
    /// How many curves cut, for the hint.
    count: usize,
}

impl Edges {
    fn read(&mut self, ed: &Editor) {
        if !self.read {
            self.read = true;
            self.against = CuttingSet::of(&ed.doc, ed.selection.iter().copied());
            self.count(ed);
        }
    }

    /// After an edit: the picked curves are the cutting curves as they are now (a trimmed
    /// one's far piece among them).
    fn follow(&mut self, ed: &mut Editor) {
        if let Some(set) = &self.against {
            ed.selection = set.members(&ed.doc).into_iter().collect();
        }
        self.count(ed);
    }

    fn count(&mut self, ed: &Editor) {
        self.count = self
            .against
            .as_ref()
            .map_or(0, |s| s.members(&ed.doc).len());
    }

    /// Esc: every curve counts again. False if that was already so.
    fn let_go(&mut self, ed: &mut Editor) -> bool {
        if self.against.take().is_none() {
            return false;
        }
        self.count = 0;
        ed.selection.clear();
        true
    }

    fn hint(&self, what: &str, rest: &str) -> String {
        match self.count {
            0 if self.against.is_none() => rest.to_owned(),
            n => format!(
                "{what} against {n} picked curve{} · {rest} · Esc for all curves",
                if n == 1 { "" } else { "s" }
            ),
        }
    }
}

/// What a drag across pieces has done so far: one undo step at the end, with one notice.
struct DragTrim {
    /// The history's reading before the first piece.
    before: u64,
    trimmed: usize,
    /// What the trims said (relations taken off, fills changed), each once.
    notes: Vec<String>,
    /// Why pieces weren't trimmed, each reason once, with how many.
    skipped: Vec<(String, usize)>,
}

impl DragTrim {
    fn summary(&self) -> Option<String> {
        let skipped: usize = self.skipped.iter().map(|(_, n)| n).sum();
        if self.trimmed <= 1 && skipped == 0 {
            return self.notes.first().cloned();
        }
        let mut out = format!(
            "Trim: {} piece{}",
            self.trimmed,
            if self.trimmed == 1 { "" } else { "s" }
        );
        for note in &self.notes {
            out.push_str(". ");
            out.push_str(note);
        }
        if skipped > 0 {
            let why: Vec<&str> = self.skipped.iter().map(|(w, _)| w.as_str()).collect();
            out.push_str(&format!(
                ". {skipped} piece{} trimmed: {}",
                if skipped == 1 { " wasn't" } else { "s weren't" },
                why.join("; ")
            ));
        }
        Some(out)
    }
}

/// Click a piece of a curve to remove it up to the nearest curves that cut it; drag across
/// pieces to trim each one the pointer passes over (power trim), as one undo step.
#[derive(Default)]
pub struct TrimTool {
    hover: Option<(EntityId, DVec2)>,
    dragging: bool,
    /// Where the drag has been, to show what it passed through.
    trail: Vec<DVec2>,
    edges: Edges,
    drag: Option<DragTrim>,
}

impl TrimTool {
    /// Trim the piece of `id` at `at`, against the picked curves if any. In a drag, what it
    /// did or why it couldn't is kept for the drag's notice.
    fn trim(&mut self, ed: &mut Editor, id: EntityId, at: DVec2) -> Result<(), EditError> {
        let said = ed.notice.take();
        let r = ed.trim_at_against(id, at, self.edges.against.as_mut());
        if let Some(d) = &mut self.drag {
            match &r {
                Ok(()) => {
                    d.trimmed += 1;
                    if let Some(n) = ed.notice.take()
                        && !d.notes.contains(&n)
                    {
                        d.notes.push(n);
                    }
                }
                Err(e) => {
                    let why = e.to_string();
                    match d.skipped.iter_mut().find(|(w, _)| *w == why) {
                        Some((_, n)) => *n += 1,
                        None => d.skipped.push((why, 1)),
                    }
                }
            }
        }
        if ed.notice.is_none() {
            ed.notice = said;
        }
        self.edges.follow(ed);
        r
    }

    /// The drag is over: its trims become one undo step, and its notice says what it did.
    fn end_drag(&mut self, ed: &mut Editor) {
        self.dragging = false;
        self.trail.clear();
        let Some(d) = self.drag.take() else {
            return;
        };
        // (A trim that failed rolled itself back and added no step.)
        let n = (ed.history.recorded() - d.before) as usize;
        if n > 1 {
            ed.history.squash_last(n, "Trim");
        }
        if let Some(summary) = d.summary() {
            ed.notice = Some(summary);
        }
    }

    /// Trim every curve the pointer crossed between its last position and `to`.
    fn sweep(&mut self, to: DVec2, tolerance: f64, ed: &mut Editor) {
        let Some(&from) = self.trail.last() else {
            self.trail.push(to);
            return;
        };
        // Tiny movements wait until they add up.
        if from.distance(to) < tolerance * 0.5 {
            return;
        }
        self.trail.push(to);
        let path = Shape::Line { a: from, b: to };
        let mut bounds = path.bounds();
        bounds.include(bounds.min - DVec2::splat(tolerance));
        bounds.include(bounds.max + DVec2::splat(tolerance));
        // Where the path crosses each curve, nearest the start first.
        let mut crossed: Vec<(f64, EntityId, DVec2)> = Vec::new();
        for id in ed.index.query(bounds) {
            if ed.is_locked(id) {
                continue;
            }
            let Some(shape) = ed
                .doc
                .get(id)
                .filter(|e| !e.kind.is_point())
                .and_then(|_| ed.doc.shape(id))
            else {
                continue;
            };
            for hit in drift_geom::intersect::intersect(&path, &shape) {
                crossed.push((hit.t[0], id, hit.at));
            }
        }
        crossed.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, id, at) in crossed {
            // (A curve an earlier trim removed is simply gone.)
            if ed.doc.contains(id) {
                let _ = self.trim(ed, id, at);
            }
        }
    }
}

impl Tool for TrimTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Trim
    }

    fn hint(&self) -> String {
        self.edges.hint(
            "Trimming",
            "Click a piece to remove it · drag across pieces to trim several",
        )
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        self.edges.read(ed);
        match input {
            Input::Hover(p) => {
                // The removal preview is the highlight; the whole curve isn't hovered.
                self.hover = curve_under(ed, &p).map(|id| (id, p.raw));
            }
            Input::Click(p, _) => {
                if let Some(id) = curve_under(ed, &p) {
                    let r = self.trim(ed, id, p.raw);
                    report(ed, "Trim", r);
                }
                self.hover = None;
            }
            Input::DragStart(p, _) => {
                self.dragging = true;
                self.trail = vec![p.raw];
                // Before the first piece, so that it is in the drag's one undo step.
                self.drag = Some(DragTrim {
                    before: ed.history.recorded(),
                    trimmed: 0,
                    notes: Vec::new(),
                    skipped: Vec::new(),
                });
                if let Some(id) = curve_under(ed, &p) {
                    let _ = self.trim(ed, id, p.raw);
                }
                self.hover = None;
            }
            Input::Drag(p) => {
                self.dragging = true;
                self.sweep(p.raw, p.tolerance, ed);
                self.hover = None;
            }
            Input::DragEnd(..) => self.end_drag(ed),
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        if self.dragging {
            // What the drag trimmed so far stays, as one step.
            self.end_drag(ed);
            return true;
        }
        self.edges.let_go(ed)
    }

    fn is_busy(&self) -> bool {
        self.dragging
    }

    fn wants_snap(&self) -> bool {
        false
    }

    /// The path dragged so far, as SolidWorks' power trim leaves a trail.
    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.trail
            .windows(2)
            .map(|w| {
                SketchItem::new(Shape::Line { a: w[0], b: w[1] }).with_emphasis(Emphasis::Preview)
            })
            .collect()
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        self.hover
            .and_then(|(id, at)| ed.trim_preview_against(id, at, self.edges.against.as_ref()))
            .map(|shape| vec![SketchItem::new(shape).with_emphasis(Emphasis::Remove)])
            .unwrap_or_default()
    }
}

/// Click near the end of a line or arc to extend it to the next curve in its way (the next
/// picked one, when curves were picked).
#[derive(Default)]
pub struct ExtendTool {
    preview: Option<Shape>,
    edges: Edges,
}

impl ExtendTool {
    fn target(ed: &Editor, p: &Pointer, against: Option<&CuttingSet>) -> Option<Shape> {
        let id = curve_under(ed, p)?;
        let (end, at, ..) = ed.extend_target_against(id, p.raw, against).ok()?;
        let from = ed.doc.point(end).ok()?;
        Some(match ed.doc.shape(id)? {
            Shape::Arc { center, .. } => {
                // The extension follows the circle from the end.
                let (s, e) =
                    if ed.doc.get(id).is_some_and(
                        |x| matches!(x.kind, EntityKind::Arc { end: e, .. } if e == end),
                    ) {
                        (from, at)
                    } else {
                        (at, from)
                    };
                arc_shape(center, s, e)
            }
            _ => Shape::Line { a: from, b: at },
        })
    }
}

impl Tool for ExtendTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Extend
    }

    fn hint(&self) -> String {
        self.edges.hint(
            "Extending",
            "Click near the end of a line or arc to extend it to the next curve",
        )
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        self.edges.read(ed);
        match input {
            Input::Hover(p) => {
                self.preview = Self::target(ed, &p, self.edges.against.as_ref());
                ed.hovered = curve_under(ed, &p);
            }
            Input::Click(p, _) => {
                if let Some(id) = curve_under(ed, &p) {
                    let r = ed.extend_at_against(id, p.raw, self.edges.against.as_ref());
                    report(ed, "Extend", r);
                    self.edges.follow(ed);
                }
                self.preview = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        self.edges.let_go(ed)
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.preview.map(preview).into_iter().collect()
    }
}

/// Click a line or arc to split it in two at that point.
#[derive(Default)]
pub struct SplitTool {
    at: Option<DVec2>,
}

impl Tool for SplitTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Split
    }

    fn hint(&self) -> String {
        "Click a curve where it should be split (a circle or ellipse into halves)".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let place = |p: &Pointer, ed: &Editor| {
            let id = curve_under(ed, p)?;
            let shape = ed.doc.shape(id)?;
            let t = crate::modify::param_of(&shape, p.pos());
            Some((id, crate::modify::point_at(&shape, t)))
        };
        match input {
            Input::Hover(p) => {
                self.at = place(&p, ed).map(|(_, at)| at);
                ed.hovered = curve_under(ed, &p);
            }
            Input::Click(p, _) => {
                if let Some((id, at)) = place(&p, ed) {
                    let r = ed.split_at(id, at).map(|_| ());
                    report(ed, "Split", r);
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

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.at.map(preview_vertex).into_iter().collect()
    }
}
