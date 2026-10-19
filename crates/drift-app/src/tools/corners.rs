//! Fillet and Chamfer: click (or press on) a corner where two lines (for a fillet, lines or
//! arcs) meet, then move the
//! pointer to size it in steps and click to apply, as Blender's bevel does. The size can be
//! typed at any time and is kept for the next corner.

use super::*;

/// The corner point under the pointer, if two lines meet there (or, for a fillet, two lines
/// or arcs).
fn corner_under(ed: &Editor, p: &Pointer, arcs: bool) -> Option<EntityId> {
    let is_corner = |id: EntityId| {
        if arcs {
            ed.fillet_corner(id).is_ok()
        } else {
            ed.corner(id).is_ok()
        }
    };
    // The point snapped to may not be the corner but another on top of it (an arc's centre
    // where a rectangle's corner happens to land): then the corner there.
    let at = p
        .snapped_point()
        .and_then(|id| ed.doc.point(id).ok())
        .unwrap_or(p.raw);
    p.snapped_point().filter(|id| is_corner(*id)).or_else(|| {
        ed.index.pick(at, p.tolerance, |id| {
            ed.doc.get(id).is_some_and(|e| e.kind.is_point()) && is_corner(id)
        })
    })
}

/// What to tell the user when a click finds no corner: why the point under the pointer
/// isn't one, or how to pick one.
fn no_corner_notice(ed: &Editor, p: &Pointer, arcs: bool) -> String {
    let (tool, how) = if arcs {
        (
            "Fillet",
            "click a corner where two lines or arcs meet, or a fillet",
        )
    } else {
        (
            "Chamfer",
            "click a corner where two lines meet, or a chamfer",
        )
    };
    let point = p.snapped_point().or_else(|| {
        ed.index.pick(p.raw, p.tolerance, |id| {
            ed.doc.get(id).is_some_and(|e| e.kind.is_point())
        })
    });
    let why = point.and_then(|q| {
        if arcs {
            ed.fillet_corner(q).err()
        } else {
            ed.corner(q).err()
        }
    });
    match why {
        Some(e) => format!("{tool}: can't use this point: {e}"),
        None => format!("{tool}: {how}"),
    }
}

/// The fillet under the pointer: an arc between two lines or arcs, tangent to both.
fn fillet_under(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    if p.snapped_point().is_some() {
        return None;
    }
    let arc = super::curve_ops::curve_under(ed, p)?;
    ed.fillet_at(arc).map(|_| arc)
}

/// The chamfer under the pointer: a line between two lines that would meet beyond it.
fn chamfer_under(ed: &Editor, p: &Pointer) -> Option<(EntityId, crate::modify::ChamferInfo)> {
    if p.snapped_point().is_some() {
        return None;
    }
    let cut = super::curve_ops::curve_under(ed, p)?;
    ed.chamfer_at(cut).map(|c| (cut, c))
}

fn default_size(ed: &Editor) -> f64 {
    // A round number in the document's units.
    if ed.doc.units().length.is_metric() {
        5.0
    } else {
        0.25 * drift_core::units::MM_PER_INCH
    }
}

/// The smallest round increment that is at least `min` (mm): 1, 2 or 5 times a power of ten
/// in metric; binary fractions of an inch, then inches and feet, in imperial.
pub(crate) fn round_step(min: f64, metric: bool) -> f64 {
    if !(min > 0.0 && min.is_finite()) {
        return 1.0;
    }
    if metric {
        let decade = 10f64.powf(min.log10().floor());
        return [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|m| m * decade)
            .find(|s| *s >= min * (1.0 - 1e-9))
            .unwrap_or(decade * 10.0);
    }
    let inches = min / drift_core::units::MM_PER_INCH;
    const STEPS: [f64; 12] = [
        1.0 / 64.0,
        1.0 / 32.0,
        1.0 / 16.0,
        1.0 / 8.0,
        1.0 / 4.0,
        1.0 / 2.0,
        1.0,
        2.0,
        3.0,
        6.0,
        12.0,
        24.0,
    ];
    let step = STEPS
        .into_iter()
        .find(|s| *s >= inches * (1.0 - 1e-9))
        .unwrap_or_else(|| 12.0 * 10f64.powf((inches / 12.0).log10().ceil()));
    step * drift_core::units::MM_PER_INCH
}

