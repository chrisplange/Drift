//! The sketch corpus (#187): the solver bar's item 8. Where the heavy matrix and the censuses
//! draw random sketches from one generator (`plan()`: lines and arcs, eleven kinds of
//! relation), this sweeps drawings as people make them: profiles, brackets, linkages, plans
//! with walls and openings, patterns, with every relation and dimension kind, circles and
//! ellipses. The drawings are `scripts/solver-bar/corpus/*.drift`, each built from the Drift
//! script beside it (drift-app's `corpus_build` test builds them and checks they match), plus
//! one real drawing kept as a fixture.
//!
//! **The sweep.** Every point that can still move and every curve with one is dragged along
//! each of [`PATHS`]: four short drags along the axes, four long ones on the diagonals, a full
//! circle about where it began, and one through the middle of its part and back (through
//! whatever folds lie there). The pointer moves a fiftieth of the part's size a frame (a
//! two-hundredth on the short drags), as the select tool's grab makes them, and every frame
//! is judged:
//!
//! | Letter | Check |
//! |---|---|
//! | `R` | **rows met**: an accepted frame leaves a relation or a structural row off |
//! | `S` | **no arc sweep flip**: an arc the drag doesn't hold by a point swept over 90° more or less in one frame (a short arc become a near-full circle with every row still met) |
//! | `W` | **no way-round flip**: a tangency changed side, a signed distance changed sign, or a Parallel, Perpendicular or Angle pair turned round (not by a line of the pair squeezed through no length) |
//! | `L` | a line the drag doesn't hold by an end turned end for end in one frame (not one squeezed through no length at the pointer's pace: see [`TURNED_SHARE`]) |
//! | `G` | **held radii**: an arc or circle the drag doesn't move grew (more than [`HELD`] of its radius as the drag began) |
//! | `J` | **no jump**: a point moved more than [`JUMP`] (50) times as far as the pointer did since the last accepted frame |
//! | `X` | **the stop rule**: a refused frame changed geometry (a drag that can't go on stops at the last shape that went) |
//! | `C` | the release: the commit was refused, or left a relation off |
//! | `P` | out and back: a path that ends where it began, its last frame accepted, left a point more than [`CREEP`] of the part's size from where it started |
//! | `O` | geometry the dragged part isn't joined to moved |
//!
//! Noted, not judged: an arc the drag holds by a point sweeping over 90° in a frame (`own`:
//! its own end taken round), held radii that shrank (`shrank`: a drag that lands may shrink
//! one), a line squeezed through no length (`thru`), and a drag that stopped and went on again
//! (`res`).
//!
//! [`corpus_sweep`] prints one `D` line per drag: the record `scripts/solver-bar/corpus.py`
//! runs, judges against `corpus-baseline.txt` and diffs between two builds.
//!
//! ```text
//! python scripts/solver-bar/corpus.py run --out DIR      # all of it, in parallel
//! cargo test --release -p drift-edit --lib drag_props::corpus::corpus_sweep -- --ignored --exact --nocapture
//! ```
//!
//! `CORPUS_SHARD=k/n` runs every n-th target from the k-th; `CORPUS_ONLY=<sketch>`,
//! `CORPUS_TARGET=<e12>` and `CORPUS_PATH=<name>` replay one, and `CORPUS_TRACE` prints its
//! frames. `DRIFT_CORPUS_PRIVATE=<folder>` adds the `.drift` files there as `private-001`
//! and so on (never by name: the repository is public, and so is every record of them).
//! `CORPUS_PRIVATE_ONLY=1` sweeps those alone.

use std::collections::HashMap;
use std::path::PathBuf;

use drift_core::{Document, EntityKind};
use drift_solver::Definition;

use super::census::env_or;
use super::*;

/// How many times the pointer's move a frame may move a point. On the long paths, whose
/// frames are a fiftieth of the part's size, that is a point thrown the whole size of its part
/// in one frame; on the short ones, a quarter of it. (`small_drag_is_continuous` and the slow
/// drags of `smooth` allow a hundredfold, in frames half to a tenth as long. Geometry that
/// has to swing, a point on a circle whose pointer crosses the centre, measures up to 46
/// here.) With no excuse for a forced swing: a drag that must fling geometry to go on is
/// listed, and the baseline says so.
const JUMP: f64 = 50.0;

