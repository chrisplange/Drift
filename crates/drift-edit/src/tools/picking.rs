//! What the select tool takes under the pointer. Hover, click, double-click and drag start
//! all ask [`target_at`], so they agree on one precedence: geometry (a point, then a line's
//! midpoint, then a curve), then sketch text, a copy of a block, and a fill (a wall's poché
//! standing for its wall). The tools that act on the point or curve under the pointer pick
//! through [`entity_at`], each with its own [`Rule`].

use drift_core::{FillId, InstanceId, TextId, WallId};

use super::*;
use crate::editor::PickFilter;
use crate::pick_cycle::{Spot, cycle_status};

/// The geometry the select tool would pick under the pointer: points first, then line
/// midpoints, then curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Entity(EntityId),
    Midpoint(EntityId),
}

impl Target {
    /// The id a dimension uses for this pick when it's a point-like reference: a point, or a
    /// line standing for its midpoint.
    pub(super) fn point_ref(self, ed: &Editor) -> Option<EntityId> {
        match self {
            Target::Midpoint(line) => Some(line),
            Target::Entity(id) => ed.doc.get(id).filter(|e| e.kind.is_point()).map(|_| id),
        }
    }
}

/// Midpoints are picked within this fraction of the normal pick radius.
pub(super) const MIDPOINT_PICK: f64 = 0.6;

/// What an entity pick passes over.
#[derive(Clone, Copy)]
pub(super) struct Rule<'a> {
    /// Entities on locked layers.
    locked: bool,
    /// Handles and wall centrelines that aren't showing.
    hidden: Option<&'a BTreeSet<EntityId>>,
    /// Centres that aren't showing (#259).
    centres: bool,
    /// What the pick filter keeps out.
    filter: Option<PickFilter>,
}

impl Rule<'static> {
    /// Anything.
    pub(super) const ANY: Self = Rule {
        locked: false,
        hidden: None,
        centres: false,
        filter: None,
    };
    /// Anything not on a locked layer.
    pub(super) const UNLOCKED: Self = Rule {
        locked: true,
        ..Self::ANY
    };
}

impl<'a> Rule<'a> {
    /// What the select tool picks: nothing locked, hidden or filtered out, and (with
    /// `centres`) no centre that isn't drawn.
    fn select(filter: PickFilter, hidden: &'a BTreeSet<EntityId>, centres: bool) -> Self {
        Rule {
            locked: true,
            hidden: Some(hidden),
            centres,
            filter: Some(filter),
        }
    }

    /// (Nothing outside a group or block open for editing, whatever the rule.)
    fn allows(&self, ed: &Editor, id: EntityId) -> bool {
        !(self.locked && ed.is_locked(id))
            && !self.hidden.is_some_and(|h| h.contains(&id))
            && !(self.centres && ed.centre_unseen(id))
            && self.filter.is_none_or(|f| f.allows(&ed.doc, id))
            && ed.in_scope(id)
    }
}

/// The point or curve within `tol` of `at` that `rule` lets pick and `accept` takes: the
/// nearest, points before curves.
pub(super) fn entity_at(
    ed: &Editor,
    at: DVec2,
    tol: f64,
    rule: Rule,
    accept: impl Fn(EntityId) -> bool,
) -> Option<EntityId> {
    ed.index
        .pick(at, tol, |id| rule.allows(ed, id) && accept(id))
}

