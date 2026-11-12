//! Geometric constraint solver for Drift.
//!
//! [`solve`] adjusts a document's geometry so every constraint holds, moving things as little
//! as possible, in two stages per component (see `newton`): first the relations, by
//! Levenberg–Marquardt steps `(JᵀJ + μ·M⁻¹ + λ·D) Δ = −JᵀF` (for small μ the weighted
//! minimum-norm Gauss–Newton step, λ a trust region), then, among their solutions, the shapes
//! a solve keeps, by Gauss–Newton steps projected into the relations' null space. Each stops at
//! a test, not a count: the relations hold and the shapes are stationary, or the relations
//! are at a least-squares stationary point (a conflict). Parameters being dragged are given a
//! heavy weight, so the rest of the sketch adapts around them. [`analyze`] reports degrees of
//! freedom and which entities are fully defined.
//!
//! Pipeline: parameters → substitution (coincident / horizontal / vertical / fix merge or pin
//! parameters) → equations → independent components → two-stage solve per component → write
//! back.

pub mod bench;
mod equations;
#[cfg(feature = "exit-counts")]
pub mod exits;
mod incremental;
mod linalg;
mod lowering;
mod sparse;
mod system;

use std::collections::{BTreeMap, HashMap, HashSet};

use drift_core::polygon::UnionFind;
use drift_core::{Constraint, ConstraintId, DVec2, Document, EntityId, EntityKind};

pub use crate::incremental::{AnalysisCache, analyze_cached};
pub use crate::lowering::{Lowered, Lowering, Relations, relations};
pub use crate::system::{TouchingEnds, touching_ends};

use crate::equations::Equation;
use crate::linalg::{Matrix, rref};
use crate::sparse::{Dd, Skyline, qr_rank};
use crate::system::{EntityShape, Shape, System};

/// A solver unknown: a point coordinate or a circle radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Param {
    X(EntityId),
    Y(EntityId),
    Radius(EntityId),
    /// Midpoint of a line that a dimension measures to (internal; never written back).
    MidX(EntityId),
    MidY(EntityId),
}

impl Param {
    /// Parameters of a point.
    pub fn point(id: EntityId) -> [Param; 2] {
        [Param::X(id), Param::Y(id)]
    }
}

/// Convergence tolerances: the bar every stop of a solve is measured against (see `Bar`).
#[derive(Clone, Copy, Debug)]
pub struct Tolerances {
    /// Length tolerance, as a share of a quarter of the size of the component it is read in
    /// (a length row, a step, a kept shape). Never below [`FLOOR_ULPS`] ulps of the stored
    /// coordinates.
    pub length: f64,
    /// Angle residual tolerance (a sine) for an ellipse's tangency. An angle between lines
    /// holds to [`length`](Self::length) over its shorter line instead (see `Bar`).
    pub angle: f64,
}

/// The solver's own length tolerance ([`Tolerances::length`]): a billionth of a quarter of
/// a component's size, 2.5 pm on a 10 mm part, 0.25 nm on a 1 m one. Far below anything
/// drawn or exported, and far above round-off in the component's frame, so the same drag
/// stops at the same step at any size and at any place up to about a million of its sizes
/// from the origin (decision 2 of the solver audit).
pub const BAR: f64 = 1e-9;

/// A length bar never goes below this many ε of the largest stored coordinate it reads:
/// geometry is stored in absolute coordinates between solves, so a row read from storage is
/// off by a few ulps of them whatever a solve left. [`BAR`] of a part's size only meets this
/// about a million part-sizes from the origin.
pub const FLOOR_ULPS: f64 = 8.0;

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            length: BAR,
            angle: 1e-12,
        }
    }
}

/// What a solve keeps, in rank: the relations always hold; among the geometry that meets
/// them, the [`dragged`](Self::dragged) stay put first, then the [`carried`](Self::carried),
/// then (with a [`focus`](Self::focus)) everything else; then the shapes
/// ([`radii`](Self::radii), [`lengths`](Self::lengths), [`directions`](Self::directions))
/// are kept where that still leaves them open, as one preference. A lower rank never
/// moves a higher one.
#[derive(Clone, Debug, Default)]
pub struct SolveOptions {
    /// Parameters that should stay where they are if at all possible (e.g. the point under the
    /// mouse while dragging).
    pub dragged: Vec<Param>,
    /// Parameters a drag carries along with it (the points of a curve a move pushes rather
    /// than reshapes): they stay where they are asked to like [`dragged`](Self::dragged), but
    /// give way before the dragged where the relations won't have both, and no one dragged
    /// them, so a line or arc they end keeps its length or radius preference like any other
    /// (dragging an end reshapes its curve; being carried doesn't).
    pub carried: Vec<Param>,
    /// Also run the degrees-of-freedom analysis.
    pub analyze: bool,
    pub tolerances: Tolerances,
    /// Solve only these entities (closed under shared points and constraints; see
    /// [`connected`]): a drag in a big sketch then costs what the dragged part costs.
    pub scope: Option<std::rc::Rc<HashSet<EntityId>>>,
    /// Arcs and circles and the radii they should keep where they can (typically their sizes
    /// before a drag moved some points): a move that could go either way follows a round with
    /// its lines rather than shrinking it. Ignored for arcs with a dragged end and circles
    /// with a dragged radius.
    pub radii: HashMap<EntityId, f64>,
    /// Parameters that should take up the change (e.g. the geometry a new relation names):
    /// when set, everything else moves only as far as it has to, so meeting a relation
    /// reshapes what it relates rather than spreading the change thinly over the sketch.
    pub focus: Vec<Param>,
    /// Lines and the directions (unit vectors) they should keep where they can, as
    /// [`radii`](Self::radii) does for arcs.
    pub directions: HashMap<EntityId, DVec2>,
    /// Lines and the lengths they should keep where they can, as [`radii`](Self::radii) does
    /// for arcs: moving what a line hangs from then carries it along rather than stretching
    /// it. Where the relations change a length anyway, the preference is let go. Ignored for
    /// lines with a dragged end: dragging an end reshapes its line.
    pub lengths: HashMap<EntityId, f64>,
    /// What each of [`lengths`](Self::lengths) weighs against the other kept shapes where they
    /// can't all be met (1 where absent: as much as a radius or a direction). An edit ranks a
    /// line's length below its direction, and the lengths of what a new relation names lower
    /// still (see `Editor::kept_shapes`); a drag ranks none.
    pub length_ranks: HashMap<EntityId, f64>,
    /// Keep shapes as a relation's solve did before the audit's PR 6: a direction's offset as
    /// `l·sin θ` (met by a line turned end for end, and smaller for a shorter line), and an
    /// edit's preference steps not bounded together. Only for an edit that keeping shapes
    /// would refuse, solved again as before (see `Editor::check_new_constraints`).
    pub shapes_as_before: bool,
    /// Relations read in the form they are lowered to (see [`Lowering`]).
    pub lowering: Option<std::rc::Rc<Lowering>>,
    /// The relations to solve, when the caller has them already: those of
    /// [`relations`] (read through [`lowering`](Self::lowering)) that relate something in
    /// [`scope`](Self::scope). A drag of many parts then doesn't look through every relation
    /// of the drawing for each part's every solve (#253). `None`: found from the document.
    pub relations: Option<std::rc::Rc<Relations>>,
    /// How each tangency touched where the relations held (see [`touching_ends`]): a drag
    /// finds them once as it begins, and every step keeps them. `None`: from the geometry.
    pub touching: Option<std::rc::Rc<TouchingEnds>>,
    /// Finish a solve at a fold: a kept shape the relations leave almost no way to meet (a
    /// radius kept where two dragged points pin its arc at its least, a line kept as long as
    /// the least distance to it), or rows with a double root (a tangent line through a point
    /// at the circle's own radius). With this set, the rows' minimum change comes down as they
    /// close, the shapes settle only where the most a step could still gain them is below the
    /// bar squared (never on their projected gradient alone, which vanishes at a fold where a
    /// shape can still be met), and the solve counts as finished only where it then meets
    /// everything: every relation, every dragged and carried parameter exactly where it was
    /// asked to be, and every kept shape (see [`FOLD_MET`]). Anything short of that is
    /// reported unfinished, with the geometry as it was: a result that keeps a shape only by
    /// leaving a dragged point behind, or that would grow a radius, is never one of these.
    /// Off by default. (A drag takes these steps only as a last resort, where its move can
    /// always land: see `Editor::drag`.)
    pub fold: bool,
    /// The coarsest length bar any component of this solve may be held to: a drag frame's
    /// later solves pass the bar its first solve was held to ([`SolveReport::bar`]), so a
    /// solve that begins from geometry a try has flung (a frame's arc thrown metres out mid-try
    /// would make a part metres across) is never held looser than the part as the frame began.
    /// A component's own finer bar stands. `None`: each component's own.
    pub bar_cap: Option<f64>,
}

/// How completely an entity's position is determined by constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Definition {
    /// Can still move.
    Under,
    /// Completely determined.
    Full,
    /// Involved in constraints that can't all be satisfied.
    Over,
}

#[derive(Clone, Debug, Default)]
pub struct SolveReport {
    /// Every constraint is satisfied within tolerance.
    pub converged: bool,
    /// Newton iterations used by the slowest component.
    pub iterations: usize,
    /// The most steps any component took on its way to the relations that went badly (their
    /// linearisation promised far more than they did, or they were refused): the mark of a
    /// solve that wanders round a fold between solutions rather than heading for the nearest
    /// one (#103), where a drag should go in smaller steps.
    pub wandered: usize,
    /// Constraints that could not be satisfied.
    pub conflicts: Vec<ConstraintId>,
    /// A part of the system failed whose rows belong to no constraint (an arc's end on its
    /// circle, a line's midpoint, an ellipse's axes): it names nothing in
    /// [`conflicts`](Self::conflicts), so a report that names only conflicts already known
    /// may still have left geometry broken.
    pub unsourced_failure: bool,
    /// Entities whose own geometry is in a part of the system that failed (an arc whose end
    /// keeps to the circle through its start, a line with a midpoint, an ellipse, an
    /// elliptical arc, a wall end on the wall it meets: the owners of the rows no relation
    /// owns, see `Row::owner`), whether or not each is off by more than its tolerance. Any
    /// solve of a failed part may share its misfit with them: a least-squares compromise
    /// between relations that conflict leaves an arc in the same part off its circle.
    ///
    /// Read from an analysis of the drawing before an edit, these are what that edit isn't
    /// blamed for breaking: geometry in a part that already fails is never counted as newly
    /// broken, even if the edit breaks it further, as a conflict already there isn't. Sorted.
    pub exposed: Vec<EntityId>,
    /// Of [`exposed`](Self::exposed), the entities whose own geometry is broken: a row of
    /// theirs isn't within its tolerance (the test a part's convergence uses, so a residual
    /// that isn't a number counts) at the values the solve wrote. A failed part always names
    /// a conflict or a broken entity. Sorted.
    ///
    /// For showing what is off, not for deciding whether an edit stands: a failed part's solve
    /// stops at a least-squares compromise, which shares the misfit among the part's rows as
    /// the rows' scaling has it, so whether a given row ends within its tolerance says little.
    /// The editor judges edits by [`exposed`](Self::exposed) alone.
    pub broken: Vec<EntityId>,
    /// Nothing was written: a value wasn't a number, or a solve with the fold steps didn't
    /// finish and put the geometry back. [`broken`](Self::broken) and
    /// [`exposed`](Self::exposed) are then empty: what they would describe was never written.
    pub unwritten: bool,
    /// Constraints already implied by others (found during substitution).
    pub redundant: Vec<ConstraintId>,
    /// Equations that are linearly dependent on others (redundant but consistent).
    pub dependent_equations: usize,
    /// Remaining degrees of freedom (with [`SolveOptions::analyze`]).
    pub dof: Option<usize>,
    /// Per-entity definition state (with [`SolveOptions::analyze`]).
    pub definitions: HashMap<EntityId, Definition>,
    /// The length bar the components holding a dragged or carried parameter were solved to
    /// (see `Bar`), the coarsest of them; for a solve with none dragged, the coarsest of all
    /// its components; zero for a solve with no component. A drag reads whether it landed
    /// against it ([`landed_within`]), as the solver's own landing test reads each dragged
    /// parameter's component's bar.
    pub bar: f64,
}

/// What a component's stops are measured against: how far off a length row may be and still
/// hold, how far a step may move and still be no move, how far off a kept shape may be and
/// still be met (all [`length`](Self::length)), and how far off an ellipse's tangency, a sine,
/// may be ([`angle`](Self::angle)).
///
/// Every solve, a drag's, a commit's or an analysis's, reads one bar per component (see
/// [`Frame`]): [`Tolerances::length`] of a quarter of the component's size, A/4, in place of
/// a share of the coordinates involved. So the same drag runs the same iterations, and stops
/// at the same step, however big the part is and wherever it sits (#280, #366); and a part
/// far from the origin is held to its own size, not to its coordinates, in a frame where its
/// angle rows can be met (#387). The component's size is what it draws (see
/// [`Frame::enter`]). Its length bar never goes below [`FLOOR_ULPS`] ε of the largest stored
/// value the component reads, what storage rounding leaves between solves: [`BAR`] of a
/// part's size meets that floor only about a million part-sizes out.
///
/// An angle between lines holds to the length bar over its shorter line: its end within the
/// bar of where the angle puts it (`row_tolerance`). So it is held as every length is, at any
/// size and place, and its floor is the length bar's (PR 3 of the solver audit). Before, it
/// kept a unitless 1e-12 floored at the ulps of its stored ends over its shorter line, which
/// took over some 560 of that line's lengths from the origin, inside a solve too: an edge in
/// where a solve with angle rows stopped (Opus's 2b on #391).
#[derive(Clone, Copy, Debug)]
struct Bar {
    length: f64,
    angle: f64,
    /// [`FLOOR_ULPS`] ε of the largest stored value the component reads.
    floor: f64,
    /// The length bar without its floor: what a solve holds its own values to once it has
    /// moved them (see `newton`).
    share: f64,
}

impl Bar {
    /// For geometry `size` across (see [`extent`]) whose stored values reach `largest`.
    fn new(size: f64, largest: f64, tol: &Tolerances) -> Self {
        let share = if size.is_finite() {
            tol.length * size / 4.0
        } else {
            0.0
        };
        let floor = FLOOR_ULPS * f64::EPSILON * largest;
        Self {
            length: share.max(floor),
            angle: tol.angle,
            floor,
            share,
        }
    }
}

/// How far geometry is across, and its middle: twice its farthest point from the points'
/// middle, a circle counting as far as it reaches (its centre's distance from the middle
/// plus its radius). The same however the geometry is turned or wherever it is. Zero for a
/// single point.
fn extent(points: &[DVec2], circles: &[(Option<DVec2>, f64)]) -> (DVec2, f64) {
    let middle = points.iter().sum::<DVec2>() / points.len().max(1) as f64;
    let mut size = points
        .iter()
        .map(|p| p.distance(middle))
        .fold(0.0, f64::max);
    for &(center, r) in circles {
        let reach = center.map_or(0.0, |c| c.distance(middle)) + r.abs();
        size = size.max(reach);
    }
    (middle, 2.0 * size)
}

/// How far a point stored at `target` may be from it and still have landed, for a solve whose
/// length bar was `bar` ([`SolveReport::bar`]): the bar, never below its floor of ulps of the
/// stored coordinates (a point reached in steps rather than set is at its target only to
/// rounding). What the editor asks whether a drag landed with (`Editor::landed`,
/// `breaks_held_shape`), so it measures with the solver's bar rather than one of its own.
pub fn landed_within(bar: f64, target: DVec2) -> f64 {
    bar.max(FLOOR_ULPS * f64::EPSILON * target.abs().max_element())
}

/// Which way a group moves when its geometry is translated: x, y, or not at all (a radius).
fn axis_of(sys: &System, g: usize) -> Option<usize> {
    match sys.params[*sys.groups[g].members.first()?] {
        Param::X(_) | Param::MidX(_) => Some(0),
        Param::Y(_) | Param::MidY(_) => Some(1),
        Param::Radius(_) => None,
    }
}

/// What a component's size is measured by, found once per pass over a system (see
/// [`Frame::enter`]): each circle's centre, by the circle, and each arc of
/// [`System::arcs`], by the groups of its ends.
struct Drawn {
    centres: HashMap<EntityId, EntityId>,
    arcs_by_group: HashMap<usize, Vec<usize>>,
}

impl Drawn {
    fn of(sys: &System) -> Self {
        let centres = sys
            .entities
            .iter()
            .filter_map(|(id, shape)| match shape {
                EntityShape::Curve(points, Some(r)) if r == id => Some((*id, *points.first()?)),
                _ => None,
            })
            .collect();
        let mut arcs_by_group: HashMap<usize, Vec<usize>> = HashMap::new();
        for (a, [_, start, end]) in sys.arcs.iter().enumerate() {
            for p in [Param::X(*start), Param::X(*end)] {
                if let Some(&i) = sys.param_index.get(&p) {
                    arcs_by_group.entry(sys.group_of[i]).or_default().push(a);
                }
            }
        }
        Self {
            centres,
            arcs_by_group,
        }
    }
}