/// The radius of a fillet whose arc passes under the pointer: its middle is r / sin(half) - r
/// from the corner. In round steps, unless Alt is held; never more than the lines have room
/// for. `None` if the pointer is on the corner.
pub(super) fn fillet_radius_at(corner: DVec2, half: f64, room: f64, p: &Pointer) -> Option<f64> {
    let mut s = Sizing {
        corner: EntityId(0),
        arc: None,
        at: corner,
        half,
        room,
        start: corner,
        moved: true,
        size: 0.0,
    };
    s.follow(p, |d, half| d / (1.0 / half.sin() - 1.0), room * half.tan());
    (s.size > 0.0).then_some(s.size)
}

/// A corner being sized: the size follows the pointer until a click applies it.
#[derive(Clone, Copy, Debug)]
struct Sizing {
    /// The corner point; or, for a fillet that is already there, its arc (the corner is
    /// then where its lines would meet).
    corner: EntityId,
    arc: Option<EntityId>,
    /// Where the corner is, half the angle between its lines, and the shorter line's length.
    at: DVec2,
    half: f64,
    room: f64,
    /// Where the pointer was when sizing began: the size stays as it was (the last one used)
    /// until the pointer moves away from here.
    start: DVec2,
    moved: bool,
    size: f64,
}

impl Sizing {
    fn begin(ed: &Editor, corner: EntityId, p: &Pointer, size: f64, arcs: bool) -> Option<Self> {
        let at = ed.doc.point(corner).ok()?;
        let (half, room) = if arcs {
            let [(_, a1), (_, a2)] = ed.fillet_corner(corner).ok()?;
            crate::modify::corner_angle(at, [a1, a2])
        } else {
            let [(_, o1), (_, o2)] = ed.corner(corner).ok()?;
            let (q1, q2) = (ed.doc.point(o1).ok()?, ed.doc.point(o2).ok()?);
            let (l1, l2) = (q1.distance(at), q2.distance(at));
            if l1 <= 0.0 || l2 <= 0.0 {
                return None;
            }
            let cos = ((q1 - at) / l1).dot((q2 - at) / l2).clamp(-1.0, 1.0);
            (cos.acos() / 2.0, l1.min(l2))
        };
        Some(Self {
            corner,
            arc: None,
            at,
            half,
            room,
            start: p.raw,
            moved: false,
            size,
        })
    }

    /// Re-size the fillet `arc`, starting from the radius it has.
    fn begin_fillet(ed: &Editor, arc: EntityId, p: &Pointer) -> Option<Self> {
        let f = ed.fillet_at(arc)?;
        let (half, room) = f.half_angle_and_room();
        Some(Self {
            corner: arc,
            arc: Some(arc),
            at: f.corner,
            half,
            room,
            start: p.raw,
            moved: false,
            size: f.radius,
        })
    }

    /// Follow the pointer: `size_at` turns its distance from the corner into a size, and
    /// `largest` is the most the lines have room for. Steps are round numbers a few pixels
    /// apart; Alt sizes freely.
    fn follow(&mut self, p: &Pointer, size_at: impl Fn(f64, f64) -> f64, largest: f64) {
        if !self.moved && p.raw.distance(self.start) < p.tolerance * 0.5 {
            return;
        }
        self.moved = true;
        let free = size_at(p.raw.distance(self.at), self.half);
        if !free.is_finite() {
            return;
        }
        let largest = largest * 0.999;
        self.size = if p.alt {
            free.min(largest)
        } else {
            let step = round_step(p.tolerance * 1.5, p.units.length.is_metric());
            let stepped = ((free / step).round() * step).max(step);
            if stepped <= largest {
                stepped
            } else if largest >= step {
                (largest / step).floor() * step
            } else {
                largest
            }
        };
    }
}

#[derive(Default)]
pub struct FilletTool {
    radius: Option<f64>,
    typed: [Option<f64>; 2],
    hover: Option<EntityId>,
    sizing: Option<Sizing>,
}

impl FilletTool {
    fn radius(&self, ed: &Editor) -> f64 {
        self.typed[0]
            .filter(|r| *r > 0.0)
            .or(self.sizing.map(|s| s.size))
            .or(self.radius)
            .unwrap_or_else(|| default_size(ed))
    }