/// The geometry under the pointer that `rule` lets pick: a point, else (with `midpoints`) a
/// line by its midpoint, else a curve.
fn geometry_at(ed: &Editor, p: &Pointer, rule: Rule, midpoints: bool) -> Option<Target> {
    let is_point = |id: EntityId| ed.doc.get(id).is_some_and(|e| e.kind.is_point());
    if let Some(id) = entity_at(ed, p.raw, p.tolerance, rule, is_point) {
        return Some(Target::Entity(id));
    }
    // A tighter radius than for picking, so clicking near (not on) the middle of a line still
    // picks the line. (Only with points and edges both on: a midpoint is a bit of each.)
    if midpoints
        && let Some(line) = ed
            .index
            // (A wall's faces have no midpoints of their own: a click there picks the wall.)
            .pick_midpoint(p.raw, MIDPOINT_PICK * p.tolerance, |id| {
                rule.allows(ed, id) && (!ed.doc.has_walls() || !ed.wall_owners().generated(id))
            })
    {
        return Some(Target::Midpoint(line));
    }
    entity_at(ed, p.raw, p.tolerance, rule, |_| true).map(Target::Entity)
}

/// The geometry under the pointer, as `filter` lets pick (locked layers too): for the tools
/// that measure or annotate what they're pointed at.
pub(super) fn pick_target_with(ed: &Editor, p: &Pointer, filter: PickFilter) -> Option<Target> {
    let hidden = ed.hidden_handles();
    // (The tools that measure or annotate still take a centre that isn't drawn.)
    let rule = Rule {
        locked: false,
        ..Rule::select(filter, &hidden, false)
    };
    geometry_at(ed, p, rule, filter.midpoints())
}

/// Which select-tool event is asking. They differ only where [`target_at`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Hover,
    Click,
    DoubleClick,
    DragStart,
}

/// What the select tool takes under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Geometry(Target),
    Text(TextId),
    Copy(InstanceId),
    /// Inside a wall's poché: the wall (the nearest, where walls merge), and the poché.
    Wall(WallId, FillId),
    Fill(FillId),
}

impl Pick {
    pub(super) fn entity(self) -> Option<EntityId> {
        match self {
            Pick::Geometry(Target::Entity(id)) => Some(id),
            _ => None,
        }
    }

    pub(super) fn midpoint(self) -> Option<EntityId> {
        match self {
            Pick::Geometry(Target::Midpoint(line)) => Some(line),
            _ => None,
        }
    }

    pub(super) fn text(self) -> Option<TextId> {
        match self {
            Pick::Text(t) => Some(t),
            _ => None,
        }
    }

    pub(super) fn copy(self) -> Option<InstanceId> {
        match self {
            Pick::Copy(c) => Some(c),
            _ => None,
        }
    }

    /// The fill, a wall's poché included.
    pub(super) fn fill(self) -> Option<FillId> {
        match self {
            Pick::Wall(_, f) | Pick::Fill(f) => Some(f),
            _ => None,
        }
    }
}

/// What the select tool takes under the pointer, first that applies: geometry (nothing
/// locked, hidden or filtered out), sketch text, a copy of a block, a wall by its poché, a
/// fill. Empty areas are the caller's: they're picked only where nothing else is. While a
/// group or block is open for editing, only what's in it: anything else is no hit (a
/// double-click on another copy still edits that one).
pub(super) fn target_at(ed: &mut Editor, p: &Pointer, purpose: Purpose) -> Option<Pick> {
    let filter = ed.prefs.pick_filter;
    let scoped = ed.edit_scope().is_some();
    let geometry = {
        let hidden = ed.hidden_handles();
        // A centre is picked only where it is drawn: while its curve is hovered or picked
        // (#259).
        // (A drag from a line's middle moves the line; a double-click there opens its group.)
        let midpoints = filter.midpoints() && matches!(purpose, Purpose::Hover | Purpose::Click);
        geometry_at(ed, p, Rule::select(filter, &hidden, true), midpoints)
    };
    if let Some(t) = geometry {
        return Some(Pick::Geometry(t));
    }
    // A double-click edits text or a block whatever the filter, but not through unlocked
    // geometry, even geometry hidden or filtered out.
    let double = purpose == Purpose::DoubleClick;
    let clear = !double || entity_at(ed, p.raw, p.tolerance, Rule::UNLOCKED, |_| true).is_none();
    if clear
        && !scoped
        && (double || filter.texts())
        && let Some(t) = ed.text_at(p.raw, p.tolerance)
    {
        return Some(Pick::Text(t));
    }
    if clear
        && (!scoped || double)
        && (double || filter.blocks())
        && let Some(c) = ed.instance_at(p.raw, p.tolerance)
    {
        return Some(Pick::Copy(c));
    }
    if !filter.fills() {
        return None;
    }
    // ([`fill_at`](Editor::fill_at) keeps to the open group's.)
    let f = ed.fill_at(p.raw)?;
    Some(match ed.wall_in_fill_at(f, p.raw) {
        Some(w) => Pick::Wall(w, f),
        None => Pick::Fill(f),
    })
}