/// How much of its radius as the drag began a held arc or circle may differ by and still be
/// kept: far above what a solve leaves a kept shape off by (`drift_solver::FOLD_MET`, 1e-9),
/// far below anything drawn.
const HELD: f64 = 1e-6;

/// How far, as a share of the part's size, a point may be from where it began after a path
/// that ends where it began (the bar of `drag_out_and_back_does_not_creep`).
const CREEP: f64 = 1e-6;

/// The pointer's move a frame, as a share of the part's size (a quarter of it on the short
/// paths).
const FRAME: f64 = 0.02;

/// A line that points the other way after a frame turned end for end if both lengths are over
/// this share of its length as the drag began, and its ends moved apart by more than
/// [`TURNED_PACE`] times the pointer's move. Otherwise it was squeezed through no length at
/// the pointer's own pace, which is the shape going inside out, not a flip.
const TURNED_SHARE: f64 = 0.25;
const TURNED_PACE: f64 = 4.0;

/// The paths each target is dragged along, in the order the baseline lists them.
pub(super) const PATHS: [&str; 10] = [
    "short-e", "short-n", "short-w", "short-s", "long-ne", "long-nw", "long-sw", "long-se",
    "circle", "through",
];

/// The most targets of one private drawing (evenly spread over what it has); the committed
/// corpus is swept whole.
const PRIVATE_TARGETS: usize = 300;

/// A drawing of the corpus.
pub(super) struct Sketch {
    pub name: String,
    pub doc: Document,
    pub private: bool,
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/solver-bar/corpus")
}

fn drift_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|f| Some(f.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "drift"))
        .collect();
    files.sort();
    files
}

/// The committed corpus, by name, and with `private` the drawings `DRIFT_CORPUS_PRIVATE`
/// names, numbered. A private drawing that doesn't load is left out and counted on stderr.
pub(super) fn sketches(private: bool) -> Vec<Sketch> {
    let mut out: Vec<Sketch> = drift_files(&corpus_dir())
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            let (doc, _) =
                drift_io::load(&text).unwrap_or_else(|e| panic!("{name} doesn't load: {e}"));
            Sketch {
                name,
                doc,
                private: false,
            }
        })
        .collect();
    if private && let Some(dir) = std::env::var_os("DRIFT_CORPUS_PRIVATE").filter(|d| !d.is_empty())
    {
        let mut unread = 0;
        for (k, path) in drift_files(std::path::Path::new(&dir))
            .into_iter()
            .enumerate()
        {
            let loaded = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| drift_io::load(&text).ok());
            match loaded {
                Some((doc, _)) => out.push(Sketch {
                    name: format!("private-{:03}", k + 1),
                    doc,
                    private: true,
                }),
                None => unread += 1,
            }
        }
        if unread > 0 {
            eprintln!("{unread} private drawings didn't load");
        }
    }
    out
}

/// A drawing as the sweep drags it: opened as Drift opens one, its relations held.
/// `Err` with what is off where they can't be.
fn opened(doc: &Document) -> Result<Document, String> {
    let mut ed = Editor::new(doc.clone());
    if broken(&ed.doc, ed.lowering().as_deref()).is_some() {
        ed.doc.begin();
        ed.commit("Open");
    }
    match broken(&ed.doc, ed.lowering().as_deref()) {
        Some(what) => Err(what),
        None => Ok(ed.doc),
    }
}