/// Points along an arc from `start` counter-clockwise to `end` about `center`, at a quarter,
/// half and three quarters of its sweep: with its ends, what it reaches as drawn, to within
/// a few per cent of its radius (a whole circle's sweep where its ends meet). Built from
/// bisectors of unit vectors, with square roots only: no trigonometry, whose last bits differ
/// between platforms' maths libraries, and would make a part's size, and so where a drag at a
/// fold stops, depend on the platform (`tangent_and_parallel` on Linux).
fn along_arc(center: DVec2, start: DVec2, end: DVec2) -> [DVec2; 3] {
    let (u, w) = (start - center, end - center);
    let r = u.length();
    let unit = |v: DVec2| v / v.length();
    let (a, b) = (unit(u), unit(w));
    // The bisector of two directions less than a half turn apart (counter-clockwise from the
    // first), and the direction a quarter turn on from one.
    let half = |a: DVec2, b: DVec2| {
        let sum = a + b;
        if sum.length() > 1e-12 {
            unit(sum)
        } else {
            a.perp()
        }
    };
    // The arc's middle: on the bisector of its ends' directions, on the side its sweep goes
    // (a quarter turn on where they are opposite, whichever side rounding puts them; opposite
    // its start where they meet, a whole circle).
    let cross = a.perp_dot(b);
    let middle = if (a + b).length() <= 1e-12 {
        a.perp()
    } else if cross > 0.0 {
        half(a, b)
    } else if cross < 0.0 {
        -half(a, b)
    } else {
        -a
    };
    [half(a, middle), middle, half(middle, b)].map(|d| center + d * r)
}

/// How far what `groups` draw is across, and its middle (see [`Frame::enter`]): their points
/// (each point an arc only centres on left out), an arc's ends and points along it, and each
/// circle's reach, at `values`.
fn drawn_extent(
    sys: &System,
    groups: &[usize],
    values: impl Fn(usize) -> f64,
    drawn: &Drawn,
) -> (DVec2, f64) {
    let group = |p: Param| sys.param_index.get(&p).map(|&i| sys.group_of[i]);
    let value = |p: Param| group(p).map(&values);
    let mut ids: Vec<(EntityId, bool)> = Vec::new();
    let mut radii: Vec<EntityId> = Vec::new();
    for &g in groups {
        for &i in &sys.groups[g].members {
            match sys.params[i] {
                Param::X(id) | Param::Y(id) => ids.push((id, false)),
                Param::MidX(id) | Param::MidY(id) => ids.push((id, true)),
                Param::Radius(id) => radii.push(id),
            }
        }
    }
    radii.sort_unstable();
    radii.dedup();
    // (One point per pair of groups: points that substitution merged, a point held on
    // another, are one point of the component, however many entities they were.)
    let mut pairs: Vec<(usize, usize)> = ids
        .iter()
        .filter(|&&(id, mid)| mid || !sys.centres_only.contains(&id))
        .filter_map(|&(id, mid)| {
            let (x, y) = if mid {
                (Param::MidX(id), Param::MidY(id))
            } else {
                (Param::X(id), Param::Y(id))
            };
            Some((group(x)?, group(y)?))
        })
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    let mut points: Vec<DVec2> = pairs
        .iter()
        .map(|&(x, y)| DVec2::new(values(x), values(y)))
        .collect();
    let mut arcs: Vec<usize> = groups
        .iter()
        .filter_map(|g| drawn.arcs_by_group.get(g))
        .flatten()
        .copied()
        .collect();
    arcs.sort_unstable();
    arcs.dedup();
    let point = |id: EntityId| Some(DVec2::new(value(Param::X(id))?, value(Param::Y(id))?));
    for a in arcs {
        let [c, s, e] = sys.arcs[a];
        if let (Some(c), Some(s), Some(e)) = (point(c), point(s), point(e)) {
            points.extend(along_arc(c, s, e).into_iter().filter(|p| p.is_finite()));
        }
    }
    let circles: Vec<(Option<DVec2>, f64)> = radii
        .iter()
        .filter_map(|&id| {
            let center = drawn
                .centres
                .get(&id)
                .and_then(|&c| Some(DVec2::new(value(Param::X(c))?, value(Param::Y(c))?)));
            Some((center, value(Param::Radius(id))?))
        })
        .collect();
    extent(&points, &circles)
}

/// A component's own frame (#280, #366, #387): its geometry translated so that its middle is
/// at the origin, and the [`Bar`] its stops are measured against.
///
/// Geometry is stored in absolute coordinates, so a part far from the origin carried the
/// round-off of its place into every residual, step and stop, and the same drag came out
/// differently there. In its frame a component's coordinates are about its own size wherever
/// it is. The shift is the middle rounded to a multiple of the largest power of two not over
/// the component's size, so translating in is exact (each value less the shift is a
/// difference on that power of two's grid), and a value the solve leaves as it found it goes
/// back as it was stored, bit for bit: a solve whose rows already hold writes nothing.
struct Frame {
    /// Each translated group: its value as stored, and the shift taken off it.
    moved: Vec<(usize, f64, f64)>,
    bar: Bar,
}

impl Frame {
    /// Translate `groups` (a component's unknowns and every pinned group its rows and kept
    /// shapes read, each once) into their frame, in `values`. `asked` is the longest length
    /// its rows ask for, which counts towards its size: a line dimensioned at 100 is a part
    /// 100 across once solved, however short it was drawn.
    ///
    /// Its size is what it draws: its points (each point an arc only centres on left out), an
    /// arc's ends and points along it, and each circle's reach. A nearly straight arc with a
    /// 10 mm chord and a 10 km radius is a part some 10 mm across, not 20 km (counted, its
    /// centre made the bar a thousand times coarser than the arc's, and a 0.003 mm misfit
    /// beside it "held"). Its centre still counts towards the floor of ulps: the rows read its
    /// stored coordinates.
    fn enter(
        sys: &System,
        groups: &[usize],
        asked: f64,
        values: &mut [f64],
        tol: &Tolerances,
        drawn: &Drawn,
    ) -> Self {
        let largest = groups
            .iter()
            .map(|&g| values[g].abs())
            .fold(0.0f64, f64::max);
        let (middle, size) = drawn_extent(sys, groups, |g| values[g], drawn);
        let size = size.max(asked);
        let mut moved = Vec::new();
        if size.is_finite() && size > 0.0 && middle.is_finite() {
            let grid = size.log2().floor().exp2();
            let shift = [
                (middle.x / grid).round() * grid,
                (middle.y / grid).round() * grid,
            ];
            for &g in groups {
                if let Some(axis) = axis_of(sys, g)
                    && shift[axis] != 0.0
                {
                    moved.push((g, values[g], shift[axis]));
                    values[g] -= shift[axis];
                }
            }
        }
        let bar = Bar::new(size, largest, tol);
        Self { moved, bar }
    }

    /// Translate back into storage: a value the solve changed is shifted back, one it left as
    /// it found it is put back exactly as it was stored.
    fn leave(self, values: &mut [f64]) {
        for (g, stored, shift) in self.moved {
            values[g] = if values[g] == stored - shift {
                stored
            } else {
                values[g] + shift
            };
        }
    }
}

/// What a component reads besides its unknowns: the indices of the kept shapes over any of
/// them ([`System::keep`]), every group its rows and those shapes read (its unknowns among
/// them, each once), and the longest length its rows ask for (see [`Frame::enter`]).
fn reach(sys: &System, unknowns: &[usize], rows: &[usize]) -> (Vec<usize>, Vec<usize>, f64) {
    let own: HashSet<usize> = unknowns.iter().copied().collect();
    let shapes: Vec<usize> = if own.is_empty() {
        Vec::new()
    } else if !sys.keep_of_group.is_empty() {
        // (Through the index: a drawing's every line kept, scanned for each component, would
        // cost components times shapes.)
        let mut shapes: Vec<usize> = own
            .iter()
            .flat_map(|g| sys.keep_of_group[*g].iter().copied())
            .collect();
        shapes.sort_unstable();
        shapes.dedup();
        shapes
    } else {
        (0..sys.keep.len())
            .filter(|&s| {
                sys.keep[s]
                    .params()
                    .iter()
                    .any(|p| own.contains(&sys.group_of[*p]))
            })
            .collect()
    };
    let mut groups: Vec<usize> = unknowns.to_vec();
    let mut asked = 0.0f64;
    for &r in rows {
        let eq = &sys.rows[r].equation;
        groups.extend(eq.params().into_iter().map(|p| sys.group_of[p]));
        if let Some(length) = eq.length_asked() {
            asked = asked.max(length.abs());
        }
    }
    for &s in &shapes {
        groups.extend(sys.keep[s].params().iter().map(|p| sys.group_of[*p]));
    }
    groups.sort_unstable();
    groups.dedup();
    (shapes, groups, asked)
}

/// Whether each row of `sys` holds where the geometry stands, as a solve judges it: in its
/// component's frame, against that component's bar; a row over pinned geometry alone (one
/// of `constant_rows`) in its own. False for a residual that isn't a number.
fn rows_holding(
    sys: &System,
    comps: &[Component],
    constant_rows: &[usize],
    tol: &Tolerances,
) -> Vec<bool> {
    let mut values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let drawn = Drawn::of(sys);
    let mut holds = vec![true; sys.rows.len()];
    let mut grad = Vec::new();
    let mut judge = |unknowns: &[usize], rows: &[usize], values: &mut [f64]| {
        let (_, groups, asked) = reach(sys, unknowns, rows);
        let frame = Frame::enter(sys, &groups, asked, values, tol, &drawn);
        for &r in rows {
            grad.clear();
            let res = sys.rows[r]
                .equation
                .eval(&|p| values[sys.group_of[p]], &mut grad);
            holds[r] = res.abs() <= row_tolerance(sys, r, frame.bar, values);
        }
        frame.leave(values);
    };
    for &r in constant_rows {
        judge(&[], &[r], &mut values);
    }
    for comp in comps {
        judge(&comp.unknowns, &comp.rows, &mut values);
    }
    holds
}

/// A kept shape's term in a step: its gradient over the component's columns, and how far off
/// it is.
type SoftTerm = (Vec<(usize, f64)>, f64);

/// The most iterations a component's Newton solve runs.
pub const MAX_ITERATIONS: usize = 100;

/// How strongly a shape is kept in a minimum-change step, relative to moving a point: being
/// off a radius, length or direction by some distance costs as much as moving a point that
/// far, this many times over.
const KEEP_SHAPE: f64 = 100.0;

/// Inverse weight for parameters outside [`SolveOptions::focus`] when it is set: moving them
/// costs this many times as much as moving the focus. The third rank of what stays put,
/// after the dragged and the carried (`system::DRAGGED_MOBILITY`, `system::CARRIED_MOBILITY`).
const UNFOCUSED_MOBILITY: f64 = 1e-3;

/// A step that delivers less than this share of what its linearisation promised went badly
/// (see [`SolveReport::wandered`]).
const WANDERING_GAIN: f64 = 0.25;

/// The minimum-change term of a step, as a share of the normal matrix's largest diagonal:
/// `μ·M⁻¹` picks the least weighted change among the solutions of the linearised rows, and
/// with it what counts as a direction the rows can't move (a singular value of the scaled rows
/// below √(μ/mobility), five decades down for freely moving geometry). The same share is what
/// projects the preferences' steps into the rows' null space (see `newton`).
const MIN_CHANGE: f64 = 1e-10;

/// How large a second-order correction may be next to the step it corrects (the most any point
/// moves in each): no larger. A step's correction is its second-order term; one larger than the
/// step itself says the step went where its linearisation says nothing, and the correction
/// would only carry it further off. (Transtrum and Sethna bound a Levenberg–Marquardt step's
/// geodesic acceleration more tightly, at 0.375 of it; that cuts the corrections that swing a
/// chain of bars held at their lengths round a joint (#349), and the chain crawls again.)
const CORRECTION_SHARE: f64 = 1.0;

/// A kept shape is met once it is off by no more than this times its own size (an arc's
/// radius, a line's length), or the solver's tolerance where that is looser: what a solve
/// with [`SolveOptions::fold`] must meet every shape to, and a drag's kept radius to (see
/// `newton`).
pub const FOLD_MET: f64 = 1e-9;

/// Solve the document's constraints in place. Changes go through the document's normal edit
/// methods, so they are recorded by an open transaction.
pub fn solve(doc: &mut Document, options: &SolveOptions) -> SolveReport {
    let mut sys = System::build_in(
        doc,
        &options.dragged,
        &options.carried,
        options.scope.as_deref(),
        &options.radii,
        options.lowering.as_deref(),
        options.relations.as_deref().map(Vec::as_slice),
        options.touching.as_deref(),
    );
    // A point merged with a dragged one (coincident with it) is dragged too: a preference
    // on a line or arc it ends would only pull against the drag, and slowly, the dragged
    // being heavy, so the solve would end part way along, wherever its tolerance let it.
    // (Both coordinates: a point level with a dragged one still moves freely across. And
    // only what the user drags: a carried point's curves keep their preferences.)
    let dragged_groups: HashSet<usize> = options
        .dragged
        .iter()
        .filter_map(|q| sys.param_index.get(q).map(|&i| sys.group_of[i]))
        .collect();
    let dragged_group = |p: EntityId| {
        Param::point(p).iter().all(|q| {
            sys.param_index
                .get(q)
                .is_some_and(|&i| dragged_groups.contains(&sys.group_of[i]))
        })
    };
    for &arc in options.radii.keys() {
        if let Some(EntityKind::Arc { center, start, end }) = doc.get(arc).map(|e| e.kind)
            && (dragged_group(start) || dragged_group(end))
        {
            let key = [
                Param::X(center),
                Param::Y(center),
                Param::X(start),
                Param::Y(start),
            ]
            .map(|p| sys.param_index.get(&p).copied());
            sys.keep
                .retain(|s| !matches!(s, Shape::Distance(k, _) if k.map(Some) == key));
        }
        if let Some(EntityKind::Circle { .. }) = doc.get(arc).map(|e| e.kind)
            && let Some(&k) = sys.param_index.get(&Param::Radius(arc))
            && dragged_groups.contains(&sys.group_of[k])
        {
            sys.keep
                .retain(|s| !matches!(s, Shape::Radius(r, _) if *r == k));
        }
    }
    sys.radii_kept = sys.keep.len();
    // Sorted, so the preferences' rows come in the same order every run (a hash map's order
    // would change the floating-point sums, and with them the result's last bits).
    let mut directions: Vec<_> = options.directions.iter().collect();
    directions.sort_unstable_by_key(|(id, _)| **id);
    for (&line, &u) in directions {
        if let Some(EntityKind::Line { a, b }) = doc.get(line).map(|e| e.kind) {
            let ends = [Param::X(a), Param::Y(a), Param::X(b), Param::Y(b)]
                .map(|p| sys.param_index.get(&p).copied());
            if let [Some(ax), Some(ay), Some(bx), Some(by)] = ends {
                let length =
                    Shape::length(&[ax, ay, bx, by], |p| sys.groups[sys.group_of[p]].value);
                sys.keep.push(Shape::Direction(
                    [ax, ay, bx, by],
                    u,
                    (!options.shapes_as_before).then_some(length),
                ));
            }
        }
    }
    sys.keep_rank = vec![1.0; sys.keep.len()];
    sys.shapes_as_before = options.shapes_as_before;
    let mut lengths: Vec<_> = options.lengths.iter().collect();
    lengths.sort_unstable_by_key(|(id, _)| **id);
    for (&line, &length) in lengths {
        if let Some(EntityKind::Line { a, b }) = doc.get(line).map(|e| e.kind)
            && !dragged_group(a)
            && !dragged_group(b)
        {
            let ends = [Param::X(a), Param::Y(a), Param::X(b), Param::Y(b)]
                .map(|p| sys.param_index.get(&p).copied());
            if let [Some(ax), Some(ay), Some(bx), Some(by)] = ends {
                sys.keep.push(Shape::Distance([ax, ay, bx, by], length));
                sys.keep_rank
                    .push(options.length_ranks.get(&line).copied().unwrap_or(1.0));
            }
        }
    }
    // Which kept shapes read each group (see `reach`).
    let mut keep_of_group = vec![Vec::new(); sys.groups.len()];
    for (s, shape) in sys.keep.iter().enumerate() {
        for p in shape.params() {
            let g = sys.group_of[*p];
            if keep_of_group[g].last() != Some(&s) {
                keep_of_group[g].push(s);
            }
        }
    }
    sys.keep_of_group = keep_of_group;
    if !options.focus.is_empty() {
        let focus: HashSet<usize> = options
            .focus
            .iter()
            .filter_map(|p| sys.param_index.get(p).map(|&i| sys.group_of[i]))
            .collect();
        for (g, group) in sys.groups.iter_mut().enumerate() {
            if !focus.contains(&g) {
                group.mobility = group.mobility.min(UNFOCUSED_MOBILITY);
            }
        }
    }
    // Where the dragged, then the carried, parameters were asked to be.
    let targets: Vec<Vec<(usize, f64)>> = [&options.dragged, &options.carried]
        .iter()
        .map(|params| {
            let mut groups: Vec<usize> = params
                .iter()
                .filter_map(|p| sys.param_index.get(p).map(|&i| sys.group_of[i]))
                .filter(|&g| !sys.groups[g].fixed)
                .collect();
            groups.sort_unstable();
            groups.dedup();
            groups
                .into_iter()
                .map(|g| (g, sys.groups[g].value))
                .collect()
        })
        .collect();
    let entry: Vec<f64> = if options.fold {
        sys.groups.iter().map(|g| g.value).collect()
    } else {
        Vec::new()
    };
    let (mut report, failed) = run(&mut sys, options, true);
    // (Landed wherever every component converged: an old conflict over pinned geometry
    // alone, which no step can change, doesn't keep a drag beside it off its targets.)
    if failed.components == 0 {
        report = land(&mut sys, options, &targets, report, &failed.rows);
    }
    // With the fold steps, finished means everything met: the relations, every rank landed
    // exactly where it was asked (`land` pins them so), and every kept shape. Otherwise
    // the solve is unfinished, and changes nothing.
    if options.fold {
        let landed = targets
            .iter()
            .flatten()
            .all(|&(g, v)| on_target(&sys, g, v));
        if !(report.converged && landed && kept_met(&sys)) {
            for (group, v) in sys.groups.iter_mut().zip(entry) {
                group.value = v;
            }
            report.converged = false;
            report.unsourced_failure = true;
            report.unwritten = true;
        }
    }
    // A result that isn't a number (a degenerate configuration the equations divide by) is
    // never written: the drawing stays as it was and the solve counts as failed.
    if !report.unwritten && !write_back(doc, &sys) {
        report.converged = false;
        report.unwritten = true;
    }
    if report.unwritten {
        report.broken.clear();
        report.exposed.clear();
    }
    report
}

