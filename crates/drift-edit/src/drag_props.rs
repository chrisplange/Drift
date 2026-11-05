//! Property tests for dragging and moving: random sketches of lines and arcs, joined by shared
//! points and held by relations that already hold (so every sketch can be met), are dragged
//! through [`Editor::drag`] in steps the way the select tool's grab does, and the result is
//! checked against what a drag should do. Solver drag bugs used to turn up one user report at
//! a time; these find the class.
//!
//! The case counts are small so `cargo test` stays quick; each property also has a heavier
//! run behind `#[ignore]` (`cargo test -p drift-edit drag_props -- --ignored`).
//!
//! The heavy runs save no seeds (a saved failure would fail every later run first), but
//! each is replayed exactly by its `PROPTEST_RNG_SEED`. To compare two builds, run every
//! `*_heavy` property with seeds 1 to 20 on each (release, `--exact`, one at a time) and
//! compare outcome by outcome. Shrunk cases found that way are pinned in `seeds`, one or two
//! per cause, ignored until fixed.

use std::collections::{BTreeSet, HashSet};

use drift_core::{Constraint, DVec2, EntityId};
use drift_solver::{Orientation, Param};
use proptest::prelude::*;

use crate::editor::Editor;

/// Cases per property in the normal test run.
const LIGHT: u32 = 24;
/// Cases per property in the ignored heavy run.
const HEAVY: u32 = 1000;

const LENGTHS: [f64; 4] = [10.0, 15.0, 20.0, 30.0];
const RADII: [f64; 3] = [8.0, 12.0, 20.0];
const SWEEPS: [f64; 5] = [45.0, 90.0, 120.0, 180.0, 270.0];
/// Where two numbers count as equal when sampling relations that hold.
const HOLDS: f64 = 1e-9;
/// How far a relation may be off after a drag (relative, as `drift_solver::unmet` takes it).
const SLACK: f64 = 1e-8;
/// How far a point may be from where it should land, relative to the sizes involved.
const LANDS: f64 = 1e-6;

/// A unit vector at a multiple of 15° (exact at the quarter turns, so horizontals are).
fn dir(step: u8) -> DVec2 {
    match step % 24 {
        0 => DVec2::X,
        6 => DVec2::Y,
        12 => -DVec2::X,
        18 => -DVec2::Y,
        s => {
            let a = (f64::from(s) * 15.0).to_radians();
            DVec2::new(a.cos(), a.sin())
        }
    }
}

