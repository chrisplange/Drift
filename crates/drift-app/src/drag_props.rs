//! Property tests for dragging and moving: random sketches of lines and arcs, joined by shared
//! points and held by relations that already hold (so every sketch can be met), are dragged
//! through [`Editor::drag`] in steps the way the select tool's grab does, and the result is
//! checked against what a drag should do. Solver drag bugs used to turn up one user report at
//! a time; these find the class.
//!
//! The case counts are small so `cargo test` stays quick; each property also has a heavier
//! run behind `#[ignore]` (`cargo test -p drift-app drag_props -- --ignored`).

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
        let quarter = self.turn / 90.0;
        if quarter == quarter.round() {
            match (quarter as i64).rem_euclid(4) {
                0 => DVec2::X,
                1 => DVec2::Y,
                2 => -DVec2::X,
                _ => -DVec2::Y,
            }
        } else {
            DVec2::from_angle(self.turn.to_radians())
        }
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
  dragged {what} {:?} by {offset} in {steps} steps",
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
    for k in 1..=steps {
        let target = offset * f64::from(k) / f64::from(steps);
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

/// Property 3: A line the move doesn't touch (not selected, no selected point, nothing holding it)
/// keeps its length.
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
    let run = drag(&mut built, &ids, offset, steps)?;
    for (l, curve) in sketch.curves.iter().enumerate() {
        let Curve::Line(a, b) = *curve else {
            continue;
        };
        let (pa, pb) = (built.points[a], built.points[b]);
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
    for i in 0..run.start.len() {
        let moved = run.start[i].distance(run.end[i]);
        prop_assert!(
            moved <= 100.0 * offset.length(),
            "point {i} moved {moved} for a drag of {}",
            offset.length()
        );
    }
    Ok(())
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
    // R2's end took R1's start with it, R1 was carried, and L went along with its centre.
    assert!(
        run.end[1].distance(run.start[1] + offset) < 1e-9,
        "centre at {}",
        run.end[1]
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