/// What the sweep drags in `doc`: every point that can still move and every curve with one
/// (`true`), and how many it leaves out. Geometry a record remakes (walls' faces, offsets) and
/// locked geometry isn't dragged. Of the geometry nothing relates and no solve touches (a
/// move puts it exactly where it is asked), two points and two curves stand for the rest.
fn targets(doc: &Document, private: bool) -> (Vec<(EntityId, bool)>, usize) {
    let ed = Editor::new(doc.clone());
    let report = drift_solver::analyze_with(doc, ed.lowering().as_deref());
    let free = |id: EntityId| report.definitions.get(&id) == Some(&Definition::Under);
    let locked = doc.locked_entities();
    let made: HashSet<EntityId> = if doc.has_walls() || doc.has_offsets() {
        doc.wall_owners().of.into_keys().collect()
    } else {
        HashSet::new()
    };
    let mut out = Vec::new();
    let mut skipped = 0;
    let mut unsolved = [0usize; 2];
    for (id, e) in doc.iter() {
        if locked.contains(&id) || made.contains(&id) {
            continue;
        }
        let curve = !e.kind.is_point();
        let moves = free(id) || (curve && doc.defining_points([id]).into_iter().any(free));
        if !moves {
            continue;
        }
        let scope = drift_solver::connected(doc, [id]);
        if !drift_solver::needs_solve(doc, &scope) {
            unsolved[usize::from(curve)] += 1;
            if unsolved[usize::from(curve)] > 2 {
                skipped += 1;
                continue;
            }
        }
        out.push((id, curve));
    }
    if private && out.len() > PRIVATE_TARGETS {
        let all = out.len();
        out = (0..PRIVATE_TARGETS)
            .map(|k| out[k * all / PRIVATE_TARGETS])
            .collect();
        skipped += all - PRIVATE_TARGETS;
    }
    (out, skipped)
}

/// The pointer's way along path `k` of [`PATHS`] for a part `size` across, as offsets from
/// where the drag began, one a frame; `toward` is from the dragged geometry to the middle of
/// its part. And whether it ends where it began.
fn pointer_path(k: usize, size: f64, toward: DVec2) -> (Vec<DVec2>, bool) {
    let frame = FRAME * size;
    let leg = |from: DVec2, to: DVec2, out: &mut Vec<DVec2>| {
        let n = (from.distance(to) / frame).ceil().max(1.0) as usize;
        out.extend((1..=n).map(|i| from.lerp(to, i as f64 / n as f64)));
    };
    let at = |degrees: f64, length: f64| {
        let a = degrees.to_radians();
        // (Exact along the axes.)
        match degrees as i32 {
            0 => DVec2::X * length,
            90 => DVec2::Y * length,
            180 => -DVec2::X * length,
            270 => -DVec2::Y * length,
            _ => DVec2::new(a.cos(), a.sin()) * length,
        }
    };
    let mut out = Vec::new();
    match k {
        // Short, along the axes: a twentieth of the part, in ten frames.
        0..=3 => {
            let to = at(90.0 * k as f64, 0.05 * size);
            out.extend((1..=10).map(|i| to * (f64::from(i) / 10.0)));
            (out, false)
        }
        // Long, on the diagonals: half the part.
        4..=7 => {
            leg(
                DVec2::ZERO,
                at(35.0 + 90.0 * (k - 4) as f64, 0.5 * size),
                &mut out,
            );
            (out, false)
        }
        // Out, a full turn about where it began, and back.
        8 => {
            let r = 0.15 * size;
            leg(DVec2::ZERO, DVec2::X * r, &mut out);
            out.extend((1..=48).map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 48.0;
                if i == 48 {
                    DVec2::X * r
                } else {
                    DVec2::new(a.cos(), a.sin()) * r
                }
            }));
            leg(DVec2::X * r, DVec2::ZERO, &mut out);
            (out, true)
        }
        // Through the middle of its part to as far beyond, and back; for geometry at the
        // middle, across the part.
        _ => {
            let to = if toward.length() > 0.05 * size {
                (2.0 * toward).clamp_length_max(size)
            } else {
                at(45.0, 0.5 * size)
            };
            leg(DVec2::ZERO, to, &mut out);
            leg(to, DVec2::ZERO, &mut out);
            (out, true)
        }
    }
}

/// Which way round a relation sits: a side or a sign, or for a pair of lines the turn from
/// one to the other.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Way {
    Side(f64),
    Turn(f64),
}

impl Way {
    fn flipped(self, now: Way) -> bool {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        match (self, now) {
            (Way::Side(a), Way::Side(b)) => a != b,
            (Way::Turn(a), Way::Turn(b)) => ((b - a + PI).rem_euclid(TAU) - PI).abs() > FRAC_PI_2,
            _ => false,
        }
    }
}