/// One step of building a sketch. Indices wrap around what exists so far.
#[derive(Clone, Debug)]
enum Op {
    /// A line from a point to a new one.
    Step { from: usize, angle: u8, len: u8 },
    /// A line between two existing points.
    Close { from: usize, to: usize },
    /// An arc starting (or ending) at a point, with a new centre and other end.
    Arc {
        from: usize,
        angle: u8,
        radius: u8,
        sweep: u8,
        ccw: bool,
    },
    /// A line from a new point at a line's midpoint.
    Mid { line: usize, angle: u8, len: u8 },
    /// A line from a new point on a line (or its extension).
    OnLine {
        line: usize,
        at: u8,
        angle: u8,
        len: u8,
    },
    /// A line from a new point on an arc's circle.
    OnArc {
        arc: usize,
        at: u8,
        angle: u8,
        len: u8,
    },
    /// A line from a new point coincident with an existing one.
    Dup { from: usize, angle: u8, len: u8 },
    /// A line tangent to an arc at one of its ends.
    Tangent {
        arc: usize,
        end: bool,
        forward: bool,
        len: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let i = || 0usize..64;
    let angle = || 0u8..24;
    let len = || 0u8..LENGTHS.len() as u8;
    prop_oneof![
        4 => (i(), angle(), len()).prop_map(|(from, angle, len)| Op::Step { from, angle, len }),
        2 => (i(), i()).prop_map(|(from, to)| Op::Close { from, to }),
        3 => (i(), angle(), 0u8..RADII.len() as u8, 0u8..SWEEPS.len() as u8, any::<bool>())
            .prop_map(|(from, angle, radius, sweep, ccw)| Op::Arc {
                from,
                angle,
                radius,
                sweep,
                ccw
            }),
        1 => (i(), angle(), len()).prop_map(|(line, angle, len)| Op::Mid { line, angle, len }),
        1 => (i(), 0u8..4, angle(), len()).prop_map(|(line, at, angle, len)| Op::OnLine {
            line,
            at,
            angle,
            len
        }),
        1 => (i(), angle(), angle(), len()).prop_map(|(arc, at, angle, len)| Op::OnArc {
            arc,
            at,
            angle,
            len
        }),
        1 => (i(), angle(), len()).prop_map(|(from, angle, len)| Op::Dup { from, angle, len }),
        2 => (i(), any::<bool>(), any::<bool>(), len()).prop_map(|(arc, end, forward, len)| {
            Op::Tangent {
                arc,
                end,
                forward,
                len,
            }
        }),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Curve {
    Line(usize, usize),
    /// Centre, start, end (counter-clockwise).
    Arc(usize, usize, usize),
}

/// A relation between points (by index) and curves (by index).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rel {
    Coincident(usize, usize),
    Horizontal(usize),
    Vertical(usize),
    Parallel(usize, usize),
    Perpendicular(usize, usize),
    Equal(usize, usize),
    Tangent(usize, usize),
    OnCurve(usize, usize),
    Midpoint(usize, usize),
    /// A point fixed where it is.
    Fix(usize),
    /// A line's length dimensioned at what it is.
    Length(usize),
}

/// A sketch in its own frame: where the points are, the curves through them, and the
/// relations that hold.
#[derive(Clone, Debug, Default)]
struct Sketch {
    points: Vec<DVec2>,
    curves: Vec<Curve>,
    rels: Vec<Rel>,
}

impl Sketch {
    fn point(&mut self, p: DVec2) -> usize {
        self.points.push(p);
        self.points.len() - 1
    }

    fn lines(&self) -> Vec<usize> {
        (0..self.curves.len())
            .filter(|&i| matches!(self.curves[i], Curve::Line(..)))
            .collect()
    }

    fn arcs(&self) -> Vec<usize> {
        (0..self.curves.len())
            .filter(|&i| matches!(self.curves[i], Curve::Arc(..)))
            .collect()
    }

    fn ends(&self, line: usize) -> (DVec2, DVec2) {
        match self.curves[line] {
            Curve::Line(a, b) => (self.points[a], self.points[b]),
            Curve::Arc(..) => unreachable!(),
        }
    }

    fn has_line(&self, a: usize, b: usize) -> bool {
        self.curves
            .iter()
            .any(|c| *c == Curve::Line(a, b) || *c == Curve::Line(b, a))
    }

    fn step(&mut self, from: usize, angle: u8, len: u8) {
        let p = self.points[from] + dir(angle) * LENGTHS[usize::from(len)];
        let to = self.point(p);
        self.curves.push(Curve::Line(from, to));
    }

    fn apply(&mut self, op: &Op) {
        let n = self.points.len();
        let lines = self.lines();
        let arcs = self.arcs();
        match *op {
            Op::Step { from, angle, len } => self.step(from % n, angle, len),
            Op::Close { from, to } => {
                let (a, b) = (from % n, to % n);
                if a != b && self.points[a].distance(self.points[b]) >= 1.0 && !self.has_line(a, b)
                {
                    self.curves.push(Curve::Line(a, b));
                }
            }
            Op::Arc {
                from,
                angle,
                radius,
                sweep,
                ccw,
            } => {
                let from = from % n;
                let r = RADII[usize::from(radius)];
                let c = self.points[from] + dir(angle) * r;
                let start = (self.points[from] - c).to_angle();
                let sweep = SWEEPS[usize::from(sweep)].to_radians();
                let end = start + if ccw { sweep } else { -sweep };
                let center = self.point(c);
                let other = self.point(c + DVec2::from_angle(end) * r);
                self.curves.push(if ccw {
                    Curve::Arc(center, from, other)
                } else {
                    Curve::Arc(center, other, from)
                });
            }
            Op::Mid { line, angle, len } if !lines.is_empty() => {
                let line = lines[line % lines.len()];
                let (a, b) = self.ends(line);
                let m = self.point((a + b) / 2.0);
                self.rels.push(Rel::Midpoint(m, line));
                self.step(m, angle, len);
            }
            Op::OnLine {
                line,
                at,
                angle,
                len,
            } if !lines.is_empty() => {
                let line = lines[line % lines.len()];
                let (a, b) = self.ends(line);
                let t = [0.25, 0.75, 1.5, -0.5][usize::from(at % 4)];
                let p = self.point(a + (b - a) * t);
                self.rels.push(Rel::OnCurve(p, line));
                self.step(p, angle, len);
            }
            Op::OnArc {
                arc,
                at,
                angle,
                len,
            } if !arcs.is_empty() => {
                let arc = arcs[arc % arcs.len()];
                let Curve::Arc(c, s, _) = self.curves[arc] else {
                    unreachable!()
                };
                let r = self.points[c].distance(self.points[s]);
                let p = self.point(self.points[c] + dir(at) * r);
                self.rels.push(Rel::OnCurve(p, arc));
                self.step(p, angle, len);
            }
            Op::Dup { from, angle, len } => {
                let from = from % n;
                let p = self.point(self.points[from]);
                self.rels.push(Rel::Coincident(p, from));
                self.step(p, angle, len);
            }
            Op::Tangent {
                arc,
                end,
                forward,
                len,
            } if !arcs.is_empty() => {
                let arc = arcs[arc % arcs.len()];
                let Curve::Arc(c, s, e) = self.curves[arc] else {
                    unreachable!()
                };
                let at = if end { e } else { s };
                let radial = (self.points[at] - self.points[c]).normalize();
                let along = if forward {
                    radial.perp()
                } else {
                    -radial.perp()
                };
                let p = self.point(self.points[at] + along * LENGTHS[usize::from(len)]);
                self.curves.push(Curve::Line(at, p));
                self.rels.push(Rel::Tangent(self.curves.len() - 1, arc));
            }
            _ => {}
        }
    }

    /// Relations that happen to hold in the sketch as it stands (and aren't already there).
    fn candidates(&self) -> Vec<Rel> {
        let mut out = Vec::new();
        let near = |a: f64, b: f64| (a - b).abs() < HOLDS;
        let lines = self.lines();
        let arcs = self.arcs();
        let unit = |l: usize| {
            let (a, b) = self.ends(l);
            (b - a).normalize()
        };
        let length = |l: usize| {
            let (a, b) = self.ends(l);
            a.distance(b)
        };
        let circle = |arc: usize| match self.curves[arc] {
            Curve::Arc(c, s, _) => (self.points[c], self.points[c].distance(self.points[s])),
            Curve::Line(..) => unreachable!(),
        };
        let on_curve = |p: usize, curve: usize| match self.curves[curve] {
            Curve::Line(a, b) => p != a && p != b,
            Curve::Arc(c, s, e) => p != c && p != s && p != e,
        };
        for &l in &lines {
            let u = unit(l);
            if near(u.y, 0.0) {
                out.push(Rel::Horizontal(l));
            }
            if near(u.x, 0.0) {
                out.push(Rel::Vertical(l));
            }
        }
        for (i, &l) in lines.iter().enumerate() {
            for &m in &lines[i + 1..] {
                let (u, v) = (unit(l), unit(m));
                if near(u.perp_dot(v), 0.0) {
                    out.push(Rel::Parallel(l, m));
                }
                if near(u.dot(v), 0.0) {
                    out.push(Rel::Perpendicular(l, m));
                }
                if near(length(l), length(m)) {
                    out.push(Rel::Equal(l, m));
                }
            }
        }
        for (i, &a) in arcs.iter().enumerate() {
            for &b in &arcs[i + 1..] {
                if near(circle(a).1, circle(b).1) {
                    out.push(Rel::Equal(a, b));
                }
            }
        }
        for &l in &lines {
            let (a, b) = self.ends(l);
            for &arc in &arcs {
                let (c, r) = circle(arc);
                let d = (b - a).normalize().perp_dot(c - a).abs();
                if near(d, r) && !self.rels.contains(&Rel::Tangent(l, arc)) {
                    out.push(Rel::Tangent(l, arc));
                }
            }
        }
        for p in 0..self.points.len() {
            let at = self.points[p];
            for q in p + 1..self.points.len() {
                if at.distance(self.points[q]) < HOLDS
                    && !self.rels.contains(&Rel::Coincident(q, p))
                {
                    out.push(Rel::Coincident(p, q));
                }
            }
            for &l in &lines {
                let (a, b) = self.ends(l);
                if !on_curve(p, l) || self.rels.contains(&Rel::OnCurve(p, l)) {
                    continue;
                }
                if near((b - a).normalize().perp_dot(at - a), 0.0) {
                    out.push(if at.distance((a + b) / 2.0) < HOLDS {
                        Rel::Midpoint(p, l)
                    } else {
                        Rel::OnCurve(p, l)
                    });
                }
            }
            for &arc in &arcs {
                let (c, r) = circle(arc);
                if on_curve(p, arc)
                    && !self.rels.contains(&Rel::OnCurve(p, arc))
                    && near(at.distance(c), r)
                {
                    out.push(Rel::OnCurve(p, arc));
                }
            }
        }
        out
    }

    fn has_hv(&self) -> bool {
        self.rels
            .iter()
            .any(|r| matches!(r, Rel::Horizontal(_) | Rel::Vertical(_)))
    }
}

/// A similarity transform: scale, then rotate, then shift.
#[derive(Clone, Copy, Debug)]
struct Similarity {
    scale: f64,
    /// Degrees counter-clockwise.
    turn: f64,
    shift: DVec2,
}

impl Similarity {
    const IDENTITY: Self = Self {
        scale: 1.0,
        turn: 0.0,
        shift: DVec2::ZERO,
    };

    fn rotation(&self) -> DVec2 {
        drift_core::blocks::exact_rotor(self.turn)
    }

    fn vector(&self, v: DVec2) -> DVec2 {
        self.rotation().rotate(v) * self.scale
    }

    fn point(&self, p: DVec2) -> DVec2 {
        self.vector(p) + self.shift
    }

    /// Whether the turn swaps horizontal and vertical.
    fn swaps(&self) -> bool {
        (self.turn / 90.0).round().rem_euclid(2.0) == 1.0
    }
}

/// A sketch put into an editor.
struct Built {
    ed: Editor,
    sketch: Sketch,
    points: Vec<EntityId>,
    curves: Vec<EntityId>,
}

impl Built {
    fn positions(&self) -> Vec<DVec2> {
        self.points
            .iter()
            .map(|&p| self.ed.doc.point(p).unwrap())
            .collect()
    }

    fn constraint(&self, rel: Rel, swap: bool) -> Constraint {
        let (p, c) = (&self.points, &self.curves);
        let doc = &self.ed.doc;
        match rel {
            Rel::Coincident(a, b) => Constraint::Coincident { a: p[a], b: p[b] },
            Rel::Horizontal(l) if swap => Constraint::Vertical { line: c[l] },
            Rel::Horizontal(l) => Constraint::Horizontal { line: c[l] },
            Rel::Vertical(l) if swap => Constraint::Horizontal { line: c[l] },
            Rel::Vertical(l) => Constraint::Vertical { line: c[l] },
            Rel::Parallel(a, b) => Constraint::Parallel { a: c[a], b: c[b] },
            Rel::Perpendicular(a, b) => Constraint::Perpendicular { a: c[a], b: c[b] },
            Rel::Equal(a, b) => Constraint::Equal { a: c[a], b: c[b] },
            Rel::Tangent(a, b) => Constraint::Tangent { a: c[a], b: c[b] },
            Rel::OnCurve(point, curve) => Constraint::PointOnCurve {
                point: p[point],
                curve: c[curve],
            },
            Rel::Midpoint(point, line) => Constraint::Midpoint {
                point: p[point],
                line: c[line],
            },
            Rel::Fix(point) => Constraint::Fix {
                point: p[point],
                at: doc.point(p[point]).unwrap(),
            },
            Rel::Length(l) => {
                let drift_core::EntityKind::Line { a, b } = doc.get(c[l]).unwrap().kind else {
                    unreachable!()
                };
                Constraint::Distance {
                    a,
                    b,
                    value: doc.point(a).unwrap().distance(doc.point(b).unwrap()),
                }
            }
        }
    }

    /// Put `sketch` (moved by `t`) into a new editor with all its relations, as one commit.
    fn new(sketch: &Sketch, t: Similarity) -> Option<Self> {
        let mut ed = Editor::default();
        ed.doc.begin();
        let points: Vec<_> = sketch
            .points
            .iter()
            .map(|&p| ed.doc.add_point(t.point(p)))
            .collect();
        let mut curves = Vec::new();
        for c in &sketch.curves {
            curves.push(match *c {
                Curve::Line(a, b) => ed.doc.add_line(points[a], points[b]).ok()?,
                Curve::Arc(c, s, e) => ed.doc.add_arc(points[c], points[s], points[e]).ok()?,
            });
        }
        let mut built = Self {
            ed,
            sketch: sketch.clone(),
            points,
            curves,
        };
        for &rel in &sketch.rels {
            let c = built.constraint(rel, t.swaps());
            built.ed.doc.add_constraint(c).ok()?;
        }
        built.ed.commit("Sketch").then_some(built)
    }

    fn fixes(&self) -> BTreeSet<EntityId> {
        self.ed
            .doc
            .constraints()
            .filter_map(|(_, c)| match *c {
                Constraint::Fix { point, .. } => Some(point),
                _ => None,
            })
            .collect()
    }

    /// Whether a fix holds anything a drag of `ids` can move.
    fn fixed_in_scope(&self, ids: &[EntityId]) -> bool {
        let scope = drift_solver::connected(&self.ed.doc, ids.iter().copied());
        self.fixes().iter().any(|p| scope.contains(p))
    }
}

/// A random sketch: its build steps, which of the relations that hold to add, and maybe a fix
/// and a length dimension.
#[derive(Clone, Debug)]
struct Plan {
    ops: Vec<Op>,
    extras: Vec<bool>,
    fix: Option<usize>,
    length: Option<usize>,
}

fn plan(fixes: bool) -> impl Strategy<Value = Plan> {
    let fix = if fixes {
        proptest::option::weighted(0.3, 0usize..64).boxed()
    } else {
        Just(None).boxed()
    };
    (
        prop::collection::vec(op(), 1..8),
        prop::collection::vec(prop::bool::weighted(0.35), 48),
        fix,
        proptest::option::weighted(0.3, 0usize..64),
    )
        .prop_map(|(ops, extras, fix, length)| Plan {
            ops,
            extras,
            fix,
            length,
        })
}

impl Plan {
    /// The sketch, with the relations an editor accepts (one that would be redundant or
    /// conflict is left out, as it would be for a user).
    fn sketch(&self) -> Option<Sketch> {
        let mut sketch = Sketch::default();
        sketch.point(DVec2::ZERO);
        for op in &self.ops {
            sketch.apply(op);
        }
        if sketch.curves.is_empty() {
            return None;
        }
        let mut wanted: Vec<Rel> = sketch
            .candidates()
            .into_iter()
            .zip(&self.extras)
            .filter_map(|(r, &on)| on.then_some(r))
            .collect();
        if let Some(i) = self.fix {
            wanted.push(Rel::Fix(i % sketch.points.len()));
        }
        let lines = sketch.lines();
        if let Some(i) = self.length
            && !lines.is_empty()
        {
            wanted.push(Rel::Length(lines[i % lines.len()]));
        }
        let mut built = Built::new(&sketch, Similarity::IDENTITY)?;
        for rel in wanted {
            let c = built.constraint(rel, false);
            if built.ed.add_constraints("Relation", &[c]).is_ok() {
                sketch.rels.push(rel);
            }
        }
        Some(sketch)
    }
}

/// What to drag: whole curves (a move) or one point.
#[derive(Clone, Debug)]
enum Pick {
    Curves(u64),
    Point(usize),
}

fn pick() -> impl Strategy<Value = Pick> {
    prop_oneof![
        2 => any::<u64>().prop_map(Pick::Curves),
        1 => (0usize..64).prop_map(Pick::Point),
    ]
}

fn curves_only() -> impl Strategy<Value = Pick> {
    any::<u64>().prop_map(Pick::Curves)
}

impl Pick {
    fn ids(&self, built: &Built) -> Vec<EntityId> {
        match *self {
            Pick::Curves(mask) => {
                let n = built.curves.len();
                let mut ids: Vec<_> = (0..n)
                    .filter(|i| (mask >> (i % 64)) & 1 == 1)
                    .map(|i| built.curves[i])
                    .collect();
                if ids.is_empty() || ids.len() == n {
                    // Not everything (that's its own property): one curve.
                    ids = vec![built.curves[(mask as usize) % n]];
                }
                ids
            }
            Pick::Point(i) => vec![built.points[i % built.points.len()]],
        }
    }
}

fn offset(max: f64) -> impl Strategy<Value = DVec2> {
    (-max..max, -max..max).prop_map(|(x, y)| DVec2::new(x, y))
}

/// What a drag did.
#[derive(Clone)]
struct Run {
    start: Vec<DVec2>,
    end: Vec<DVec2>,
    /// The offset the last accepted step reached (as the grab's `applied`).
    applied: DVec2,
    refused: usize,
    committed: bool,
}

thread_local! {
    /// The drags of the current case, written out for a failure message (a shrunk case then
    /// reads as geometry rather than as build steps).
    static DRAGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `result`, with the drags made added to a failure.
fn explained(result: Result<(), TestCaseError>) -> Result<(), TestCaseError> {
    let drags = DRAGS.with(|d| std::mem::take(&mut *d.borrow_mut()));
    result.map_err(|e| {
        TestCaseError::fail(format!(
            "{e}
{}",
            drags.join(
                "
"
            )
        ))
    })
}

fn close(a: DVec2, b: DVec2, size: f64) -> bool {
    a.distance(b) <= LANDS * size.max(1.0)
}

/// Drag `ids` by `offset` in `steps` steps the way `Grab::apply` does (each step from the
/// last accepted offset), then commit, checking after every step that an accepted drag meets
/// every relation and a refused one changes nothing (property 1).
fn drag(
    built: &mut Built,
    ids: &[EntityId],
    offset: DVec2,
    steps: u32,
) -> Result<Run, TestCaseError> {
    drag_along(built, ids, &[offset], steps)
}

/// [`drag`] through each offset of `path` in turn (in `steps` steps each), as one drag and
/// one commit.
fn drag_along(
    built: &mut Built,
    ids: &[EntityId],
    path: &[DVec2],
    steps: u32,
) -> Result<Run, TestCaseError> {
    let start = built.positions();
    DRAGS.with(|d| {
        let names = |ids: &[EntityId]| -> Vec<usize> {
            ids.iter()
                .map(|id| match built.curves.iter().position(|c| c == id) {
                    Some(i) => i,
                    None => built.points.iter().position(|p| p == id).unwrap(),
                })
                .collect()
        };
        let what = if built.curves.contains(&ids[0]) {
            "curves"
        } else {
            "point"
        };
        d.borrow_mut().push(format!(
            "  points {:?}
  curves {:?}
  relations {:?}
  dragged {what} {:?} through {path:?} in {steps} steps each",
            start,
            built.sketch.curves,
            built.sketch.rels,
            names(ids)
        ));
    });
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    built.ed.doc.begin();
    let mut applied = DVec2::ZERO;
    let mut refused = 0;
    let mut from = DVec2::ZERO;
    let targets: Vec<DVec2> = path
        .iter()
        .flat_map(|&to| {
            let leg = (1..=steps).map(move |k| from.lerp(to, f64::from(k) / f64::from(steps)));
            from = to;
            leg
        })
        .collect();
    for (k, target) in targets.into_iter().enumerate() {
        let k = k + 1;
        let before = built.positions();
        if built.ed.drag(ids, target - applied, &dragged) {
            applied = target;
            let unmet = drift_solver::unmet(&built.ed.doc, SLACK);
            prop_assert!(
                unmet.is_empty(),
                "step {k}: relations unmet after a drag: {unmet:?}"
            );
        } else {
            refused += 1;
            prop_assert_eq!(
                &built.positions(),
                &before,
                "step {}: a refused drag moved geometry",
                k
            );
        }
    }
    let committed = built.ed.commit("Move");
    let end = built.positions();
    if committed {
        let unmet = drift_solver::unmet(&built.ed.doc, SLACK);
        prop_assert!(
            unmet.is_empty(),
            "relations unmet after the commit: {unmet:?}"
        );
    } else {
        prop_assert_eq!(&end, &start, "a refused commit moved geometry");
    }
    Ok(Run {
        start,
        end,
        applied: if committed { applied } else { DVec2::ZERO },
        refused,
        committed,
    })
}

/// Property 1: Accepted drags meet every relation; refused ones change nothing.
fn relations_hold(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    drag(&mut built, &ids, offset, steps)?;
    Ok(())
}

/// Property 2: A move of whole curves lands exactly (where no fix stops it).
fn moves_land(plan: &Plan, pick: &Pick, offset: DVec2, steps: u32) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    if built.fixed_in_scope(&ids) {
        return Ok(());
    }
    let selected = built.ed.doc.defining_points(ids.iter().copied());
    let run = drag(&mut built, &ids, offset, steps)?;
    for (i, &p) in built.points.iter().enumerate() {
        if selected.contains(&p) {
            let want = run.start[i] + run.applied;
            prop_assert!(
                close(run.end[i], want, want.length() + offset.length()),
                "point {i} ended at {} instead of {want}",
                run.end[i]
            );
        }
    }
    Ok(())
}

/// Property 2b: a move of whole curves that nothing fixed stops is never refused (moving the
/// whole connected part with it meets every relation).
fn free_moves_go(plan: &Plan, pick: &Pick, offset: DVec2, steps: u32) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    if built.fixed_in_scope(&ids) {
        return Ok(());
    }
    let run = drag(&mut built, &ids, offset, steps)?;
    prop_assert!(
        run.committed && run.refused == 0,
        "a free move was refused ({} of {steps} steps)",
        run.refused
    );
    Ok(())
}

/// Property 3: A line hanging from what the move takes (its far end free: on no other curve, in
/// no relation) and not itself touched by it (not selected, no selected point, nothing holding
/// it) keeps its length: it is carried along, not stretched (#90). A line whose far end holds
/// on to more geometry may stretch instead, so the move doesn't shove that along (#143).
fn no_unforced_stretch(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    // With nothing fixed the whole sketch could follow the move, so every line could keep
    // its length.
    if built.fixed_in_scope(&ids) {
        return Ok(());
    }
    // A point joined to a selected one by a coincident relation moves with it, as a shared
    // point would.
    let mut selected = built.ed.doc.defining_points(ids.iter().copied());
    loop {
        let joined: Vec<EntityId> = built
            .ed
            .doc
            .constraints()
            .filter_map(|(_, c)| match *c {
                Constraint::Coincident { a, b } if selected.contains(&a) => Some(b),
                Constraint::Coincident { a, b } if selected.contains(&b) => Some(a),
                _ => None,
            })
            .filter(|p| !selected.contains(p))
            .collect();
        if joined.is_empty() {
            break;
        }
        selected.extend(joined);
    }
    let dimensioned: HashSet<EntityId> = built
        .ed
        .doc
        .constraints()
        .filter_map(|(_, c)| match *c {
            Constraint::Distance { a, b, .. } => Some([a, b]),
            _ => None,
        })
        .flatten()
        .collect();
    let mut uses: std::collections::HashMap<EntityId, usize> = Default::default();
    for (_, e) in built.ed.doc.iter().filter(|(_, e)| !e.kind.is_point()) {
        for p in e.kind.points() {
            *uses.entry(p).or_default() += 1;
        }
    }
    let named: HashSet<EntityId> = built
        .ed
        .doc
        .constraints()
        .flat_map(|(_, c)| c.entities())
        .collect();
    let free = |p: &EntityId| uses.get(p) == Some(&1) && !named.contains(p);
    let run = drag(&mut built, &ids, offset, steps)?;
    for (l, curve) in sketch.curves.iter().enumerate() {
        let Curve::Line(a, b) = *curve else {
            continue;
        };
        let (pa, pb) = (built.points[a], built.points[b]);
        if !free(&pa) && !free(&pb) {
            continue;
        }
        if ids.contains(&built.curves[l])
            || [pa, pb]
                .iter()
                .any(|p| selected.contains(p) || dimensioned.contains(p))
        {
            continue;
        }
        let before = run.start[a].distance(run.start[b]);
        let after = run.end[a].distance(run.end[b]);
        prop_assert!(
            (after - before).abs() <= LANDS * before.max(1.0),
            "line {l} (points {a}, {b}) went from {before} to {after} long"
        );
    }
    Ok(())
}

/// Property 4: Moving, turning or scaling the sketch first gives the same drag, moved the same way.
fn invariant(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    steps: u32,
    mut t: Similarity,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    if sketch.has_hv() {
        // Horizontal and vertical only stay that way under quarter turns.
        t.turn = (t.turn / 90.0).round() * 90.0;
    }
    let mut plain = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let Some(mut moved) = Built::new(&sketch, t) else {
        return Ok(());
    };
    let (ids_a, ids_b) = (pick.ids(&plain), pick.ids(&moved));
    let a = drag(&mut plain, &ids_a, offset, steps)?;
    let b = drag(&mut moved, &ids_b, t.vector(offset), steps)?;
    prop_assert_eq!(
        (a.committed, a.refused),
        (b.committed, b.refused),
        "refused differently"
    );
    for i in 0..sketch.points.len() {
        let want = t.point(a.end[i]);
        let size = t.scale * (a.end[i].length() + offset.length());
        prop_assert!(
            close(b.end[i], want, size),
            "point {i}: {} transformed is {want}, but the transformed drag put it at {}",
            a.end[i],
            b.end[i]
        );
    }
    Ok(())
}

/// Property 5: A small drag makes a small change: no arc turns inside out, no tangency flips side, and
/// nothing jumps far.
fn continuous(plan: &Plan, pick: &Pick, offset: DVec2) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let orientation = Orientation::of(&built.ed.doc, None);
    let run = drag(&mut built, &ids, offset, 1)?;
    prop_assert!(
        !orientation.turned(&built.ed.doc),
        "a small drag turned an arc or tangency"
    );
    // A dragged point that starts on a point a line through it must keep running through gives
    // that line no direction: any drag at all, however small, sets it, and the line swings round
    // to follow. That is the geometry's own answer, not a jump (#278, seed 1), where the drag
    // landed exactly and the line does run through that point afterwards. A point is excused as
    // far as what hangs on the swinging line goes (`SWING_MIRROR` times the line's own reach),
    // and judged on anything beyond that like any move, as is everything in a drag that isn't a
    // swing.
    let swing = forced_swing(&built, &ids, &run, offset);
    for i in 0..run.start.len() {
        let moved = judged_move(&run, i, swing);
        // (A solve rounds: a drag of nothing may still move a point by an ulp or so, as
        // `close` allows. Without that floor a shrunk case ends at a drag of exactly 0, and
        // the failure it stands for is lost.)
        prop_assert!(
            moved <= 100.0 * offset.length() + LANDS * run.start[i].length().max(1.0),
            "point {i} moved {moved} for a drag of {}",
            offset.length()
        );
    }
    Ok(())
}