/// How far off a kept shape over `k` may be and still be met: [`FOLD_MET`] of its length, or
/// the length bar it is solved to (`bar`) where that is looser.
fn met_within(shape: &Shape, value: impl Fn(usize) -> f64, bar: f64) -> f64 {
    (FOLD_MET * shape.size(&value)).max(bar)
}

/// Whether every kept shape the solve could move is met ([`met_within`]), each against the
/// bar of the component its parameters were last solved in ([`System::bars`]), and never
/// finer than a few ulps of its stored coordinates (it is read from storage). A shape whose
/// parameters are all pinned is as the solve found it, and isn't judged.
fn kept_met(sys: &System) -> bool {
    let value = |p: usize| sys.groups[sys.group_of[p]].value;
    sys.keep.iter().all(|shape| {
        let k = shape.params();
        let bar = k
            .iter()
            .map(|p| sys.bars[sys.group_of[*p]].max(FLOOR_ULPS * f64::EPSILON * value(*p).abs()))
            .fold(0.0, f64::max);
        k.iter().all(|p| sys.groups[sys.group_of[*p]].fixed)
            || shape
                .offset(value)
                .is_none_or(|(_, off)| off.abs() <= met_within(shape, value, bar))
    })
}

/// Whether group `g` is where it was asked to be, `v`, to the bar it was solved to
/// ([`System::bars`]): a dragged point an ulp off its target (at one size, and on it to the bit
/// at another) has landed as surely as one on it, and gets no landing pass (#280).
fn on_target(sys: &System, g: usize, v: f64) -> bool {
    (sys.groups[g].value - v).abs() <= sys.bars.get(g).copied().unwrap_or(0.0)
}

/// After a drag's solve: the dragged and carried parameters are only heavy, not pinned, so
/// where other geometry has to follow them they give a little (and, steps being measured from
/// where the solve is rather than from the targets, nothing pulls them back). Pin them where
/// they were asked to be and solve again, rank by rank (the dragged first, then the carried
/// with them); keep each that fails nothing the solve didn't (`failed`, the rows it failed:
/// none, or only rows over pinned geometry, an old conflict), so a move lands exactly wherever
/// it can, and what it pushes along lands with it where that still leaves room.
fn land(
    sys: &mut System,
    options: &SolveOptions,
    targets: &[Vec<(usize, f64)>],
    mut report: SolveReport,
    failed: &[usize],
) -> SolveReport {
    let mut pinned: Vec<(usize, f64)> = Vec::new();
    for rank in targets {
        pinned.extend(rank);
        if rank.is_empty() || pinned.iter().all(|&(g, v)| on_target(sys, g, v)) {
            continue;
        }
        let solved: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
        // (A pass not kept leaves the bars the solve was held to, as well as its values.)
        let bars = sys.bars.clone();
        for &(g, v) in &pinned {
            sys.groups[g].value = v;
            sys.groups[g].fixed = true;
        }
        let (landed, landed_failed) = run(sys, options, true);
        for &(g, _) in &pinned {
            sys.groups[g].fixed = false;
        }
        // (Kept where it fails nothing the solve didn't.)
        if landed_failed.components == 0 && landed_failed.rows == failed {
            report = SolveReport {
                iterations: landed.iterations.max(report.iterations),
                wandered: landed.wandered.max(report.wandered),
                bar: landed.bar.max(report.bar),
                ..landed
            };
        } else {
            for (group, v) in sys.groups.iter_mut().zip(solved) {
                group.value = v;
            }
            sys.bars = bars;
            return report;
        }
    }
    report
}

/// The constraints the geometry doesn't meet as it stands (nothing is moved): those with an
/// equation off by more than `slack`, as a share of a quarter of its component's size in that
/// component's frame, as a solve judges it with `slack` for its tolerance (see `Bar`; never
/// finer than [`FLOOR_ULPS`] ulps of the stored coordinates); an angle between lines to that
/// over its shorter line, and an ellipse's tangency, a sine, within `slack`.
pub fn unmet(doc: &Document, slack: f64) -> HashSet<ConstraintId> {
    unmet_with(doc, slack, None)
}

/// [`unmet`], reading relations through `lowering`.
pub fn unmet_with(
    doc: &Document,
    slack: f64,
    lowering: Option<&Lowering>,
) -> HashSet<ConstraintId> {
    let tol = Tolerances {
        length: slack,
        angle: slack,
    };
    let sys = System::build(doc, &[], lowering);
    let (comps, constant_rows) = components(&sys, &tol);
    let holds = rows_holding(&sys, &comps, &constant_rows, &tol);
    sys.rows
        .iter()
        .zip(holds)
        .filter(|(_, holds)| !holds)
        .filter_map(|(row, _)| row.source)
        .collect()
}

/// Whether every row of `scope`'s system (its `relations` as lowered, and each curve's own
/// geometry) holds as a solve with the default [`Tolerances`] judges it: in its component's
/// frame, against the solver's own bar. Only the scope is built and read, so geometry
/// elsewhere in the drawing, however far off, has no say, and the cost is the scope's.
pub fn meets_within(
    doc: &Document,
    scope: &HashSet<EntityId>,
    relations: &Relations,
    lowering: Option<&Lowering>,
    touching: Option<&TouchingEnds>,
) -> bool {
    let tol = Tolerances::default();
    let sys = System::build_in(
        doc,
        &[],
        &[],
        Some(scope),
        &HashMap::new(),
        lowering,
        Some(relations.as_slice()),
        touching,
    );
    let (comps, constant_rows) = components(&sys, &tol);
    rows_holding(&sys, &comps, &constant_rows, &tol)
        .into_iter()
        .all(|holds| holds)
}

/// Evaluate constraints and degrees of freedom without changing the document.
pub fn analyze(doc: &Document) -> SolveReport {
    analyze_with(doc, None)
}

/// [`analyze`], reading relations through `lowering`.
pub fn analyze_with(doc: &Document, lowering: Option<&Lowering>) -> SolveReport {
    let options = SolveOptions {
        analyze: true,
        ..Default::default()
    };
    let mut sys = System::build(doc, &[], lowering);
    let (mut report, _) = run(&mut sys, &options, false);
    // (In one order however the relations were found redundant: what an analysis kept
    // cluster by cluster reports too.)
    report.redundant.sort();
    report
}

/// The entities that can move with `seeds`: everything joined to them through shared points
/// and (non-driven) constraints.
pub fn connected(doc: &Document, seeds: impl IntoIterator<Item = EntityId>) -> HashSet<EntityId> {
    connected_with(doc, seeds, None)
}

/// [`connected`], reading relations through `lowering` (a lowered relation joins what it was
/// lowered to, and the lowering's extras join theirs).
pub fn connected_with(
    doc: &Document,
    seeds: impl IntoIterator<Item = EntityId>,
    lowering: Option<&Lowering>,
) -> HashSet<EntityId> {
    connected_parts_with(doc, seeds, lowering)
        .into_iter()
        .flatten()
        .collect()
}

/// [`connected_with`], part by part: each set of entities joined to one another (and to
/// some of `seeds`) through shared points and constraints, in id order, the parts in the
/// order of their first ids. Nothing in one part moves anything in another, so a drag can
/// solve each on its own.
pub fn connected_parts_with(
    doc: &Document,
    seeds: impl IntoIterator<Item = EntityId>,
    lowering: Option<&Lowering>,
) -> Vec<Vec<EntityId>> {
    let ids: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
    let index: HashMap<EntityId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parent = UnionFind::new(ids.len());
    let join = |parent: &mut UnionFind, a: EntityId, b: EntityId| {
        if let (Some(&i), Some(&j)) = (index.get(&a), index.get(&b)) {
            parent.join(i, j);
        }
    };
    for (id, e) in doc.iter() {
        for p in e.kind.points() {
            join(&mut parent, id, p);
        }
    }
    for (_, l) in lowering::relations(doc, lowering) {
        let ents = l.entities();
        if let Some((&first, rest)) = ents.split_first() {
            for &other in rest {
                join(&mut parent, first, other);
            }
        }
    }
    let roots: HashSet<usize> = seeds
        .into_iter()
        .filter_map(|id| index.get(&id).copied())
        .map(|i| parent.find(i))
        .collect();
    let mut parts: Vec<Vec<EntityId>> = Vec::new();
    let mut part_of: HashMap<usize, usize> = HashMap::new();
    for (i, &id) in ids.iter().enumerate() {
        let root = parent.find(i);
        if roots.contains(&root) {
            let k = *part_of.entry(root).or_insert_with(|| {
                parts.push(Vec::new());
                parts.len() - 1
            });
            parts[k].push(id);
        }
    }
    parts
}

/// Relations whose geometry is gone or no longer of the kinds they need (a trim or a
/// split changed a curve under them), with why. The solver leaves them out.
pub fn invalid_constraints(doc: &Document) -> Vec<(drift_core::ConstraintId, String)> {
    doc.constraints()
        .filter_map(|(id, c)| doc.validate(c).err().map(|e| (id, e.to_string())))
        .collect()
}

/// Whether solving `scope` (closed as [`connected`] closes it) can do anything a plain move
/// doesn't: it holds a relation or dimension, geometry pinned on a locked layer, or a curve
/// that keeps a shape of its own (an arc's ends on its circle, an ellipse's axes square).
/// Relations the solver leaves out (see [`invalid_constraints`]) don't count. Free points,
/// lines, circles and Béziers go exactly where they are put, so a drag of them needn't solve
/// at all.
pub fn needs_solve(doc: &Document, scope: &HashSet<EntityId>) -> bool {
    needs_solve_with(doc, scope, None)
}

/// [`needs_solve`], reading relations through `lowering`. Its extras (relations of the
/// geometry itself) don't count: alone they move nothing a plain move doesn't.
pub fn needs_solve_with(
    doc: &Document,
    scope: &HashSet<EntityId>,
    lowering: Option<&Lowering>,
) -> bool {
    !solve_reasons_with(doc, scope, lowering, |_| false).is_empty()
}

/// The entities of `scope` that give it something to solve (see [`needs_solve_with`]): those
/// in a relation or dimension, pinned on a locked layer, or keeping a shape of their own
/// unless `moved_whole` says every point of it is moved as one (a move keeps its shape). A
/// part of the scope ([`connected_parts_with`]) needs solving when it holds one of them.
pub fn solve_reasons_with(
    doc: &Document,
    scope: &HashSet<EntityId>,
    lowering: Option<&Lowering>,
    moved_whole: impl Fn(EntityId) -> bool,
) -> HashSet<EntityId> {
    let mut out = HashSet::new();
    for (cid, c) in doc.constraints() {
        if doc.is_driven(cid) || doc.validate(c).is_err() {
            continue;
        }
        match lowering.and_then(|l| l.of.get(&cid)) {
            Some(list) => out.extend(
                list.iter()
                    .flat_map(Lowered::entities)
                    .filter(|e| scope.contains(e)),
            ),
            None => out.extend(c.entities().filter(|e| scope.contains(e))),
        }
    }
    // (A wall's arc faces are generated, not solved.)
    let generated = doc.generated_entities();
    out.extend(
        scope
            .iter()
            .filter(|id| !generated.contains(id))
            .filter(|&&id| {
                matches!(
                    doc.get(id).map(|e| e.kind),
                    Some(
                        EntityKind::Arc { .. }
                            | EntityKind::Ellipse { .. }
                            | EntityKind::EllipseArc { .. }
                    )
                ) && !moved_whole(id)
            }),
    );
    out.extend(
        doc.locked_entities()
            .into_iter()
            .filter(|id| scope.contains(id)),
    );
    out
}

/// Whether solving the whole drawing can do anything (see [`needs_solve`]): it holds a
/// relation or dimension, geometry on a locked layer, or a curve that keeps a shape of its
/// own. A drawing of free lines (a plan of walls with no relations) has nothing to solve.
pub fn anything_to_solve(doc: &Document) -> bool {
    if doc
        .constraints()
        .any(|(cid, c)| !doc.is_driven(cid) && doc.validate(c).is_ok())
    {
        return true;
    }
    if doc.layers().any(|(_, l)| l.locked) && !doc.locked_entities().is_empty() {
        return true;
    }
    let generated = doc.generated_entities();
    doc.iter().any(|(id, e)| {
        matches!(
            e.kind,
            EntityKind::Arc { .. } | EntityKind::Ellipse { .. } | EntityKind::EllipseArc { .. }
        ) && !generated.contains(&id)
    })
}

/// Entities whose geometry has collapsed: zero-length lines, zero-radius circles and arcs.
/// Constraints can be satisfied by collapsing geometry (e.g. horizontal + vertical on one
/// line), which a sketcher should refuse.
pub fn degenerate_entities(doc: &Document) -> Vec<EntityId> {
    let point = |id| doc.point(id).ok();
    let tiny = |len: f64, scale: f64| len <= drift_core::tolerance::same(scale);
    doc.iter()
        .filter(|(_, e)| match e.kind {
            EntityKind::Point(_) => false,
            EntityKind::Line { a, b } => match (point(a), point(b)) {
                (Some(pa), Some(pb)) => tiny(pa.distance(pb), pa.abs().max_element()),
                _ => false,
            },
            EntityKind::Circle { radius, center } => {
                point(center).is_some_and(|c| tiny(radius, c.abs().max_element()))
            }
            EntityKind::Arc { center, start, end } => {
                match (point(center), point(start), point(end)) {
                    (Some(c), Some(s), Some(e)) => {
                        let scale = c.abs().max_element();
                        tiny(c.distance(s), scale) || tiny(s.distance(e), scale)
                    }
                    _ => false,
                }
            }
            EntityKind::Bezier { a, c1, c2, b } => {
                match (point(a), point(c1), point(c2), point(b)) {
                    (Some(pa), Some(p1), Some(p2), Some(pb)) => {
                        let scale = pa.abs().max_element();
                        tiny(pa.distance(pb), scale)
                            && tiny(pa.distance(p1), scale)
                            && tiny(pa.distance(p2), scale)
                    }
                    _ => false,
                }
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            }
            | EntityKind::EllipseArc {
                center,
                major,
                minor,
                ..
            } => match (point(center), point(major), point(minor)) {
                (Some(c), Some(m), Some(n)) => {
                    let scale = c.abs().max_element();
                    let arc_collapsed = match e.kind {
                        EntityKind::EllipseArc { start, end, .. } => {
                            match (point(start), point(end)) {
                                (Some(s), Some(t)) => tiny(s.distance(t), scale),
                                _ => false,
                            }
                        }
                        _ => false,
                    };
                    tiny(c.distance(m), scale) || tiny(c.distance(n), scale) || arc_collapsed
                }
                _ => false,
            },
        })
        .map(|(id, _)| id)
        .collect()
}

