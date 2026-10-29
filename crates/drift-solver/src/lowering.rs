//! Relations the solver sees in another form than they are stored in.
//!
//! Some geometry is never a solver unknown: it is made from other geometry (a wall's faces,
//! from its centreline). A relation stored on such geometry is *lowered* by whoever makes it
//! onto what it is made from, and the solver works with what it was lowered to: the same
//! relation over other entities, or a signed offset from a curve ([`Lowered::OffsetFrom`]).
//! The solver knows nothing of what the geometry is; it only reads relations through a
//! [`Lowering`] where one is given (`connected`, `needs_solve`, `System::build_in`).

use std::collections::HashMap;

use drift_core::{Constraint, ConstraintId, Document, EntityId};

/// What one relation is lowered to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lowered {
    /// A relation over other entities, which the solver treats as it would a stored one.
    Relation(Constraint),
    /// The signed distance of `point` from `curve` is `offset`: to the left of a line (from
    /// its `a` to its `b`), or outside an arc's or a circle's circle. `point` may be a line,
    /// for its midpoint.
    OffsetFrom {
        point: EntityId,
        curve: EntityId,
        offset: f64,
    },
}

impl Lowered {
    /// The entities it relates.
    pub fn entities(&self) -> Vec<EntityId> {
        match self {
            Lowered::Relation(c) => c.entities().collect(),
            Lowered::OffsetFrom { point, curve, .. } => vec![*point, *curve],
        }
    }
}

/// Stored relations in the form the solver sees them, and relations no stored constraint
/// holds that come with the geometry (a wall's end on the wall it meets).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lowering {
    /// Each lowered relation's stand-ins (none: it holds nothing the solver can act on).
    pub of: HashMap<ConstraintId, Vec<Lowered>>,
    /// Relations of the geometry itself, held wherever a solve reaches them (but which alone
    /// give a drag nothing to solve: see [`crate::needs_solve_with`]).
    pub extra: Vec<Lowered>,
}

impl Lowering {
    pub fn is_empty(&self) -> bool {
        self.of.is_empty() && self.extra.is_empty()
    }

    /// The entities a stored relation relates as the solver sees it.
    pub fn entities_of(&self, id: ConstraintId, c: &Constraint) -> Vec<EntityId> {
        match self.of.get(&id) {
            Some(list) => list.iter().flat_map(Lowered::entities).collect(),
            None => c.entities().collect(),
        }
    }
}

/// Relations as the solver reads them ([`relations`]): each with its stored constraint's id
/// (none for a lowering's extras).
pub type Relations = Vec<(Option<ConstraintId>, Lowered)>;

/// The driving relations as the solver sees them: each stored constraint (or what it was
/// lowered to), with its id, then the extras (no id). A drag hands each part of its scope
/// its own share of these ([`SolveOptions::relations`](crate::SolveOptions::relations)).
pub fn relations(doc: &Document, lowering: Option<&Lowering>) -> Relations {
    let mut out = Vec::new();
    for (id, c) in doc.constraints() {
        if doc.is_driven(id) {
            continue;
        }
        match lowering.and_then(|l| l.of.get(&id)) {
            Some(list) => out.extend(list.iter().map(|l| (Some(id), *l))),
            None => out.push((Some(id), Lowered::Relation(*c))),
        }
    }
    if let Some(l) = lowering {
        out.extend(l.extra.iter().map(|x| (None, *x)));
    }
    out
}