/// How far a point may go on a forced swing's account, as a share of how far the swinging line
/// went (see [`judged_move`]): a point mirrored through one held on the swing moves up to twice
/// the swing's reach (#278, seed 1: a line's end mirrored through its midpoint, which hangs on
/// the swinging line).
const SWING_MIRROR: f64 = 2.0;

/// How far point `i` moved in `run`, as [`continuous`] judges it: the whole move, or, in a
/// forced swing whose line went `swing` at most (see [`forced_swing`]), only what it went beyond
/// [`SWING_MIRROR`] times that: what hangs on the line follows it round, but nothing goes farther
/// on its account.
fn judged_move(run: &Run, i: usize, swing: Option<f64>) -> f64 {
    let moved = run.start[i].distance(run.end[i]);
    match swing {
        Some(reach) => (moved - SWING_MIRROR * reach).max(0.0),
        None => moved,
    }
}

/// How far the swinging line went (its ends' largest move), where `run` (the drag of `ids` by
/// `offset`) is a forced swing (see [`continuous`]): `ids` is one point that starts exactly (to
/// rounding) on another point, either the other end of a line a relation holds it on
/// (`OnCurve`) or a fixed point (`Fix`); the drag landed exactly; and afterwards a line the
/// dragged point ends, or is held on, runs through that other point: the swinging line. (In the
/// first form that line is the held one, whose end the other point is: the form rests on the
/// start alone, which is what leaves the line no direction.) Not merely a point on another
/// point: points often share a place, and the drag of one may still jump; nor a drag that
/// didn't land.
fn forced_swing(built: &Built, ids: &[EntityId], run: &Run, offset: DVec2) -> Option<f64> {
    let [id] = ids else {
        return None;
    };
    let i = built.points.iter().position(|p| p == id)?;
    let sketch = &built.sketch;
    let at = sketch.points[i];
    let on = |j: usize| j != i && sketch.points[j].distance(at) <= 1e-12 * at.length().max(1.0);
    // The other point: a held line's other end, or a fixed point.
    let j = sketch.rels.iter().find_map(|r| match *r {
        Rel::OnCurve(p, c) if p == i => match sketch.curves[c] {
            Curve::Line(a, b) => [a, b].into_iter().find(|&e| on(e)),
            Curve::Arc(..) => None,
        },
        Rel::Fix(j) if on(j) => Some(j),
        _ => None,
    })?;
    let want = run.start[i] + offset;
    if !(run.committed && run.applied == offset && close(run.end[i], want, want.length())) {
        return None;
    }
    // The lines the dragged point ends or is held on that run through the other point now, and
    // the farthest any of their ends went.
    let through = run.end[j];
    sketch
        .curves
        .iter()
        .enumerate()
        .filter_map(|(c, curve)| match *curve {
            Curve::Line(a, b) if a == i || b == i || sketch.rels.contains(&Rel::OnCurve(i, c)) => {
                Some((a, b))
            }
            _ => None,
        })
        .filter(|&(a, b)| {
            let (pa, pb) = (run.end[a], run.end[b]);
            let along = pb - pa;
            along.length() > 0.0
                && along.perp_dot(through - pa).abs()
                    <= 1e-9 * along.length() * through.length().max(pa.length()).max(1.0)
        })
        .map(|(a, b)| {
            run.start[a]
                .distance(run.end[a])
                .max(run.start[b].distance(run.end[b]))
        })
        .reduce(f64::max)
}

/// Property 6: Moving everything is a pure translation.
fn select_all_translates(plan: &Plan, offset: DVec2, steps: u32) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = built.curves.clone();
    let run = drag(&mut built, &ids, offset, steps)?;
    prop_assert!(
        run.committed && run.refused == 0,
        "moving everything was refused"
    );
    for i in 0..run.start.len() {
        let want = run.start[i] + run.applied;
        prop_assert!(
            close(run.end[i], want, want.length() + offset.length()),
            "point {i} ended at {} instead of {want}",
            run.end[i]
        );
    }
    Ok(())
}

/// Property 7 (minimal movement): what the drag can't reach (no relation or shared point
/// joins it to the dragged) doesn't move at all, to the bit.
fn untouched_stays_put(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let reach = drift_solver::connected(&built.ed.doc, ids.iter().copied());
    let run = drag(&mut built, &ids, offset, steps)?;
    for (i, p) in built.points.iter().enumerate() {
        if !reach.contains(p) {
            prop_assert!(
                run.end[i] == run.start[i],
                "point {i}, out of the drag's reach, moved from {} to {}",
                run.start[i],
                run.end[i]
            );
        }
    }
    Ok(())
}

/// The points a drag may move: the dragged, everything relations link to them (a relation
/// links all the points it names, an arc its centre and ends; a line's ends only through
/// relations), and the free end of a line hanging from those, which a move of curves carries
/// along. #143: the rest of an outline that only shares ends with them stays put.
fn related_to(doc: &drift_core::Document, ids: &[EntityId], curves: bool) -> HashSet<EntityId> {
    let points = |id: EntityId| -> Vec<EntityId> {
        match doc.get(id) {
            Some(e) if e.kind.is_point() => vec![id],
            Some(e) => e.kind.points().collect(),
            None => Vec::new(),
        }
    };
    let mut related: HashSet<EntityId> = doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .collect();
    let groups: Vec<Vec<EntityId>> = doc
        .constraints()
        .filter(|(id, _)| !doc.is_driven(*id))
        .map(|(_, c)| c.entities().flat_map(points).collect())
        .chain(
            doc.iter()
                .filter(|(_, e)| {
                    !e.kind.is_point() && !matches!(e.kind, drift_core::EntityKind::Line { .. })
                })
                .map(|(id, _)| points(id)),
        )
        .collect();
    loop {
        let before = related.len();
        for group in &groups {
            if group.iter().any(|p| related.contains(p)) {
                related.extend(group.iter().copied());
            }
        }
        if related.len() == before {
            break;
        }
    }
    if curves {
        let mut uses: std::collections::HashMap<EntityId, usize> = Default::default();
        for (_, e) in doc.iter().filter(|(_, e)| !e.kind.is_point()) {
            for p in e.kind.points() {
                *uses.entry(p).or_default() += 1;
            }
        }
        let named: HashSet<EntityId> = doc.constraints().flat_map(|(_, c)| c.entities()).collect();
        let free = |p: EntityId| uses.get(&p) == Some(&1) && !named.contains(&p);
        let hanging: Vec<EntityId> = doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                drift_core::EntityKind::Line { a, b } if related.contains(&a) && free(b) => Some(b),
                drift_core::EntityKind::Line { a, b } if related.contains(&b) && free(a) => Some(a),
                _ => None,
            })
            .collect();
        related.extend(hanging);
    }
    related
}