/// Everything a plain Select click could pick under the pointer, one per click on the same
/// spot (see [`crate::pick_cycle`]): first what a single click picks ([`target_at`]), then the
/// rest of the geometry (points, then curves, nearest first), sketch texts (smallest first),
/// the copy of a block, and the fills (topmost first; a wall by its poché), as the filter and
/// the open group let pick. Two that would pick the same count once: a line and its
/// midpoint, members of one closed group, a wall's face and its poché.
pub(super) fn candidates_at(ed: &mut Editor, p: &Pointer) -> Vec<Pick> {
    let filter = ed.prefs.pick_filter;
    let scoped = ed.edit_scope().is_some();
    let mut picks: Vec<Pick> = target_at(ed, p, Purpose::Click).into_iter().collect();
    {
        let hidden = ed.hidden_handles();
        let rule = Rule::select(filter, &hidden, true);
        // In the order [`entity_at`] picks: points before curves, nearest first, then older.
        let mut near: Vec<(EntityId, bool, f64)> = ed
            .index
            .near(p.raw, p.tolerance)
            .filter(|(id, ..)| rule.allows(ed, *id))
            .map(|(id, shape, d)| (id, !matches!(shape, drift_core::Shape::Point(_)), d))
            .collect();
        near.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)).then(a.0.cmp(&b.0)));
        picks.extend(
            near.into_iter()
                .map(|(id, ..)| Pick::Geometry(Target::Entity(id))),
        );
    }
    if !scoped && filter.texts() {
        picks.extend(ed.texts_at(p.raw, p.tolerance).into_iter().map(Pick::Text));
    }
    if !scoped
        && filter.blocks()
        && let Some(c) = ed.instance_at(p.raw, p.tolerance)
    {
        picks.push(Pick::Copy(c));
    }
    if filter.fills() {
        for f in ed.fills_at(p.raw) {
            picks.push(match ed.wall_in_fill_at(f, p.raw) {
                Some(w) => Pick::Wall(w, f),
                None => Pick::Fill(f),
            });
        }
    }
    let mut seen: Vec<Picks> = Vec::new();
    picks.retain(|pick| {
        let key = picks_of(ed, *pick);
        let new = !seen.contains(&key);
        if new {
            seen.push(key);
        }
        new
    });
    picks
}

/// What a candidate picks, to count each once (as [`Editor::pick_set`] decides, without
/// gathering a big group's members for each).
#[derive(PartialEq)]
enum Picks {
    Opening(drift_core::OpeningId),
    Wall(WallId),
    Offset(drift_core::OffsetId),
    Group(drift_core::GroupId),
    One(EntityId),
    Other(Pick),
}

fn picks_of(ed: &Editor, pick: Pick) -> Picks {
    // (A line's midpoint stands for the line: picked alone near its middle, as before.)
    let id = match pick {
        Pick::Geometry(Target::Entity(id) | Target::Midpoint(id)) => id,
        Pick::Wall(w, _) => return Picks::Wall(w),
        // (A grouped shape's fill picks the group, as its edges do.)
        Pick::Fill(f) => {
            return match ed.fill_group(f) {
                Some(g) if ed.open_group != Some(g) => Picks::Group(g),
                _ => Picks::Other(pick),
            };
        }
        other => return Picks::Other(other),
    };
    if let Some(o) = ed.opening_of(id) {
        return Picks::Opening(o);
    }
    if ed.doc.has_walls()
        && ed.wall_owners().by_wall(id)
        && let Some(w) = ed.wall_of(id)
    {
        return Picks::Wall(w);
    }
    if let Some(x) = ed.offset_of(id) {
        return Picks::Offset(x);
    }
    match ed.doc.group_of(id) {
        Some(g) if ed.open_group != Some(g) => Picks::Group(g),
        _ => Picks::One(id),
    }
}