    fn begin(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        // A fillet that is already there is re-sized in place.
        if let Some(arc) = fillet_under(ed, p) {
            self.sizing = Sizing::begin_fillet(ed, arc, p);
            self.hover = None;
            return self.sizing.is_some();
        }
        let Some(corner) = corner_under(ed, p, true) else {
            ed.notice = Some(no_corner_notice(ed, p, true));
            return false;
        };
        self.sizing = Sizing::begin(ed, corner, p, self.radius(ed), true);
        self.hover = None;
        self.sizing.is_some()
    }

    fn follow(&mut self, p: &Pointer) {
        if let Some(s) = &mut self.sizing {
            // The arc passes under the pointer: its middle is r / sin(half) - r from the
            // corner. The tangent points must stay on the lines.
            s.follow(
                p,
                |d, half| d / (1.0 / half.sin() - 1.0),
                s.room * s.half.tan(),
            );
        }
    }

    fn apply(&mut self, ed: &mut Editor) {
        let Some(s) = self.sizing.take() else {
            return;
        };
        let r = self.radius_of(&s);
        let done = match s.arc {
            Some(arc) => ed.refillet(arc, r),
            None => ed.fillet(s.corner, r).map(|_| ()),
        };
        match done {
            Ok(()) => self.radius = Some(r),
            Err(e) => ed.notice = Some(format!("Fillet: {e}")),
        }
    }

    fn radius_of(&self, s: &Sizing) -> f64 {
        self.typed[0].filter(|r| *r > 0.0).unwrap_or(s.size)
    }
}

impl Tool for FilletTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Fillet
    }

    fn hint(&self) -> String {
        if self.sizing.is_some() {
            "Move to size the fillet (Alt: freely) · click to apply · type the radius · Esc cancels"
                .into()
        } else {
            "Click a corner where two lines or arcs meet (or a fillet), then move to size it".into()
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) if self.sizing.is_some() => self.follow(&p),
            Input::Hover(p) => {
                ed.hovered = fillet_under(ed, &p);
                self.hover = corner_under(ed, &p, true);
            }
            Input::Click(p, _) if self.sizing.is_some() => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Click(p, _) => {
                self.begin(&p, ed);
            }
            // Press on the corner, drag to size, release to apply.
            Input::DragStart(p, _) if self.sizing.is_none() => {
                self.begin(&p, ed);
            }
            Input::Drag(p) => self.follow(&p),
            Input::DragEnd(p, _) => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Finish => self.apply(ed),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.sizing.take().is_some()
    }

    fn is_busy(&self) -> bool {
        self.sizing.is_some()
    }

    fn is_modal(&self) -> bool {
        self.sizing.is_some()
    }

    fn wants_snap(&self) -> bool {
        self.sizing.is_none()
    }

    fn fields(&self) -> &'static [&'static str] {
        &["Radius"]
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, _pointer: &Pointer, ed: &mut Editor) {
        if self.sizing.is_some() {
            // Typed while sizing: that radius, on this corner.
            self.apply(ed);
        } else if let Some(r) = self.typed[0].filter(|r| *r > 0.0) {
            self.radius = Some(r);
        }
        self.typed = [None; 2];
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let r = self.typed[0]
            .or(self.sizing.map(|s| s.size))
            .or(self.radius);
        // No size yet: the default (not a number, which fractional inches would show as 0").
        Some(format!(
            "R {}",
            r.map_or_else(|| "default".into(), |r| p.units.format_readout(r))
        ))
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let r = self.radius(ed);
        match self.sizing {
            Some(Sizing { arc: Some(arc), .. }) => ed.refillet_preview(arc, r),
            Some(s) => ed.fillet_preview(s.corner, r),
            None => self.hover.and_then(|c| ed.fillet_preview(c, r)),
        }
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
    sizing: Option<Sizing>,
}

impl ChamferTool {
    fn distances(&self, ed: &Editor) -> (f64, f64) {
        let (d1, d2) = match (self.sizing, self.distances) {
            // Sized with the pointer: the same distance along both lines.
            (Some(s), _) if s.moved => (s.size, s.size),
            // A chamfer that is already there, not yet moved: as it is.
            (Some(Sizing { arc: Some(cut), .. }), _) => ed
                .chamfer_at(cut)
                .map_or((default_size(ed), default_size(ed)), |c| {
                    (c.legs[0], c.legs[1])
                }),
            (_, Some(d)) => d,
            _ => (default_size(ed), default_size(ed)),
        };
        let d1 = self.typed[0].unwrap_or(d1);
        // A single typed distance is used for both.
        let d2 = self.typed[1].or(self.typed[0]).unwrap_or(d2);
        (d1, d2)
    }