/// Which way round the curved pieces of a sketch sit: how far each arc sweeps, and on which
/// side each tangency touches. A solve can meet every constraint by turning an arc inside out
/// (its centre carried past its ends, so it runs the long way round) or by moving a tangent
/// curve over to the line's other side; comparing [`Orientation::of`] before and after shows
/// it ([`Orientation::turned`]), so a drag can take smaller steps or refuse.
///
/// A solve can also crush an arc to a point or close it on itself (its ends together). Its
/// sweep is then whatever round-off makes it: ~0 or ~2π for ends a hair apart either way, any
/// angle at all for no radius. Read as a sweep, whether it "turned" would hang on the last
/// bit, and the same drag would be halved at one size or place and not at another (#280). So
/// an arc that a step leaves collapsed counts as turned, as one turned inside out does: the
/// drag goes in smaller steps, and pushes the arc along or stops short rather than collapse
/// it. A circle a step crushes counts as turned the same way (it has no sweep to judge
/// otherwise). "Crushed" is against the arc's own radius as a drag began
/// ([`among_kept`](Self::among_kept)), or as the orientation is taken: an arc crushed earlier
/// in the same drag, met again where a halved step begins, has no way round to keep and isn't
/// judged, nor are its tangencies. (Not [`degenerate_entities`]: that is measured against the
/// size of the coordinates, the round-off of where a drawing is, to refuse an edit that
/// collapses geometry. A decision mid-drag must not depend on where the drawing is, nor on
/// what else is in the part, so each test here is against the arc's own size.)
#[derive(Clone, Debug, Default)]
pub struct Orientation {
    /// Arcs, their counter-clockwise sweeps (radians) and the radii they are judged crushed
    /// against.
    arcs: Vec<(EntityId, f64, f64)>,
    /// Circles and the radii they are judged crushed against (only that: see above).
    circles: Vec<(EntityId, f64)>,
    /// Tangent pairs and their side: which side of the line a curve's centre is on (±1), for
    /// two curves whether they touch from outside (+1) or one inside the other (−1), and at a
    /// shared end whether one carries on from the other (−1) or turns back in a cusp (+1).
    sides: Vec<((EntityId, EntityId), f64)>,
    /// Distances whose sign a solve could flip, and the side each is on: which way a
    /// horizontal or vertical distance runs, which side of its line a point at a distance from
    /// it is (±1). The distance holds just as well the other way, mirrored, and one big step
    /// lands there whenever it's the nearer fit.
    signs: Vec<((EntityId, EntityId), Constraint, f64)>,
}

/// A sweep that changes by more than this in one solve has jumped, not turned.
const SWEEP_JUMP: f64 = std::f64::consts::FRAC_PI_2;

impl Orientation {
    /// The orientation of the arcs and tangencies among `scope` (everything if `None`).
    pub fn of(doc: &Document, scope: Option<&HashSet<EntityId>>) -> Self {
        let constraints: Vec<(ConstraintId, Constraint)> =
            doc.constraints().map(|(id, c)| (id, *c)).collect();
        Self::among(doc, scope, &constraints)
    }

    /// [`of`](Self::of), looking only at `constraints` (those of the drawing that relate
    /// something in `scope`: a drag of many parts hands each its own).
    pub fn among(
        doc: &Document,
        scope: Option<&HashSet<EntityId>>,
        constraints: &[(ConstraintId, Constraint)],
    ) -> Self {
        Self::among_kept(doc, scope, constraints, &HashMap::new(), &[])
    }

