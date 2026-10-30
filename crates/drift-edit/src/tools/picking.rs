//! What the select tool takes under the pointer. Hover, click, double-click and drag start
//! all ask [`target_at`], so they agree on one precedence: geometry (a point, then a line's
//! midpoint, then a curve), then sketch text, a copy of a block, and a fill (a wall's poché
//! standing for its wall). The tools that act on the point or curve under the pointer pick
//! through [`entity_at`], each with its own [`Rule`].

use drift_core::{FillId, InstanceId, TextId, WallId};

use super::*;
use crate::editor::PickFilter;

/// The geometry the select tool would pick under the pointer: points first, then line
/// midpoints, then curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
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
    /// What the pick filter keeps out.
    filter: Option<PickFilter>,
}

impl Rule<'static> {
    /// Anything.
    pub(super) const ANY: Self = Rule {
        locked: false,
        hidden: None,
        filter: None,
    };
    /// Anything not on a locked layer.
    pub(super) const UNLOCKED: Self = Rule {
        locked: true,
        ..Self::ANY
    };
}

impl<'a> Rule<'a> {
    /// What the select tool picks: nothing locked, hidden or filtered out.
    fn select(filter: PickFilter, hidden: &'a BTreeSet<EntityId>) -> Self {
        Rule {
            locked: true,
            hidden: Some(hidden),
            filter: Some(filter),
        }
    }

    fn allows(&self, ed: &Editor, id: EntityId) -> bool {
        !(self.locked && ed.is_locked(id))
            && !self.hidden.is_some_and(|h| h.contains(&id))
            && self.filter.is_none_or(|f| f.allows(&ed.doc, id))
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
    let rule = Rule {
        locked: false,
        ..Rule::select(filter, &hidden)
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
pub(super) enum Pick {
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
/// fill. Empty areas are the caller's: they're picked only where nothing else is.
pub(super) fn target_at(ed: &mut Editor, p: &Pointer, purpose: Purpose) -> Option<Pick> {
    let filter = ed.prefs.pick_filter;
    let geometry = {
        let hidden = ed.hidden_handles();
        // (A drag from a line's middle moves the line; a double-click there opens its group.)
        let midpoints = filter.midpoints() && matches!(purpose, Purpose::Hover | Purpose::Click);
        geometry_at(ed, p, Rule::select(filter, &hidden), midpoints)
    };
    if let Some(t) = geometry {
        return Some(Pick::Geometry(t));
    }
    // A double-click edits text or a block whatever the filter, but not through unlocked
    // geometry, even geometry hidden or filtered out.
    let double = purpose == Purpose::DoubleClick;
    let clear = !double || entity_at(ed, p.raw, p.tolerance, Rule::UNLOCKED, |_| true).is_none();
    if clear
        && (double || filter.texts())
        && let Some(t) = ed.text_at(p.raw, p.tolerance)
    {
        return Some(Pick::Text(t));
    }
    if clear
        && (double || filter.blocks())
        && let Some(c) = ed.instance_at(p.raw, p.tolerance)
    {
        return Some(Pick::Copy(c));
    }
    if !filter.fills() {
        return None;
    }
    let f = ed.fill_at(p.raw)?;
    Some(match ed.wall_in_fill_at(f, p.raw) {
        Some(w) => Pick::Wall(w, f),
        None => Pick::Fill(f),
    })
}