/// Property 11 (#143): a drag leaves the curves no relation links to it as they were, even where
/// they share an end with what moves (that end moves; their far ends don't), unless the drag
/// had to push a curve that couldn't give way any further. (Relations hold throughout: every
/// drag checks that, coincidences among them.)
fn unrelated_stay_put(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let curves = ids
        .iter()
        .all(|id| built.ed.doc.get(*id).is_some_and(|e| !e.kind.is_point()));
    let related = related_to(&built.ed.doc, &ids, curves);
    let run = drag(&mut built, &ids, offset, steps)?;
    if built.ed.drag_pushed() {
        return Ok(());
    }
    for (i, p) in built.points.iter().enumerate() {
        if !related.contains(p) {
            // (Within round-off: unlike the unreached, these are in the solve.)
            prop_assert!(
                close(
                    run.end[i],
                    run.start[i],
                    run.start[i].length() + offset.length()
                ),
                "point {i}, which no relation links to the drag, moved from {} to {}",
                run.start[i],
                run.end[i]
            );
        }
    }
    Ok(())
}

/// Property 8 (path independence): a drag straight to `offset` and one by way of `via` end in
/// the same place, where neither is refused nor turns an arc or tangency (a branch switch).
fn path_independent(
    plan: &Plan,
    pick: &Pick,
    offset: DVec2,
    via: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut direct = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let mut around = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let orientation = Orientation::of(&direct.ed.doc, None);
    let ids = pick.ids(&direct);
    let a = drag(&mut direct, &ids, offset, steps)?;
    let b = drag_along(&mut around, &ids, &[offset + via, offset], steps)?;
    if a.refused + b.refused > 0
        || !a.committed
        || !b.committed
        || orientation.turned(&direct.ed.doc)
        || orientation.turned(&around.ed.doc)
    {
        return Ok(());
    }
    let size = offset.length() + via.length();
    for i in 0..a.end.len() {
        prop_assert!(
            close(a.end[i], b.end[i], a.end[i].length() + size),
            "point {i} ended at {} going straight but at {} going round",
            a.end[i],
            b.end[i]
        );
    }
    Ok(())
}

/// Property 9 (no creep): dragging out and back to the start, in one drag, puts everything
/// back where it was (where no step is refused and nothing turns).
fn no_creep(plan: &Plan, pick: &Pick, offset: DVec2, steps: u32) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let orientation = Orientation::of(&built.ed.doc, None);
    let ids = pick.ids(&built);
    let run = drag_along(&mut built, &ids, &[offset, DVec2::ZERO], steps)?;
    if run.refused > 0 || orientation.turned(&built.ed.doc) {
        return Ok(());
    }
    for i in 0..run.start.len() {
        prop_assert!(
            close(
                run.end[i],
                run.start[i],
                run.start[i].length() + offset.length()
            ),
            "point {i} crept from {} to {} out and back",
            run.start[i],
            run.end[i]
        );
    }
    Ok(())
}

/// Property 10 (determinism): the same drag on the same sketch gives bit-identical results.
fn deterministic(plan: &Plan, pick: &Pick, offset: DVec2, steps: u32) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut first = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let mut second = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&first);
    let a = drag(&mut first, &ids, offset, steps)?;
    let b = drag(&mut second, &ids, offset, steps)?;
    prop_assert_eq!((a.committed, a.refused), (b.committed, b.refused));
    prop_assert!(
        a.end == b.end,
        "{:?} the first time, {:?} the second",
        a.end,
        b.end
    );
    Ok(())
}

fn similarity() -> impl Strategy<Value = Similarity> {
    (0.25..4.0f64, 0.0..360.0f64, offset(500.0)).prop_map(|(scale, turn, shift)| Similarity {
        scale,
        turn,
        shift,
    })
}

/// A light test in the normal run and a heavy one behind `#[ignore]`, for one property.
/// Attributes before the light test's name (e.g. an `#[ignore]` for a known bug) apply to it.
macro_rules! property {
    ($(#[$attr:meta])* $light:ident, $heavy:ident, ($($arg:ident in $strategy:expr),+) => $check:expr) => {
        proptest! {
            // A fixed seed, so a rare failure can't make CI flaky (the heavy run explores).
            #![proptest_config(ProptestConfig {
                cases: LIGHT,
                rng_seed: proptest::test_runner::RngSeed::Fixed(0x0d21f7),
                ..ProptestConfig::default()
            })]
            $(#[$attr])*
            #[test]
            fn $light($($arg in $strategy),+) {
                explained($check)?;
            }
        }
        proptest! {
            // No saved seeds: a rare case the heavy run finds would otherwise fail the light run.
            #![proptest_config(ProptestConfig {
                cases: HEAVY,
                failure_persistence: None,
                ..ProptestConfig::default()
            })]
            #[test]
            #[ignore = "heavy: run with --ignored"]
            fn $heavy($($arg in $strategy),+) {
                explained($check)?;
            }
        }
    };
}

property!(drag_meets_relations, drag_meets_relations_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=20)
        => relations_hold(&plan, &pick, offset, steps));

property!(
    move_lands_exactly, move_lands_exactly_heavy,
    (plan in plan(true), pick in curves_only(), offset in offset(20.0), steps in 1u32..=20)
        => moves_land(&plan, &pick, offset, steps));

property!(
    free_move_is_accepted, free_move_is_accepted_heavy,
    (plan in plan(true), pick in curves_only(), offset in offset(20.0), steps in 1u32..=20)
        => free_moves_go(&plan, &pick, offset, steps));

property!(
    move_keeps_untouched_lengths, move_keeps_untouched_lengths_heavy,
    (plan in plan(false), pick in curves_only(), offset in offset(20.0), steps in 1u32..=20)
        => no_unforced_stretch(&plan, &pick, offset, steps));

property!(
    drag_is_invariant_under_similarity, drag_is_invariant_under_similarity_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=20,
        t in similarity())
        => invariant(&plan, &pick, offset, steps, t));

property!(small_drag_is_continuous, small_drag_is_continuous_heavy,
    (plan in plan(true), pick in pick(), offset in offset(0.5))
        => continuous(&plan, &pick, offset));

property!(select_all_moves_rigidly, select_all_moves_rigidly_heavy,
    (plan in plan(false), offset in offset(20.0), steps in 1u32..=20)
        => select_all_translates(&plan, offset, steps));

/// Free sketches: lines only, no relations (arcs keep a shape of their own, and the other
/// build steps add relations).
fn free_plan() -> impl Strategy<Value = Plan> {
    prop::collection::vec(op(), 1..8).prop_map(|ops| Plan {
        ops: ops
            .into_iter()
            .filter(|op| matches!(op, Op::Step { .. } | Op::Close { .. }))
            .collect(),
        extras: vec![false; 48],
        fix: None,
        length: None,
    })
}

/// Property 8: dragging a point of free geometry moves that point and nothing else, the
/// others not by a bit (no solve nudges them), as in a drawing program.
fn free_point_drag_is_exact(
    plan: &Plan,
    point: usize,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let i = point % built.points.len();
    let id = built.points[i];
    let run = drag(&mut built, &[id], offset, steps)?;
    prop_assert!(run.committed && run.refused == 0, "a free drag was refused");
    for (j, (&end, &start)) in run.end.iter().zip(&run.start).enumerate() {
        if j == i {
            let want = start + offset;
            prop_assert!(
                close(end, want, want.length() + offset.length()),
                "the dragged point ended at {end} instead of {want}"
            );
        } else {
            prop_assert_eq!(end, start, "point {} moved", j);
        }
    }
    Ok(())
}

property!(free_point_drag_moves_only_it, free_point_drag_moves_only_it_heavy,
    (plan in free_plan(), point in 0usize..64, offset in offset(20.0), steps in 1u32..=20)
        => free_point_drag_is_exact(&plan, point, offset, steps));

/// Step 1: moving free lines moves them exactly, as in a drawing program: a line hanging from
/// a moved one stretches, its far end staying put. Moved without its ends dragged (the line's
/// shape left open), the hanging line is carried along at its length instead, by the solve.
#[test]
fn free_lines_move_exactly() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![p(0.0, 0.0), p(10.0, 0.0), p(13.0, 4.0)],
        curves: vec![Curve::Line(0, 1), Curve::Line(1, 2)],
        rels: vec![],
    };
    let offset = p(3.3, -7.1);
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = built.curves.clone();
    let run = drag(&mut built, &ids, offset, 5).unwrap();
    assert!(run.committed && run.refused == 0);
    for (start, end) in run.start.iter().zip(&run.end) {
        assert!(
            end.distance(*start + offset) < 1e-12,
            "{start} went to {end}"
        );
    }
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[0]];
    let run = drag(&mut built, &ids, offset, 5).unwrap();
    assert!(run.committed && run.refused == 0);
    assert_eq!(run.end[2], run.start[2], "the far end moved");
    for i in 0..2 {
        assert!(run.end[i].distance(run.start[i] + offset) < 1e-12);
    }
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    built.ed.doc.begin();
    assert!(built.ed.drag(&[built.curves[0]], offset, &[]));
    built.ed.commit("Move");
    let end = built.positions();
    let length = end[2].distance(end[1]);
    assert!(
        (length - 5.0).abs() < 1e-9,
        "the hanging line is {length} long"
    );
}

/// #93: a move that makes an unselected point follow (a line's far end, kept by its midpoint
/// and an equal length) lands exactly, even in one step.
#[test]
fn move_with_a_follower_lands_exactly() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(7.5, -12.990381056766578),
            p(3.75, -6.495190528383289),
            p(13.409258262890685, -3.9070000773580817),
            p(14.488887394336025, 3.882285676537811),
        ],
        // A, B standing on A's midpoint, C from A's start as long as A.
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3), Curve::Line(0, 4)],
        rels: vec![Rel::Midpoint(2, 0), Rel::Equal(0, 2)],
    };
    let offset = p(0.0, -16.27957049340601);
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[1], built.curves[2]];
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(run.committed && run.refused == 0);
    for (start, end) in run.start.iter().zip(&run.end) {
        // Everything follows: the far end of A has to, and the rest is selected.
        assert!(
            end.distance(*start + offset) < 1e-9,
            "{start} went to {end}"
        );
    }
}

/// #95: moving a line whose end is shared with an arc and a line tangent to it squeezes the
/// arc; once it can't shrink any further the move pushes it along instead of stopping.
#[test]
fn move_pushes_a_squeezed_arc_along() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(2.5881904510252074, 9.659258262890683),
            p(2.0705523608201624, -7.727406610312547),
            p(6.070552360820166, -0.7992033800370404),
            p(9.659258262890685, 2.588190451025203),
        ],
        // A, an arc ending at A's start, B from there tangent to the arc.
        curves: vec![Curve::Line(0, 1), Curve::Arc(2, 3, 0), Curve::Line(0, 4)],
        rels: vec![Rel::Tangent(2, 1)],
    };
    for steps in [1, 15] {
        let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
        let ids = vec![built.curves[0]];
        let offset = p(19.261691266504407, 0.0);
        let run = drag(&mut built, &ids, offset, steps).unwrap();
        assert!(
            run.committed && run.refused == 0,
            "{steps} steps: refused {}",
            run.refused
        );
        for i in [0, 1] {
            assert!(run.end[i].distance(run.start[i] + offset) < 1e-9);
        }
    }
}

/// #102: a line hanging from an arc that a move carries along (#95: the arc can't shrink any
/// further, so it is pushed) is carried too, at its length, like one hanging from the
/// selection: the arc's points are carried, not dragged.
#[test]
fn a_line_hanging_from_a_carried_arc_follows_it() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(-8.0, 0.0),
            p(-2.3431457505076194, 5.656854249492381),
            p(2.0, 0.0),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
        ],
        // R1 ending at the origin, L from R1's centre, R2 ending at the origin too.
        curves: vec![Curve::Arc(1, 0, 2), Curve::Line(1, 3), Curve::Arc(4, 5, 0)],
        rels: vec![],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[2]];
    let offset = p(-14.355787012675158, 0.0);
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(run.committed && run.refused == 0);
    // R2's end took R1's start with it; R1 shrank as far as it could, then was pushed (its
    // centre left behind the start no more), and L went along with its centre.
    let centre = run.end[1];
    assert!(
        centre.x < run.start[1].x - 5.0 && centre.y.abs() < 1e-9,
        "centre at {centre}"
    );
    let length = run.end[1].distance(run.end[3]);
    assert!((length - 10.0).abs() < 1e-9, "L is {length} long");
}

/// #94: a line hanging from the midpoint of a moved line is carried along at its length,
/// step after step: the solve settles the preference that keeps it rather than stopping as
/// soon as the relations hold, with the length a little off each time.
#[test]
fn carried_line_keeps_its_length_step_after_step() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![p(0.0, 0.0), p(20.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)],
        // A, and B standing on A's midpoint.
        curves: vec![Curve::Line(0, 1), Curve::Line(2, 3)],
        rels: vec![Rel::Midpoint(2, 0)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[0]];
    let run = drag(&mut built, &ids, p(0.0, 20.0), 20).unwrap();
    assert!(run.committed && run.refused == 0);
    let length = run.end[2].distance(run.end[3]);
    assert!((length - 10.0).abs() < 1e-9, "B is {length} long");
}