/// The way round `c` sits in `doc`, where it has one and it is decided.
fn way_round(doc: &Document, c: &Constraint) -> Option<Way> {
    let ends = |line: EntityId| match doc.get(line)?.kind {
        EntityKind::Line { a, b } => Some((doc.point(a).ok()?, doc.point(b).ok()?)),
        _ => None,
    };
    let dir = |line: EntityId| ends(line).map(|(a, b)| b - a);
    // (A distance may be to a line's midpoint.)
    let at = |id: EntityId| match ends(id) {
        Some((a, b)) => Some((a + b) / 2.0),
        None => doc.point(id).ok(),
    };
    let round = |id: EntityId| match doc.get(id)?.kind {
        EntityKind::Circle { center, radius } => Some((doc.point(center).ok()?, radius)),
        EntityKind::Arc { center, start, .. } => {
            let c = doc.point(center).ok()?;
            Some((c, c.distance(doc.point(start).ok()?)))
        }
        _ => None,
    };
    let side = |v: f64| (v != 0.0 && v.is_finite()).then(|| Way::Side(v.signum()));
    let turn = |a: EntityId, b: EntityId| {
        let (u, v) = (dir(a)?, dir(b)?);
        (u.length() > 0.0 && v.length() > 0.0).then(|| Way::Turn(u.perp_dot(v).atan2(u.dot(v))))
    };
    match *c {
        Constraint::Tangent { a, b } => match (ends(a), ends(b), round(a), round(b)) {
            (Some((p, q)), _, _, Some((c, _))) | (_, Some((p, q)), Some((c, _)), _) => {
                side((q - p).perp_dot(c - p))
            }
            (_, _, Some((c1, r1)), Some((c2, r2))) => {
                let d = c1.distance(c2);
                side((d - (r1 - r2).abs()).abs() - (d - (r1 + r2)).abs())
            }
            _ => None,
        },
        Constraint::HorizontalDistance { a, b, value } if value != 0.0 => side(at(b)?.x - at(a)?.x),
        Constraint::VerticalDistance { a, b, value } if value != 0.0 => side(at(b)?.y - at(a)?.y),
        Constraint::PointLineDistance { point, line, value } if value != 0.0 => {
            let (a, b) = ends(line)?;
            side((b - a).perp_dot(at(point)? - a))
        }
        Constraint::Parallel { a, b }
        | Constraint::Perpendicular { a, b }
        | Constraint::Angle { a, b, .. } => turn(a, b),
        _ => None,
    }
}

/// An arc's counter-clockwise sweep from start to end, in (0, 2π].
fn sweep(c: DVec2, s: DVec2, e: DVec2) -> f64 {
    let (s, e) = (s - c, e - c);
    let turn = s.perp_dot(e).atan2(s.dot(e));
    if turn <= 0.0 {
        turn + std::f64::consts::TAU
    } else {
        turn
    }
}

/// What one drag watches: the dragged part's points, rounds, lines and relations.
struct Watch {
    points: Vec<EntityId>,
    /// Arcs by their points' places in `points` (centre, start, end), and whether the drag
    /// moves one of them.
    arcs: Vec<([usize; 3], bool)>,
    /// Circles, and whether the drag moves their centres.
    circles: Vec<(EntityId, bool)>,
    /// Lines by their ends' places in `points`, and whether the drag moves one.
    lines: Vec<([usize; 2], bool)>,
    /// Each line's place in `lines`.
    line_place: HashMap<EntityId, usize>,
    relations: Vec<Constraint>,
}

/// The watched geometry at one moment.
#[derive(Clone, PartialEq)]
struct State {
    points: Vec<DVec2>,
    radii: Vec<f64>,
    ways: Vec<Option<Way>>,
}