    /// [`among`](Self::among), judging an arc or circle crushed against the radius it had as
    /// a drag began (`radii`, `circles`: a drag's starting geometry) rather than as this is
    /// taken: where a drag halves a step, an arc crushed in one half is still crushed where
    /// the next begins, and a crush spread over many small steps is caught at the step that
    /// finishes it. Arcs and circles not given keep their radius as they are now. (`circles`
    /// sorted by id, as a drag's geometry has them.)
    pub fn among_kept(
        doc: &Document,
        scope: Option<&HashSet<EntityId>>,
        constraints: &[(ConstraintId, Constraint)],
        radii: &HashMap<EntityId, f64>,
        circles: &[(EntityId, f64)],
    ) -> Self {
        let keep = |id: EntityId| scope.is_none_or(|s| s.contains(&id));
        let ids: Vec<EntityId> = match scope {
            Some(ids) => ids.iter().copied().collect(),
            None => doc.iter().map(|(id, _)| id).collect(),
        };
        let mut collapsed = HashSet::new();
        let mut arcs = Vec::new();
        let mut kept_circles = Vec::new();
        for &id in &ids {
            match doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Arc { .. }) => {
                    let Some(shape) = ArcShape::of(doc, id) else {
                        continue;
                    };
                    let reference = radii.get(&id).copied().unwrap_or(shape.radius);
                    if shape.collapsed_from(reference) {
                        collapsed.insert(id);
                    } else {
                        arcs.push((id, shape.sweep, reference));
                    }
                }
                Some(EntityKind::Circle { radius, .. }) => {
                    let reference = circles
                        .binary_search_by_key(&id, |&(c, _)| c)
                        .map_or(radius, |i| circles[i].1);
                    if crushed(radius, reference) {
                        collapsed.insert(id);
                    } else {
                        kept_circles.push((id, reference));
                    }
                }
                _ => {}
            }
        }
        let sides = constraints
            .iter()
            .copied()
            .filter_map(|(id, c)| match c {
                Constraint::Tangent { a, b }
                    if keep(a)
                        && keep(b)
                        && !doc.is_driven(id)
                        && !collapsed.contains(&a)
                        && !collapsed.contains(&b) =>
                {
                    Some(((a, b), tangent_side(doc, a, b)?))
                }
                _ => None,
            })
            .collect();
        let signs = constraints
            .iter()
            .copied()
            .filter_map(|(id, c)| {
                let (a, b) = match c {
                    Constraint::HorizontalDistance { a, b, value }
                    | Constraint::VerticalDistance { a, b, value } => {
                        (value != 0.0).then_some((a, b))?
                    }
                    Constraint::PointLineDistance { point, line, value } => {
                        (value != 0.0).then_some((point, line))?
                    }
                    _ => return None,
                };
                if !(keep(a) || keep(b)) || doc.is_driven(id) {
                    return None;
                }
                Some(((a, b), c, distance_sign(doc, &c)?))
            })
            .collect();
        Self {
            arcs,
            circles: kept_circles,
            sides,
            signs,
        }
    }

    /// True if `doc` has an arc turned inside out or collapsed, a circle crushed, or a
    /// tangency moved to the other side, since this orientation was taken. Entities added
    /// since don't count.
    pub fn turned(&self, doc: &Document) -> bool {
        !self.turned_curves(doc).is_empty()
    }

    /// The curves [`turned`](Self::turned) would blame: arcs turned inside out or collapsed,
    /// circles crushed, and both curves of a tangency that moved to the other side.
    pub fn turned_curves(&self, doc: &Document) -> Vec<EntityId> {
        let mut collapsed = HashSet::new();
        let mut out: Vec<EntityId> = self
            .arcs
            .iter()
            .filter(|&&(id, before, reference)| {
                ArcShape::of(doc, id).is_some_and(|now| {
                    if now.collapsed_from(reference) {
                        collapsed.insert(id);
                        return true;
                    }
                    (now.sweep - before).abs() > SWEEP_JUMP
                })
            })
            .map(|&(id, ..)| id)
            .collect();
        for &(id, reference) in &self.circles {
            if circle_radius(doc, id).is_some_and(|r| crushed(r, reference)) {
                collapsed.insert(id);
                out.push(id);
            }
        }
        for &((a, b), before) in &self.sides {
            // (A tangency to an arc the step collapsed has no side: the arc is blamed already.)
            if !collapsed.contains(&a)
                && !collapsed.contains(&b)
                && tangent_side(doc, a, b).is_some_and(|now| now != before)
            {
                out.extend([a, b]);
            }
        }
        for &((a, b), c, before) in &self.signs {
            if distance_sign(doc, &c).is_some_and(|now| now != before) {
                out.extend([a, b]);
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Which way a horizontal or vertical distance runs, or which side of its line a point at a
/// distance from it is (see [`Orientation`]); `None` where it's undecided (no distance).
fn distance_sign(doc: &Document, c: &Constraint) -> Option<f64> {
    // (A distance may be to a line's midpoint.)
    let at = |id: EntityId| match doc.get(id)?.kind {
        EntityKind::Line { a, b } => Some((doc.point(a).ok()? + doc.point(b).ok()?) / 2.0),
        _ => doc.point(id).ok(),
    };
    let sign = |v: f64| (v != 0.0).then(|| v.signum());
    match *c {
        Constraint::HorizontalDistance { a, b, .. } => sign(at(b)?.x - at(a)?.x),
        Constraint::VerticalDistance { a, b, .. } => sign(at(b)?.y - at(a)?.y),
        Constraint::PointLineDistance { point, line, .. } => {
            let EntityKind::Line { a, b } = doc.get(line)?.kind else {
                return None;
            };
            let (pa, pb) = (doc.point(a).ok()?, doc.point(b).ok()?);
            sign((pb - pa).perp_dot(at(point)? - pa))
        }
        _ => None,
    }
}

/// An arc (or circle) whose radius has shrunk to this share of the radius it should have
/// (see [`Orientation`]) has none left. A solve that crushes an arc stops at its tolerance on
/// the way to no radius at all, which leaves anything from round-off up to some 1e-7 of what
/// it had, depending on where the drawing is; a drag that shrinks a real arc that far, in one
/// go, has crushed it. Measured over the heavy drag properties and a census of random drags
/// (#280): crushed arcs end almost all below 1e-8 of their radius, and the counts thin out
/// between 1e-8 and 1e-6; moving this tenfold either way changes no heavy outcome and one
/// census drag in 17,562 (a crush that stalls near this share, which stops a frame or two
/// sooner or later).
const COLLAPSED_RADIUS: f64 = 1e-6;

/// An arc as [`Orientation`] sees it.
#[derive(Clone, Copy, Debug)]
struct ArcShape {
    /// Counter-clockwise sweep from start to end, in (0, 2π].
    sweep: f64,
    /// The farther of its ends from its centre.
    radius: f64,
    /// How far apart its ends are.
    chord: f64,
}

impl ArcShape {
    fn of(doc: &Document, arc: EntityId) -> Option<Self> {
        let EntityKind::Arc { center, start, end } = doc.get(arc)?.kind else {
            return None;
        };
        let c = doc.point(center).ok()?;
        let (start, end) = (doc.point(start).ok()?, doc.point(end).ok()?);
        let (s, e) = (start - c, end - c);
        let turn = s.perp_dot(e).atan2(s.dot(e));
        Some(Self {
            sweep: if turn <= 0.0 {
                turn + std::f64::consts::TAU
            } else {
                turn
            },
            radius: s.length().max(e.length()),
            chord: start.distance(end),
        })
    }

    /// Its ends together, within [`SAME`](drift_core::tolerance::SAME) of its radius: its
    /// sweep reads ~0 or ~2π by round-off.
    fn ends_together(&self) -> bool {
        self.chord <= drift_core::tolerance::SAME * self.radius
    }

    /// Collapsed, against the radius it should have (`reference`): crushed (see [`crushed`]),
    /// or its ends together.
    fn collapsed_from(&self, reference: f64) -> bool {
        crushed(self.radius, reference) || self.ends_together()
    }
}

/// A radius crushed to nothing next to the radius it should have: within
/// [`COLLAPSED_RADIUS`] of it.
fn crushed(radius: f64, reference: f64) -> bool {
    radius <= COLLAPSED_RADIUS * reference
}

/// A circle's radius.
fn circle_radius(doc: &Document, id: EntityId) -> Option<f64> {
    match doc.get(id)?.kind {
        EntityKind::Circle { radius, .. } => Some(radius),
        _ => None,
    }
}

/// The side a tangency touches on (see [`Orientation`]); `None` where it is undecided (a
/// centre on the line, or circles equally near both kinds of contact).
fn tangent_side(doc: &Document, a: EntityId, b: EntityId) -> Option<f64> {
    let center = |id: EntityId| match doc.get(id)?.kind {
        EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. } => {
            doc.point(center).ok()
        }
        _ => None,
    };
    let sign = |v: f64| (v != 0.0).then(|| v.signum());
    if let kind_b @ (EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. }) = doc.get(b)?.kind {
        // Tangent at a shared end: one carries on from the other (−1), or turns back along
        // it in a cusp (+1).
        let t = doc.get(a)?.kind.shared_end(&kind_b)?;
        return sign(doc.end_direction(a, t)?.dot(doc.end_direction(b, t)?));
    }
    match doc.get(a)?.kind {
        EntityKind::Line { a: la, b: lb } => {
            let (pa, pb) = (doc.point(la).ok()?, doc.point(lb).ok()?);
            sign((pb - pa).perp_dot(center(b)? - pa))
        }
        _ => {
            let d = center(a)?.distance(center(b)?);
            let (r1, r2) = (radius_of(doc, a)?, radius_of(doc, b)?);
            sign((d - (r1 - r2).abs()).abs() - (d - (r1 + r2)).abs())
        }
    }
}

/// Radius of a circle or arc.
pub fn radius_of(doc: &Document, curve: EntityId) -> Option<f64> {
    match doc.get(curve)?.kind {
        EntityKind::Circle { radius, .. } => Some(radius),
        EntityKind::Arc { center, start, .. } => {
            Some(doc.point(center).ok()?.distance(doc.point(start).ok()?))
        }
        _ => None,
    }
}

/// The current value of a dimension in the document's geometry (mm, or degrees in [0, 180)).
pub fn measure(doc: &Document, constraint: &Constraint) -> Option<f64> {
    let point = |id| doc.ref_point(id);
    let line = |id| match doc.get(id)?.kind {
        EntityKind::Line { a, b } => Some(point(b)? - point(a)?),
        _ => None,
    };
    match *constraint {
        Constraint::Distance { a, b, .. } => Some(point(a)?.distance(point(b)?)),
        Constraint::Radius { curve, .. } => radius_of(doc, curve),
        Constraint::Diameter { curve, .. } => radius_of(doc, curve).map(|r| 2.0 * r),
        Constraint::HorizontalDistance { a, b, .. } => Some((point(b)?.x - point(a)?.x).abs()),
        Constraint::VerticalDistance { a, b, .. } => Some((point(b)?.y - point(a)?.y).abs()),
        Constraint::PointLineDistance {
            point: p, line: l, ..
        } => {
            let EntityKind::Line { a, b } = doc.get(l)?.kind else {
                return None;
            };
            let (pa, pb, pp) = (point(a)?, point(b)?, point(p)?);
            let d = pb - pa;
            (d.length() > 0.0).then(|| d.perp_dot(pp - pa).abs() / d.length())
        }
        Constraint::Angle { a, b, .. } => {
            let (d1, d2) = (line(a)?, line(b)?);
            let degrees = d1.perp_dot(d2).atan2(d1.dot(d2)).to_degrees();
            Some(degrees.rem_euclid(180.0))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------

struct Component {
    /// Free groups solved here (column order).
    unknowns: Vec<usize>,
    rows: Vec<usize>,
}

/// Split free groups into independent components connected by equations. Rows with no free
/// unknowns form their own (constant) components.
fn components(sys: &System, tol: &Tolerances) -> (Vec<Component>, Vec<usize>) {
    let n = sys.groups.len();
    let mut parent = UnionFind::new(n);
    let mut constant_rows = Vec::new();
    let mut row_groups = Vec::with_capacity(sys.rows.len());
    for (r, row) in sys.rows.iter().enumerate() {
        let mut free: Vec<usize> = row
            .equation
            .params()
            .into_iter()
            .map(|p| sys.group_of[p])
            .filter(|g| !sys.groups[*g].fixed)
            .collect();
        free.sort_unstable();
        free.dedup();
        if free.is_empty() {
            constant_rows.push(r);
        }
        for w in free.windows(2) {
            parent.join(w[0], w[1]);
        }
        row_groups.push(free);
    }
    // A preference joins its parameters too (a line kept to its length joins its ends, so a
    // free end can follow the other), and one that is off makes a component of them where
    // no equation does: with the other end pinned (the landing pass of a drag, say) the
    // free end has no equation left, and would otherwise not be solved for at all. (Only
    // one that is off: a rigid move of a big sketch has thousands that aren't. Off by more
    // than the bar of the shape's own size, never more than its component's, which a solve
    // then reads.)
    let value = |p: usize| sys.groups[sys.group_of[p]].value;
    let bar = |shape: &Shape| {
        let largest = shape
            .params()
            .iter()
            .map(|p| value(*p).abs())
            .fold(0.0, f64::max);
        Bar::new(shape.size(value), largest, tol).length
    };
    let mut soft_roots: Vec<usize> = Vec::new();
    for shape in &sys.keep {
        let k = shape.params();
        let off = shape.offset(value).map_or(0.0, |(_, off)| off);
        let free: Vec<usize> = k
            .iter()
            .map(|p| sys.group_of[*p])
            .filter(|g| !sys.groups[*g].fixed)
            .collect();
        for w in free.windows(2) {
            parent.join(w[0], w[1]);
        }
        if let Some(&g) = free.first()
            && off.abs() > bar(shape)
        {
            soft_roots.push(g);
        }
    }
    let mut by_root: BTreeMap<usize, Component> = BTreeMap::new();
    for g in soft_roots {
        by_root.entry(parent.find(g)).or_insert_with(|| Component {
            unknowns: Vec::new(),
            rows: Vec::new(),
        });
    }
    for (r, free) in row_groups.iter().enumerate() {
        if let Some(&g) = free.first() {
            let root = parent.find(g);
            by_root
                .entry(root)
                .or_insert_with(|| Component {
                    unknowns: Vec::new(),
                    rows: Vec::new(),
                })
                .rows
                .push(r);
        }
    }
    for g in 0..n {
        if !sys.groups[g].fixed
            && let Some(c) = by_root.get_mut(&parent.find(g))
        {
            c.unknowns.push(g);
        }
    }
    let mut comps: Vec<Component> = by_root.into_values().collect();
    comps.sort_by_key(|c| c.unknowns.first().copied());
    (comps, constant_rows)
}

/// Residual tolerance for one row at `values`, in its component's frame: the component's
/// length bar; for an angle between lines, a sine, the bar over the shorter line (its end
/// within the length bar of where the angle puts it), so that an angle row is held to the
/// same bar as every length, at any size and place (the floor of ulps of stored values only
/// meets it about a million part-sizes out, as it meets the length bar: no edge between);
/// for an ellipse's tangency, the unitless angle bar.
fn row_tolerance(sys: &System, r: usize, bar: Bar, values: &[f64]) -> f64 {
    match sys.rows[r].equation {
        Equation::Angle { l1, l2, .. } => {
            let length = |l: [usize; 4]| Shape::length(&l, |p| values[sys.group_of[p]]);
            let shortest = length(l1).min(length(l2));
            if shortest > 0.0 {
                bar.length / shortest
            } else {
                bar.angle
            }
        }
        ref eq if eq.is_angular() => bar.angle,
        _ => bar.length,
    }
}

/// Evaluate rows at `values` (group values), returning residuals and sparse Jacobian rows over
/// component columns.
fn evaluate(
    sys: &System,
    rows: &[usize],
    values: &[f64],
    col_of: &HashMap<usize, usize>,
) -> (Vec<f64>, Vec<Vec<(usize, f64)>>) {
    evaluate_with_own(sys, rows, values, col_of).0
}

/// Rows' residuals and their gradients over a component's columns, as [`evaluate`] gives
/// them.
type Evaluated = (Vec<f64>, Vec<Vec<(usize, f64)>>);

/// [`evaluate`], with each row's gradient over its own parameters taken one by one, before
/// substitution merges them or a solve pins them (its length): what [`row_scales`] measures
/// the row's gradient over the unknowns against.
fn evaluate_with_own(
    sys: &System,
    rows: &[usize],
    values: &[f64],
    col_of: &HashMap<usize, usize>,
) -> (Evaluated, Vec<f64>) {
    let val = |p: usize| values[sys.group_of[p]];
    let mut residuals = Vec::with_capacity(rows.len());
    let mut jacobian = Vec::with_capacity(rows.len());
    let mut own = Vec::with_capacity(rows.len());
    let mut grad = Vec::new();
    for &r in rows {
        grad.clear();
        residuals.push(sys.rows[r].equation.eval(&val, &mut grad));
        own.push(grad.iter().map(|(_, g)| g * g).sum::<f64>().sqrt());
        let mut row: Vec<(usize, f64)> = Vec::with_capacity(grad.len());
        for &(p, g) in &grad {
            if let Some(&c) = col_of.get(&sys.group_of[p]) {
                match row.iter_mut().find(|(cc, _)| *cc == c) {
                    Some(entry) => entry.1 += g,
                    None => row.push((c, g)),
                }
            }
        }
        jacobian.push(row);
    }
    ((residuals, jacobian), own)
}

/// A row whose gradient is this small next to the largest is left unscaled by
/// [`scaled_rows`]: it is round-off (an angle between lines whose directions substitution
/// has pinned, say), and scaled up to a unit gradient its residual, round-off too, would
/// be an equation of its own. The same share of a row's own gradient over its parameters
/// is what [`row_scales`] leaves out of the step altogether.
const NEGLIGIBLE_ROW: f64 = 1e-9;

/// Per row, the factor that scales its gradient to unit length (one, for a row with next
/// to no gradient: it has nothing to say about the step).
///
/// Zero for a row that `holds` with no gradient left over the unknowns, next to its own
/// over its parameters (`own`, see [`evaluate_with_own`]): substitution or pinning took
/// every way it had to move. A line held vertical and square to a level one, with only its
/// free end left to move, slides along itself, which no angle, no point on it and no
/// tangency feels. What is left of such a row's gradient is round-off, and of its residual
/// too; scaled up to a unit gradient it would be an equation of its own, which a step as
/// far as the drawing is across meets as well as any other (#277). Measured against the
/// largest row of its component instead it may be as large as any: where every row is such
/// a row, as in a landing pass with the dragged pinned. A row like it that doesn't hold
/// keeps its scale: its residual is real, and leaving it out stops drags that go. Left out
/// of the step, a row still counts for whether the solve has converged, which reads the
/// rows unscaled.
fn row_scales(jac: &[Vec<(usize, f64)>], own: &[f64], holds: impl Fn(usize) -> bool) -> Vec<f64> {
    let norms: Vec<f64> = jac
        .iter()
        .map(|row| row.iter().map(|(_, g)| g * g).sum::<f64>().sqrt())
        .collect();
    let largest = norms.iter().copied().fold(0.0, f64::max);
    norms
        .into_iter()
        .zip(own)
        .enumerate()
        .map(|(k, (norm, own))| {
            if norm <= NEGLIGIBLE_ROW * own && holds(k) {
                0.0
            } else if norm > NEGLIGIBLE_ROW * largest {
                1.0 / norm
            } else {
                1.0
            }
        })
        .collect()
}

/// The Jacobian rows and residuals scaled by [`row_scales`].
fn scaled_rows(
    jac: &[Vec<(usize, f64)>],
    res: &[f64],
    scales: &[f64],
) -> (Vec<Vec<(usize, f64)>>, Vec<f64>) {
    let rows = jac
        .iter()
        .zip(scales)
        .map(|(row, scale)| row.iter().map(|&(c, g)| (c, g * scale)).collect())
        .collect();
    let residuals = res.iter().zip(scales).map(|(f, scale)| f * scale).collect();
    (rows, residuals)
}

/// Squared length of the residuals scaled by [`row_scales`].
fn scaled_norm2(res: &[f64], scales: &[f64]) -> f64 {
    res.iter().zip(scales).map(|(f, s)| (f * s).powi(2)).sum()
}

struct ComponentOutcome {
    converged: bool,
    iterations: usize,
    /// Steps on the way to the relations that its linearisation badly mispredicted (or that
    /// were refused): see [`SolveReport::wandered`].
    wandered: usize,
    /// A kept radius that the free geometry could change was left off: see `newton`.
    unkept: bool,
    /// The length bar it was solved to: the component's, or tighter where the solve made the
    /// part smaller.
    bar: f64,
}

/// How a component's solve ended (see `newton`).
#[derive(Clone, Copy)]
enum Stop {
    /// The stop test: its rows hold, and its kept shapes are as near as the rows let them be.
    Settled,
    /// Its rows are off at a least-squares stationary point.
    Conflict,
    /// The safety net: it ran to [`MAX_ITERATIONS`].
    Cap,
    /// A residual or gradient wasn't a number.
    NoStep,
}

/// One component's solve, in two stages (PR 3 of the solver audit).
///
/// **The relations first.** While a row is off its bar, each step is Levenberg–Marquardt on
/// the rows scaled to unit gradients: `(JᵀJ + μ·M⁻¹ + λ·D) Δ = −JᵀF`. The minimum change
/// `μ·M⁻¹` is weighted by mobility (`M`: the dragged and carried heavy, outside a relation's
/// focus a thousand times heavier), with the kept shapes' curvature in it as a preference
/// among the rows' solutions (`μ·KEEP_SHAPE·GᵀG`, each shape as heavy as its points). The
/// damping `λ·D` is a trust region, a distance (`D` the identity, the dragged and carried kept
/// heavy), set by how well each step did what its linearisation promised (Nielsen); a step
/// longer than the drawing is across is refused, so the region's step is the damped one of
/// that size. Each step is followed by a second-order correction: Newton steps to where its
/// linearisation promised to take the rows, kept where they leave the rows nearer and move no
/// point farther than the step did ([`CORRECTION_SHARE`]). The stage ends where the rows hold,
/// to the bar of the part as the solve found it and as it leaves it, or at a least-squares
/// stationary point with a row still off: the best step the rows' linearisation offers in the
/// region brings them no more than their finest bar closer and wouldn't make them hold. That
/// is a conflict, judged by rank (what `μ` leaves unmovable), not by running out of
/// iterations (#349, #351).
///
/// **Then the preferences, among the rows' solutions.** Once the rows hold, the kept shapes
/// (radii, lengths, directions: half the sum of their offsets squared) are minimised over the
/// rows' null space by a Gauss–Newton step, solved by projected conjugate gradients in the
/// mobility-weighted metric inside a trust region (a distance, no wider than the drawing,
/// Steihaug's). Each conjugate-gradient residual is projected exactly into the rows' null
/// space (`μ·A⁻¹`, with `A = JᵀJ + μ·M⁻¹`: the weighted projection as μ goes to zero, with the
/// directions `μ` can't move counted as free), and the trial point is brought back onto the
/// rows by minimum-change Newton steps, each at least halving what is left, moving no point
/// farther than the step did. A trial is kept where the rows hold there and the shapes are
/// nearer by at least a quarter of what the step promised; the region grows where the
/// linearisation was good and shrinks where it wasn't. The dragged and carried don't take
/// part: they are where the user put them. The solve has settled where the shapes' projected
/// gradient (in the weighted metric) is below the bar (not while a kept radius is off, nor
/// with `fold`: at a fold that gradient vanishes where a shape can still be met), or the most
/// the step could gain is below the bar squared, or the region has shrunk below the bar. A
/// kept radius the free geometry can change but leaves off is reported (`unkept`): a drag
/// whose radii can't follow stops (see `run`).
///
/// The iteration cap ([`MAX_ITERATIONS`]) is a safety net, counted (`exits`), not a stop.
///
/// `dragging` is a drag's solve (for the counts, and `fold`). `fold`: a drag's last resort
/// (see [`SolveOptions::fold`]), where the rows' minimum change comes down as they close, so
/// a double root of the rows is met rather than stalled a hair from it. `values` are in the
/// component's frame, `bar` is what its stops are measured against (`bar_now` that bar where
/// the geometry stands: the bar only ever tightens within a solve), and `shapes` are the kept
/// shapes over its unknowns ([`reach`]).
#[allow(clippy::too_many_arguments)]
fn newton(
    sys: &System,
    comp: &Component,
    shapes: &[usize],
    values: &mut [f64],
    bar: Bar,
    bar_now: &dyn Fn(&[f64]) -> f64,
    dragging: bool,
    across: f64,
    fold: bool,
) -> ComponentOutcome {
    // (The bar only ever tightens within a solve: see where the rows first hold.)
    let bar = std::cell::Cell::new(bar);
    // The floor of ulps is for values read from storage, as they are at the solve's first
    // test (a solve whose rows hold there writes nothing). Once the solve has moved them, they
    // are its own values, in the frame, where the bar itself is reachable at any place; held
    // to the floor there, a solve far out stopped a hair inside it, and storing its result
    // rounded the rows over it (#377's seeded census, 100 km out).
    let leave_storage = || {
        let b = bar.get();
        if b.share > 0.0 {
            bar.set(Bar {
                length: b.length.min(b.share),
                ..b
            });
        }
    };
    let n = comp.unknowns.len();
    let col_of: HashMap<usize, usize> = comp
        .unknowns
        .iter()
        .enumerate()
        .map(|(c, g)| (*g, c))
        .collect();
    let mobility: Vec<f64> = comp
        .unknowns
        .iter()
        .map(|g| sys.groups[*g].mobility)
        .collect();
    // The dragged and carried: the preferences' steps leave them where they are.
    let pinned: Vec<bool> = mobility
        .iter()
        .map(|m| *m <= system::CARRIED_MOBILITY)
        .collect();
    let converged = |res: &[f64], values: &[f64]| {
        res.iter()
            .zip(&comp.rows)
            .all(|(f, r)| f.abs() <= row_tolerance(sys, *r, bar.get(), values))
    };

    let ((mut res, mut jac), mut own) = evaluate_with_own(sys, &comp.rows, values, &col_of);
    // Nothing to do where the rows already hold and every kept shape is met (most of a big
    // drawing's components at a commit): settled before any matrix is built.
    if converged(&res, values)
        && shapes.iter().all(|&k| {
            sys.keep[k]
                .offset(|p| values[sys.group_of[p]])
                .is_none_or(|(_, off)| off.abs() <= bar.get().length)
        })
    {
        #[cfg(feature = "exit-counts")]
        exits::record(exits::Exit::Settled, dragging, 0);
        return ComponentOutcome {
            converged: true,
            iterations: 0,
            wandered: 0,
            unkept: false,
            bar: bar.get().length,
        };
    }
    // Normal matrix N = JᵀJ over the unknowns. Its sparsity is fixed for the component (two
    // unknowns interact when an equation uses both), so build the bandwidth-reduced structure
    // once. Working over unknowns rather than equations keeps a widely shared unknown a single
    // "hub" node instead of coupling every equation that uses it.
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for row in &jac {
        for &(c1, _) in row {
            for &(c2, _) in row {
                if c1 != c2 {
                    neighbors[c1].push(c2);
                }
            }
        }
    }
    // The shapes kept (see below) couple their parameters too.
    // (And which of them are arcs' radii: see the second stage's stop.)
    let is_radius: Vec<bool> = shapes.iter().map(|&s| s < sys.radii_kept).collect();
    // What each shape weighs in the second stage, where they can't all be met
    // (`System::keep_rank`: 1 but where the caller ranks a length lower), as a multiple of the
    // lightest here: the stage's stops read the shapes' offsets as lengths against the bar,
    // and the lightest are met to it like any (weighed below 1, a shape's projected gradient
    // would fall below the bar while it was still off by the bar over its weight).
    let ranks: Vec<f64> = shapes
        .iter()
        .map(|&s| sys.keep_rank.get(s).copied().unwrap_or(1.0))
        .collect();
    let shape_weights: Vec<f64> = {
        let lightest = ranks.iter().copied().fold(f64::INFINITY, f64::min);
        ranks
            .iter()
            .map(|r| if lightest < 1.0 { r / lightest } else { *r })
            .collect()
    };
    let shapes: Vec<&Shape> = shapes.iter().map(|&s| &sys.keep[s]).collect();
    for shape in &shapes {
        let cols: Vec<usize> = shape
            .params()
            .iter()
            .filter_map(|p| col_of.get(&sys.group_of[*p]).copied())
            .collect();
        for &c1 in &cols {
            for &c2 in &cols {
                if c1 != c2 {
                    neighbors[c1].push(c2);
                }
            }
        }
    }
    for ns in &mut neighbors {
        ns.sort_unstable();
        ns.dedup();
    }
    let mut normal = Skyline::new(n, &neighbors);
    // (The rows' matrix where a preference's trial is, to bring it back onto them.)
    let mut back_normal: Option<Skyline> = None;
    // The columns of each point (x, then y) and of each radius, for the scale of the
    // regularisation: the largest diagonal of JᵀJ averaged over a point's two columns. (One
    // column's diagonal alone would turn with the sketch: a row's gradient at a point is a
    // vector, and the diagonals are its components squared. The regularisation would then
    // differ for the same sketch turned, and with it, slightly, the result; #94.)
    let mut scales: BTreeMap<(EntityId, bool), [Option<usize>; 2]> = BTreeMap::new();
    let mut radii_cols: Vec<usize> = Vec::new();
    for (c, g) in comp.unknowns.iter().enumerate() {
        for &i in &sys.groups[*g].members {
            match sys.params[i] {
                Param::X(id) => scales.entry((id, false)).or_default()[0] = Some(c),
                Param::Y(id) => scales.entry((id, false)).or_default()[1] = Some(c),
                Param::MidX(id) => scales.entry((id, true)).or_default()[0] = Some(c),
                Param::MidY(id) => scales.entry((id, true)).or_default()[1] = Some(c),
                Param::Radius(_) => radii_cols.push(c),
            }
        }
    }
    let scales: Vec<[Option<usize>; 2]> = scales
        .into_values()
        .chain(radii_cols.into_iter().map(|c| [Some(c), None]))
        .collect();
    let max_diag_of = |normal: &Skyline| {
        let max_diag = scales
            .iter()
            .map(|cols| {
                let cols: Vec<usize> = cols.iter().flatten().copied().collect();
                cols.iter().map(|&c| normal.diagonal(c)).sum::<f64>() / cols.len() as f64
            })
            .fold(0.0, f64::max);
        // (Without equations, preferences alone: their gradients are unit vectors.)
        if max_diag > 0.0 { max_diag } else { 1.0 }
    };
    // (How far a point moves, not a coordinate: the same however the drawing is turned.)
    let moved = |delta: &[f64]| {
        scales
            .iter()
            .map(|cols| {
                cols.iter()
                    .flatten()
                    .map(|&c| delta[c] * delta[c])
                    .sum::<f64>()
                    .sqrt()
            })
            .fold(0.0f64, f64::max)
    };
    // Inner products of steps as distances, for the preferences' trust region: how far the
    // linearisation can be trusted is a distance, the same for every point whatever rank of
    // what stays put it is in (the mobility-weighted metric picks among the solutions; see
    // the first stage's damping). The dragged and carried are left out: those steps don't
    // move them.
    let distance = |a: &[f64], b: &[f64]| {
        a.iter()
            .zip(b)
            .zip(&pinned)
            .filter(|(_, pinned)| !**pinned)
            .map(|((a, b), _)| a * b)
            .sum::<f64>()
    };

    // Bring `trial` onto the rows at the values `target` asks of them (zero: back onto the
    // rows; a step's linearisation: where the step promised to take them), moving only the
    // columns `fixed` doesn't: minimum-change Newton steps (the weighted least-change step as μ
    // goes to zero, with the rows' gradients where the trial is, so the steps close in
    // quadratically however the rows curve), each at least halving what is left, until every
    // row is within its tolerance of its target. The rows and gradients where it ends, or
    // `None` where it doesn't close in.
    let reproject = |trial: &mut Vec<f64>,
                     target: &[f64],
                     fixed: &[bool],
                     back_normal: &mut Option<Skyline>|
     -> Option<(Evaluated, Vec<f64>)> {
        let mut last = f64::INFINITY;
        loop {
            let ((trial_res, trial_jac), trial_own) =
                evaluate_with_own(sys, &comp.rows, trial, &col_of);
            let left: Vec<f64> = trial_res.iter().zip(target).map(|(f, t)| f - t).collect();
            let holds =
                |k: usize| left[k].abs() <= row_tolerance(sys, comp.rows[k], bar.get(), trial);
            if (0..left.len()).all(holds) {
                return Some(((trial_res, trial_jac), trial_own));
            }
            let scale = row_scales(&trial_jac, &trial_own, holds);
            let (back_jac, back_res) = scaled_rows(&trial_jac, &left, &scale);
            let off: f64 = back_res.iter().map(|f| f * f).sum();
            if !(off.is_finite() && off <= 0.25 * last) {
                return None;
            }
            last = off;
            let back_normal = back_normal.get_or_insert_with(|| Skyline::new(n, &neighbors));
            back_normal.clear();
            let mut rhs = vec![Dd::default(); n];
            for (row, f) in back_jac.iter().zip(&back_res) {
                for (k, &(c1, g1)) in row.iter().enumerate() {
                    if fixed[c1] {
                        continue;
                    }
                    rhs[c1] = rhs[c1].add_product(-g1, *f);
                    for &(c2, g2) in &row[..=k] {
                        if !fixed[c2] {
                            back_normal.add_product(c1, c2, g1, g2);
                        }
                    }
                }
            }
            let mu = MIN_CHANGE * max_diag_of(back_normal);
            let shift: Vec<f64> = mobility
                .iter()
                .zip(fixed)
                .map(|(m, fixed)| if *fixed { 1.0 } else { mu / m })
                .collect();
            let back = back_normal.factor(&shift)?.solve_refined(&rhs);
            for (c, g) in comp.unknowns.iter().enumerate() {
                if !fixed[c] {
                    trial[*g] += back[c];
                }
            }
        }
    };
    let on_the_rows = vec![0.0; comp.rows.len()];
    let no_column = vec![false; n];

    let mut lambda = 0.0f64;
    let mut wandered = 0;
    // Levenberg–Marquardt damping, raised by `nu` on a refused step and lowered by how well
    // the step did what its linearisation promised (Nielsen).
    let mut nu = 2.0f64;
    // With `fold`: at a double root of the rows, Gauss–Newton halves what is left each step
    // until the minimum-change term outweighs the vanishing gradient, and the rows stall a
    // hair from holding. So while the rows don't hold, μ and λ's floors are scaled down by how
    // far the rows are off against the drawing's size (only ever down within a solve, so the
    // step can't swing back).
    let mut fold_scale = 1.0f64;
    // The preferences' trust region, as a distance: never wider than the drawing, so one
    // step can't fling geometry across it.
    let mut radius = across;
    // (Where the solve began: in a drag, its steps together move no point farther from it than
    // the drawing is across; see the first stage.)
    let entry: Vec<f64> = comp.unknowns.iter().map(|g| values[*g]).collect();
    // (Where the second stage began: in an edit, the preferences' steps together move no
    // point farther from it than the drawing is across. The first stage's aren't bounded so:
    // a dimension typed ten times longer rightly moves geometry far.)
    let mut preferences_entry: Option<Vec<f64>> = None;
    // Which kept radii the free geometry can change (found where the second stage begins):
    // more than √μ of a radius's gradient, squared in the weighted metric, is left in the rows'
    // null space. (The projection `μ·A⁻¹` leaves μ/σ² of a direction the rows move with
    // singular value σ: this counts a direction as the rows' down to σ² of √μ, five decades
    // down, and a radius whose gradient the rows take whole as one they decide.) One that can't is the drag's
    // to dictate: the dragged and pinned geometry decide it, and it follows. One that can, but
    // is left off, is at a fold of the free geometry: the drag can't keep it, and would only go
    // on by growing (or shrinking) it past where the free geometry turns (#282's e13). A drag
    // stops there (the binding rule; see `SolveReport::converged`).
    let mut free_radius: Option<Vec<bool>> = None;
    let unkept = |free_radius: &Option<Vec<bool>>, values: &[f64]| {
        let value = |p: usize| values[sys.group_of[p]];
        free_radius.as_ref().is_some_and(|free| {
            shapes.iter().zip(free).any(|(shape, free)| {
                *free
                    && shape.offset(value).is_some_and(|(_, off)| {
                        off.abs() > met_within(shape, value, bar.get().length)
                    })
            })
        })
    };
    let finish = |exit: Stop, iteration: usize, converged: bool, wandered: usize, unkept: bool| {
        #[cfg(feature = "exit-counts")]
        exits::record(
            match exit {
                Stop::Settled => exits::Exit::Settled,
                Stop::Conflict => exits::Exit::Conflict,
                Stop::Cap => exits::Exit::Cap,
                Stop::NoStep => exits::Exit::NoStep,
            },
            dragging,
            iteration,
        );
        #[cfg(not(feature = "exit-counts"))]
        let _ = exit;
        ComponentOutcome {
            converged,
            iterations: iteration,
            wandered,
            unkept: converged && unkept,
            bar: bar.get().length.max(bar.get().floor),
        }
    };
    // (Test-only counts of how the second stage ends: see `exits::STAGE2`.)
    #[cfg(feature = "exit-counts")]
    let stage2 = |stop: usize, stationary: f64| {
        let b = bar.get().length;
        exits::record_stage2(stop, dragging, stationary > b * b);
    };
    #[cfg(not(feature = "exit-counts"))]
    let stage2 = |_: usize, _: f64| {};
    let mut last_stationary = f64::NAN;
    let mut in_stage2 = false;
    for iteration in 0..MAX_ITERATIONS {
        if !res.iter().all(|f| f.is_finite()) || !jac.iter().flatten().all(|(_, g)| g.is_finite()) {
            return finish(Stop::NoStep, iteration, false, wandered, false);
        }
        let mut rows_hold = converged(&res, values);
        // Where the rows hold, they hold to the bar of the part as the solve leaves it too, as
        // an analysis of what it writes reads it: a solve that moves the geometry can make the
        // part smaller than the solve found it, and a row met to the bar it began with would
        // then be off to the drawing's own (`perturbed_satisfiable_sketches_converge`).
        if rows_hold {
            let now = bar_now(values);
            if now < bar.get().length {
                bar.set(Bar {
                    length: now,
                    ..bar.get()
                });
                rows_hold = converged(&res, values);
            }
        }
        // The shapes kept: arcs' radii, lines' lengths and directions, each as how far off
        // it is and the gradient of that over its columns.
        let value = |p: usize| values[sys.group_of[p]];
        let radii_met = shapes.iter().zip(&is_radius).all(|(shape, is_radius)| {
            !is_radius
                || shape
                    .offset(value)
                    .is_none_or(|(_, off)| off.abs() <= met_within(shape, value, bar.get().length))
        });
        // (With each one's weight in the second stage, as its square root (see
        // `shape_weights`), and its rank in the first.)
        let (soft_terms, soft_roots): (Vec<SoftTerm>, Vec<(f64, f64)>) = shapes
            .iter()
            .zip(shape_weights.iter().zip(&ranks))
            .filter_map(|(shape, (w, rank))| {
                let (gradient, off) = shape.offset(value)?;
                let mut grad: Vec<(usize, f64)> = Vec::with_capacity(4);
                for (p, g) in shape.params().iter().zip(gradient) {
                    if let Some(&c) = col_of.get(&sys.group_of[*p]) {
                        match grad.iter_mut().find(|(cc, _)| *cc == c) {
                            Some(entry) => entry.1 += g,
                            None => grad.push((c, g)),
                        }
                    }
                }
                Some(((grad, off), (w.sqrt(), *rank)))
            })
            .unzip();
        // N = JᵀJ and the right-hand side −JᵀF, with every row scaled to a unit gradient: an
        // angle's residual is a sine and its gradient goes as one over the lines' lengths,
        // a distance's residual is a length and its gradient a unit vector, so unscaled the
        // rows would be weighted against each other by the size of the sketch wherever the
        // step isn't exact, and the same drag would come out differently on the same sketch
        // at another size (#94). Scaled, every row counts as the distance it is off by.
        let row_scale = row_scales(&jac, &own, |k| {
            res[k].abs() <= row_tolerance(sys, comp.rows[k], bar.get(), values)
        });
        let (scaled_jac, scaled_res) = scaled_rows(&jac, &res, &row_scale);
        let before = scaled_norm2(&res, &row_scale);

        if rows_hold {
            // The second stage: the preferences, among the rows' solutions.
            in_stage2 = true;
            if soft_terms
                .iter()
                .all(|(_, off)| off.abs() <= bar.get().length)
            {
                stage2(0, 0.0);
                return finish(
                    Stop::Settled,
                    iteration,
                    true,
                    wandered,
                    unkept(&free_radius, values),
                );
            }
            if preferences_entry.is_none() {
                preferences_entry = Some(comp.unknowns.iter().map(|g| values[*g]).collect());
            }
            // The shapes as this stage weighs them (see `shape_weights`).
            let soft_terms: Vec<SoftTerm> = soft_terms
                .iter()
                .zip(&soft_roots)
                .map(|((grad, off), &(root, _))| {
                    (
                        grad.iter().map(|&(c, g)| (c, root * g)).collect(),
                        root * off,
                    )
                })
                .collect();
            // A = JᵀJ + μ·M⁻¹ over the columns that take part (the dragged and carried as
            // the identity, apart from the rest).
            normal.clear();
            for row in &scaled_jac {
                for (k, &(c1, g1)) in row.iter().enumerate() {
                    if pinned[c1] {
                        continue;
                    }
                    for &(c2, g2) in &row[..=k] {
                        if !pinned[c2] {
                            normal.add_product(c1, c2, g1, g2);
                        }
                    }
                }
            }
            let mu = MIN_CHANGE * max_diag_of(&normal);
            let shift: Vec<f64> = mobility
                .iter()
                .zip(&pinned)
                .map(|(m, pinned)| if *pinned { 1.0 } else { mu / m })
                .collect();
            let Some(factor) = normal.factor(&shift) else {
                // (Positive definite by construction; a factor that fails is round-off
                // swamping the rows, with nothing a preference's step could rely on.)
                stage2(5, f64::NAN);
                return finish(
                    Stop::Settled,
                    iteration,
                    true,
                    wandered,
                    unkept(&free_radius, values),
                );
            };
            // The weighted projection into the rows' null space of a gradient (`μ·A⁻¹`).
            // (Refined against the matrix's exact values: `A⁻¹` amplifies round-off by 1/μ in
            // the directions the rows leave free, and the projection takes that back by μ, so
            // an unrefined solve would leave round-off of a millionth of the step in the
            // directions neither the rows nor the shapes move, as much at one size or place as
            // its round-off happened to make it.)
            //
            //
            // For the stop, twice: once leaves μ/σ² of a direction the rows move with singular
            // value σ, which where the rows rule a shape out (a hanging line's length an Equal
            // dictates) is all the shape's gradient there is, and the solve chased it, refused
            // trial after trial. The projection is idempotent in the weighted metric, so a
            // second pass changes nothing else and leaves that share squared.
            let once = |r: &[f64]| {
                let rhs: Vec<Dd> = r
                    .iter()
                    .zip(&pinned)
                    .map(|(r, pinned)| Dd::default().add(if *pinned { 0.0 } else { *r }))
                    .collect();
                let mut v = factor.solve_refined(&rhs);
                for (v, pinned) in v.iter_mut().zip(&pinned) {
                    *v = if *pinned { 0.0 } else { *v * mu };
                }
                v
            };
            let project = |r: &[f64]| {
                let v = once(r);
                let weighted: Vec<f64> = v.iter().zip(&mobility).map(|(v, m)| v / m).collect();
                once(&weighted)
            };
            // The shapes' Gauss–Newton curvature GᵀG applied to a step.
            let curvature = |p: &[f64]| {
                let mut out = vec![0.0; n];
                for (grad, _) in &soft_terms {
                    let s: f64 = grad.iter().map(|&(c, g)| g * p[c]).sum();
                    for &(c, g) in grad {
                        out[c] += g * s;
                    }
                }
                out
            };
            if free_radius.is_none() {
                free_radius = Some(
                    shapes
                        .iter()
                        .zip(&is_radius)
                        .map(|(shape, is_radius)| {
                            let Some((gradient, _)) = shape.offset(|p| values[sys.group_of[p]])
                            else {
                                return false;
                            };
                            let mut g = vec![0.0; n];
                            for (p, d) in shape.params().iter().zip(gradient) {
                                if let Some(&c) = col_of.get(&sys.group_of[*p]) {
                                    g[c] += d;
                                }
                            }
                            let whole: f64 = g
                                .iter()
                                .zip(&mobility)
                                .zip(&pinned)
                                .filter(|(_, pinned)| !**pinned)
                                .map(|((g, m), _)| g * g * m)
                                .sum();
                            *is_radius && dot(&g, &once(&g)) > MIN_CHANGE.sqrt() * whole
                        })
                        .collect(),
                );
            }
            let phi = |terms: &[f64]| 0.5 * terms.iter().map(|off| off * off).sum::<f64>();
            let phi_now = phi(&soft_terms.iter().map(|(_, off)| *off).collect::<Vec<_>>());
            // The shapes' gradient Gᵀ·off, and the step by projected conjugate gradients in
            // the trust region (Steihaug): exact in as many steps as there are shapes.
            let mut r = vec![0.0; n];
            for (grad, off) in &soft_terms {
                for &(c, g) in grad {
                    r[c] += g * off;
                }
            }
            let mut v = once(&r);
            let mut rv = dot(&r, &v);
            // (The stop reads the gradient projected twice; the steps, once.)
            let stationary = dot(&r, &project(&r));
            // Stationary: the shapes' projected gradient, in the weighted metric, is below the
            // bar. (Not while a kept radius is off, nor with the fold steps, which finish only
            // where every shape is met: at a fold a shape's gradient vanishes where it can
            // still be met, and only what the step can still gain says so (see below). A
            // drag's kept radii are held, not only preferred: where they can be met, they are,
            // and a drag whose radii can't follow stops (`Editor::breaks_held_shape`).)
            last_stationary = stationary;
            if rv <= 0.0
                || (!fold && radii_met && stationary <= bar.get().length * bar.get().length)
            {
                stage2(1, stationary);
                return finish(
                    Stop::Settled,
                    iteration,
                    true,
                    wandered,
                    unkept(&free_radius, values),
                );
            }
            let mut x = vec![0.0; n];
            let mut p: Vec<f64> = v.iter().map(|v| -v).collect();
            // How far along `d` from `x` the region's edge is.
            let to_edge = |x: &[f64], d: &[f64]| {
                let (a, b, c) = (
                    distance(d, d),
                    2.0 * distance(x, d),
                    distance(x, x) - radius * radius,
                );
                if a > 0.0 {
                    (-b + (b * b - 4.0 * a * c).max(0.0).sqrt()) / (2.0 * a)
                } else {
                    0.0
                }
            };
            for _ in 0..=soft_terms.len() {
                let hp = curvature(&p);
                let kappa = dot(&p, &hp);
                if kappa <= 0.0 {
                    let t = to_edge(&x, &p);
                    x.iter_mut().zip(&p).for_each(|(x, p)| *x += t * p);
                    break;
                }
                let alpha = rv / kappa;
                let next: Vec<f64> = x.iter().zip(&p).map(|(x, p)| x + alpha * p).collect();
                if distance(&next, &next) >= radius * radius {
                    let t = to_edge(&x, &p);
                    x.iter_mut().zip(&p).for_each(|(x, p)| *x += t * p);
                    break;
                }
                x = next;
                r.iter_mut().zip(&hp).for_each(|(r, h)| *r += alpha * h);
                v = once(&r);
                let rv_next = dot(&r, &v);
                if rv_next.max(0.0).sqrt() <= bar.get().length {
                    break;
                }
                let beta = rv_next / rv;
                p.iter_mut().zip(&v).for_each(|(p, v)| *p = -v + beta * *p);
                rv = rv_next;
            }
            // What the step promises: the shapes' linearisation, half their offsets squared.
            let promised = {
                let terms: Vec<f64> = soft_terms
                    .iter()
                    .map(|(grad, off)| off + grad.iter().map(|&(c, g)| g * x[c]).sum::<f64>())
                    .collect();
                phi_now - phi(&terms)
            };
            // Settled where the most the step could gain is below the bar squared, or (but
            // with `fold`, which meets every shape it can) where it would bring the shapes'
            // offsets, as one length, no more than a bar nearer: at a shape the relations hold
            // off, a step can gain a sliver of a large misfit (more than the bar squared) while
            // moving the misfit itself by far less than the bar, and the solve would creep on.
            let nearer = (2.0 * phi_now).sqrt() - (2.0 * (phi_now - promised).max(0.0)).sqrt();
            if promised <= bar.get().length * bar.get().length
                || (!fold && nearer <= bar.get().length)
            {
                stage2(
                    if promised <= bar.get().length * bar.get().length {
                        2
                    } else {
                        3
                    },
                    stationary,
                );
                return finish(
                    Stop::Settled,
                    iteration,
                    true,
                    wandered,
                    unkept(&free_radius, values),
                );
            }
            let stepped = distance(&x, &x).sqrt();
            // The trial, back onto the rows: minimum-change Newton steps (the weighted
            // least-change step as μ goes to zero), each at least halving what the rows are
            // off by, until they hold.
            let mut trial = values.to_vec();
            for (c, g) in comp.unknowns.iter().enumerate() {
                trial[*g] += x[c];
            }
            let landed = reproject(&mut trial, &on_the_rows, &pinned, &mut back_normal);
            // (Brought back by no more than a share of the step: farther, the step went where
            // the rows' tangent says nothing about them.)
            let back: Vec<f64> = comp
                .unknowns
                .iter()
                .enumerate()
                .map(|(c, g)| trial[*g] - values[*g] - x[c])
                .collect();
            let landed = landed.filter(|_| moved(&back) <= CORRECTION_SHARE * moved(&x));
            // The region's outer bound holds for the preferences' steps together, not only each
            // one: each is no wider than the drawing, but many of them could walk a point a
            // shape barely feels as far as the shape keeps gaining (a direction met by sliding an
            // end along a line it nearly parallels), and the next solve, measuring the drawing
            // with that point in it, would let it go farther still (`flung_centre`: kilometres
            // out on a 20 mm sketch, for one ulp of offset).
            let from_entry: Vec<f64> = comp
                .unknowns
                .iter()
                .zip(&entry)
                .map(|(g, e)| trial[*g] - e)
                .collect();
            let from_preferences: Vec<f64> = comp
                .unknowns
                .iter()
                .zip(
                    preferences_entry
                        .get_or_insert_with(|| entry.clone())
                        .iter(),
                )
                .map(|(g, e)| trial[*g] - e)
                .collect();
            let landed = landed.filter(|_| {
                if dragging {
                    fold || moved(&from_entry) <= across
                } else if sys.shapes_as_before {
                    true
                } else {
                    // (In an edit, from where the preferences' stage began: a preference never
                    // carries geometry farther than the drawing is across, as a line kept
                    // square would be, stretched along itself to meet a line it nearly
                    // parallels.)
                    moved(&from_preferences) <= across
                }
            });
            let kept = landed.and_then(|evaluated| {
                let terms: Vec<f64> = shapes
                    .iter()
                    .zip(&shape_weights)
                    .filter_map(|(shape, w)| {
                        Some(w.sqrt() * shape.offset(|p| trial[sys.group_of[p]])?.1)
                    })
                    .collect();
                // (Kept where it delivered at least a quarter of what it promised, the
                // region's own test: a step that gains a sliver of what its linearisation
                // promised went somewhere the linearisation says nothing about, and taken, it
                // would slide geometry along a direction the shapes hardly feel, as far as
                // round-off had it go.)
                let gain = (phi_now - phi(&terms)) / promised;
                (gain >= 0.25).then_some((evaluated, gain))
            });
            match kept {
                Some((((trial_res, trial_jac), trial_own), gain)) => {
                    if gain > 0.75 && stepped >= 0.99 * radius {
                        radius = (2.0 * radius).min(across);
                    }
                    values.copy_from_slice(&trial);
                    leave_storage();
                    res = trial_res;
                    jac = trial_jac;
                    own = trial_own;
                }
                None => radius = stepped / 4.0,
            }
            if radius <= bar.get().length {
                stage2(4, stationary);
                return finish(
                    Stop::Settled,
                    iteration + 1,
                    converged(&res, values),
                    wandered,
                    unkept(&free_radius, values),
                );
            }
            continue;
        }

        // The first stage: the relations.
        normal.clear();
        let mut rhs = vec![Dd::default(); n];
        for (row, f) in scaled_jac.iter().zip(&scaled_res) {
            for (k, &(c1, g1)) in row.iter().enumerate() {
                rhs[c1] = rhs[c1].add_product(-g1, *f);
                for &(c2, g2) in &row[..=k] {
                    normal.add_product(c1, c2, g1, g2);
                }
            }
        }
        let max_diag = max_diag_of(&normal);
        let mu = MIN_CHANGE * max_diag;
        // The shapes kept, as part of the change a step makes: where the rows' solutions
        // could go either way (a concentric round's lines follow it, or the round shrinks),
        // being off a radius, length or direction costs as much as moving its points that
        // far, KEEP_SHAPE times over: its points' own weight, the least of them (a line of the
        // rest of the sketch, where a relation's focus takes up the change, is kept against
        // moving the rest, not against moving the focus: #349's chain, whose bars would
        // otherwise turn rather than move). In the metric only: the second stage meets them.
        for ((grad, _), &(_, rank)) in soft_terms.iter().zip(&soft_roots) {
            let own = grad
                .iter()
                .map(|&(c, _)| 1.0 / mobility[c])
                .fold(f64::INFINITY, f64::min);
            // (A shape a caller ranks lower is kept that much less: see
            // `SolveOptions::length_ranks`.)
            let own = rank * if own.is_finite() { own } else { 1.0 };
            for (i, &(c1, g1)) in grad.iter().enumerate() {
                for &(c2, g2) in &grad[..=i] {
                    normal.add_product(c1, c2, mu * KEEP_SHAPE * own * g1, g2);
                }
            }
        }
        // The minimum change is weighted by mobility: it picks among the rows' solutions. The
        // damping is the trust region, how far the rows' linearisation can be trusted, which is
        // a distance, the same for every point, whatever rank of what stays put it is in (a
        // step that swings the focus far and the rest a hair, where only the rest can meet the
        // rows, is as far off its linearisation as any: #349's chain, a relation's focus among
        // bars held at their lengths). The dragged and carried keep their weight in it too:
        // they are where the user put them, and a damped step doesn't move them either. (Damped
        // as a distance like the rest, a drag that can't land would let them give way, and land
        // nowhere near where they were asked to be: `held_shapes::`' refused drags then go.)
        let shift: Vec<f64> = mobility
            .iter()
            .zip(&pinned)
            .map(|(m, pinned)| mu * fold_scale / m + if *pinned { lambda / m } else { lambda })
            .collect();
        let refuse = |lambda: &mut f64, nu: &mut f64, fold_scale: f64| {
            *lambda = lambda.max(1e-6 * max_diag * fold_scale) * *nu;
            *nu *= 2.0;
        };
        let Some(factor) = normal.factor(&shift) else {
            // (Not numerically positive definite at this damping: a step refused.)
            wandered += 1;
            refuse(&mut lambda, &mut nu, fold_scale);
            continue;
        };
        let delta = factor.solve_refined(&rhs);
        // The trust region's outer bound: a single iteration can't fling geometry onto another
        // branch, no further than the drawing is across. (Not the spread of this component's
        // own unknowns: a lone unknown, such as the free end of a dimension from a fixed
        // point, has none, and would crawl a millimetre at a time.) A step that goes farther
        // is refused like one that does worse, and the damping rises until one fits: the step
        // the region allows is the damped step of its size, which gives up the directions the
        // rows barely move first, not the long step cut short along its own way, which goes
        // mostly along them, and walked a part across the drawing to another branch frame
        // after frame (census case 2432). It is one its linearisation got badly wrong too.
        // (And in a drag, no point farther from where the solve began than that, all its steps
        // together: a step each no wider than the drawing could still walk a part across it to
        // another branch, and the next solve, measuring the drawing with it there, go farther
        // still: `flung_centre`, 21 of 39 one-ulp nudges of its offset thrown 6 to 23 m out
        // on a 20 mm sketch. A drag's solve begins with its targets in place, so its own way
        // to them is short; a commit's may rightly go far, a dimension typed ten times longer.)
        let from_entry: Vec<f64> = comp
            .unknowns
            .iter()
            .zip(&entry)
            .enumerate()
            .map(|(c, (g, e))| values[*g] + delta[c] - e)
            .collect();
        if moved(&delta) > across || (dragging && !fold && moved(&from_entry) > across) {
            wandered += 1;
            refuse(&mut lambda, &mut nu, fold_scale);
            continue;
        }
        let mut trial = values.to_vec();
        for (c, g) in comp.unknowns.iter().enumerate() {
            trial[*g] += delta[c];
        }
        let ((mut trial_res, mut trial_jac), mut trial_own) =
            evaluate_with_own(sys, &comp.rows, &trial, &col_of);
        // (Judged in the step's own scaling of the rows.)
        let mut after = scaled_norm2(&trial_res, &row_scale);
        // (What the step did by itself: whether it wandered is its linearisation's own say,
        // corrected or not.)
        let plain = after;
        // Then onto where its linearisation promised to take the rows: the linearisation is a
        // tangent, so a step that turns geometry (a chain of bars held at their lengths, swung
        // about a joint) leaves every length it turns off at second order, and judged so, the
        // damping would answer with ever smaller steps (#349). Newton's steps from there to
        // the values the step promised take that second order out, so that the step follows
        // the rows round and is judged by what it does to the rows that are off (a second-order
        // correction). Where they don't close in, or the correction is more than a share of the
        // step ([`CORRECTION_SHARE`]), the step as it was.
        {
            let promise: Vec<f64> = res
                .iter()
                .zip(&jac)
                .map(|(f, row)| f + row.iter().map(|&(c, g)| g * delta[c]).sum::<f64>())
                .collect();
            let mut corrected = trial.clone();
            let got = reproject(&mut corrected, &promise, &no_column, &mut back_normal);
            let correction: Vec<f64> = comp
                .unknowns
                .iter()
                .map(|g| corrected[*g] - trial[*g])
                .collect();
            if let Some(((res2, jac2), own2)) = got
                && moved(&correction) <= CORRECTION_SHARE * moved(&delta)
            {
                let after2 = scaled_norm2(&res2, &row_scale);
                if after2 < after
                    || (converged(&res2, &corrected) && !converged(&trial_res, &trial))
                {
                    trial = corrected;
                    (trial_res, trial_jac, trial_own, after) = (res2, jac2, own2, after2);
                }
            }
        }
        let model = scaled_jac
            .iter()
            .zip(&scaled_res)
            .map(|(row, f)| (f + row.iter().map(|&(c, g)| g * delta[c]).sum::<f64>()).powi(2))
            .sum::<f64>();
        let promised = before - model;
        let lands = converged(&trial_res, &trial);
        if after < before || lands {
            // How much of the drop the linearisation promised the step delivered.
            let gain = if promised > 0.0 {
                (before - after) / promised
            } else {
                1.0
            };
            // (Judged by the step alone: a correction that rescues a step its linearisation
            // got badly wrong still leaves a solve that went round a fold between solutions,
            // where a drag should go in smaller steps.)
            let plain_gain = if promised > 0.0 {
                (before - plain) / promised
            } else {
                1.0
            };
            if plain_gain < WANDERING_GAIN {
                wandered += 1;
            }
            if fold
                && dragging
                && soft_terms
                    .iter()
                    .all(|(_, off)| off.abs() <= bar.get().length)
            {
                fold_scale = fold_scale.min((after.sqrt() / across).min(1.0));
            }
            values.copy_from_slice(&trial);
            leave_storage();
            res = trial_res;
            jac = trial_jac;
            own = trial_own;
            lambda *= (1.0 - (2.0 * gain - 1.0).powi(3)).max(1.0 / 3.0);
            nu = 2.0;
            if lambda < 1e-12 * max_diag * fold_scale {
                lambda = 0.0;
            }
            if lands {
                continue;
            }
        }
        let refused = !(after < before || lands);
        if refused {
            wandered += 1;
            refuse(&mut lambda, &mut nu, fold_scale);
        }
        let finest = res
            .iter()
            .zip(&comp.rows)
            .zip(&row_scale)
            .filter(|((f, r), s)| **s > 0.0 && f.abs() > row_tolerance(sys, **r, bar.get(), values))
            .map(|((_, r), s)| row_tolerance(sys, *r, bar.get(), values) * s)
            .fold(f64::INFINITY, f64::min);
        // A least-squares stationary point with the rows off: the step brings them no more
        // than the finest bar of the rows still off closer, and by its linearisation they
        // still wouldn't hold. Every further step is smaller still (λ only grows on a refused
        // step), so the rows are as near as they get: a conflict, or the round-off floor.
        if before.sqrt() - model.max(0.0).sqrt() <= finest && model.max(0.0).sqrt() > finest {
            let rows_hold = converged(&res, values);
            return finish(
                if rows_hold {
                    Stop::Settled
                } else {
                    Stop::Conflict
                },
                iteration + 1,
                rows_hold,
                wandered,
                false,
            );
        }
    }
    if in_stage2 && converged(&res, values) {
        stage2(6, last_stationary);
    }
    finish(
        Stop::Cap,
        MAX_ITERATIONS,
        converged(&res, values),
        wandered,
        unkept(&free_radius, values),
    )
}

/// The dot product of two vectors.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// How far the drawing is across, for the step limit in [`newton`]: twice its farthest point
/// from its middle, which is the same however the drawing is turned or wherever it is. (The
/// spread of the coordinates, x and y together, grows as the drawing moves away from the
/// origin, so the same step would be cut short at one place and not another, and the same
/// drag land elsewhere: #103.) Infinite for a drawing of one point.
///
/// With `circles` (a drag's solve), each circle counts as far as it reaches: its centre's
/// distance from the middle plus its radius. A circle's radius is a parameter the step moves
/// like a point, so a drag that changes it by more than the points' own spread (a point on a
/// circle dragged towards its centre) would otherwise have every step cut short, count as a
/// solve that wandered, and be taken again in small steps along the pointer's line, which
/// crosses the centre, where a crushed circle refuses it: the drag stopped for a stretch
/// past the centre, decided by the cut, not by the geometry. Other solves count points only,
/// as they always have.
fn drawing_across(sys: &System, values: &[f64], circles: bool) -> f64 {
    let mut points: BTreeMap<EntityId, [Option<f64>; 2]> = BTreeMap::new();
    for (i, p) in sys.params.iter().enumerate() {
        match *p {
            Param::X(id) => points.entry(id).or_default()[0] = Some(values[sys.group_of[i]]),
            Param::Y(id) => points.entry(id).or_default()[1] = Some(values[sys.group_of[i]]),
            _ => {}
        }
    }
    let points: Vec<DVec2> = points
        .into_values()
        .filter_map(|[x, y]| Some(DVec2::new(x?, y?)))
        .collect();
    let middle = points.iter().sum::<DVec2>() / points.len().max(1) as f64;
    let mut size = points
        .iter()
        .map(|p| p.distance(middle))
        .fold(0.0, f64::max);
    if circles {
        let value = |p: &Param| sys.param_index.get(p).map(|&i| values[sys.group_of[i]]);
        for (id, shape) in &sys.entities {
            if let system::EntityShape::Curve(defining, Some(_)) = shape
                && let Some(&center) = defining.first()
                && let (Some(r), Some(x), Some(y)) = (
                    value(&Param::Radius(*id)),
                    value(&Param::X(center)),
                    value(&Param::Y(center)),
                )
            {
                size = size.max(DVec2::new(x, y).distance(middle) + r.abs());
            }
        }
    }
    if size > 0.0 {
        2.0 * size
    } else {
        f64::INFINITY
    }
}

/// Whether none of `rows` belongs to a constraint (see [`SolveReport::unsourced_failure`]).
fn unsourced(sys: &System, rows: &[usize]) -> bool {
    rows.iter().all(|&r| sys.rows[r].source.is_none())
}

/// The entities whose own geometry is in the failed `rows` ([`SolveReport::exposed`]), and
/// of them those with a row that doesn't hold (`holds`, the test convergence uses: a
/// residual that isn't a number doesn't) ([`SolveReport::broken`]), added to `exposed` and
/// `broken`.
pub(crate) fn exposed_and_broken(
    sys: &System,
    rows: &[usize],
    holds: impl Fn(usize) -> bool,
    exposed: &mut Vec<EntityId>,
    broken: &mut Vec<EntityId>,
) {
    for &r in rows {
        let Some(owner) = sys.rows[r].owner else {
            continue;
        };
        exposed.push(owner);
        if !holds(r) {
            broken.push(owner);
        }
    }
}

/// What failed in a pass of the solve ([`run`]): its rows, sorted, and how many of its
/// components failed (the rest of the rows are over pinned geometry alone).
struct Failed {
    rows: Vec<usize>,
    components: usize,
}

/// One pass of the solve over `sys`: each component in its own [`Frame`], against its bar.
/// The length bar each free group was solved to is left in [`System::bars`].
fn run(sys: &mut System, options: &SolveOptions, iterate: bool) -> (SolveReport, Failed) {
    let tol = &options.tolerances;
    let mut report = SolveReport {
        conflicts: sys.conflicts.clone(),
        redundant: sys.redundant.clone(),
        ..Default::default()
    };
    let (comps, constant_rows) = components(sys, tol);
    let mut values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    let drawn = Drawn::of(sys);
    let mut bars = vec![0.0; sys.groups.len()];
    let mut failed_rows: Vec<usize> = Vec::new();
    let mut failed_components = 0;
    let (mut dragged_bar, mut any_bar) = (0.0f64, 0.0f64);
    // Drag components whose rows hold but that left a kept radius off that their free
    // geometry could change (see `newton`).
    let mut unkept = 0;
    let across = drawing_across(sys, &values, !options.dragged.is_empty());
    let mut grad = Vec::new();

    for &r in &constant_rows {
        let (_, groups, asked) = reach(sys, &[], &[r]);
        let frame = Frame::enter(sys, &groups, asked, &mut values, tol, &drawn);
        grad.clear();
        let res = sys.rows[r]
            .equation
            .eval(&|p| values[sys.group_of[p]], &mut grad);
        if res.abs() <= row_tolerance(sys, r, frame.bar, &values) {
            // Satisfied, but everything it involves is pinned: it adds nothing.
            report.dependent_equations += 1;
        } else {
            failed_rows.push(r);
            report.unsourced_failure |= sys.rows[r].source.is_none();
            exposed_and_broken(
                sys,
                &[r],
                |_| false,
                &mut report.exposed,
                &mut report.broken,
            );
        }
        frame.leave(&mut values);
    }
    for comp in &comps {
        let (shapes, groups, asked) = reach(sys, &comp.unknowns, &comp.rows);
        let mut frame = Frame::enter(sys, &groups, asked, &mut values, tol, &drawn);
        if let Some(cap) = options.bar_cap {
            frame.bar.length = frame.bar.length.min(cap).max(frame.bar.floor);
            frame.bar.share = frame.bar.share.min(cap);
        }
        let bar = frame.bar;
        for &g in &comp.unknowns {
            bars[g] = bar.length;
        }
        if comp
            .unknowns
            .iter()
            .any(|&g| sys.groups[g].mobility <= system::CARRIED_MOBILITY)
        {
            dragged_bar = dragged_bar.max(bar.length);
        }
        any_bar = any_bar.max(bar.length);
        let converged = if iterate {
            // (The component's length bar at other values in its frame: its size there,
            // measured on the values as they would be stored, as `Frame::enter` measured it, so
            // the two agree to the bit where nothing moved; its floor and cap as it began.)
            let shifts: HashMap<usize, f64> = frame
                .moved
                .iter()
                .map(|&(g, _, shift)| (g, shift))
                .collect();
            let bar_now = |values: &[f64]| {
                let stored = |g: usize| values[g] + shifts.get(&g).copied().unwrap_or(0.0);
                let (_, size) = drawn_extent(sys, &groups, stored, &drawn);
                let mut length = Bar::new(size.max(asked), 0.0, tol).length.max(bar.floor);
                if let Some(cap) = options.bar_cap {
                    length = length.min(cap).max(bar.floor);
                }
                length
            };
            let outcome = newton(
                sys,
                comp,
                &shapes,
                &mut values,
                bar,
                &bar_now,
                !options.dragged.is_empty(),
                across,
                options.fold,
            );
            for &g in &comp.unknowns {
                bars[g] = outcome.bar;
            }
            report.iterations = report.iterations.max(outcome.iterations);
            report.wandered = report.wandered.max(outcome.wandered);
            // (A drag's: its kept radii are held where the free geometry can keep them.)
            if outcome.unkept && !options.dragged.is_empty() {
                unkept += 1;
            }
            outcome.converged
        } else {
            let (res, _) = evaluate(sys, &comp.rows, &values, &HashMap::new());
            res.iter()
                .zip(&comp.rows)
                .all(|(f, r)| f.abs() <= row_tolerance(sys, *r, bar, &values))
        };
        if !converged {
            failed_components += 1;
            failed_rows.extend(&comp.rows);
            report.unsourced_failure |= unsourced(sys, &comp.rows);
            let at = &values;
            exposed_and_broken(
                sys,
                &comp.rows,
                |r| {
                    let res = sys.rows[r]
                        .equation
                        .eval(&|p| at[sys.group_of[p]], &mut Vec::new());
                    res.abs() <= row_tolerance(sys, r, bar, at)
                },
                &mut report.exposed,
                &mut report.broken,
            );
        }
        frame.leave(&mut values);
    }
    for (g, v) in values.iter().enumerate() {
        sys.groups[g].value = *v;
    }
    sys.bars = bars;
    report.bar = if dragged_bar > 0.0 {
        dragged_bar
    } else {
        any_bar
    };
    for &r in &failed_rows {
        if let Some(cid) = sys.rows[r].source
            && !report.conflicts.contains(&cid)
        {
            report.conflicts.push(cid);
        }
    }
    report.conflicts.sort();
    for list in [&mut report.exposed, &mut report.broken] {
        list.sort_unstable();
        list.dedup();
    }
    report.converged = failed_rows.is_empty() && sys.conflicts.is_empty() && unkept == 0;
    report.unsourced_failure |= unkept > 0;

    if options.analyze {
        analyze_dof(sys, &comps, &values, &failed_rows, &mut report);
    }
    failed_rows.sort_unstable();
    (
        report,
        Failed {
            rows: failed_rows,
            components: failed_components + unkept,
        },
    )
}

/// Reduced row echelon form of the equilibrated Jacobian, with its pivot columns. Used as the
/// reference for the sparse QR analysis in tests.
#[cfg(test)]
fn dense_rref(jac: &[Vec<(usize, f64)>], n: usize) -> (Vec<usize>, Matrix) {
    let eq = equilibrate(jac, n);
    let mut j = Matrix::zeros(eq.len(), n);
    for (i, row) in eq.iter().enumerate() {
        for &(c, g) in row {
            *j.at_mut(i, c) += g;
        }
    }
    let pivots = rref(&mut j, 1e-9);
    (pivots, j)
}

/// Rank of the sparse Jacobian (rows over `n` columns) and, per column, whether that unknown
/// can still move (has a component in the null space). Uses Givens QR, which costs about
/// O(n · b²) for envelope width b.
fn sparse_rank(jac: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    qr_rank(&equilibrate(jac, n), n)
}

/// Prepare a Jacobian for rank decisions: drop negligible entries, scale rows to unit length,
/// then columns to unit length. Scaling changes neither the rank nor which unknowns are free,
/// but it makes one tolerance meaningful for sketches mixing tiny and huge features.
///
/// An entry below [`NEGLIGIBLE`] of its row's largest is dropped: round-off (e.g. gradients of
/// two members of one substitution group that cancel), or geometry that is all but degenerate
/// (two points a few nanometres off level, an angle of 1e-8 rad). Kept, column scaling would
/// blow it up to unit size and rank decisions would turn on rounding.
fn equilibrate(jac: &[Vec<(usize, f64)>], n: usize) -> Vec<Vec<(usize, f64)>> {
    const NEGLIGIBLE: f64 = 1e-7;
    let mut rows: Vec<Vec<(usize, f64)>> = jac
        .iter()
        .map(|row| {
            let max = row.iter().fold(0.0f64, |m, (_, g)| m.max(g.abs()));
            let kept: Vec<(usize, f64)> = row
                .iter()
                .copied()
                .filter(|(_, g)| g.abs() > NEGLIGIBLE * max)
                .collect();
            let norm = kept.iter().map(|(_, g)| g * g).sum::<f64>().sqrt();
            kept.into_iter().map(|(c, g)| (c, g / norm)).collect()
        })
        .collect();
    let mut col_norm = vec![0.0f64; n];
    for row in &rows {
        for &(c, g) in row {
            col_norm[c] += g * g;
        }
    }
    for row in &mut rows {
        for (c, g) in row.iter_mut() {
            *g /= col_norm[*c].sqrt();
        }
    }
    rows
}

/// Rank analysis of each component's Jacobian at the solution.
fn analyze_dof(
    sys: &System,
    comps: &[Component],
    values: &[f64],
    failed_rows: &[usize],
    report: &mut SolveReport,
) {
    // Groups that are pinned or determined by equations.
    let mut determined: Vec<bool> = sys.groups.iter().map(|g| g.fixed).collect();
    let mut dof = 0;
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    dof += (0..sys.groups.len())
        .filter(|g| !sys.groups[*g].fixed && !in_component.contains(g))
        .count();

    for comp in comps {
        let n = comp.unknowns.len();
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = evaluate(sys, &comp.rows, values, &col_of);
        let (rank, movable) = sparse_rank(&jac, n);
        dof += n - rank;
        report.dependent_equations += comp.rows.len().saturating_sub(rank);
        for (c, g) in comp.unknowns.iter().enumerate() {
            if !movable[c] {
                determined[*g] = true;
            }
        }
    }
    report.dof = Some(dof);

    let param_determined = |p: Param| {
        sys.param_index
            .get(&p)
            .is_some_and(|&i| determined[sys.group_of[i]])
    };
    let point_full =
        |id: EntityId| param_determined(Param::X(id)) && param_determined(Param::Y(id));
    // Entities touched by failing equations are over-defined.
    let mut failing_params: HashSet<usize> = HashSet::new();
    for &r in failed_rows {
        for p in sys.rows[r].equation.params() {
            failing_params.insert(p);
        }
    }
    let failing = |p: Param| {
        sys.param_index
            .get(&p)
            .is_some_and(|i| failing_params.contains(i))
    };
    for (id, entity) in &sys.entities {
        let id = *id;
        let (full, over) = match entity {
            EntityShape::Point => (
                point_full(id),
                failing(Param::X(id)) || failing(Param::Y(id)),
            ),
            EntityShape::Curve(points, radius) => {
                let full = points.iter().all(|p| point_full(*p))
                    && radius.is_none_or(|r| param_determined(Param::Radius(r)));
                let over = points
                    .iter()
                    .any(|p| failing(Param::X(*p)) || failing(Param::Y(*p)))
                    || radius.is_some_and(|r| failing(Param::Radius(r)));
                (full, over)
            }
        };
        report.definitions.insert(
            id,
            if over {
                Definition::Over
            } else if full {
                Definition::Full
            } else {
                Definition::Under
            },
        );
    }
}

/// How a point can still move with every constraint kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Freedom {
    /// Free along one direction (unit vector; a rotation shows as the tangent direction).
    Line(DVec2),
    /// Free in every direction.
    Plane,
}

/// Components larger than this are skipped by [`free_directions`] (dense analysis).
const FREEDOM_LIMIT: usize = 400;

/// The directions in which each under-defined point can move, from the null space of the
/// constraint Jacobian. Fully defined points are omitted, as are points in very large
/// components (see [`FREEDOM_LIMIT`]).
#[cfg(test)]
pub fn free_directions(doc: &Document) -> HashMap<EntityId, Freedom> {
    free_directions_with(doc, None)
}

/// `free_directions`, reading relations through `lowering`.
pub fn free_directions_with(
    doc: &Document,
    lowering: Option<&Lowering>,
) -> HashMap<EntityId, Freedom> {
    let options = SolveOptions::default();
    let sys = System::build(doc, &[], lowering);
    let (comps, _) = components(&sys, &options.tolerances);
    let values: Vec<f64> = sys.groups.iter().map(|g| g.value).collect();
    // Null-space basis vectors, as group → component, indexed by the groups they touch.
    let mut basis: Vec<BTreeMap<usize, f64>> = Vec::new();
    let mut skipped: HashSet<usize> = HashSet::new();
    let in_component: HashSet<usize> = comps
        .iter()
        .flat_map(|c| c.unknowns.iter().copied())
        .collect();
    for g in 0..sys.groups.len() {
        if !sys.groups[g].fixed && !in_component.contains(&g) {
            basis.push(BTreeMap::from([(g, 1.0)]));
        }
    }
    for comp in &comps {
        let n = comp.unknowns.len();
        if n > FREEDOM_LIMIT {
            skipped.extend(&comp.unknowns);
            continue;
        }
        let col_of: HashMap<usize, usize> = comp
            .unknowns
            .iter()
            .enumerate()
            .map(|(c, g)| (*g, c))
            .collect();
        let (_, jac) = evaluate(&sys, &comp.rows, &values, &col_of);
        // Equilibrate like the rank analysis, keeping the column scales to undo them.
        let rows: Vec<Vec<(usize, f64)>> = jac
            .iter()
            .map(|row| {
                let max = row.iter().fold(0.0f64, |m, (_, g)| m.max(g.abs()));
                let kept: Vec<_> = row
                    .iter()
                    .copied()
                    .filter(|(_, g)| g.abs() > 1e-12 * max)
                    .collect();
                let norm = kept.iter().map(|(_, g)| g * g).sum::<f64>().sqrt();
                kept.into_iter().map(|(c, g)| (c, g / norm)).collect()
            })
            .collect();
        let mut col_norm = vec![0.0f64; n];
        for row in &rows {
            for &(c, g) in row {
                col_norm[c] += g * g;
            }
        }
        let col_norm: Vec<f64> = col_norm
            .iter()
            .map(|s| if *s > 0.0 { s.sqrt() } else { 1.0 })
            .collect();
        let mut m = Matrix::zeros(rows.len(), n);
        for (i, row) in rows.iter().enumerate() {
            for &(c, g) in row {
                *m.at_mut(i, c) += g / col_norm[c];
            }
        }
        let pivots = rref(&mut m, 1e-9);
        let pivot_row: BTreeMap<usize, usize> =
            pivots.iter().enumerate().map(|(r, c)| (*c, r)).collect();
        for free in (0..n).filter(|c| !pivot_row.contains_key(c)) {
            // x_free = 1, x_pivot = -R[row, free]; undo the column scaling.
            let mut v = BTreeMap::new();
            v.insert(comp.unknowns[free], 1.0 / col_norm[free]);
            for (&pc, &r) in &pivot_row {
                let x = -m.at(r, free);
                if x != 0.0 {
                    v.insert(comp.unknowns[pc], x / col_norm[pc]);
                }
            }
            basis.push(v);
        }
    }
    let mut by_group: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, v) in basis.iter().enumerate() {
        for g in v.keys() {
            by_group.entry(*g).or_default().push(i);
        }
    }
    let group = |p: Param| sys.param_index.get(&p).map(|&i| sys.group_of[i]);
    let mut out = HashMap::new();
    for (id, entity) in doc.iter() {
        if !entity.kind.is_point() {
            continue;
        }
        let (Some(gx), Some(gy)) = (group(Param::X(id)), group(Param::Y(id))) else {
            continue;
        };
        if skipped.contains(&gx) || skipped.contains(&gy) {
            continue;
        }
        let mut vectors: Vec<usize> = by_group
            .get(&gx)
            .into_iter()
            .chain(by_group.get(&gy))
            .flatten()
            .copied()
            .collect();
        vectors.sort_unstable();
        vectors.dedup();
        // A = M Mᵀ for the 2 × k matrix of motions; its eigenvalues give the rank.
        let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
        for i in vectors {
            let mx = basis[i].get(&gx).copied().unwrap_or(0.0);
            let my = basis[i].get(&gy).copied().unwrap_or(0.0);
            let norm = basis[i].values().map(|x| x * x).sum::<f64>();
            let (mx, my) = (mx / norm.sqrt(), my / norm.sqrt());
            a += mx * mx;
            b += mx * my;
            c += my * my;
        }
        let trace = a + c;
        if trace < 1e-12 {
            continue;
        }
        let disc = (((a - c) / 2.0).powi(2) + b * b).sqrt();
        let (l1, l2) = (trace / 2.0 + disc, trace / 2.0 - disc);
        let freedom = if l2 > 1e-6 * l1 {
            Freedom::Plane
        } else {
            // Eigenvector of the larger eigenvalue.
            let dir = if b.abs() > 1e-15 {
                DVec2::new(l1 - c, b)
            } else if a >= c {
                DVec2::X
            } else {
                DVec2::Y
            };
            Freedom::Line(dir.normalize())
        };
        out.insert(id, freedom);
    }
    out
}

/// Write the solved values into the document, unless any of them isn't finite: then write
/// nothing and return false.
fn write_back(doc: &mut Document, sys: &System) -> bool {
    if (0..sys.params.len()).any(|i| !sys.value(i).is_finite()) {
        return false;
    }
    let mut points: HashMap<EntityId, DVec2> = HashMap::new();
    for (i, param) in sys.params.iter().enumerate() {
        let v = sys.value(i);
        match *param {
            Param::X(id) => points.entry(id).or_insert(DVec2::ZERO).x = v,
            Param::Y(id) => points.entry(id).or_insert(DVec2::ZERO).y = v,
            Param::MidX(_) | Param::MidY(_) => {}
            Param::Radius(id) => {
                if let Some(EntityKind::Circle { radius, .. }) = doc.get(id).map(|e| e.kind)
                    && radius != v
                {
                    let _ = doc.set_radius(id, v);
                }
            }
        }
    }
    let mut ids: Vec<_> = points.into_iter().collect();
    ids.sort_by_key(|(id, _)| *id);
    for (id, at) in ids {
        if doc.point(id).is_ok_and(|p| p != at) {
            let _ = doc.set_point(id, at);
        }
    }
    true
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn dense_rref_for_test(jac: &[Vec<(usize, f64)>], n: usize) -> (Vec<usize>, Matrix) {
    dense_rref(jac, n)
}

#[cfg(test)]
pub(crate) fn sparse_rank_for_test(jac: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    sparse_rank(jac, n)
}