/// #104: a move that can't land (B's end on A's midpoint can only move on a circle round the
/// fixed P0) gives the same compromise at any size: a preference that only the dragged could
/// meet doesn't inch them along, and the solve goes on until nothing moves.
#[test]
fn an_unlandable_move_gives_the_same_compromise_at_any_size() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(2.5881904510252074, 9.659258262890683),
            p(-3.10582854123025, 11.59110991546882),
            p(-6.211657082460498, 23.18221983093764),
            p(1.2940952255126037, 4.8296291314453415),
            p(11.294095225512603, 4.8296291314453415),
            p(2.4485969553838896, 28.18221983093764),
        ],
        // A from P0, B standing on A's midpoint, an arc from P0, C from its start as long as A.
        curves: vec![
            Curve::Line(0, 1),
            Curve::Arc(2, 3, 0),
            Curve::Line(4, 5),
            Curve::Line(3, 6),
        ],
        rels: vec![Rel::Midpoint(4, 0), Rel::Equal(0, 3), Rel::Fix(0)],
    };
    let offset = p(0.0, -5.261171146507463);
    let t = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: p(0.0, -71.07389948054578),
    };
    let mut plain = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let mut moved = Built::new(&sketch, t).unwrap();
    let (ia, ib) = (
        vec![plain.curves[2], plain.curves[3]],
        vec![moved.curves[2], moved.curves[3]],
    );
    let a = drag(&mut plain, &ia, offset, 3).unwrap();
    let b = drag(&mut moved, &ib, t.vector(offset), 3).unwrap();
    assert!(a.committed && b.committed);
    for i in 0..sketch.points.len() {
        let want = t.point(a.end[i]);
        assert!(
            b.end[i].distance(want) < 1e-9,
            "point {i}: {} vs {want}",
            b.end[i]
        );
    }
}

/// What a solve keeps that no relation names, checked directly (`drift_solver::unmet` reads
/// only relations' rows): every arc's end on the circle through its start, every midpoint
/// on its line's middle. The first broken one, if any.
fn broken_structure(ed: &Editor) -> Option<String> {
    let at = |p| ed.doc.point(p).unwrap();
    for (id, e) in ed.doc.iter() {
        if let drift_core::EntityKind::Arc { center, start, end } = e.kind {
            let (r0, r1) = (at(center).distance(at(start)), at(center).distance(at(end)));
            if (r0 - r1).abs() > 1e-9 * r0.max(1.0) {
                return Some(format!("arc {id:?}: ends {r0} and {r1} from its centre"));
            }
        }
    }
    for (_, c) in ed.doc.constraints() {
        if let Constraint::Midpoint { point, line } = *c
            && let Some(drift_core::EntityKind::Line { a, b }) = ed.doc.get(line).map(|e| e.kind)
        {
            let (a, b) = (at(a), at(b));
            let off = at(point).distance((a + b) / 2.0);
            if off > 1e-9 * a.distance(b).max(1.0) {
                return Some(format!("midpoint {point:?} is {off} off its line's middle"));
            }
        }
    }
    None
}

/// The parameters a move of `ids` drags (all their points, as the Select tool's).
fn dragged_by(ed: &Editor, ids: &[EntityId]) -> Vec<Param> {
    ed.doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect()
}

/// Drag `ids` by `offset` in `steps` calls (each from the last accepted offset), checking
/// after every accepted call that no arc or midpoint was left broken. How many calls were
/// accepted.
fn drag_keeping_structure(ed: &mut Editor, ids: &[EntityId], offset: DVec2, steps: u32) -> u32 {
    let dragged = dragged_by(ed, ids);
    ed.doc.begin();
    let mut applied = DVec2::ZERO;
    let mut accepted = 0;
    for k in 1..=steps {
        let target = offset * f64::from(k) / f64::from(steps);
        if ed.drag(ids, target - applied, &dragged) {
            applied = target;
            accepted += 1;
            if let Some(broken) = broken_structure(ed) {
                panic!("call {k} of {steps} was accepted with {broken}");
            }
        }
    }
    ed.doc.rollback();
    accepted
}

/// e13's sketch (#280): two arcs sharing an end, the first's centre fixed; the second's
/// centre is dragged by [`TWO_ARCS_DRAG`].
fn two_arcs_one_fixed() -> Sketch {
    let p = DVec2::new;
    Sketch {
        points: vec![
            p(0.0, 0.0),
            p(8.0, 0.0),
            p(2.3431457505076203, -5.65685424949238),
            p(-9.656854249492381, -5.65685424949238),
            p(-1.1715728752538102, -14.142135623730951),
        ],
        curves: vec![Curve::Arc(1, 0, 2), Curve::Arc(3, 4, 2)],
        rels: vec![Rel::Fix(1)],
    }
}

const TWO_ARCS_DRAG: DVec2 = DVec2::new(0.0, -8.147331698170428);

/// #281 J1: a step whose solve stopped at its iteration cap naming no conflict (the arcs'
/// rows belong to no relation) was kept as it was, an arc's end left 1.18 mm off its circle,
/// and the drag ended differently at another size. A step that doesn't finish is taken in
/// smaller steps or refused, never kept: the same calls go at either size.
#[test]
fn a_step_the_solve_does_not_finish_is_not_kept() {
    let sketch = two_arcs_one_fixed();
    let small = Similarity {
        scale: 0.25,
        turn: 0.0,
        shift: DVec2::new(0.0, 435.56956075701453),
    };
    for steps in [1, 4] {
        let accepted = [Similarity::IDENTITY, small].map(|t| {
            let mut built = Built::new(&sketch, t).unwrap();
            let ids = [built.points[3]];
            drag_keeping_structure(&mut built.ed, &ids, t.vector(TWO_ARCS_DRAG), steps)
        });
        assert_eq!(accepted[0], accepted[1], "in {steps} calls");
        // (Not by refusing everything: in 4 calls the first ones go.)
        assert!(steps == 1 || accepted[0] >= 1, "in {steps} calls none went");
    }
}

/// #281 J2 (f10, #282): moving a line whose neighbouring arc turns at every step size
/// carries the arc along, and that carried step doesn't finish whole; in halves it does. It
/// was refused unhalved ("a conflict is the same at any size"), though a rigid move meets
/// every relation.
#[test]
fn a_carried_step_that_does_not_finish_whole_goes_in_halves() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(-9.659258262890683, -2.588190451025208),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
            p(-4.8296291314453415, -1.294095225512604),
            p(5.1703708685546585, -1.294095225512604),
            p(-5.65685424949238, 5.656854249492381),
            p(-5.656854249492379, -2.3431457505076194),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Arc(2, 3, 0),
            Curve::Line(4, 5),
            Curve::Arc(6, 7, 0),
        ],
        rels: vec![Rel::Midpoint(4, 0)],
    };
    let offset = p(-17.99131491084439, 0.0);
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[2]];
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(run.committed && run.refused == 0, "the move was refused");
    for i in [4, 5] {
        let want = run.start[i] + offset;
        assert!(
            close(run.end[i], want, want.length()),
            "point {i} at {}",
            run.end[i]
        );
    }
    assert_eq!(broken_structure(&built.ed), None);
}

/// f3's sketch (#282), its arc's far end fixed, and its move: after the first small step,
/// the drag's solve doesn't finish at any step size (the two dragged points held on the arc
/// are a diameter apart, a fold), and with the fix nothing carries the arc along.
fn never_finishes() -> (Sketch, DVec2) {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
            p(2.3431457505076203, 5.656854249492381),
            p(-7.316112512383063, 3.0686637984671727),
            p(9.414213562373096, 12.727922061357855),
            p(15.0, 0.0),
            p(13.65685424949238, -5.6568542494923815),
            p(28.145741643828405, -1.7745685729545704),
        ],
        curves: vec![
            Curve::Arc(1, 2, 0),
            Curve::Line(3, 4),
            Curve::Line(2, 5),
            Curve::Line(0, 6),
            Curve::Line(7, 8),
        ],
        rels: vec![
            Rel::OnCurve(3, 0),
            Rel::Tangent(2, 0),
            Rel::OnCurve(7, 0),
            Rel::Fix(0),
        ],
    };
    (sketch, p(0.0, -18.003814381075802))
}

/// #281: a move whose solve doesn't finish even in its smallest steps is refused, leaving
/// everything where it was, and pushes nothing along (a solve left part way says nothing of
/// what turns).
#[test]
fn a_move_that_never_finishes_is_refused_pushing_nothing() {
    let (sketch, offset) = never_finishes();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[1], built.curves[4]];
    let dragged = dragged_by(&built.ed, &ids);
    let before = built.positions();
    built.ed.doc.begin();
    assert!(!built.ed.drag(&ids, offset, &dragged));
    assert!(
        !built.ed.drag_pushed(),
        "a refused frame pushed curves along"
    );
    assert_eq!(built.positions(), before);
    built.ed.doc.rollback();
}

/// #281: a frame whose steps don't finish spends at most
/// `UNFINISHED_WORK` (`editor/drag.rs`) on smaller steps (each such solve runs
/// to the solver's iteration cap over the whole part), then one solve for each step it still
/// tries (the whole move, the first half at each of a stepped step's 5 depths): not tens of
/// them. With 300 bars hanging from it, one of f3's unfinished solves spends it all.
#[test]
fn a_big_part_that_cannot_finish_spends_its_budget_only() {
    let (mut sketch, offset) = never_finishes();
    let mut last = 6;
    for k in 0..300 {
        let next = sketch.points.len();
        sketch.points.push(DVec2::new(15.0, -1.0 - f64::from(k)));
        sketch.curves.push(Curve::Line(last, next));
        last = next;
    }
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[1], built.curves[4]];
    let dragged = dragged_by(&built.ed, &ids);
    built.ed.doc.begin();
    assert!(!built.ed.drag(&ids, offset, &dragged));
    let unfinished = built.ed.drag_unfinished;
    assert!(
        unfinished <= 1 + 6,
        "the refused frame ran {unfinished} unfinished solves of {}",
        built.ed.drag_solves()
    );
    built.ed.doc.rollback();
}

/// A drawing that already conflicts: a line held 10 long from a fixed point to one held
/// both level with it and plumb under it, which can't all hold; `extra` adds more, given the
/// fixed point. Every solve of a drag in that part fails on those old conflicts. Returns the
/// editor and what `extra` returned.
fn already_conflicting<T>(
    extra: impl FnOnce(&mut drift_core::Document, EntityId) -> T,
) -> (Editor, T) {
    let mut doc = drift_core::Document::new();
    doc.begin();
    let a = doc.add_point(DVec2::new(8.0, 0.0));
    let b = doc.add_point(DVec2::new(18.0, 0.0));
    let line = doc.add_line(a, b).unwrap();
    for c in [
        Constraint::Fix {
            point: a,
            at: DVec2::new(8.0, 0.0),
        },
        Constraint::Horizontal { line },
        Constraint::Vertical { line },
        Constraint::Distance { a, b, value: 10.0 },
    ] {
        doc.add_constraint(c).unwrap();
    }
    let out = extra(&mut doc, a);
    doc.commit("Conflicting");
    let ed = Editor::new(doc);
    assert!(
        !ed.analysis().conflicts.is_empty(),
        "the drawing should conflict"
    );
    (ed, out)
}

/// A drag in a drawing that already conflicts goes as it did before #281: its solves fail
/// only on conflicts the drawing had, which don't stop every later edit.
#[test]
fn a_drawing_already_in_conflict_still_drags() {
    let (mut ed, tip) = already_conflicting(|doc, a| {
        let tip = doc.add_point(DVec2::new(8.0, 12.0));
        doc.add_line(a, tip).unwrap();
        tip
    });
    let offset = DVec2::new(3.0, 2.0);
    ed.doc.begin();
    assert!(ed.drag(&[tip], offset, &Param::point(tip)));
    assert_eq!(ed.doc.point(tip).unwrap(), DVec2::new(11.0, 14.0));
    ed.doc.rollback();
}