impl Watch {
    fn new(doc: &Document, ids: &[EntityId]) -> Self {
        let scope = drift_solver::connected(doc, ids.iter().copied());
        let mut points: Vec<EntityId> = scope
            .iter()
            .copied()
            .filter(|id| doc.get(*id).is_some_and(|e| e.kind.is_point()))
            .collect();
        points.sort_unstable();
        let place: HashMap<EntityId, usize> =
            points.iter().enumerate().map(|(k, p)| (*p, k)).collect();
        // What the drag moves: its points, and the points merged with them.
        let mut moved: HashSet<EntityId> = doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .collect();
        let joined: Vec<(EntityId, EntityId)> = doc
            .constraints()
            .filter(|(id, _)| !doc.is_driven(*id))
            .filter_map(|(_, c)| match *c {
                Constraint::Coincident { a, b } => Some((a, b)),
                _ => None,
            })
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &joined {
                if moved.contains(&a) != moved.contains(&b) {
                    moved.insert(a);
                    moved.insert(b);
                    changed = true;
                }
            }
        }
        let mut ordered: Vec<EntityId> = scope.iter().copied().collect();
        ordered.sort_unstable();
        let (mut arcs, mut circles, mut lines) = (Vec::new(), Vec::new(), Vec::new());
        let mut line_place = HashMap::new();
        for id in ordered {
            match doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Arc { center, start, end }) => {
                    if let (Some(&c), Some(&s), Some(&e)) =
                        (place.get(&center), place.get(&start), place.get(&end))
                    {
                        let own = [center, start, end].iter().any(|p| moved.contains(p));
                        arcs.push(([c, s, e], own));
                    }
                }
                Some(EntityKind::Circle { center, .. }) => {
                    circles.push((id, moved.contains(&center)));
                }
                Some(EntityKind::Line { a, b }) => {
                    if let (Some(&pa), Some(&pb)) = (place.get(&a), place.get(&b)) {
                        line_place.insert(id, lines.len());
                        lines.push(([pa, pb], moved.contains(&a) || moved.contains(&b)));
                    }
                }
                _ => {}
            }
        }
        let relations = doc
            .constraints()
            .filter(|(id, c)| !doc.is_driven(*id) && c.entities().any(|e| scope.contains(&e)))
            .map(|(_, c)| *c)
            .collect();
        Self {
            points,
            arcs,
            circles,
            lines,
            line_place,
            relations,
        }
    }

    fn state(&self, doc: &Document) -> State {
        State {
            points: self
                .points
                .iter()
                .map(|p| doc.point(*p).unwrap_or(DVec2::NAN))
                .collect(),
            radii: self
                .circles
                .iter()
                .map(|(id, _)| drift_solver::radius_of(doc, *id).unwrap_or(f64::NAN))
                .collect(),
            ways: self.relations.iter().map(|c| way_round(doc, c)).collect(),
        }
    }
}

/// What a drag did, frame against frame: one line of the record.
#[derive(Default)]
struct Outcome {
    frames: usize,
    refused: usize,
    resumes: usize,
    rows: usize,
    sweeps: usize,
    own_sweeps: usize,
    ways: usize,
    reversed: usize,
    through: usize,
    grew: usize,
    shrank: usize,
    jumps: usize,
    worst: f64,
    stop_moved: usize,
    commit: bool,
    creep: f64,
    outside: bool,
    solves: u32,
    exits: [u64; 4],
    end: u64,
    /// The first frame each check failed on, for a trace.
    first: Vec<String>,
}

impl Outcome {
    /// The checks that failed, as letters (see the module's table), or `ok`.
    fn verdict(&self) -> String {
        let mut v = String::new();
        for (failed, letter) in [
            (self.rows > 0, 'R'),
            (self.sweeps > 0, 'S'),
            (self.ways > 0, 'W'),
            (self.reversed > 0, 'L'),
            (self.grew > 0, 'G'),
            (self.jumps > 0, 'J'),
            (self.stop_moved > 0, 'X'),
            (self.commit, 'C'),
            (self.creep > CREEP, 'P'),
            (self.outside, 'O'),
        ] {
            if failed {
                v.push(letter);
            }
        }
        if v.is_empty() { "ok".into() } else { v }
    }
}