/// What a plain Select click goes to, with `lead` of the app's annotations under the pointer
/// ahead of the tool's candidates.
pub(super) enum Step {
    /// The annotation: the app's to pick.
    Lead,
    /// The tool's pick (or empty canvas).
    Tool(Option<Pick>),
}

/// A plain Select click at `p` (see [`crate::pick_cycle`]): a first click there picks what a
/// single click always has, without gathering the rest; a click on the spot of the last one
/// picks the next, round to the first, and the status bar says which.
pub(super) fn cycle_step(ed: &mut Editor, p: &Pointer, lead: usize) -> Step {
    ed.pick_cycle.forget_if_changed(ed.doc.revision());
    let Some(last) = ed.pick_cycle.same_spot(p.raw, lead) else {
        ed.pick_cycle.record(Spot {
            at: p.raw,
            reach: p.tolerance,
            step: 0,
            len: None,
            lead,
            advanced: false,
            rev: ed.doc.revision(),
        });
        return match lead {
            0 => Step::Tool(target_at(ed, p, Purpose::Click)),
            _ => Step::Lead,
        };
    };
    // Gathered at the spot, not wherever this click landed in it: a pixel's jitter near a
    // crossing would reorder them.
    let picks = candidates_at(ed, &p.at(last.at));
    let len = lead + picks.len();
    // (Not the same things under it any more: from the first.)
    let step = match last.len {
        Some(l) if l != len => 0,
        _ => (last.step + 1) % len.max(1),
    };
    ed.pick_cycle.record(Spot {
        // (The spot stays where the cycle began, so it can't creep away.)
        at: last.at,
        reach: p.tolerance,
        step,
        len: Some(len),
        lead,
        advanced: true,
        rev: ed.doc.revision(),
    });
    if len > 1 {
        ed.out.inform(cycle_status(step, len));
    }
    match step.checked_sub(lead) {
        Some(k) => Step::Tool(picks.get(k).copied()),
        None => Step::Lead,
    }
}

/// What the next plain click at `p` would pick, for the hover to show, when `p` is on the
/// spot of the last click and there are several things there (see [`crate::pick_cycle`]):
/// `Some(None)` when it's the app's annotation, none of the tool's. None elsewhere: the
/// hover shows what a single click picks, as ever.
pub(super) fn hover_next(ed: &mut Editor, p: &Pointer) -> Option<Option<Pick>> {
    ed.pick_cycle.set_hover_past(false);
    ed.pick_cycle.forget_if_changed(ed.doc.revision());
    let last = ed.pick_cycle.on_spot(p.raw)?;
    let picks = match ed.pick_cycle.cached() {
        Some(picks) => picks.clone(),
        None => {
            let picks = candidates_at(ed, &p.at(last.at));
            ed.pick_cycle.cache(picks.clone());
            picks
        }
    };
    let len = last.lead + picks.len();
    if len <= 1 || last.len.is_some_and(|l| l != len) {
        return None;
    }
    let next = (last.step + 1) % len;
    let k = next.checked_sub(last.lead);
    ed.pick_cycle.set_hover_past(last.lead > 0 && k.is_some());
    Some(k.and_then(|k| picks.get(k).copied()))
}

impl Pointer {
    /// The same pointer at `raw`.
    pub(super) fn at(&self, raw: DVec2) -> Pointer {
        Pointer { raw, ..*self }
    }
}