    fn begin(&mut self, p: &Pointer, ed: &mut Editor) -> bool {
        // A chamfer that is already there is re-sized in place.
        if let Some((cut, c)) = chamfer_under(ed, p) {
            let (half, room) = c.half_angle_and_room();
            self.sizing = Some(Sizing {
                corner: cut,
                arc: Some(cut),
                at: c.corner,
                half,
                room,
                start: p.raw,
                moved: false,
                size: c.legs[0],
            });
            self.hover = None;
            return true;
        }
        let Some(corner) = corner_under(ed, p, false) else {
            ed.notice = Some(no_corner_notice(ed, p, false));
            return false;
        };
        let size = self.distances(ed).0;
        self.sizing = Sizing::begin(ed, corner, p, size, false);
        self.hover = None;
        self.sizing.is_some()
    }

    fn follow(&mut self, p: &Pointer) {
        if let Some(s) = &mut self.sizing {
            // The cut passes under the pointer: its middle is d · cos(half) from the corner.
            s.follow(p, |d, half| d / half.cos(), s.room);
        }
    }

    fn apply(&mut self, ed: &mut Editor) {
        let (d1, d2) = self.distances(ed);
        let Some(s) = self.sizing.take() else {
            return;
        };
        let done = match s.arc {
            Some(cut) => ed.rechamfer(cut, d1, d2),
            None => ed.chamfer(s.corner, d1, d2).map(|_| ()),
        };
        match done {
            Ok(()) => self.distances = Some((d1, d2)),
            Err(e) => ed.notice = Some(format!("Chamfer: {e}")),
        }
    }
}

impl Tool for ChamferTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Chamfer
    }

    fn hint(&self) -> String {
        if self.sizing.is_some() {
            "Move to size the chamfer (Alt: freely) · click to apply · type the distance (Tab: \
             second distance) · Esc cancels"
                .into()
        } else {
            "Click a corner where two lines meet, then move to size the chamfer".into()
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) if self.sizing.is_some() => self.follow(&p),
            Input::Hover(p) => {
                self.hover = corner_under(ed, &p, false);
                // A chamfer that would be re-sized lights up (a corner comes first).
                ed.hovered = if self.hover.is_none() {
                    chamfer_under(ed, &p).map(|(cut, _)| cut)
                } else {
                    None
                };
            }
            Input::Click(p, _) if self.sizing.is_some() => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Click(p, _) => {
                self.begin(&p, ed);
            }
            Input::DragStart(p, _) if self.sizing.is_none() => {
                self.begin(&p, ed);
            }
            Input::Drag(p) => self.follow(&p),
            Input::DragEnd(p, _) => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Finish => self.apply(ed),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.sizing.take().is_some()
    }

    fn is_busy(&self) -> bool {
        self.sizing.is_some()
    }

    fn is_modal(&self) -> bool {
        self.sizing.is_some()
    }

    fn wants_snap(&self) -> bool {
        self.sizing.is_none()
    }

    fn fields(&self) -> &'static [&'static str] {
        &["Distance", "Distance 2"]
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, _pointer: &Pointer, ed: &mut Editor) {
        let valid = self.typed.iter().flatten().all(|d| *d > 0.0) && any_typed(self.typed);
        if self.sizing.is_some() {
            if valid {
                self.apply(ed);
            }
        } else if valid {
            self.distances = Some(self.distances(ed));
        }
        self.typed = [None; 2];
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let s = self.sizing.filter(|s| s.moved)?;
        Some(p.units.format_readout(self.typed[0].unwrap_or(s.size)))
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let (d1, d2) = self.distances(ed);
        match self.sizing {
            Some(Sizing { arc: Some(cut), .. }) => ed.rechamfer_preview(cut, d1, d2),
            Some(s) => ed.chamfer_preview(s.corner, d1, d2),
            None => self.hover.and_then(|c| ed.chamfer_preview(c, d1, d2)),
        }
        .map(preview)
        .into_iter()
        .collect()
    }
}