/// #281: an old conflict elsewhere in the part doesn't excuse a solve that broke an arc. The
/// report names only the old conflicts (the arcs' rows belong to no relation), yet the step
/// didn't finish: it goes as it does without the conflict. A drag the old conflict alone
/// fails still goes.
#[test]
fn an_old_conflict_does_not_excuse_a_broken_arc() {
    let (mut ed, (centre, tip)) = already_conflicting(|doc, a| {
        // e13's arcs, the first centred on the fixed point, and a line hanging from it.
        let p = |doc: &mut drift_core::Document, x, y| doc.add_point(DVec2::new(x, y));
        let p0 = p(doc, 0.0, 0.0);
        let p2 = p(doc, 2.3431457505076203, -5.65685424949238);
        let p3 = p(doc, -9.656854249492381, -5.65685424949238);
        let p4 = p(doc, -1.1715728752538102, -14.142135623730951);
        doc.add_arc(a, p0, p2).unwrap();
        doc.add_arc(p3, p4, p2).unwrap();
        let tip = p(doc, 8.0, 12.0);
        doc.add_line(a, tip).unwrap();
        (p3, tip)
    });
    let alone = {
        let mut built = Built::new(&two_arcs_one_fixed(), Similarity::IDENTITY).unwrap();
        let ids = [built.points[3]];
        drag_keeping_structure(&mut built.ed, &ids, TWO_ARCS_DRAG, 4)
    };
    assert_eq!(
        drag_keeping_structure(&mut ed, &[centre], TWO_ARCS_DRAG, 4),
        alone
    );
    ed.doc.begin();
    assert!(ed.drag(&[tip], DVec2::new(3.0, 2.0), &Param::point(tip)));
    assert_eq!(ed.doc.point(tip).unwrap(), DVec2::new(11.0, 14.0));
    ed.doc.rollback();
}

/// `sketch` built (moved by `t`), `extra` done to it, and its curves `picked` moved by
/// `offset` (moved by `t`) in one call, carrying the curves holding dragged points where
/// it may or (`no_carry`) as a drag did before #282: whether it went, where the points end,
/// and whether it pushed curves along.
fn move_once(
    sketch: &Sketch,
    t: Similarity,
    picked: &[usize],
    offset: DVec2,
    no_carry: bool,
    extra: impl FnOnce(&mut Built),
) -> (bool, Vec<DVec2>, bool) {
    let mut built = Built::new(sketch, t).unwrap();
    extra(&mut built);
    let ids: Vec<EntityId> = picked.iter().map(|&i| built.curves[i]).collect();
    let dragged = dragged_by(&built.ed, &ids);
    built.ed.drag_no_carry = no_carry;
    built.ed.doc.begin();
    let went = built.ed.drag(&ids, t.vector(offset), &dragged);
    let out = (went, built.positions(), built.ed.drag_pushed());
    built.ed.doc.rollback();
    out
}

/// A quarter of the size, far from the origin.
const SMALL: Similarity = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(0.0, 435.56956075701453),
};

/// Two lines whose ends are held on an arc a diameter apart (f3, #282, without its tangent
/// line), and a move of both lines.
fn held_a_diameter_apart() -> (Sketch, DVec2) {
    let p = DVec2::new;
    let at = |deg: f64| p(8.0, 0.0) + DVec2::from_angle(deg.to_radians()) * 8.0;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(8.0, 0.0),
            at(135.0),
            at(135.0),
            at(135.0) + p(-10.0, 0.0),
            at(315.0),
            at(315.0) + p(10.0, 0.0),
        ],
        curves: vec![Curve::Arc(1, 2, 0), Curve::Line(3, 4), Curve::Line(5, 6)],
        rels: vec![Rel::OnCurve(3, 0), Rel::OnCurve(5, 0)],
    };
    (sketch, p(0.0, -18.0))
}

/// #282: two lines whose ends are held on an arc a diameter apart, moved together. The arc's
/// kept radius sits on a fold there (its centre must be exactly midway), so no solve
/// finished, at any step size, and the move was refused. The arc they are held on is carried
/// along whole. (A quarter of the size far from the origin, it goes too, but some of its
/// stalled solves read as finished within the coordinates' tolerance and are kept as they
/// are: #280, `seeds::e_result_depends_on_size_at_a_fold`.)
#[test]
fn a_move_held_on_a_circle_at_a_fold_carries_the_circle_along() {
    let (sketch, offset) = held_a_diameter_apart();
    for t in [Similarity::IDENTITY, SMALL] {
        let start = Built::new(&sketch, t).unwrap().positions();
        let (went, end, pushed) = move_once(&sketch, t, &[1, 2], offset, false, |_| ());
        assert!(went, "refused at scale {}", t.scale);
        if t.scale != 1.0 {
            continue;
        }
        assert!(pushed, "the arc wasn't carried");
        for (i, (a, b)) in start.iter().zip(&end).enumerate() {
            let want = *a + t.vector(offset);
            assert!(
                close(*b, want, want.length()),
                "point {i} ended at {b} instead of {want} at scale {}",
                t.scale
            );
        }
    }
}

/// g7's sketch (#283): a line's midpoint coincident with an arc's end, the arc tangent to the
/// line there; a second line kept parallel to and as long as a dragged one, hanging from the
/// first line's end.
fn midpoint_at_a_touching_end() -> Sketch {
    let p = DVec2::new;
    Sketch {
        points: vec![
            p(0.0, 0.0),
            p(15.0, 0.0),
            p(7.5, 0.0),
            p(17.15925826289068, 2.5881904510252074),
            p(22.071067811865476, -7.071067811865477),
            p(7.5, 0.0),
            p(14.571067811865474, -7.071067811865477),
            p(7.5, 12.0),
            p(19.5, 12.000000000000004),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(2, 3),
            Curve::Line(1, 4),
            Curve::Line(5, 6),
            Curve::Line(4, 2),
            Curve::Arc(7, 8, 2),
        ],
        rels: vec![
            Rel::Midpoint(2, 0),
            Rel::Coincident(5, 2),
            Rel::Parallel(2, 3),
            Rel::Equal(2, 3),
            Rel::Tangent(0, 5),
        ],
    }
}

/// g7's move: the arc and the lines from its end.
const MIDPOINT_MOVE: DVec2 = DVec2::new(-2.7019888370729976, 17.00429950861392);

/// #283: a move whose one solve converged on the far branch (a line turned past square to the
/// one it is kept parallel to), its dragged points 7 off target, was kept. The line holding
/// the dragged midpoint is carried along instead: everything lands, the line moved whole.
#[test]
fn a_move_that_lands_off_carries_the_line_its_midpoint_is_held_on() {
    let sketch = midpoint_at_a_touching_end();
    for t in [Similarity::IDENTITY, SMALL] {
        let start = Built::new(&sketch, t).unwrap().positions();
        let (went, end, pushed) = move_once(&sketch, t, &[3, 4, 5], MIDPOINT_MOVE, false, |_| ());
        assert!(went && pushed, "at scale {}", t.scale);
        // The dragged points (the arc's, both lines') and the carried line's ends.
        for i in [0, 1, 2, 4, 5, 6, 7, 8] {
            let want = start[i] + t.vector(MIDPOINT_MOVE);
            assert!(
                close(end[i], want, want.length()),
                "point {i} ended at {} instead of {want} at scale {}",
                end[i],
                t.scale
            );
        }
    }
}

/// #283 (g14): the dragged end of an arc held on a line at the line's tangent end, which a
/// dragged centre keeps exactly a radius away: a fold, where the move landed 6e-5 off. The
/// line is carried along and the move lands to rounding.
#[test]
fn a_move_held_on_a_line_at_its_tangent_end_lands() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(8.660254037844387, 4.999999999999999),
            p(12.0, 0.0),
            p(3.5147186257614305, 8.485281374238571),
            p(0.0, -8.0),
            p(-1.4695761589768238e-15, -16.0),
            p(-10.000000000000002, -15.999999999999998),
            p(0.0, -16.0),
            p(5.656854249492381, -10.34314575050762),
            p(14.488887394336025, 3.882285676537811),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Arc(2, 3, 0),
            Curve::Arc(4, 0, 5),
            Curve::Line(5, 6),
            Curve::Arc(7, 8, 4),
            Curve::Line(0, 9),
        ],
        rels: vec![Rel::Tangent(3, 2), Rel::OnCurve(7, 3)],
    };
    let offset = p(0.0, 12.725262082906049);
    let start = Built::new(&sketch, Similarity::IDENTITY)
        .unwrap()
        .positions();
    let (went, end, _) = move_once(
        &sketch,
        Similarity::IDENTITY,
        &[4, 5],
        offset,
        false,
        |_| (),
    );
    assert!(went);
    // The dragged points: the arc's and the line's from its start.
    for i in [0, 4, 7, 8, 9] {
        let off = end[i].distance(start[i] + offset);
        assert!(
            off <= drift_core::tolerance::same(30.0),
            "point {i} landed {off} off"
        );
    }
}

/// #282, #283: where something pins the part, a move is never carried: a fixed point (or a
/// fixed x), a locked layer, or geometry generated from the part (an offset) keeps the part
/// from moving whole, and the drag is what it was without the carry (g7's move, landing off).
/// What pins a part, by name, done to a built sketch.
type Pin = (&'static str, fn(&mut Built));

#[test]
fn a_pinned_part_is_never_carried() {
    let sketch = midpoint_at_a_touching_end();
    let pins: [Pin; 4] = [
        ("a fixed x", |b| {
            let point = b.points[3];
            let x = b.ed.doc.point(point).unwrap().x;
            b.ed.add_constraints("Fix x", &[Constraint::FixX { point, x }])
                .unwrap();
        }),
        ("a fixed point", |b| {
            let point = b.points[3];
            let at = b.ed.doc.point(point).unwrap();
            b.ed.add_constraints("Fix", &[Constraint::Fix { point, at }])
                .unwrap();
        }),
        ("a locked layer", |b| {
            let layer = b.ed.add_layer("Locked");
            let mut l = b.ed.doc.layer(layer).unwrap().clone();
            l.locked = true;
            b.ed.doc.begin();
            b.ed.doc.set_layer(layer, l);
            b.ed.doc.set_entity_layer(b.points[3], layer).unwrap();
            b.ed.commit("Lock");
        }),
        ("an offset", |b| {
            let line = b.curves[1];
            b.ed.offset(line, 2.0, DVec2::new(12.0, -2.0)).unwrap();
        }),
    ];
    for (what, pin) in pins {
        let with = move_once(
            &sketch,
            Similarity::IDENTITY,
            &[3, 4, 5],
            MIDPOINT_MOVE,
            false,
            pin,
        );
        let without = move_once(
            &sketch,
            Similarity::IDENTITY,
            &[3, 4, 5],
            MIDPOINT_MOVE,
            true,
            pin,
        );
        assert!(!with.2, "with {what}, curves were carried");
        assert_eq!(with, without, "with {what}");
    }
}

/// #283: a carry that doesn't land is not kept: the step ends where it did without it
/// (seed 17's sketch, a fold the carried line can't get past either), nothing pushed.
#[test]
fn a_carry_that_does_not_land_leaves_the_step_as_it_was() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(15.000000000000004, -25.980762113533157),
            p(-7.4999999999999964, 12.99038105676658),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
            p(-3.7499999999999982, 6.49519052838329),
            p(-17.892135623730947, 20.63732615211424),
            p(3.750000000000001, -6.495190528383289),
            p(16.74038105676658, 1.00480947161671),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
            p(16.74038105676658, -18.99519052838329),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(0, 2),
            Curve::Arc(3, 4, 0),
            Curve::Line(5, 6),
            Curve::Line(7, 8),
            Curve::Arc(9, 10, 0),
            Curve::Line(8, 11),
        ],
        rels: vec![
            Rel::Midpoint(5, 1),
            Rel::OnCurve(7, 0),
            Rel::Equal(1, 4),
            Rel::OnCurve(7, 1),
        ],
    };
    let offset = p(0.0, -14.516466897443614);
    let with = move_once(
        &sketch,
        Similarity::IDENTITY,
        &[3, 5, 6],
        offset,
        false,
        |_| (),
    );
    let without = move_once(
        &sketch,
        Similarity::IDENTITY,
        &[3, 5, 6],
        offset,
        true,
        |_| (),
    );
    assert!(with.0 && !with.2);
    assert_eq!(with, without);
}

/// #97: a move that leaves a tangent line behind. The line hangs from the moved arc's end and
/// lies along the moved line A (ending where A ends, at its own point), so it stays tangent
/// only by keeping its direction while its far end stays put: the move squeezes it to nothing
/// and then through, which turns the tangency and the solve fails on the collapsed line. The
/// move carries it along instead of refusing.
#[test]
fn move_that_squeezes_a_tangent_line_carries_it_along() {
    let p = DVec2::new;
    let d = |deg: f64| DVec2::from_angle(deg.to_radians());
    let p0 = p(0.0, 0.0);
    let p1 = d(105.0) * 10.0;
    let mid = (p0 + p1) / 2.0;
    let centre = d(15.0) * 12.0;
    let sketch = Sketch {
        points: vec![
            p0,
            p1,
            mid,
            mid + d(15.0) * 10.0,
            centre,
            centre + d(240.0) * 12.0,
            p1,
        ],
        // A, B standing on A's midpoint, the arc from A's start, C from there along A.
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(2, 3),
            Curve::Arc(4, 0, 5),
            Curve::Line(0, 6),
        ],
        rels: vec![Rel::Midpoint(2, 0), Rel::Tangent(3, 2)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[0], built.curves[1], built.curves[2]];
    let offset = p(-6.58, 14.15);
    let run = drag(&mut built, &ids, offset, 17).unwrap();
    assert!(
        run.committed && run.refused == 0,
        "refused {} of 17 steps",
        run.refused
    );
    for i in [0, 1, 2, 3, 4, 5] {
        assert!(run.end[i].distance(run.start[i] + offset) < 1e-9);
    }
}

