//! Fillet and Chamfer: click (or press on) a corner where two lines (for a fillet, lines or
//! arcs) meet, then move the
//! pointer to size it in steps and click to apply, as Blender's bevel does. The size can be
//! typed at any time and is kept for the next corner. Shift+click picks several corners, and
//! a shape picked when the tool starts brings all of its corners: one size then rounds (or
//! cuts) them all in one undo step (#293).

use std::collections::HashMap;

use super::picking::{Rule, entity_at};
use super::*;
use crate::modify::{Arm, Run, corner_angle};
use drift_core::tolerance;

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
        entity_at(ed, at, p.tolerance, Rule::ANY, |id| {
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
        entity_at(ed, p.raw, p.tolerance, Rule::ANY, |id| {
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
pub fn round_step(min: f64, metric: bool) -> f64 {
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
        arc: None,
        at: corner,
        half,
        room,
        largest: None,
        start: corner,
        moved: true,
        size: 0.0,
    };
    s.follow(p, |d, half| d / (1.0 / half.sin() - 1.0), room * half.tan());
    (s.size > 0.0).then_some(s.size)
}

/// The two curves meeting at corner `p` and how each runs from it: lines or arcs for a
/// fillet (`arcs`), lines for a chamfer.
fn arms(ed: &Editor, p: EntityId, arcs: bool) -> Option<[(EntityId, Arm); 2]> {
    if arcs {
        return ed.fillet_corner(p).ok();
    }
    let [(l1, _), (l2, _)] = ed.corner(p).ok()?;
    Some([(l1, ed.arm(l1, p)?), (l2, ed.arm(l2, p)?)])
}

/// Whether point `p` is a corner the tool can round (or cut): two curves end there, and not
/// in line (where a fillet meets its line, say).
fn is_corner(ed: &Editor, p: EntityId, arcs: bool) -> bool {
    let (Some(at), Some([(_, a1), (_, a2)])) = (ed.doc.point(p).ok(), arms(ed, p, arcs)) else {
        return false;
    };
    let alpha = 2.0 * corner_angle(at, [a1, a2]).0;
    (tolerance::ANGLE..=PI - tolerance::ANGLE).contains(&alpha)
}

/// The largest fillet `fillet_geometry` makes at `at` between `arms`.
fn largest_fillet(at: DVec2, arms: [Arm; 2]) -> f64 {
    let fits = |r: f64| crate::modify::fillet_geometry(at, arms, r).is_ok();
    let (half, room) = corner_angle(at, arms);
    let (mut lo, mut hi) = (0.0, (room * half.tan()).max(tolerance::SAME));
    for _ in 0..64 {
        if !fits(hi) {
            break;
        }
        (lo, hi) = (hi, hi * 2.0);
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The largest size (a fillet's radius, or a chamfer's distance along both lines) that fits
/// every corner in `set` at once. Corners are rounded one after another, so a side between
/// two of them gives each only what the other leaves: for lines between lines, their cuts
/// together must fit along it. A corner with an arc takes its share of each curve (half of
/// one it shares), which is always enough for both.
fn largest(ed: &Editor, set: &[EntityId], arcs: bool) -> f64 {
    let corners: Vec<(DVec2, [(EntityId, Arm); 2])> = set
        .iter()
        .filter_map(|&c| Some((ed.doc.point(c).ok()?, arms(ed, c, arcs)?)))
        .collect();
    let straight =
        |arms: &[(EntityId, Arm); 2]| arms.iter().all(|(_, a)| matches!(a.run, Run::Line { .. }));
    // How far along each line a corner's cut reaches, per unit of size.
    let reach = |at: DVec2, arms: &[(EntityId, Arm); 2]| {
        if arcs {
            1.0 / corner_angle(at, [arms[0].1, arms[1].1]).0.tan()
        } else {
            1.0
        }
    };
    // Each curve: how many corners of the set it runs from, whether they are all between
    // lines, and (if so) how far their cuts reach along it together, per unit of size.
    let mut on: HashMap<EntityId, (u32, bool, f64)> = HashMap::new();
    for (at, arms) in &corners {
        for (curve, _) in arms {
            let e = on.entry(*curve).or_insert((0, true, 0.0));
            e.0 += 1;
            e.1 &= straight(arms);
            e.2 += reach(*at, arms);
        }
    }
    let mut largest = f64::INFINITY;
    for (at, arms) in &corners {
        if straight(arms) {
            for (curve, arm) in arms {
                let (n, lines, along) = on[curve];
                largest = largest.min(if lines {
                    arm.length / along
                } else {
                    arm.length / f64::from(n) / reach(*at, arms)
                });
            }
        } else {
            let share = (*arms).map(|(curve, arm)| Arm {
                length: arm.length / f64::from(on[&curve].0),
                ..arm
            });
            largest = largest.min(largest_fillet(*at, share));
        }
    }
    largest
}

/// The corners picked to round or cut together (#293): Shift+click adds or removes one, and
/// a shape picked when the tool starts brings all of its corners.
#[derive(Default)]
struct CornerSet {
    corners: Vec<EntityId>,
    /// The picks have been read (once each time the tool is taken up).
    seeded: bool,
    /// The drawing's make-up and the group open for editing when the set was last checked.
    checked: Option<(u64, Option<drift_core::GroupId>)>,
    /// The corner the click that began sizing added (Esc takes it out again).
    added: Option<EntityId>,
    /// A refusal lit the corner it was refused at (in `ed.highlight`).
    marked: bool,
}

impl CornerSet {
    /// Before the tool acts: the picks read the first time, and corners that are no longer
    /// corners (undone, changed by the AI link) or are outside a group being edited dropped.
    fn ready(&mut self, ed: &mut Editor, arcs: bool) {
        if !self.seeded {
            self.seed(ed, arcs);
        }
        let now = (ed.doc.structure(), ed.edit_group());
        if self.checked != Some(now) {
            self.checked = Some(now);
            self.corners
                .retain(|c| ed.in_scope(*c) && is_corner(ed, *c, arcs));
            self.added = self.added.filter(|c| self.corners.contains(c));
        }
    }

    /// The corners of what is picked: points picked themselves, or whose two curves are
    /// both picked (a shape's sides, a filled shape's or an area's outline).
    fn seed(&mut self, ed: &mut Editor, arcs: bool) {
        self.seeded = true;
        let is_point =
            |ed: &Editor, id: &EntityId| ed.doc.get(*id).is_some_and(|e| e.kind.is_point());
        let mut curves: BTreeSet<EntityId> = ed
            .selection
            .iter()
            .filter(|id| !is_point(ed, id))
            .copied()
            .collect();
        for f in &ed.selected_fills {
            let Some(fill) = ed.doc.fill(*f) else {
                continue;
            };
            match fill.path.and_then(|p| ed.doc.path(p)) {
                Some(p) if fill.boundary.is_empty() => curves.extend(p.curves.iter().copied()),
                _ => curves.extend(fill.boundary.iter().copied()),
            }
        }
        if ed.selected_region.is_some()
            && let Some(region) = ed.region()
        {
            curves.extend(ed.region_curves(&region));
        }
        let picked: BTreeSet<EntityId> = ed
            .selection
            .iter()
            .filter(|id| is_point(ed, id))
            .copied()
            .collect();
        let ends = curves
            .iter()
            .filter_map(|c| ed.doc.get(*c).and_then(|e| e.kind.ends()))
            .flatten();
        let candidates: BTreeSet<EntityId> = picked.iter().copied().chain(ends).collect();
        self.corners = candidates
            .into_iter()
            .filter(|p| {
                let both =
                    arms(ed, *p, arcs).is_some_and(|a| a.iter().all(|(c, _)| curves.contains(c)));
                (picked.contains(p) || both) && ed.in_scope(*p) && is_corner(ed, *p, arcs)
            })
            .collect();
    }

    /// The corners as they stand for a preview (which can't drop the stale ones itself).
    fn live<'a>(&'a self, ed: &'a Editor, arcs: bool) -> impl Iterator<Item = EntityId> + 'a {
        let fresh = self.checked == Some((ed.doc.structure(), ed.edit_group()));
        self.corners
            .iter()
            .copied()
            .filter(move |c| fresh || (ed.in_scope(*c) && is_corner(ed, *c, arcs)))
    }

    /// Shift+click: the corner under the pointer in or out of the set.
    fn toggle(&mut self, ed: &mut Editor, p: &Pointer, arcs: bool) {
        self.unmark(ed);
        let Some(c) = corner_under(ed, p, arcs) else {
            ed.out.notice = Some(no_corner_notice(ed, p, arcs));
            return;
        };
        if let Some(i) = self.corners.iter().position(|x| *x == c) {
            self.corners.remove(i);
        } else if is_corner(ed, c, arcs) {
            self.corners.push(c);
        } else {
            ed.out.notice = Some(if arcs {
                "Fillet: the curves here are in line: nothing to round".into()
            } else {
                "Chamfer: the lines here are in line: nothing to cut".into()
            });
        }
    }

    /// A click on corner `c` begins sizing: it joins the set (if it isn't in it).
    fn add(&mut self, ed: &mut Editor, c: EntityId) {
        self.unmark(ed);
        self.added = (!self.corners.contains(&c)).then_some(c);
        if self.added.is_some() {
            self.corners.push(c);
        }
    }

    /// Sizing was cancelled: the corner its click added goes again.
    fn drop_added(&mut self) {
        if let Some(c) = self.added.take() {
            self.corners.retain(|x| *x != c);
        }
    }

    fn unmark(&mut self, ed: &mut Editor) {
        if std::mem::take(&mut self.marked) {
            ed.highlight.clear();
        }
    }

    /// Applied: the set is used up (the picks don't bring it back).
    fn clear(&mut self) {
        self.corners.clear();
        self.added = None;
    }

    /// The edit refused: say why, and light the corner it was refused at (the edit names it
    /// by its id, which the user never sees). `fits` is what size would have done.
    fn refuse(&mut self, ed: &mut Editor, tool: &str, e: EditError, arcs: bool, fits: String) {
        self.added = None;
        let why = e.to_string();
        let (corner, text) = match self.corners[..] {
            [only] => (Some(only), format!("{tool}: {why}{fits}")),
            _ => self
                .corners
                .iter()
                .find_map(|c| {
                    let rest = why.strip_prefix(&format!("at {c}: "))?;
                    Some((
                        Some(*c),
                        format!("{tool}: at the corner marked, {rest}{fits}"),
                    ))
                })
                .unwrap_or((None, format!("{tool}: {why}"))),
        };
        ed.out.notice = Some(text);
        if let Some(c) = corner {
            ed.highlight = std::iter::once(c)
                .chain(
                    arms(ed, c, arcs)
                        .into_iter()
                        .flatten()
                        .map(|(curve, _)| curve),
                )
                .collect();
            self.marked = true;
        }
    }

    /// For a refusal's notice: the size that fits them all, if `size` doesn't (rounded down
    /// so that what it shows fits).
    fn fits(&self, ed: &Editor, arcs: bool, size: f64, label: &str) -> String {
        let cap = largest(ed, &self.corners, arcs);
        if !(cap.is_finite() && cap > 0.0 && cap < size) {
            return String::new();
        }
        let units = ed.doc.units();
        let step = round_step(cap * 0.01, units.length.is_metric());
        let shown = (cap * 0.999 / step).floor() * step;
        if shown <= 0.0 {
            return String::new();
        }
        let every = if self.corners.len() > 1 {
            " every corner"
        } else {
            ""
        };
        format!(" ({label}{} fits{every})", units.format_readout(shown))
    }

    /// Previews of every corner in the set (and the one under the pointer), with a mark on
    /// each in the set unless the one being sized is all there is.
    fn preview(
        &self,
        ed: &Editor,
        arcs: bool,
        hover: Option<EntityId>,
        sizing: bool,
        shape: impl Fn(EntityId) -> Option<Shape>,
    ) -> Vec<SketchItem> {
        let set: Vec<EntityId> = self.live(ed, arcs).collect();
        let extra = hover.filter(|h| !sizing && !set.contains(h));
        let mut items: Vec<SketchItem> = set
            .iter()
            .copied()
            .chain(extra)
            .filter_map(&shape)
            .map(preview)
            .collect();
        if !(sizing && set.len() == 1) {
            items.extend(
                set.iter()
                    .filter_map(|c| ed.doc.point(*c).ok())
                    .map(preview_vertex),
            );
        }
        items
    }

    fn hint(&self, sizing: bool, what: &str, size: &str) -> Option<String> {
        let n = self.corners.len();
        match (sizing, n) {
            (_, 0) | (true, 1) => None,
            (true, n) => Some(format!(
                "Move to size {n} {what}s (Alt: freely) · click to apply · type the {size} · \
                 Esc cancels"
            )),
            (false, n) => Some(format!(
                "{n} corner{} · click one to size them all · type the {size}, Enter · \
                 Shift+click adds or removes · Esc clears",
                if n == 1 { "" } else { "s" }
            )),
        }
    }
}

/// A corner being sized: the size follows the pointer until a click applies it.
#[derive(Clone, Copy, Debug)]
struct Sizing {
    /// A fillet (or chamfer) that is already there, being re-sized: its arc (or cut line).
    arc: Option<EntityId>,
    /// Where the corner is, half the angle between its lines, and the shorter line's length.
    at: DVec2,
    half: f64,
    room: f64,
    /// The most the set's corners have room for together (see [`largest`]); None when one
    /// fillet or chamfer that is already there is re-sized.
    largest: Option<f64>,
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
            arc: None,
            at,
            half,
            room,
            largest: None,
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
            arc: Some(arc),
            at: f.corner,
            half,
            room,
            largest: None,
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
    set: CornerSet,
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
        // A fillet that is already there is re-sized in place (on its own).
        if let Some(arc) = fillet_under(ed, p) {
            self.set.unmark(ed);
            self.sizing = Sizing::begin_fillet(ed, arc, p);
            self.hover = None;
            return self.sizing.is_some();
        }
        let Some(corner) = corner_under(ed, p, true) else {
            ed.out.notice = Some(no_corner_notice(ed, p, true));
            return false;
        };
        self.set.add(ed, corner);
        self.sizing = Sizing::begin(ed, corner, p, self.radius(ed), true).map(|s| Sizing {
            largest: Some(largest(ed, &self.set.corners, true)),
            ..s
        });
        if self.sizing.is_none() {
            self.set.drop_added();
        }
        self.hover = None;
        self.sizing.is_some()
    }

    fn follow(&mut self, p: &Pointer) {
        if let Some(s) = &mut self.sizing {
            // The arc passes under the pointer: its middle is r / sin(half) - r from the
            // corner. The tangent points must stay on the lines.
            let largest = s.largest.unwrap_or(s.room * s.half.tan());
            s.follow(p, |d, half| d / (1.0 / half.sin() - 1.0), largest);
        }
    }

    /// Round every corner in the set (one undo step), or re-size the fillet being sized.
    fn apply(&mut self, ed: &mut Editor) {
        let r = self.radius(ed);
        let sizing = self.sizing.take();
        if let Some(Sizing { arc: Some(arc), .. }) = sizing {
            match ed.refillet(arc, r) {
                Ok(()) => self.radius = Some(r),
                Err(e) => ed.out.notice = Some(format!("Fillet: {e}")),
            }
            return;
        }
        self.set.unmark(ed);
        if self.set.corners.is_empty() {
            return;
        }
        let corners = self.set.corners.clone();
        match ed.fillet_corners(&corners, r) {
            Ok(_) => {
                self.radius = Some(r);
                self.set.clear();
            }
            Err(e) => {
                let fits = self.set.fits(ed, true, r, "R ");
                self.set.refuse(ed, "Fillet", e, true, fits);
            }
        }
    }
}

impl Tool for FilletTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Fillet
    }

    fn hint(&self) -> String {
        if let Some(h) = self.set.hint(self.sizing.is_some(), "fillet", "radius") {
            h
        } else if self.sizing.is_some() {
            "Move to size the fillet (Alt: freely) · click to apply · type the radius · Esc cancels"
                .into()
        } else {
            "Click a corner where two lines or arcs meet (or a fillet), then move to size it · \
             Shift+click: several corners"
                .into()
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        self.set.ready(ed, true);
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
            Input::Click(p, m) if m.shift => {
                if fillet_under(ed, &p).is_some() {
                    ed.out.notice = Some(
                        "Fillet: Shift+click adds corners; click a fillet to re-size it".into(),
                    );
                } else {
                    self.set.toggle(ed, &p, true);
                }
            }
            Input::Click(p, _) => {
                self.begin(&p, ed);
            }
            // Press on the corner, drag to size, release to apply.
            Input::DragStart(p, _) if self.sizing.is_none() => {
                self.begin(&p, ed);
            }
            Input::Drag(p) => self.follow(&p),
            Input::DragEnd(p, _) if self.sizing.is_some() => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Finish => self.apply(ed),
            _ => {}
        }
    }

    // A right-click (for the canvas menu) leaves the corners picked: only Enter applies them.
    fn secondary_click(&mut self, _ed: &mut Editor) {}

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        self.set.unmark(ed);
        if self.sizing.take().is_some() {
            self.set.drop_added();
            return true;
        }
        // Then the set.
        let had = !self.set.corners.is_empty();
        self.set.clear();
        had
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
        self.set.ready(ed, true);
        if self.sizing.is_some() {
            // Typed while sizing: that radius, on these corners.
            self.apply(ed);
        } else if let Some(r) = self.typed[0].filter(|r| *r > 0.0) {
            if self.set.corners.is_empty() {
                self.radius = Some(r);
            } else {
                // Typed with corners picked: that radius on all of them.
                self.apply(ed);
            }
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
        if let Some(Sizing { arc: Some(arc), .. }) = self.sizing {
            return ed
                .refillet_preview(arc, r)
                .map(preview)
                .into_iter()
                .collect();
        }
        self.set
            .preview(ed, true, self.hover, self.sizing.is_some(), |c| {
                ed.fillet_preview(c, r)
            })
    }
}

#[derive(Default)]
pub struct ChamferTool {
    distances: Option<(f64, f64)>,
    typed: [Option<f64>; 2],
    hover: Option<EntityId>,
    sizing: Option<Sizing>,
    set: CornerSet,
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
        // A chamfer that is already there is re-sized in place (on its own).
        if let Some((cut, c)) = chamfer_under(ed, p) {
            self.set.unmark(ed);
            let (half, room) = c.half_angle_and_room();
            self.sizing = Some(Sizing {
                arc: Some(cut),
                at: c.corner,
                half,
                room,
                largest: None,
                start: p.raw,
                moved: false,
                size: c.legs[0],
            });
            self.hover = None;
            return true;
        }
        let Some(corner) = corner_under(ed, p, false) else {
            ed.out.notice = Some(no_corner_notice(ed, p, false));
            return false;
        };
        let size = self.distances(ed).0;
        self.set.add(ed, corner);
        self.sizing = Sizing::begin(ed, corner, p, size, false).map(|s| Sizing {
            largest: Some(largest(ed, &self.set.corners, false)),
            ..s
        });
        if self.sizing.is_none() {
            self.set.drop_added();
        }
        self.hover = None;
        self.sizing.is_some()
    }

    fn follow(&mut self, p: &Pointer) {
        if let Some(s) = &mut self.sizing {
            // The cut passes under the pointer: its middle is d · cos(half) from the corner.
            let largest = s.largest.unwrap_or(s.room);
            s.follow(p, |d, half| d / half.cos(), largest);
        }
    }

    /// Cut every corner in the set (one undo step), or re-size the chamfer being sized.
    fn apply(&mut self, ed: &mut Editor) {
        let (d1, d2) = self.distances(ed);
        let sizing = self.sizing.take();
        if let Some(Sizing { arc: Some(cut), .. }) = sizing {
            match ed.rechamfer(cut, d1, d2) {
                Ok(()) => self.distances = Some((d1, d2)),
                Err(e) => ed.out.notice = Some(format!("Chamfer: {e}")),
            }
            return;
        }
        self.set.unmark(ed);
        if self.set.corners.is_empty() {
            return;
        }
        // Which line takes the first distance is only clear at one corner.
        if self.set.corners.len() > 1 && (d1 - d2).abs() > tolerance::SAME {
            self.set.added = None;
            ed.out.notice = Some("Chamfer: two distances work on one corner at a time".into());
            return;
        }
        let corners = self.set.corners.clone();
        match ed.chamfer_corners(&corners, d1, d2) {
            Ok(_) => {
                self.distances = Some((d1, d2));
                self.set.clear();
            }
            Err(e) => {
                let fits = self.set.fits(ed, false, d1.max(d2), "");
                self.set.refuse(ed, "Chamfer", e, false, fits);
            }
        }
    }
}

impl Tool for ChamferTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Chamfer
    }

    fn hint(&self) -> String {
        if let Some(h) = self.set.hint(self.sizing.is_some(), "chamfer", "distance") {
            h
        } else if self.sizing.is_some() {
            "Move to size the chamfer (Alt: freely) · click to apply · type the distance (Tab: \
             second distance) · Esc cancels"
                .into()
        } else {
            "Click a corner where two lines meet, then move to size the chamfer · Shift+click: \
             several corners"
                .into()
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        self.set.ready(ed, false);
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
            Input::Click(p, m) if m.shift => {
                if corner_under(ed, &p, false).is_none() && chamfer_under(ed, &p).is_some() {
                    ed.out.notice = Some(
                        "Chamfer: Shift+click adds corners; click a chamfer to re-size it".into(),
                    );
                } else {
                    self.set.toggle(ed, &p, false);
                }
            }
            Input::Click(p, _) => {
                self.begin(&p, ed);
            }
            Input::DragStart(p, _) if self.sizing.is_none() => {
                self.begin(&p, ed);
            }
            Input::Drag(p) => self.follow(&p),
            Input::DragEnd(p, _) if self.sizing.is_some() => {
                self.follow(&p);
                self.apply(ed);
            }
            Input::Finish => self.apply(ed),
            _ => {}
        }
    }

    // A right-click (for the canvas menu) leaves the corners picked: only Enter applies them.
    fn secondary_click(&mut self, _ed: &mut Editor) {}

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        self.set.unmark(ed);
        if self.sizing.take().is_some() {
            self.set.drop_added();
            return true;
        }
        // Then the set.
        let had = !self.set.corners.is_empty();
        self.set.clear();
        had
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
        self.set.ready(ed, false);
        let valid = self.typed.iter().flatten().all(|d| *d > 0.0) && any_typed(self.typed);
        if valid {
            if self.sizing.is_some() || !self.set.corners.is_empty() {
                self.apply(ed);
            } else {
                self.distances = Some(self.distances(ed));
            }
        }
        self.typed = [None; 2];
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let s = self.sizing.filter(|s| s.moved)?;
        Some(p.units.format_readout(self.typed[0].unwrap_or(s.size)))
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let (d1, d2) = self.distances(ed);
        if let Some(Sizing { arc: Some(cut), .. }) = self.sizing {
            return ed
                .rechamfer_preview(cut, d1, d2)
                .map(preview)
                .into_iter()
                .collect();
        }
        self.set
            .preview(ed, false, self.hover, self.sizing.is_some(), |c| {
                ed.chamfer_preview(c, d1, d2)
            })
    }
}