/// FNV-1a over the bits of the watched geometry: two builds that end a drag at the same
/// place to the last bit print the same.
fn digest(state: &State) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let values = state
        .points
        .iter()
        .flat_map(|p| [p.x, p.y])
        .chain(state.radii.iter().copied());
    for v in values {
        for byte in v.to_bits().to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Drag `id` of `base` (a curve whole, or a point) along path `k`, judging every frame.
fn sweep_one(base: &Document, id: EntityId, k: usize, trace: bool) -> Outcome {
    let mut out = Outcome::default();
    let mut ed = Editor::new(base.clone());
    let ids = [id];
    let dragged: Vec<Param> = ed
        .doc
        .defining_points(ids)
        .into_iter()
        .flat_map(Param::point)
        .collect();
    let watch = Watch::new(&ed.doc, &ids);
    let lowering = ed.lowering();
    let start = watch.state(&ed.doc);
    // The part's size and middle (the whole drawing's for a part of one place).
    let extent = |points: &[DVec2]| {
        let middle = points.iter().sum::<DVec2>() / points.len().max(1) as f64;
        let size = 2.0
            * points
                .iter()
                .map(|p| p.distance(middle))
                .fold(0.0, f64::max);
        (middle, size)
    };
    let (middle, mut size) = extent(&start.points);
    if size.is_nan() || size <= 0.0 {
        let all: Vec<DVec2> = ed
            .doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Point(p) => Some(p),
                _ => None,
            })
            .collect();
        size = extent(&all).1;
    }
    if size.is_nan() || size <= 0.0 {
        size = 100.0;
    }
    let grabbed: Vec<DVec2> = ed
        .doc
        .defining_points(ids)
        .into_iter()
        .filter_map(|p| ed.doc.point(p).ok())
        .collect();
    let toward = middle - extent(&grabbed).0;
    let (path, closed) = pointer_path(k, size, toward);
    let start_arc_radii: Vec<f64> = watch
        .arcs
        .iter()
        .map(|&([c, s, _], _)| start.points[c].distance(start.points[s]))
        .collect();
    let floor = 1e-9 * size;
    let mut first: Vec<String> = Vec::new();
    let fail = |first: &mut Vec<String>, count: usize, what: String| {
        if count == 1 {
            first.push(what);
        }
    };

    let _ = drift_solver::exits::take();
    ed.doc.begin();
    let mut applied = DVec2::ZERO;
    let mut last = start.clone();
    let mut stopped = false;
    for (n, &target) in path.iter().enumerate() {
        let n = n + 1;
        let went = ed.drag(&ids, target - applied, &dragged);
        out.frames += 1;
        out.solves += ed.drag_solves;
        let now = watch.state(&ed.doc);
        if !went {
            out.refused += 1;
            stopped = true;
            if now != last {
                out.stop_moved += 1;
                fail(
                    &mut first,
                    out.stop_moved,
                    format!("frame {n}: refused, and geometry moved"),
                );
            }
            if trace {
                println!(
                    "  frame {n} to {target:?}: refused ({} solves)",
                    ed.drag_solves
                );
            }
            continue;
        }
        if std::mem::take(&mut stopped) {
            out.resumes += 1;
        }
        let travel = (target - applied).length();
        applied = target;
        if let Some(what) = broken(&ed.doc, lowering.as_deref()) {
            out.rows += 1;
            fail(&mut first, out.rows, format!("frame {n}: {what}"));
        }
        // The farthest any point went, against the pointer.
        let (mut most, mut mover) = (0.0f64, 0);
        for (i, (a, b)) in last.points.iter().zip(&now.points).enumerate() {
            let d = a.distance(*b);
            if d > most || d.is_nan() {
                (most, mover) = (d, i);
            }
        }
        // (Not where the pointer hardly moved: a frame back onto a place a refused frame
        // left it is a move of round-off.)
        if travel > floor {
            out.worst = out.worst.max(most / travel);
        }
        if most.is_nan() || most > JUMP * travel + floor {
            out.jumps += 1;
            fail(
                &mut first,
                out.jumps,
                format!(
                    "frame {n}: {} moved {most:.6}, {:.1} times the pointer",
                    watch.points[mover],
                    most / travel
                ),
            );
        }
        for (i, &([c, s, e], own)) in watch.arcs.iter().enumerate() {
            let was = sweep(last.points[c], last.points[s], last.points[e]);
            let is = sweep(now.points[c], now.points[s], now.points[e]);
            if (is - was).abs() > std::f64::consts::FRAC_PI_2 {
                if own {
                    out.own_sweeps += 1;
                } else {
                    out.sweeps += 1;
                    fail(
                        &mut first,
                        out.sweeps,
                        format!(
                            "frame {n}: an arc swept {:.1}° and now {:.1}°",
                            was.to_degrees(),
                            is.to_degrees()
                        ),
                    );
                }
            }
            if !own {
                let (r0, r) = (start_arc_radii[i], now.points[c].distance(now.points[s]));
                if r - r0 > HELD * r0 {
                    out.grew += 1;
                    fail(
                        &mut first,
                        out.grew,
                        format!("frame {n}: a held arc grew {r0} to {r}"),
                    );
                } else if r0 - r > HELD * r0 {
                    out.shrank += 1;
                }
            }
        }
        for (i, &(circle, moved)) in watch.circles.iter().enumerate() {
            let (r0, r) = (start.radii[i], now.radii[i]);
            if moved {
                continue;
            }
            if r - r0 > HELD * r0 {
                out.grew += 1;
                fail(
                    &mut first,
                    out.grew,
                    format!("frame {n}: held {circle} grew {r0} to {r}"),
                );
            } else if r0 - r > HELD * r0 {
                out.shrank += 1;
            }
        }
        // Each line that points the other way: turned end for end, or squeezed through no
        // length (see `TURNED_SHARE`).
        let mut squeezed = vec![false; watch.lines.len()];
        for (i, &([a, b], moved)) in watch.lines.iter().enumerate() {
            let (was, is) = (
                last.points[b] - last.points[a],
                now.points[b] - now.points[a],
            );
            if was.dot(is).is_nan() || was.dot(is) >= 0.0 {
                continue;
            }
            let began = start.points[a].distance(start.points[b]);
            let turned = was.length().min(is.length()) > TURNED_SHARE * began
                && was.distance(is) > TURNED_PACE * travel;
            if !turned {
                squeezed[i] = true;
                out.through += 1;
            } else if !moved {
                out.reversed += 1;
                fail(
                    &mut first,
                    out.reversed,
                    format!("frame {n}: a line turned end for end, {was:?} to {is:?}"),
                );
            }
        }
        for (i, (was, is)) in last.ways.iter().zip(&now.ways).enumerate() {
            if let (Some(was), Some(is)) = (was, is)
                && was.flipped(*is)
            {
                // (A pair one of whose lines was squeezed through no length turned with it.)
                if matches!(was, Way::Turn(_))
                    && watch.relations[i]
                        .entities()
                        .any(|e| watch.line_place.get(&e).is_some_and(|l| squeezed[*l]))
                {
                    continue;
                }
                out.ways += 1;
                fail(
                    &mut first,
                    out.ways,
                    format!(
                        "frame {n}: {:?} went from {was:?} to {is:?}",
                        watch.relations[i]
                    ),
                );
            }
        }
        if trace {
            println!(
                "  frame {n} to {target:?}: went ({} solves), most {} by {:.3} x the pointer",
                ed.drag_solves,
                watch.points[mover],
                most / travel
            );
        }
        last = now;
    }
    let came_back = closed && applied == DVec2::ZERO;
    let moved = last != start;
    let committed = ed.commit("Move");
    let end = watch.state(&ed.doc);
    // (A commit refused with nothing to keep is no failure: nothing was accepted.)
    out.commit = (!committed && moved) || broken(&ed.doc, ed.lowering().as_deref()).is_some();
    if out.commit {
        first.push(format!(
            "the release: committed {committed}, {:?}",
            broken(&ed.doc, ed.lowering().as_deref())
        ));
    }
    if came_back && committed {
        out.creep = start
            .points
            .iter()
            .zip(&end.points)
            .map(|(a, b)| a.distance(*b))
            .fold(0.0, f64::max)
            / size;
    }
    let scope: HashSet<EntityId> = watch.points.iter().copied().collect();
    out.outside = base.iter().any(|(id, e)| match e.kind {
        EntityKind::Point(p) => !scope.contains(&id) && ed.doc.point(id).ok() != Some(p),
        _ => false,
    });
    out.end = digest(&end);
    let n = drift_solver::exits::take();
    out.exits = n.drag;
    out.first = first;
    out
}