property!(drag_leaves_the_unreached_alone, drag_leaves_the_unreached_alone_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=20)
        => untouched_stays_put(&plan, &pick, offset, steps));

property!(drag_leaves_the_unrelated_alone, drag_leaves_the_unrelated_alone_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=20)
        => unrelated_stay_put(&plan, &pick, offset, steps));

property!(drag_is_path_independent, drag_is_path_independent_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), via in offset(10.0),
        steps in 1u32..=10)
        => path_independent(&plan, &pick, offset, via, steps));

property!(drag_out_and_back_does_not_creep, drag_out_and_back_does_not_creep_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=10)
        => no_creep(&plan, &pick, offset, steps));

property!(drag_is_deterministic, drag_is_deterministic_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0), steps in 1u32..=20)
        => deterministic(&plan, &pick, offset, steps));

/// #143: an L-shaped outline with an R10 corner (tangent to a horizontal top line and an upright
/// left side) and a rectangle sharing the inner corner. Dragging the corner arc, or its centre,
/// moves the arc, its tangent lines and what those hold on to (the bottom-left corner, which
/// the upright side takes along; the far rounded corner, which the top line's end takes), and
/// nothing else: the lower leg, the bar's underside and the rectangle stay where they are.
#[test]
fn dragging_a_corner_arc_leaves_the_rest_of_the_outline() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(0.0, 90.0),
            p(10.0, 90.0),
            p(10.0, 100.0),
            p(170.0, 100.0),
            p(170.0, 70.0),
            p(200.0, 70.0),
            p(200.0, 50.0),
            p(60.0, 50.0),
            p(60.0, 0.0),
            p(120.0, 50.0),
            p(120.0, 0.0),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Arc(2, 3, 1),
            Curve::Line(3, 4),
            Curve::Arc(5, 6, 4),
            Curve::Line(6, 7),
            Curve::Line(7, 8),
            Curve::Line(8, 9),
            Curve::Line(9, 0),
            Curve::Line(8, 10),
            Curve::Line(10, 11),
            Curve::Line(11, 9),
            Curve::Line(9, 8),
        ],
        rels: vec![
            Rel::Tangent(0, 1),
            Rel::Tangent(2, 1),
            Rel::Horizontal(2),
            Rel::Vertical(0),
        ],
    };
    let offset = p(-21.0, 18.0);
    for arc in [false, true] {
        for steps in [1, 20] {
            let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
            let ids = if arc {
                vec![built.curves[1]]
            } else {
                vec![built.points[2]]
            };
            let run = drag(&mut built, &ids, offset, steps).unwrap();
            assert!(run.committed && run.refused == 0);
            for i in 7..12 {
                assert_eq!(run.end[i], run.start[i], "point {i} moved");
            }
            // The corner went where it was taken, the left side upright and the top level.
            assert!(run.end[2].distance(run.start[2] + offset) < 1e-9);
            assert!((run.end[0].x - run.end[1].x).abs() < 1e-9);
            assert!((run.end[3].y - run.end[4].y).abs() < 1e-9);
        }
    }
}

/// #148: a drag of a line whose end starts a line with an arc tangent at its midpoint leaves
/// the tangency off by about 2e-6 rad (the touching-end form of the tangency is chosen after
/// the translation, when the midpoint is off the line).
#[test]
fn drag_keeps_a_tangency_at_a_midpoint() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(19.318516525781366, 5.176380902050415),
            p(9.659258262890683, 2.5881904510252074),
            p(12.24744871391589, 12.24744871391589),
            p(11.729810623710845, -5.13921615928734),
            p(4.801607393435337, -1.1392161592873373),
            p(5.176380902050415, 19.318516525781366),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(2, 3),
            Curve::Arc(4, 2, 5),
            Curve::Line(0, 6),
        ],
        rels: vec![Rel::Midpoint(2, 0), Rel::Tangent(0, 2)],
    };
    let offset = p(0.0, -18.264919971439852);
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = vec![built.curves[3]];
    let run = drag(&mut built, &ids, offset, 1).unwrap();
    assert!(run.committed && run.refused == 0);
}

// ----- Walls and dimensions on their faces (W2) ----------------------------------------------

/// A scene of walls with dimensions on their faces, as the gallery's T and L (and two walls
/// apart): what to drag, and a wall nothing relates to.
struct WallScene {
    ed: Editor,
    /// Centreline points to drag.
    points: Vec<EntityId>,
    /// A free wall away from the rest: it never moves.
    lone: Vec<EntityId>,
}

/// A run of wall along `pts`, 200 thick, in the open transaction.
fn wall_run(
    ed: &mut Editor,
    pts: &[DVec2],
    share: Option<EntityId>,
) -> (drift_core::WallId, Vec<EntityId>) {
    let ids: Vec<EntityId> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| match share.filter(|_| i == 0) {
            Some(s) => s,
            None => ed.doc.add_point(*p),
        })
        .collect();
    let curves: Vec<EntityId> = ids
        .windows(2)
        .map(|w| {
            let c = ed.doc.add_line(w[0], w[1]).unwrap();
            ed.doc.set_style(c, drift_core::LineKind::Construction);
            c
        })
        .collect();
    let path = ed
        .doc
        .add_path(drift_core::Path {
            curves,
            closed: false,
        })
        .unwrap();
    let w = ed
        .doc
        .add_wall(drift_core::Wall::new(
            path,
            200.0,
            drift_core::LayerId::DEFAULT,
        ))
        .unwrap();
    (w, ids)
}

/// The face piece of wall `w`'s segment `seg` (left or right) nearest `near`.
fn wall_face(ed: &Editor, w: drift_core::WallId, seg: u16, left: bool, near: DVec2) -> EntityId {
    let part = if left {
        drift_core::Part::Left
    } else {
        drift_core::Part::Right
    };
    ed.doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .iter()
        .filter(|(k, _)| k.seg == seg && k.part == part)
        .map(|(_, id)| *id)
        .min_by(|a, b| {
            let d = |id: &EntityId| drift_geom::distance(&ed.doc.shape(*id).unwrap(), near);
            d(a).total_cmp(&d(b))
        })
        .unwrap()
}

/// A face dimension at its current value, driving.
fn face_dimension(ed: &mut Editor, point: EntityId, line: EntityId) {
    let c = Constraint::PointLineDistance {
        point,
        line,
        value: 0.0,
    };
    let c = c.with_value(drift_solver::measure(&ed.doc, &c).unwrap());
    let id = ed.add_dimension(c, None).expect("a face dimension");
    assert!(!ed.doc.is_driven(id), "{:?}", ed.out.notice);
}

fn wall_scene(kind: u8) -> WallScene {
    let mut ed = Editor::new(drift_core::Document::new());
    let v = DVec2::new;
    ed.doc.begin();
    let (lone_wall, lone) = wall_run(&mut ed, &[v(20000.0, 0.0), v(24000.0, 0.0)], None);
    let _ = lone_wall;
    let mark = ed.doc.add_point(v(-1000.0, 1000.0));
    let scene = match kind % 3 {
        // Two walls apart, a clear width between their faces.
        0 => {
            let (a, pa) = wall_run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)], None);
            let (b, pb) = wall_run(&mut ed, &[v(0.0, 3000.0), v(4000.0, 3000.0)], None);
            ed.commit("Walls");
            let (fa, fb) = (
                wall_face(&ed, a, 0, true, v(2000.0, 0.0)),
                wall_face(&ed, b, 0, false, v(2000.0, 3000.0)),
            );
            face_dimension(&mut ed, fa, fb);
            // (A clear width is between parallel faces.)
            let centre = |w| ed.doc.path(ed.doc.wall(w).unwrap().centre).unwrap().curves[0];
            let parallel = Constraint::Parallel {
                a: centre(a),
                b: centre(b),
            };
            ed.add_constraints("Parallel", &[parallel]).unwrap();
            [pa, pb].concat()
        }
        // A T: the branch's face from a mark.
        1 => {
            let (_, ph) = wall_run(&mut ed, &[v(0.0, 0.0), v(3000.0, 0.0)], None);
            let (b, pb) = wall_run(&mut ed, &[v(1500.0, 2000.0), v(1500.0, 0.0)], None);
            ed.commit("Walls");
            let f = wall_face(&ed, b, 0, false, v(1400.0, 1000.0));
            face_dimension(&mut ed, mark, f);
            [ph, pb[..1].to_vec()].concat()
        }
        // An L: its inside face from a mark.
        _ => {
            let (w, p) = wall_run(
                &mut ed,
                &[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 2000.0)],
                None,
            );
            ed.commit("Walls");
            let f = wall_face(&ed, w, 1, true, v(2900.0, 1000.0));
            face_dimension(&mut ed, mark, f);
            p
        }
    };
    let fix = Constraint::Fix {
        point: mark,
        at: ed.doc.point(mark).unwrap(),
    };
    ed.add_constraints("Fix", &[fix]).unwrap();
    WallScene {
        ed,
        points: scene,
        lone,
    }
}

/// Through a drag of a wall's centreline point in steps, and its commit: every driving
/// dimension on a face holds, each wall keeps its thickness, the joins of what isn't dragged
/// stay what they were until the commit, and a wall nothing relates to stays put.
fn face_dimensions_hold(
    kind: u8,
    which: usize,
    offset: DVec2,
    steps: u32,
) -> Result<(), TestCaseError> {
    let mut s = wall_scene(kind);
    let ed = &mut s.ed;
    let p = s.points[which % s.points.len()];
    let lone_at: Vec<DVec2> = s.lone.iter().map(|q| ed.doc.point(*q).unwrap()).collect();
    let thick: Vec<f64> = ed.doc.walls().map(|(_, w)| w.thickness).collect();
    // Each run end's point and the wall it tees into.
    let ends = |ed: &Editor| -> Vec<(EntityId, Option<drift_core::WallId>)> {
        ed.doc
            .walls()
            .flat_map(|(_, w)| {
                let (_, pts, _) = drift_geom::walls::regen::centreline(&ed.doc, w).unwrap();
                [
                    (pts[0], w.ends[0].tee.map(|t| t.0)),
                    (pts[pts.len() - 1], w.ends[1].tee.map(|t| t.0)),
                ]
            })
            .collect()
    };
    let tees = ends(ed);
    let dragged = Param::point(p).to_vec();
    let unmet = |ed: &Editor| drift_solver::unmet_with(&ed.doc, SLACK, ed.lowering().as_deref());
    prop_assert!(unmet(ed).is_empty(), "holds to begin with");
    ed.doc.begin();
    for k in 1..=steps {
        let to = offset * (f64::from(k) / f64::from(steps));
        let ok = ed.drag(&[p], to, &dragged);
        prop_assert!(
            unmet(ed).is_empty(),
            "step {k} (moved: {ok}): {:?}",
            unmet(ed)
        );
        // (The dragged end itself may join something as it goes.)
        for (was, is) in tees.iter().zip(ends(ed)) {
            prop_assert!(was.0 == p || *was == is, "a join flipped at step {k}");
        }
    }
    ed.commit("Drag");
    prop_assert!(unmet(ed).is_empty(), "after the commit");
    let thick_now: Vec<f64> = ed.doc.walls().map(|(_, w)| w.thickness).collect();
    prop_assert_eq!(thick, thick_now);
    for (q, at) in s.lone.iter().zip(lone_at) {
        prop_assert_eq!(ed.doc.point(*q).unwrap(), at, "the lone wall stayed");
    }
    // Every dimension measures its value on the regenerated faces.
    for (id, c) in ed.doc.constraints() {
        if c.is_dimension() && !ed.doc.is_driven(id) {
            let m = drift_solver::measure(&ed.doc, c).unwrap();
            let want = c.value().unwrap();
            prop_assert!((m - want).abs() <= 1e-6 * want.max(1.0), "{m} for {want}");
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: LIGHT, ..ProptestConfig::default() })]

    #[test]
    fn face_dimensions_hold_through_drags(
        kind in 0u8..3,
        which in 0usize..8,
        dx in -1500.0f64..1500.0,
        dy in -1500.0f64..1500.0,
        steps in 1u32..6,
    ) {
        face_dimensions_hold(kind, which, DVec2::new(dx, dy), steps)?;
    }
}

proptest! {
    // (No saved seeds, as for the other heavy runs.)
    #![proptest_config(ProptestConfig {
        cases: HEAVY / 10,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    #[ignore = "heavy: run with --ignored"]
    fn face_dimensions_hold_through_drags_heavy(
        kind in 0u8..3,
        which in 0usize..8,
        dx in -1500.0f64..1500.0,
        dy in -1500.0f64..1500.0,
        steps in 1u32..6,
    ) {
        face_dimensions_hold(kind, which, DVec2::new(dx, dy), steps)?;
    }
}

/// From CI: dragging a wall's end along it, in steps, with a clear width held.
#[test]
fn face_dimension_holds_dragging_a_wall_end_along_it() {
    face_dimensions_hold(0, 2, DVec2::new(1420.8408966554066, 0.0), 5).unwrap();
}

/// From the property: a host dragged until its end comes near the branch's far end, where
/// their lines cross far off (at the T): the walls aren't welded there at the commit.
#[test]
fn face_dimension_holds_when_an_end_passes_near_another() {
    face_dimensions_hold(1, 3, DVec2::new(777.1960625046115, 1361.8778343154509), 2).unwrap();
}

/// e13's sketch dragged in `steps` calls: how many go. (#282's fold steps never change it: a
/// part a fixed point pins is never tried with them, and a solve with them that would grow
/// the radii is never finished.)
fn two_arcs_calls(steps: u32) -> u32 {
    let sketch = two_arcs_one_fixed();
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.points[3]];
    drag_keeping_structure(&mut built.ed, &ids, TWO_ARCS_DRAG, steps)
}

/// The user's rule for a drag the kept radii can't follow: it stops where the arcs still
/// meet, as before #282's fold steps; it never grows the radii to go on.
#[test]
fn a_drag_the_kept_radii_cannot_follow_still_stops() {
    assert_eq!([1, 4, 16].map(two_arcs_calls), [0, 2, 8]);
}

/// Moving `picked` of `sketch` by `offset` in one call, with the curves holding its dragged
/// points carried where they may or not (`no_carry`), and tried with the fold steps where
/// it can't be made otherwise or not (`no_fold`): whether it went, where the points end,
/// whether it pushed curves along, how many times it was tried with the fold steps, and
/// how many solves it ran.
fn move_with_folds(
    sketch: &Sketch,
    picked: &[usize],
    offset: DVec2,
    no_carry: bool,
    no_fold: bool,
) -> (bool, Vec<DVec2>, bool, u32, u32) {
    let mut built = Built::new(sketch, Similarity::IDENTITY).unwrap();
    let ids: Vec<EntityId> = picked.iter().map(|&i| built.curves[i]).collect();
    let dragged = dragged_by(&built.ed, &ids);
    built.ed.drag_no_carry = no_carry;
    built.ed.drag_no_fold = no_fold;
    built.ed.doc.begin();
    let went = built.ed.drag(&ids, offset, &dragged);
    let out = (
        went,
        built.positions(),
        built.ed.drag_pushed(),
        built.ed.drag_folds,
        built.ed.drag_solves(),
    );
    built.ed.doc.rollback();
    out
}

/// #282: a pinned part is never tried with the fold steps: f3's sketch with its arc's far
/// end fixed, whose move can't be made, is refused as before and tries them not once.
#[test]
fn a_pinned_part_is_never_tried_with_the_fold_steps() {
    let (sketch, offset) = never_finishes();
    let (went, _, pushed, folds, _) = move_with_folds(&sketch, &[1, 4], offset, false, false);
    assert!(!went && !pushed);
    assert_eq!(folds, 0);
}

/// #282: the two lines held on an arc a diameter apart (its kept radius at a fold), with
/// nothing carrying the arc: the move goes with the fold steps, lands, and keeps the radius.
#[test]
fn a_move_held_at_a_fold_goes_with_the_fold_steps() {
    let (sketch, offset) = held_a_diameter_apart();
    let (went, end, _, folds, _) = move_with_folds(&sketch, &[1, 2], offset, true, false);
    assert!(went && folds >= 1, "went {went}, tried {folds} times");
    let start = Built::new(&sketch, Similarity::IDENTITY)
        .unwrap()
        .positions();
    for i in [3, 4, 5, 6] {
        let want = start[i] + offset;
        assert!(
            close(end[i], want, want.length()),
            "point {i} at {}",
            end[i]
        );
    }
    assert!((end[1].distance(end[2]) - 8.0).abs() < 1e-8);
    let (went, ..) = move_with_folds(&sketch, &[1, 2], offset, true, true);
    assert!(!went, "it went without the fold steps");
}

/// #282: a part too big for a solve that runs to the solver's cap to fit in what a frame may
/// spend on the fold steps is never tried with them: the held-a-diameter move with 2,000
/// bars held at their lengths hanging from the arc, nothing carrying the arc, is refused
/// with the same solves as without them.
#[test]
fn a_big_part_is_never_tried_with_the_fold_steps() {
    let (mut sketch, offset) = held_a_diameter_apart();
    let mut last = 0;
    for k in 0..2_000 {
        let next = sketch.points.len();
        sketch.points.push(DVec2::new(-1.0 - f64::from(k), -1.0));
        sketch.curves.push(Curve::Line(last, next));
        sketch.rels.push(Rel::Length(sketch.curves.len() - 1));
        last = next;
    }
    let with = move_with_folds(&sketch, &[1, 2], offset, true, false);
    let without = move_with_folds(&sketch, &[1, 2], offset, true, true);
    assert!(!with.0);
    assert_eq!(with.3, 0, "tried with the fold steps");
    assert_eq!(with.4, without.4);
}

/// An arc a line ends on, tangent to it, with a second, vertical line as long as it holding
/// its midpoint, and two arcs sharing an end beside them (a found case,
/// `move_keeps_untouched_lengths_heavy` seed 1), and a move of the first arc and the other two
/// that today's steps land off.
fn lands_off() -> (Sketch, [usize; 3], DVec2) {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(8.0, 0.0),
            p(2.3431457505076203, 5.656854249492381),
            p(16.0, 0.0),
            p(16.0, 10.0),
            p(16.0, 5.0),
            p(16.0, 15.0),
            p(9.271348980783127, 1.656854249492377),
            p(1.5439423704705781, -0.41369811132778533),
            p(20.862458896251944, -5.590079013378199),
            p(40.18097542203331, -10.766459915428609),
            p(26.60660171779821, -0.606601717798215),
        ],
        curves: vec![
            Curve::Arc(1, 2, 0),
            Curve::Line(3, 4),
            Curve::Line(5, 6),
            Curve::Arc(7, 2, 8),
            Curve::Arc(9, 10, 8),
            Curve::Line(4, 11),
        ],
        rels: vec![
            Rel::OnCurve(3, 0),
            Rel::Midpoint(5, 1),
            Rel::Vertical(1),
            Rel::Vertical(2),
            Rel::Equal(1, 2),
            Rel::Tangent(1, 0),
        ],
    };
    (sketch, [0, 3, 5], p(16.377975070732383, 2.966956818320761))
}

/// `lands_off` with lines hanging from point `from`: `pair` two, perpendicular, free to turn
/// about it as a whole (which no row or kept length feels); otherwise a chain of three, each
/// square to the next. Returns the sketch, what is moved, the move, and the first hung point.
fn lands_off_hung(from: usize, pair: bool) -> (Sketch, [usize; 3], DVec2, usize) {
    let (mut sketch, picked, offset) = lands_off();
    let p = DVec2::new;
    let a = sketch.points.len();
    let mut at = sketch.points[from];
    let mut last = from;
    let legs: &[DVec2] = if pair {
        &[p(3.0, -7.0), p(7.0, 3.0)]
    } else {
        &[p(3.0, -7.0), p(7.0, 3.0), p(-2.0, 5.0)]
    };
    for (k, leg) in legs.iter().enumerate() {
        at += *leg;
        sketch.points.push(at);
        sketch.curves.push(Curve::Line(last, a + k));
        last = a + k;
        if k > 0 {
            let n = sketch.curves.len();
            sketch.rels.push(Rel::Perpendicular(n - 2, n - 1));
        }
    }
    (sketch, picked, offset, a)
}

/// #282: a kept try with the fold steps flings nothing: `lands_off` with a pair of
/// perpendicular lines hanging from point 3, a sixteenth of the move in one call. With the
/// minimum change scaled down and nothing limiting the kept shapes' step, round-off turned
/// the pair 320 mm away across a drawing some 40 mm across. The hung points move no more
/// than a few millimetres further than today's steps move them.
#[test]
fn a_try_with_the_fold_steps_flings_nothing() {
    let (sketch, picked, offset, a) = lands_off_hung(3, true);
    let start = Built::new(&sketch, Similarity::IDENTITY)
        .unwrap()
        .positions();
    let (went, end, _, folds, _) = move_with_folds(&sketch, &picked, offset / 16.0, false, false);
    let today = move_with_folds(&sketch, &picked, offset / 16.0, false, true).1;
    assert!(went && folds > 0, "went {went}, tried {folds} times");
    for i in [a, a + 1] {
        let (moved, was) = (start[i].distance(end[i]), start[i].distance(today[i]));
        assert!(
            moved <= 2.0 * was + 2.0,
            "hung point {i} moved {moved}, today {was}"
        );
    }
}

/// #282: a try with the fold steps never refuses a move today's steps take, nor sends it a
/// way they wouldn't: `lands_off` with a chain of three square lines hanging from point 11,
/// 15/16 of the move, in 1, 2 and 4 calls. Today's steps take each call, landing a little
/// off; with the fold steps each goes too, as today's steps leave it to the bit or landed
/// exactly. (A try's solves that wandered, left in the frame, sent the whole move down the
/// stepped path today's steps never took, where a step failed and the move was refused.)
#[test]
fn a_try_with_the_fold_steps_never_refuses_what_today_takes() {
    let (sketch, picked, offset, _) = lands_off_hung(11, false);
    let to = offset * 15.0 / 16.0;
    let start = Built::new(&sketch, Similarity::IDENTITY)
        .unwrap()
        .positions();
    let dragged = [0, 1, 2, 4, 7, 8, 11];
    for calls in [1u32, 2, 4] {
        let run = |no_fold: bool| {
            let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
            let ids: Vec<EntityId> = picked.iter().map(|&i| built.curves[i]).collect();
            let dragged = dragged_by(&built.ed, &ids);
            built.ed.drag_no_fold = no_fold;
            built.ed.doc.begin();
            let mut out = Vec::new();
            let mut folds = 0;
            for _ in 0..calls {
                let went = built.ed.drag(&ids, to / f64::from(calls), &dragged);
                folds += built.ed.drag_folds;
                out.push((went, built.positions()));
            }
            built.ed.doc.rollback();
            (out, folds)
        };
        let (with, folds) = run(false);
        let (without, _) = run(true);
        assert!(
            folds > 0,
            "never tried with the fold steps in {calls} calls"
        );
        for (k, (a, b)) in with.iter().zip(&without).enumerate() {
            assert!(
                b.0,
                "call {} of {calls} refused without the fold steps",
                k + 1
            );
            assert!(a.0, "call {} of {calls} refused", k + 1);
            let at = to * f64::from(k as u32 + 1) / f64::from(calls);
            let landed = dragged
                .iter()
                .all(|&i| close(a.1[i], start[i] + at, (start[i] + at).length()));
            assert!(
                a.1 == b.1 || landed,
                "call {} of {calls}: neither today's nor landed",
                k + 1
            );
        }
    }
}

/// #282: a try with the fold steps that fails leaves the frame exactly as today's steps left
/// it. Each of 16 lengths of `lands_off`'s move, in one call: where it is tried with the fold
/// steps, the move either lands (the try was kept) or goes or not and leaves every point (to
/// the bit), and what it pushed, as it does without the fold steps; and there are tries of
/// both kinds, the failed ones having moved the geometry before they were undone.
#[test]
fn a_try_with_the_fold_steps_that_fails_leaves_the_frame_as_it_was() {
    let (sketch, picked, offset) = lands_off();
    let start = Built::new(&sketch, Similarity::IDENTITY)
        .unwrap()
        .positions();
    let dragged = [0, 1, 2, 4, 7, 8, 11];
    let (mut kept, mut undone) = (0, 0);
    for k in 1..=16 {
        let to = offset * f64::from(k) / 16.0;
        let with = move_with_folds(&sketch, &picked, to, false, false);
        let without = move_with_folds(&sketch, &picked, to, false, true);
        if with.3 == 0 {
            continue;
        }
        let landed = with.0
            && dragged
                .iter()
                .all(|&i| close(with.1[i], start[i] + to, (start[i] + to).length()));
        if landed && with.1 != without.1 {
            kept += 1;
        } else {
            undone += 1;
            assert_eq!(
                (with.0, &with.1, with.2),
                (without.0, &without.1, without.2),
                "{k}/16 of the move"
            );
        }
    }
    assert!(kept > 0 && undone > 0, "{kept} tries kept, {undone} undone");
}

mod held_shapes;
mod seeds;