/// One line of the record.
fn record(sketch: &str, id: EntityId, curve: bool, k: usize, o: &Outcome) -> String {
    format!(
        "D {sketch} {id}{} {} {} frames {} ref {} res {} rows {} sweep {} own {} way {} rev {} \
         thru {} grew {} shrank {} jumps {} worst {:.3e} stop {} creep {:.3e} solves {} ex {} \
         end {:016x}",
        if curve { "c" } else { "p" },
        PATHS[k],
        o.verdict(),
        o.frames,
        o.refused,
        o.resumes,
        o.rows,
        o.sweeps,
        o.own_sweeps,
        o.ways,
        o.reversed,
        o.through,
        o.grew,
        o.shrank,
        o.jumps,
        o.worst,
        o.stop_moved,
        o.creep,
        o.solves,
        o.exits.map(|n| n.to_string()).join(","),
        o.end,
    )
}

/// The sweep: every target of every sketch along every path, one `D` line a drag, and one
/// `H` line a sketch (its size and what is dragged). See the module's notes for the
/// environment.
#[test]
#[ignore = "the corpus sweep: run in release with --ignored --exact --nocapture (scripts/solver-bar/corpus.py)"]
fn corpus_sweep() {
    let (shard, of) = std::env::var("CORPUS_SHARD")
        .ok()
        .and_then(|v| {
            let (k, n) = v.split_once('/')?;
            Some((k.parse::<usize>().ok()?, n.parse::<usize>().ok()?))
        })
        .unwrap_or((0, 1));
    let only = std::env::var("CORPUS_ONLY").ok();
    let only_target = std::env::var("CORPUS_TARGET").ok();
    let only_path = std::env::var("CORPUS_PATH").ok();
    let trace = std::env::var_os("CORPUS_TRACE").is_some();
    let private: usize = env_or("CORPUS_PRIVATE_ONLY", 0);
    let mut unit = 0;
    // (The harness has printed the test's name with no line end.)
    println!();
    for sketch in sketches(true) {
        if only.as_ref().is_some_and(|o| *o != sketch.name) || (private > 0 && !sketch.private) {
            continue;
        }
        let doc = match opened(&sketch.doc) {
            Ok(doc) => doc,
            Err(what) => {
                if shard == 0 {
                    println!(
                        "H {} LOAD {}",
                        sketch.name,
                        if sketch.private { "" } else { &what }
                    );
                }
                continue;
            }
        };
        let (targets, skipped) = targets(&doc, sketch.private);
        if shard == 0 {
            println!(
                "H {} entities {} relations {} targets {} skipped {skipped}",
                sketch.name,
                doc.iter().count(),
                doc.constraints().count(),
                targets.len(),
            );
        }
        for (id, curve) in targets {
            unit += 1;
            if (unit - 1) % of != shard
                || only_target.as_ref().is_some_and(|t| *t != id.to_string())
            {
                continue;
            }
            for (k, path) in PATHS.iter().enumerate() {
                if only_path.as_ref().is_some_and(|p| p != path) {
                    continue;
                }
                let outcome = sweep_one(&doc, id, k, trace);
                println!("{}", record(&sketch.name, id, curve, k, &outcome));
                if trace {
                    for what in &outcome.first {
                        println!("  {what}");
                    }
                }
            }
        }
    }
}

/// The committed corpus is fit to sweep: at least 40 drawings, each opening with its relations
/// met and something to drag; and one short drag of each drawing's first target breaks none of
/// the hard rules (rows met, a refused frame changes nothing, the release keeps the relations,
/// nothing outside the part moves).
#[test]
fn the_corpus_opens_and_drags() {
    let all = sketches(false);
    assert!(all.len() >= 40, "{} sketches", all.len());
    for sketch in all {
        let doc = opened(&sketch.doc).unwrap_or_else(|e| panic!("{}: {e}", sketch.name));
        let (targets, _) = targets(&doc, false);
        assert!(!targets.is_empty(), "{}: nothing to drag", sketch.name);
        let (id, _) = targets[0];
        let o = sweep_one(&doc, id, 0, false);
        assert!(
            o.rows == 0 && o.stop_moved == 0 && !o.commit && !o.outside,
            "{} {id}: {} ({:?})",
            sketch.name,
            o.verdict(),
            o.first
        );
    }
}
