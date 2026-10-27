//! Running a script: each statement lowered to tool calls, run in order, with the ids they
//! return bound to the script's names.

use std::collections::{BTreeSet, HashMap};

use drift_core::{Constraint, DVec2, Document, EntityId, EntityKind, Kind as VKind};
use serde_json::{Map, Value, json};

use super::errors::ScriptError;
use super::names::is_entity_id;
use super::parse::{
    Along, ArrayKind, Axis, DimKind, Dir, Expr, OpeningKind, Part, Path, Place, Pos, Rel, Side,
    Spacing, Statement, Stmt, VarKind, WallSide,
};
use crate::agent::Session;

type R<T> = Result<T, String>;

/// One face of a wall: the line pieces it's cut into by openings, between its two ends.
#[derive(Clone, Debug)]
struct Face {
    pieces: Vec<EntityId>,
    start: EntityId,
    end: EntityId,
    /// The wall's other face.
    partner: Option<usize>,
}

/// What a name stands for. Faces are indices into [`Runner::faces`].
#[derive(Clone, Debug)]
enum Obj {
    Point(EntityId),
    Line(EntityId, EntityId, EntityId),
    Poly {
        lines: Vec<EntityId>,
        points: Vec<EntityId>,
    },
    /// A rectangle or a room: sides south, east, north, west; corners sw, se, ne, nw.
    Rect {
        faces: [usize; 4],
        corners: [EntityId; 4],
        outer: Option<([usize; 4], [EntityId; 4])>,
    },
    Wall {
        faces: [usize; 2],
        axis: Option<EntityId>,
        caps: [EntityId; 2],
    },
    Opening([EntityId; 2]),
    Grid(Vec<(String, EntityId)>),
    Circle(EntityId, EntityId),
    Arc {
        id: EntityId,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// Each copy's curves; the first is the original.
    Array(Vec<Vec<EntityId>>),
    Used(Vec<EntityId>),
}

/// What a name with its parts refers to.
#[derive(Clone, Debug)]
enum Target {
    Point(EntityId),
    Line(EntityId, EntityId, EntityId),
    Face(usize),
    /// A circle or arc and its centre.
    Curve(EntityId, EntityId),
    /// A room's or rectangle's inside, by its corners.
    Area(Vec<EntityId>),
    /// The band between faces and their partners (a room's or wall's walls).
    Band(Vec<usize>),
    Group(Vec<EntityId>),
}

pub(super) struct Runner<'s> {
    s: &'s mut Session,
    objs: HashMap<String, Obj>,
    faces: Vec<Face>,
    /// Where each name was made.
    lines_of: HashMap<String, usize>,
    /// The tool calls made, as a batch takes them.
    pub calls: Vec<Value>,
    pub did: Vec<Value>,
    pub skipped: Vec<Value>,
    /// Things that ran but look like mistakes (a door a few millimetres wide).
    pub warnings: Vec<Value>,
    rooms: Vec<String>,
    image: Option<Vec<u8>>,
    /// Each statement's source, by line, for errors that involve earlier lines.
    pub sources: HashMap<usize, String>,
    /// The inside of each name as it was before a `trim` first cut it, so that a shape
    /// trimmed open (the first half of a union) can still be trimmed by.
    trim_areas: HashMap<String, Value>,
    /// The curves trims cut off a name's curves: they are that name's too.
    trimmed_into: HashMap<String, Vec<EntityId>>,
}

fn eid(v: &Value) -> R<EntityId> {
    v.as_str()
        .and_then(|s| s.strip_prefix('e'))
        .and_then(|n| n.parse().ok())
        .map(EntityId)
        .ok_or_else(|| format!("expected an id, got {v}"))
}

fn ids(v: &Value) -> R<Vec<EntityId>> {
    v.as_array()
        .ok_or_else(|| format!("expected ids, got {v}"))?
        .iter()
        .map(eid)
        .collect()
}

fn s(id: EntityId) -> Value {
    json!(id.to_string())
}

fn xy(p: DVec2) -> Value {
    json!([p.x, p.y])
}

/// Every name a statement refers to.
fn paths_of(st: &Stmt) -> Vec<&Path> {
    fn pos(p: &Pos) -> Option<&Path> {
        match p {
            Pos::Ref(path) => Some(path),
            Pos::At(..) => None,
        }
    }
    let mut out: Vec<&Path> = Vec::new();
    match st {
        Stmt::Point { at, .. } => out.extend(pos(at)),
        Stmt::Line { from, to, .. } | Stmt::Wall { from, to, .. } => {
            out.extend(pos(from));
            out.extend(pos(to));
        }
        Stmt::Polyline { points, .. } => out.extend(points.iter().filter_map(pos)),
        Stmt::Rect { at, .. } | Stmt::Circle { at, .. } | Stmt::Text { at, .. } => {
            out.extend(pos(at))
        }
        Stmt::Arc {
            centre, from, to, ..
        } => out.extend([centre, from, to].into_iter().filter_map(pos)),
        Stmt::Room { place, .. } => match place {
            Place::At(p) => out.extend(pos(p)),
            Place::Beside(_, o) => out.push(o),
        },
        Stmt::Opening { host, along, .. } => {
            out.push(host);
            if let Along::Offset(_, from) = along {
                out.push(from);
            }
        }
        Stmt::Grid { at, .. } => out.extend(at.as_ref().and_then(pos)),
        Stmt::Array { of, .. } => out.push(of),
        Stmt::Relation { args, .. } => out.extend(args),
        Stmt::Dim { a, b, .. } => {
            out.push(a);
            out.extend(b);
        }
        Stmt::Label { of, .. } | Stmt::Hatch { of, .. } | Stmt::Fill { of, .. } => out.push(of),
        Stmt::Use { ids, .. } => out.extend(ids),
        Stmt::Trim { what, by, .. } => {
            out.push(what);
            out.push(by);
        }
        Stmt::Let { .. } | Stmt::Layer { .. } | Stmt::On(_) => {}
    }
    out
}

fn pos_text(p: &Pos) -> String {
    match p {
        Pos::At(x, y) => format!("({}, {})", x.text, y.text),
        Pos::Ref(path) => path.to_string(),
    }
}

/// How a statement reads, in plain words.
pub(super) fn meant(st: &Stmt) -> String {
    let opt = |e: &Option<Expr>, what: &str| {
        e.as_ref()
            .map(|e| format!(", {what} {}", e.text))
            .unwrap_or_default()
    };
    match st {
        Stmt::Let { name, value, .. } => format!("set the variable {name} to {}", value.text),
        Stmt::Layer { name, .. } => format!("make or change the layer {name}"),
        Stmt::On(l) => format!("put new geometry on the layer {l}"),
        Stmt::Point { name, at, fixed } => format!(
            "a point {name} at {}{}",
            pos_text(at),
            if *fixed { ", fixed" } else { "" }
        ),
        Stmt::Line {
            name,
            from,
            to,
            length,
            ..
        } => format!(
            "a line {name} from {} to {}{}",
            pos_text(from),
            pos_text(to),
            opt(length, "length")
        ),
        Stmt::Polyline {
            name,
            points,
            closed,
            ..
        } => format!(
            "a{} polyline {name} through {} points",
            if *closed { " closed" } else { "n open" },
            points.len()
        ),
        Stmt::Rect {
            name,
            w,
            h,
            at,
            centred,
            about,
            ..
        } => format!(
            "a rectangle {name}, {}{} wide and {} high, with its {} at {}",
            if *about { "about " } else { "" },
            w.text,
            h.text,
            if *centred {
                "centre"
            } else {
                "lower-left corner"
            },
            pos_text(at)
        ),
        Stmt::Circle {
            name,
            size,
            diameter,
            at,
            ..
        } => format!(
            "a circle {name} of {} {} centred at {}",
            if *diameter { "diameter" } else { "radius" },
            size.text,
            pos_text(at)
        ),
        Stmt::Arc {
            name,
            centre,
            from,
            to,
            ..
        } => format!(
            "an arc {name} about {} from {} to {}, counter-clockwise",
            pos_text(centre),
            pos_text(from),
            pos_text(to)
        ),
        Stmt::Room {
            name,
            w,
            h,
            place,
            walls,
            ..
        } => {
            let place = match place {
                Place::At(p) => format!("with its inside lower-left corner at {}", pos_text(p)),
                Place::Beside(side, o) => format!(
                    "{} {o}, sharing the wall between them",
                    match side {
                        Side::Right => "right of",
                        Side::Left => "left of",
                        Side::Above => "above",
                        Side::Below => "below",
                    }
                ),
            };
            let walls = match walls {
                Some((t, side)) => format!(
                    ", walls {} thick {}",
                    t.text,
                    match side {
                        WallSide::Inside => "inside its outline",
                        WallSide::Centre => "centred on its outline",
                        _ => "outside it",
                    }
                ),
                None => String::new(),
            };
            format!(
                "a room {name}, {} × {} inside, {place}{walls}",
                w.text, h.text
            )
        }
        Stmt::Wall {
            name,
            from,
            to,
            thick,
            side,
            ..
        } => format!(
            "a wall {name}, {} thick, from {} to {}, {}",
            thick.text,
            pos_text(from),
            pos_text(to),
            match side {
                WallSide::Left => "on the left of that line",
                WallSide::Right => "on the right of that line",
                _ => "centred on that line",
            }
        ),
        Stmt::Opening {
            kind,
            name,
            width,
            host,
            along,
        } => format!(
            "a {} {name} {} wide in {host}, {}",
            match kind {
                OpeningKind::Door => "door",
                OpeningKind::Window => "window",
                OpeningKind::Opening => "opening",
            },
            width.text,
            match along {
                Along::Centred => "centred".to_owned(),
                Along::Offset(by, from) => format!("{} from {from}", by.text),
            }
        ),
        Stmt::Grid {
            name, axis, labels, ..
        } => format!(
            "{} grid lines {} of grid {name}",
            if *axis == Axis::Cols {
                "vertical"
            } else {
                "horizontal"
            },
            labels.join(", ")
        ),
        Stmt::Array { name, of, kind } => match kind {
            ArrayKind::Grid { first, second, up } => {
                let mut t = format!(
                    "copies {name} of {of}: {} at {} {}",
                    first.0.text,
                    first.1.text,
                    if *up { "upwards" } else { "along x" }
                );
                if let Some((c, sp)) = second {
                    t.push_str(&format!(", in {} rows {} apart", c.text, sp.text));
                }
                t
            }
            ArrayKind::By(c, by) => format!(
                "copies {name} of {of}: {} in all, each moved by {}",
                c.text,
                pos_text(by)
            ),
            ArrayKind::Around(c, about, _) => format!(
                "copies {name} of {of}: {} in all around {}",
                c.text,
                pos_text(about)
            ),
        },
        Stmt::Relation { rel, args, .. } => {
            let names: Vec<String> = args.iter().map(ToString::to_string).collect();
            match rel {
                Rel::Symmetric => format!(
                    "make {} and {} symmetric about {}",
                    names[0], names[1], names[2]
                ),
                Rel::Midpoint => format!("put {} at the middle of {}", names[0], names[1]),
                Rel::On => format!("put {} on {}", names[0], names[1]),
                Rel::Fix => format!("pin {} in place", names[0]),
                _ => format!(
                    "make {} {}",
                    names.join(" and "),
                    format!("{rel:?}").to_lowercase()
                ),
            }
        }
        Stmt::Dim {
            kind,
            a,
            b,
            dir,
            value,
            reference,
        } => {
            let what = match (kind, b) {
                (DimKind::Radius, _) => format!("the radius of {a}"),
                (DimKind::Diameter, _) => format!("the diameter of {a}"),
                (DimKind::Angle, Some(b)) => format!("the angle from {a} to {b}"),
                (_, Some(b)) => format!(
                    "the {}distance from {a} to {b}",
                    match dir {
                        Some(Dir::Horizontal) => "horizontal ",
                        Some(Dir::Vertical) => "vertical ",
                        None => "",
                    }
                ),
                (_, None) => format!("the length of {a}"),
            };
            match (value, reference) {
                (_, true) => format!("show {what} (a reference dimension)"),
                (Some(v), false) => format!("make {what} {}", v.text),
                (None, false) => what,
            }
        }
        Stmt::Label { of, text, .. } => format!("label {of} \"{text}\""),
        Stmt::Text { text, at, .. } => format!("the text \"{text}\" at {}", pos_text(at)),
        Stmt::Hatch { of, pattern, .. } => {
            format!("hatch {of} with {}", pattern.as_deref().unwrap_or("ANSI31"))
        }
        Stmt::Fill { of, .. } => format!("fill {of} solid"),
        Stmt::Trim { what, inside, by } => format!(
            "remove what of {what} lies {} {by}",
            if *inside { "inside" } else { "outside" }
        ),
        Stmt::Use { ids, name } => format!(
            "call {} `{name}`",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The foot of `p` on the line through `a` and `b`.
fn foot(p: DVec2, a: DVec2, b: DVec2) -> DVec2 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 == 0.0 {
        return a;
    }
    a + d * ((p - a).dot(d) / len2)
}

impl<'s> Runner<'s> {
    pub fn new(s: &'s mut Session) -> Self {
        Self {
            s,
            objs: HashMap::new(),
            faces: Vec::new(),
            lines_of: HashMap::new(),
            calls: Vec::new(),
            did: Vec::new(),
            skipped: Vec::new(),
            warnings: Vec::new(),
            rooms: Vec::new(),
            image: None,
            sources: HashMap::new(),
            trim_areas: HashMap::new(),
            trimmed_into: HashMap::new(),
        }
    }

    fn doc(&self) -> &Document {
        &self.s.editor.doc
    }

    fn at(&self, id: EntityId) -> DVec2 {
        self.doc().point(id).unwrap_or_default()
    }

    fn ends(&self, line: EntityId) -> R<(EntityId, EntityId)> {
        match self.doc().get(line).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => Ok((a, b)),
            _ => Err(format!("{line} isn't a line")),
        }
    }

    /// Run one tool, as part of the script.
    fn exec(&mut self, tool: &str, args: Value) -> R<Value> {
        self.calls.push(json!({ "tool": tool, "args": args }));
        let out = self.s.run_call(tool, &args, &mut self.image);
        out.map_err(|e| e.trim_end_matches(" (nothing was changed)").to_owned())
    }

    fn relations(&mut self, line: usize, rels: Vec<Value>) -> R<()> {
        if rels.is_empty() {
            return Ok(());
        }
        let v = self.exec("add_relations", json!({ "relations": rels }))?;
        if let Some(sk) = v.get("skipped").and_then(Value::as_array)
            && !sk.is_empty()
        {
            self.skipped.push(
                json!({ "line": line, "why": format!("{} relation(s) already implied", sk.len()) }),
            );
        }
        Ok(())
    }

    fn dims(&mut self, dims: Vec<Value>) -> R<Value> {
        self.exec("add_dimensions", json!({ "dimensions": dims }))
    }

    /// A value's text as written or, when that doesn't evaluate, with plain numbers that are
    /// added to or taken from something read as millimetres (`2400 + T`).
    fn text_of(&self, e: &Expr) -> String {
        let ok = |t: &str| {
            [VKind::Length, VKind::Angle, VKind::Number]
                .into_iter()
                .any(|k| self.s.editor.evaluate_text(t, k).is_ok())
        };
        if ok(&e.text) {
            return e.text.clone();
        }
        match super::lex::with_unit(&e.text, self.unit().symbol()) {
            Some(t) if ok(&t) => t,
            _ => e.text.clone(),
        }
    }

    /// The drawing's unit of length: bare numbers are in it, as when typed into Drift.
    pub(super) fn unit(&self) -> drift_core::LengthFormat {
        self.doc().units().length
    }

    fn eval(&self, e: &Expr, kind: VKind) -> R<f64> {
        if let Some(x) = e.plain() {
            return Ok(if kind == VKind::Length {
                x * self.unit().mm_per_unit()
            } else {
                x
            });
        }
        self.s
            .editor
            .evaluate_text(&self.text_of(e), kind)
            .map(|v| v.value)
            .map_err(|err| format!("`{}`: {err}", e.text))
    }

    fn len(&self, e: &Expr) -> R<f64> {
        self.eval(e, VKind::Length)
    }

    fn count(&self, e: &Expr) -> R<usize> {
        let n = match e.plain() {
            Some(n) => n,
            None => self
                .eval(e, VKind::Number)
                .or_else(|_| self.eval(e, VKind::Length))?,
        };
        if (1.0..=10_000.0).contains(&n) && (n - n.round()).abs() < 1e-9 {
            Ok(n.round() as usize)
        } else {
            Err(format!(
                "`{}` isn't a count (a whole number from 1)",
                e.text
            ))
        }
    }

    /// A value for a dimension: a number in mm, or the text as typed (so it stays linked).
    fn val(&self, e: &Expr) -> Value {
        self.val_as(e, VKind::Length)
    }

    /// A value for a tool: a bare number in mm (lengths, from the drawing's unit) or degrees,
    /// or else the text as typed.
    fn val_as(&self, e: &Expr, kind: VKind) -> Value {
        match e.plain() {
            Some(x) if kind == VKind::Length => json!(x * self.unit().mm_per_unit()),
            Some(x) => json!(x),
            None => json!(self.text_of(e)),
        }
    }

    fn xy(&self, p: &Pos) -> R<DVec2> {
        match p {
            Pos::At(x, y) => Ok(DVec2::new(self.len(x)?, self.len(y)?)),
            Pos::Ref(path) => Ok(self.at(self.point(path)?)),
        }
    }

    /// A position for a tool: coordinates, or a point's id (so shapes join).
    fn pos_arg(&self, p: &Pos) -> R<Value> {
        match p {
            Pos::At(..) => Ok(xy(self.xy(p)?)),
            Pos::Ref(path) => Ok(s(self.point(path)?)),
        }
    }

    fn face_part(&self, f: usize, rest: &[Part], path: &Path) -> R<Target> {
        let face = &self.faces[f];
        let piece = |k: usize| -> R<EntityId> {
            face.pieces.get(k - 1).copied().ok_or_else(|| {
                format!(
                    "{path}: that face is in {} piece(s), so [{k}] is past its end",
                    face.pieces.len()
                )
            })
        };
        let line = |id: EntityId| -> R<Target> {
            let (a, b) = self.ends(id)?;
            Ok(Target::Line(id, a, b))
        };
        match rest {
            [] => Ok(Target::Face(f)),
            [Part::Index(k)] => line(piece(*k)?),
            [Part::Field(e)] => Ok(Target::Point(if e == "start" {
                face.start
            } else {
                face.end
            })),
            [Part::Index(k), Part::Field(e)] => {
                let (a, b) = self.ends(piece(*k)?)?;
                Ok(Target::Point(if e == "start" { a } else { b }))
            }
            _ => Err(format!("{path}: can't find that part")),
        }
    }

    fn side_index(f: &str) -> Option<usize> {
        match f {
            "south" | "bottom" | "width" => Some(0),
            "east" | "right" => Some(1),
            "north" | "top" => Some(2),
            "west" | "left" | "depth" | "height" => Some(3),
            _ => None,
        }
    }

    fn corner_index(f: &str) -> Option<usize> {
        ["sw", "se", "ne", "nw"].iter().position(|c| *c == f)
    }

    fn entity_target(&self, id: EntityId) -> R<Target> {
        Ok(match self.doc().get(id).map(|e| e.kind) {
            Some(EntityKind::Point(_)) => Target::Point(id),
            Some(EntityKind::Line { a, b }) => Target::Line(id, a, b),
            Some(EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. }) => {
                Target::Curve(id, center)
            }
            Some(_) => Target::Group(vec![id]),
            None => return Err(format!("{id} doesn't exist")),
        })
    }

    fn obj_curves(&self, obj: &Obj) -> Vec<EntityId> {
        let face = |f: usize| self.faces[f].pieces.clone();
        match obj {
            Obj::Point(p) => vec![*p],
            Obj::Line(id, ..) | Obj::Circle(id, _) | Obj::Arc { id, .. } => vec![*id],
            Obj::Poly { lines, .. } => lines.clone(),
            Obj::Rect { faces, outer, .. } => {
                let mut v: Vec<EntityId> = faces.iter().flat_map(|f| face(*f)).collect();
                if let Some((o, _)) = outer {
                    v.extend(o.iter().flat_map(|f| face(*f)));
                }
                let set: BTreeSet<EntityId> = v.into_iter().collect();
                set.into_iter().collect()
            }
            Obj::Wall { faces, axis, caps } => {
                let mut v: Vec<EntityId> = faces.iter().flat_map(|f| face(*f)).collect();
                v.extend(caps);
                v.extend(axis);
                v
            }
            Obj::Opening(j) => j.to_vec(),
            Obj::Grid(axes) => axes.iter().map(|(_, l)| *l).collect(),
            Obj::Array(copies) => copies.concat(),
            Obj::Used(ids) => ids.clone(),
        }
    }

    fn resolve(&self, path: &Path) -> R<Target> {
        let Some(obj) = self.objs.get(&path.root) else {
            if is_entity_id(&path.root) {
                let id = EntityId(
                    path.root[1..]
                        .parse()
                        .map_err(|_| format!("bad id {}", path.root))?,
                );
                return self.entity_target(id);
            }
            return Err(format!("`{}` isn't defined", path.root));
        };
        let parts = path.parts.as_slice();
        fn field(p: Option<&Part>) -> Option<&str> {
            match p {
                Some(Part::Field(f)) => Some(f.as_str()),
                _ => None,
            }
        }
        let bad = || format!("{path}: no such part");
        Ok(match (obj, parts) {
            (Obj::Point(p), []) => Target::Point(*p),
            (Obj::Line(id, a, b), []) => Target::Line(*id, *a, *b),
            (Obj::Line(_, a, b), [Part::Field(f)]) => {
                Target::Point(if f == "start" { *a } else { *b })
            }
            (Obj::Poly { points, .. }, [Part::Field(f)]) if f == "inside" => {
                Target::Area(points.clone())
            }
            (Obj::Poly { points, .. }, [Part::Field(f)]) => Target::Point(if f == "start" {
                points[0]
            } else {
                *points.last().unwrap_or(&points[0])
            }),
            (Obj::Poly { lines, .. }, [Part::Index(k)]) => {
                let id = *lines.get(k - 1).ok_or_else(bad)?;
                let (a, b) = self.ends(id)?;
                Target::Line(id, a, b)
            }
            (Obj::Rect { corners, .. }, []) => Target::Area(corners.to_vec()),
            (
                Obj::Rect {
                    faces,
                    corners,
                    outer,
                    ..
                },
                [first, rest @ ..],
            ) => {
                let f = field(Some(first)).ok_or_else(bad)?;
                if let Some(i) = Self::side_index(f) {
                    return self.face_part(faces[i], rest, path);
                }
                if let Some(i) = Self::corner_index(f) {
                    return Ok(Target::Point(corners[i]));
                }
                match (f, outer) {
                    ("inside", _) => Target::Area(corners.to_vec()),
                    ("walls", Some(_)) => Target::Band(faces.to_vec()),
                    ("outer", Some((of, oc))) => {
                        let g = field(rest.first()).ok_or_else(bad)?;
                        if let Some(i) = Self::side_index(g) {
                            return self.face_part(of[i], &rest[1..], path);
                        }
                        Target::Point(oc[Self::corner_index(g).ok_or_else(bad)?])
                    }
                    _ => return Err(bad()),
                }
            }
            (Obj::Wall { faces, .. }, []) => Target::Band(vec![faces[0]]),
            (Obj::Wall { faces, axis, caps }, [first, rest @ ..]) => {
                match field(Some(first)).ok_or_else(bad)? {
                    "left" => return self.face_part(faces[0], rest, path),
                    "right" => return self.face_part(faces[1], rest, path),
                    "start" => return self.entity_target(caps[0]),
                    "end" => return self.entity_target(caps[1]),
                    "axis" => return self.entity_target(axis.ok_or_else(bad)?),
                    _ => return Err(bad()),
                }
            }
            (Obj::Opening(j), [Part::Field(f)]) => {
                return self.entity_target(if f == "start" { j[0] } else { j[1] });
            }
            (Obj::Opening(j), []) => Target::Group(j.to_vec()),
            (Obj::Grid(axes), [Part::Field(f)]) => {
                let id = axes
                    .iter()
                    .find(|(l, _)| l == f)
                    .map(|(_, id)| *id)
                    .ok_or_else(bad)?;
                return self.entity_target(id);
            }
            (Obj::Grid(axes), []) => Target::Group(axes.iter().map(|(_, l)| *l).collect()),
            (Obj::Circle(id, c), []) => Target::Curve(*id, *c),
            (Obj::Circle(_, c), [_]) => Target::Point(*c),
            (Obj::Arc { id, center, .. }, []) => Target::Curve(*id, *center),
            (
                Obj::Arc {
                    center, start, end, ..
                },
                [Part::Field(f)],
            ) => Target::Point(match f.as_str() {
                "start" => *start,
                "end" => *end,
                _ => *center,
            }),
            (Obj::Array(copies), []) => Target::Group(copies.concat()),
            (Obj::Array(copies), [Part::Index(k), rest @ ..]) => {
                let copy = copies
                    .get(k - 1)
                    .ok_or_else(|| format!("{path}: {} has {} copies", path.root, copies.len()))?;
                match (copy.as_slice(), rest) {
                    ([one], []) => return self.entity_target(*one),
                    ([one], [Part::Field(f)]) => match (self.entity_target(*one)?, f.as_str()) {
                        (Target::Curve(_, c), "center" | "centre") => Target::Point(c),
                        (Target::Line(_, a, b), "start" | "end") => {
                            Target::Point(if f == "start" { a } else { b })
                        }
                        _ => return Err(format!("{path}: that copy has no part `{f}`")),
                    },
                    (many, []) => Target::Group(many.to_vec()),
                    _ => return Err(format!("{path}: copies of several curves have no parts")),
                }
            }
            (Obj::Used(ids), []) if ids.len() == 1 => return self.entity_target(ids[0]),
            (Obj::Used(ids), []) => Target::Group(ids.clone()),
            (Obj::Used(ids), [Part::Index(k)]) => {
                return self.entity_target(*ids.get(k - 1).ok_or_else(bad)?);
            }
            _ => return Err(bad()),
        })
    }

    fn kind_words(t: &Target) -> &'static str {
        match t {
            Target::Point(_) => "a point",
            Target::Line(..) | Target::Face(_) => "a line",
            Target::Curve(..) => "a circle or arc",
            Target::Area(_) => "an area",
            Target::Band(_) => "a set of walls",
            Target::Group(_) => "a group of curves",
        }
    }

    fn point(&self, path: &Path) -> R<EntityId> {
        match self.resolve(path)? {
            Target::Point(p) => Ok(p),
            t => Err(format!(
                "{path} is {}, not a point{}",
                Self::kind_words(&t),
                match t {
                    Target::Line(..) | Target::Face(_) =>
                        format!(" (try {path}.start or {path}.end)"),
                    Target::Curve(..) => format!(" (try {path}.center)"),
                    _ => String::new(),
                }
            )),
        }
    }

    /// A line: its id and its ends (a face: its first piece, between the face's ends).
    fn line(&self, path: &Path) -> R<(EntityId, EntityId, EntityId)> {
        match self.resolve(path)? {
            Target::Line(id, a, b) => Ok((id, a, b)),
            Target::Face(f) => {
                let face = &self.faces[f];
                Ok((face.pieces[0], face.start, face.end))
            }
            t => Err(format!("{path} is {}, not a line", Self::kind_words(&t))),
        }
    }

    /// A single entity: a point, line or curve.
    fn entity(&self, path: &Path) -> R<EntityId> {
        match self.resolve(path)? {
            Target::Point(id) | Target::Line(id, ..) | Target::Curve(id, _) => Ok(id),
            Target::Face(f) => Ok(self.faces[f].pieces[0]),
            Target::Group(g) if g.len() == 1 => Ok(g[0]),
            t => Err(format!(
                "{path} is {}; name one of its parts",
                Self::kind_words(&t)
            )),
        }
    }

    fn curves(&self, path: &Path) -> R<Vec<EntityId>> {
        if path.parts.is_empty()
            && let Some(obj) = self.objs.get(&path.root)
        {
            return Ok(self.obj_curves(obj));
        }
        Ok(match self.resolve(path)? {
            Target::Point(id) | Target::Line(id, ..) | Target::Curve(id, _) => vec![id],
            Target::Face(f) => self.faces[f].pieces.clone(),
            Target::Group(g) => g,
            t => {
                return Err(format!(
                    "{path} is {}; name its curves",
                    Self::kind_words(&t)
                ));
            }
        })
    }

    fn define(&mut self, name: &str, line: usize, obj: Obj) {
        self.objs.insert(name.to_owned(), obj);
        self.lines_of.insert(name.to_owned(), line);
    }

    fn face(&mut self, line: EntityId, start: EntityId, end: EntityId) -> usize {
        self.faces.push(Face {
            pieces: vec![line],
            start,
            end,
            partner: None,
        });
        self.faces.len() - 1
    }

    fn pair(&mut self, a: usize, b: usize) {
        self.faces[a].partner = Some(b);
        self.faces[b].partner = Some(a);
    }

    fn fix(&mut self, line: usize, p: EntityId) -> R<()> {
        self.relations(line, vec![json!({ "type": "fix", "point": s(p) })])
    }

    /// Run a statement. Errors are plain messages; the caller adds the line.
    pub fn statement(&mut self, st: &Statement) -> R<()> {
        let previous = if let Some(l) = &st.layer {
            let current = self.doc().current_layer();
            self.exec("layers", json!({ "current": l }))?;
            Some(current)
        } else {
            None
        };
        self.lower(st)?;
        if let Some(prev) = previous {
            self.exec("layers", json!({ "current": prev.to_string() }))?;
        }
        Ok(())
    }

    fn lower(&mut self, st: &Statement) -> R<()> {
        let ln = st.line;
        match &st.stmt {
            Stmt::Let { name, value, kind } => {
                let existing = self
                    .doc()
                    .variable_named(name)
                    .and_then(|id| self.doc().variable(id))
                    .map(|v| v.kind);
                let kind = match kind {
                    Some(k) => Some(*k),
                    None if existing.is_some() => None,
                    None => Some(self.infer_kind(value)),
                };
                let as_kind = match kind.or(match existing {
                    Some(VKind::Angle) => Some(VarKind::Angle),
                    Some(VKind::Number) => Some(VarKind::Number),
                    _ => None,
                }) {
                    Some(VarKind::Angle) => VKind::Angle,
                    Some(VarKind::Number) => VKind::Number,
                    _ => VKind::Length,
                };
                let mut item = json!({ "name": name, "value": self.val_as(value, as_kind) });
                if let Some(k) = kind {
                    item["kind"] = json!(match k {
                        VarKind::Length => "length",
                        VarKind::Angle => "angle",
                        VarKind::Number => "number",
                    });
                }
                self.exec("set_variables", json!({ "set": [item] }))?;
            }
            Stmt::Layer {
                name,
                color,
                weight,
                line,
                hidden,
                locked,
            } => {
                let mut props = json!({ "name": name, "visible": !hidden, "locked": locked });
                if let Some(c) = color {
                    props["color"] = json!(c);
                }
                if let Some(w) = weight {
                    props["weight_mm"] = json!(self.eval(w, VKind::Number)?);
                }
                if let Some(t) = line {
                    let t = match t.to_lowercase().as_str() {
                        "centre" | "center" => "center".to_owned(),
                        "solid" => "continuous".to_owned(),
                        other => other.to_owned(),
                    };
                    props["line_type"] = json!(t);
                }
                let exists = self
                    .doc()
                    .layers()
                    .any(|(_, l)| l.name.eq_ignore_ascii_case(name));
                if exists {
                    props["layer"] = json!(name);
                    self.exec("layers", json!({ "update": [props] }))?;
                } else {
                    self.exec("layers", json!({ "create": [props] }))?;
                }
            }
            Stmt::On(l) => {
                self.exec("layers", json!({ "current": l }))?;
            }
            Stmt::Point { name, at, fixed } => {
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "point", "at": self.pos_arg(at)? }] }),
                )?;
                let id = eid(&v["created"][0]["id"])?;
                if *fixed {
                    self.fix(ln, id)?;
                }
                self.define(name, ln, Obj::Point(id));
            }
            Stmt::Line {
                name,
                from,
                to,
                length,
                fixed,
                construction,
            } => {
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "line", "from": self.pos_arg(from)?, "to": self.pos_arg(to)? }], "construction": construction }),
                )?;
                let c = &v["created"][0];
                let (id, a, b) = (eid(&c["id"])?, eid(&c["a"])?, eid(&c["b"])?);
                if let Some(l) = length {
                    self.dims(vec![
                        json!({ "type": "length", "line": s(id), "value": self.val(l) }),
                    ])?;
                }
                if *fixed {
                    self.fix(ln, a)?;
                }
                self.define(name, ln, Obj::Line(id, a, b));
            }
            Stmt::Polyline {
                name,
                points,
                closed,
                construction,
            } => {
                let pts: Vec<Value> = points.iter().map(|p| self.pos_arg(p)).collect::<R<_>>()?;
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "polyline", "points": pts, "closed": closed }], "construction": construction }),
                )?;
                let c = &v["created"][0];
                self.define(
                    name,
                    ln,
                    Obj::Poly {
                        lines: ids(&c["lines"])?,
                        points: ids(&c["points"])?,
                    },
                );
            }
            Stmt::Rect {
                name,
                w,
                h,
                at,
                centred,
                about,
                fixed,
                construction,
            } => {
                let (wv, hv) = (self.len(w)?, self.len(h)?);
                let mut corner = self.xy(at)?;
                if *centred {
                    corner -= DVec2::new(wv, hv) / 2.0;
                }
                let (faces, corners) = self.rectangle(corner, wv, hv, *construction, true)?;
                if !about {
                    self.size_dims(&faces, w, h)?;
                }
                if *fixed {
                    self.fix(ln, corners[0])?;
                }
                self.define(
                    name,
                    ln,
                    Obj::Rect {
                        faces,
                        corners,
                        outer: None,
                    },
                );
            }
            Stmt::Circle {
                name,
                size,
                diameter,
                at,
                fixed,
                construction,
            } => {
                let mut r = self.len(size)?;
                if *diameter {
                    r /= 2.0;
                }
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "circle", "center": self.pos_arg(at)?, "radius": r }], "construction": construction }),
                )?;
                let c = &v["created"][0];
                let (id, center) = (eid(&c["id"])?, eid(&c["center"])?);
                self.dims(vec![json!({ "type": if *diameter { "diameter" } else { "radius" }, "curve": s(id), "value": self.val(size) })])?;
                if *fixed {
                    self.fix(ln, center)?;
                }
                self.define(name, ln, Obj::Circle(id, center));
            }
            Stmt::Arc {
                name,
                centre,
                from,
                to,
                construction,
            } => {
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "arc", "center": self.pos_arg(centre)?, "start": self.pos_arg(from)?, "end": self.pos_arg(to)? }], "construction": construction }),
                )?;
                let c = &v["created"][0];
                self.define(
                    name,
                    ln,
                    Obj::Arc {
                        id: eid(&c["id"])?,
                        center: eid(&c["center"])?,
                        start: eid(&c["start"])?,
                        end: eid(&c["end"])?,
                    },
                );
            }
            Stmt::Room {
                name,
                w,
                h,
                place,
                walls,
                fixed,
            } => self.room(ln, name, w, h, place, walls, *fixed)?,
            Stmt::Wall {
                name,
                from,
                to,
                thick,
                side,
                length,
                fixed,
            } => {
                self.wall(ln, name, from, to, thick, *side, length.as_ref(), *fixed)?;
            }
            Stmt::Opening {
                name,
                width,
                host,
                along,
                ..
            } => self.opening(ln, name, width, host, along)?,
            Stmt::Grid {
                name,
                axis,
                labels,
                spacing,
                length,
                at,
                fixed,
            } => {
                self.grid(
                    ln,
                    name,
                    *axis,
                    labels,
                    spacing,
                    length,
                    at.as_ref(),
                    *fixed,
                )?;
            }
            Stmt::Array { name, of, kind } => self.array(ln, name, of, kind)?,
            Stmt::Relation { rel, args, at } => self.relation(ln, *rel, args, at.as_ref())?,
            Stmt::Dim {
                kind,
                a,
                b,
                dir,
                value,
                reference,
            } => {
                self.dimension(*kind, a, b.as_ref(), *dir, value.as_ref(), *reference)?;
            }
            Stmt::Label { of, text, height } => {
                let mut at = self.centre_of(of)?;
                let h = match height {
                    Some(h) => self.len(h)?,
                    None => {
                        let style = self.doc().draft_style();
                        style.text_height * style.scale
                    }
                };
                at.y -= h / 2.0;
                self.exec(
                    "add_text",
                    json!({ "text": text, "at": xy(at), "height": h, "align": "center" }),
                )?;
            }
            Stmt::Text {
                text,
                at,
                height,
                align,
                angle,
            } => {
                let mut args = json!({ "text": text, "at": xy(self.xy(at)?) });
                if let Some(h) = height {
                    args["height"] = json!(self.len(h)?);
                }
                if let Some(a) = align {
                    args["align"] = json!(a.to_lowercase());
                }
                if let Some(a) = angle {
                    args["angle"] = json!(self.eval(a, VKind::Angle)?);
                }
                self.exec("add_text", args)?;
            }
            Stmt::Hatch {
                of,
                pattern,
                scale,
                angle,
                color,
            } => {
                let mut base =
                    json!({ "kind": "hatch", "pattern": pattern.as_deref().unwrap_or("ANSI31") });
                if let Some(sc) = scale {
                    base["scale"] = json!(self.eval(sc, VKind::Number)?);
                }
                if let Some(a) = angle {
                    base["angle"] = json!(self.eval(a, VKind::Angle)?);
                }
                if let Some(c) = color {
                    base["color"] = json!(c);
                }
                self.fill_at(of, base)?;
            }
            Stmt::Fill { of, color, opacity } => {
                let mut base = json!({ "kind": "solid" });
                if let Some(c) = color {
                    base["color"] = json!(c);
                }
                if let Some(o) = opacity {
                    base["opacity"] = json!(self.eval(o, VKind::Number)?);
                }
                self.fill_at(of, base)?;
            }
            Stmt::Trim { what, inside, by } => self.trim(ln, what, *inside, by)?,
            Stmt::Use { ids: list, name } => {
                let found: Vec<EntityId> = list
                    .iter()
                    .map(|p| self.entity_or_any(p))
                    .collect::<R<_>>()?;
                self.define(name, ln, Obj::Used(found));
            }
        }
        Ok(())
    }

    /// A name's curves as they are now: the ones left of it, and the pieces trims cut off them.
    fn curves_now(&self, p: &Path) -> R<Vec<EntityId>> {
        let mut out = self.curves(p)?;
        if p.parts.is_empty()
            && let Some(more) = self.trimmed_into.get(&p.root)
        {
            out.extend(more);
        }
        let doc = self.doc();
        out.retain(|c| doc.get(*c).is_some_and(|e| !e.kind.is_point()));
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// The inside of a shape's curves, as `trim_extend_split` takes it (`inside_of`).
    fn area_json(&self, curves: &[EntityId]) -> Option<Value> {
        let area = self.s.editor.closed_area(curves)?;
        Some(json!(
            area.loops
                .iter()
                .map(|lp| lp.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        ))
    }

    /// `trim what inside by`: every piece of `what` on that side of `by`, cut by `by` alone.
    fn trim(&mut self, ln: usize, what: &Path, inside: bool, by: &Path) -> R<()> {
        let (a, b) = (self.curves_now(what)?, self.curves_now(by)?);
        let Some(&first) = a.first() else {
            return Err(format!("{what} has no curves left to trim"));
        };
        if b.is_empty() {
            return Err(format!("{by} has no curves left to trim by"));
        }
        // `by`'s inside as it was before this script first trimmed it, else as it is.
        let area = match self
            .trim_areas
            .get(&by.root)
            .filter(|_| by.parts.is_empty())
        {
            Some(a) => a.clone(),
            None => self.area_json(&b).ok_or(format!(
                "{by} isn't closed: trim … inside needs a closed shape"
            ))?,
        };
        // And `what`'s, before it is cut, for a later line that trims by it.
        if what.parts.is_empty()
            && !self.trim_areas.contains_key(&what.root)
            && let Some(own) = self.area_json(&a)
        {
            self.trim_areas.insert(what.root.clone(), own);
        }
        if by.parts.is_empty() {
            self.trim_areas
                .entry(by.root.clone())
                .or_insert(area.clone());
        }
        let ids: Vec<Value> = a[1..].iter().map(|c| s(*c)).collect();
        let by_ids: Vec<Value> = b.iter().map(|c| s(*c)).collect();
        let v = self.exec(
            "trim_extend_split",
            json!({
                "id": s(first),
                "ids": ids,
                "remove": if inside { "inside" } else { "outside" },
                "by": by_ids,
                "inside_of": area,
            }),
        )?;
        let made: Vec<EntityId> = v["made"]
            .as_array()
            .into_iter()
            .flatten()
            .map(eid)
            .collect::<R<_>>()?;
        if what.parts.is_empty() {
            self.trimmed_into
                .entry(what.root.clone())
                .or_default()
                .extend(made);
        }
        if v["trimmed"].as_u64() == Some(0) {
            let side = if inside { "inside" } else { "outside" };
            self.warnings.push(json!({
                "line": ln,
                "warning": format!("{what} has nothing {side} {by}: nothing was trimmed"),
            }));
        }
        for why in v["skipped"].as_array().into_iter().flatten() {
            self.warnings.push(json!({
                "line": ln,
                "warning": format!("a piece of {what} wasn't trimmed: {}", why.as_str().unwrap_or("")),
            }));
        }
        Ok(())
    }

    fn entity_or_any(&self, p: &Path) -> R<EntityId> {
        match self.resolve(p)? {
            Target::Point(id) | Target::Line(id, ..) | Target::Curve(id, _) => Ok(id),
            Target::Group(g) if g.len() == 1 => Ok(g[0]),
            _ => Err(format!("{p} isn't one entity")),
        }
    }

    fn infer_kind(&self, value: &Expr) -> VarKind {
        let t = &self.text_of(value);
        if t.contains('°') || t.contains("deg") || t.contains("rad") {
            return VarKind::Angle;
        }
        if value.plain().is_some() || self.s.editor.evaluate_text(t, VKind::Length).is_ok() {
            return VarKind::Length;
        }
        if self.s.editor.evaluate_text(t, VKind::Angle).is_ok() {
            VarKind::Angle
        } else {
            VarKind::Number
        }
    }

    /// A rectangle's faces (south, east, north, west) and corners (sw, se, ne, nw).
    fn rectangle(
        &mut self,
        corner: DVec2,
        w: f64,
        h: f64,
        construction: bool,
        join: bool,
    ) -> R<([usize; 4], [EntityId; 4])> {
        let v = self.exec(
            "add_geometry",
            json!({ "items": [{ "type": "rectangle", "corner": xy(corner), "width": w, "height": h }], "construction": construction, "join": join }),
        )?;
        let c = &v["created"][0];
        let side = |k: &str| eid(&c["sides"][k]);
        let corner = |k: &str| eid(&c["corners"][k]);
        let corners = [
            corner("bottom_left")?,
            corner("bottom_right")?,
            corner("top_right")?,
            corner("top_left")?,
        ];
        let sides = [side("bottom")?, side("right")?, side("top")?, side("left")?];
        let faces = [0, 1, 2, 3].map(|i| self.face(sides[i], corners[i], corners[(i + 1) % 4]));
        Ok((faces, corners))
    }

    fn size_dims(&mut self, faces: &[usize; 4], w: &Expr, h: &Expr) -> R<()> {
        let (s0, s3) = (&self.faces[faces[0]], &self.faces[faces[3]]);
        let dims = vec![
            json!({ "type": "distance", "a": s(s0.start), "b": s(s0.end), "value": self.val(w) }),
            json!({ "type": "distance", "a": s(s3.end), "b": s(s3.start), "value": self.val(h) }),
        ];
        self.dims(dims)?;
        Ok(())
    }

    fn constraint_ids(&self) -> BTreeSet<drift_core::ConstraintId> {
        self.doc().constraints().map(|(id, _)| id).collect()
    }

    /// Link the offset distances an `offset` call made to the thickness as written.
    fn link_offsets(
        &mut self,
        before: &BTreeSet<drift_core::ConstraintId>,
        text: Option<String>,
    ) -> R<()> {
        let Some(text) = text else {
            return Ok(());
        };
        let new: Vec<drift_core::ConstraintId> = self
            .doc()
            .constraints()
            .filter(|(id, c)| {
                !before.contains(id) && matches!(c, Constraint::PointLineDistance { .. })
            })
            .map(|(id, _)| id)
            .collect();
        for id in new {
            self.exec(
                "set_dimension",
                json!({ "id": id.to_string(), "value": text }),
            )?;
        }
        Ok(())
    }

    /// Offset a closed chain of four faces; returns the new faces and corners, side by side
    /// with the old.
    fn offset_rect(
        &mut self,
        faces: &[usize; 4],
        corners: &[EntityId; 4],
        distance: f64,
        outward: bool,
        text: Option<String>,
        restyle: bool,
    ) -> R<([usize; 4], [EntityId; 4])> {
        let pts: Vec<DVec2> = corners.iter().map(|c| self.at(*c)).collect();
        let centre = (pts[0] + pts[2]) / 2.0;
        let side = if outward {
            pts[0] + (pts[0] - pts[2])
        } else {
            centre
        };
        let before = self.constraint_ids();
        let v = self.exec(
            "offset",
            json!({ "id": s(self.faces[faces[0]].pieces[0]), "distance": distance, "side": xy(side) }),
        )?;
        let made = ids(&v["created"])?;
        if made.len() != 4 {
            return Err(format!(
                "offsetting the outline made {} curves, not 4",
                made.len()
            ));
        }
        if restyle {
            self.exec(
                "set_construction",
                json!({ "ids": made.iter().map(|i| s(*i)).collect::<Vec<_>>(), "style": "solid" }),
            )?;
        }
        // Match each new line to the side it's parallel to and nearest.
        let mut new = [EntityId(0); 4];
        for &id in &made {
            let (a, b) = self.ends(id)?;
            let (pa, pb) = (self.at(a), self.at(b));
            let m = (pa + pb) / 2.0;
            let d = (pb - pa).normalize_or_zero();
            let best = (0..4)
                .filter(|i| {
                    let (q0, q1) = (pts[*i], pts[(*i + 1) % 4]);
                    (q1 - q0).normalize_or_zero().perp_dot(d).abs() < 1e-6
                })
                .min_by(|i, j| {
                    let dist = |k: usize| m.distance(foot(m, pts[k], pts[(k + 1) % 4]));
                    dist(*i).total_cmp(&dist(*j))
                })
                .ok_or("an offset line matches no side")?;
            new[best] = id;
        }
        let shared = |x: EntityId, y: EntityId| -> R<EntityId> {
            let (a, b) = self.ends(x)?;
            let (c, d) = self.ends(y)?;
            [a, b]
                .into_iter()
                .find(|p| *p == c || *p == d)
                .ok_or_else(|| "the offset outline isn't closed".to_owned())
        };
        // Corner i is where side i meets the side before it.
        let mut oc = [EntityId(0); 4];
        for i in 0..4 {
            oc[i] = shared(new[i], new[(i + 3) % 4])?;
        }
        self.link_offsets(&before, text)?;
        let of = [0, 1, 2, 3].map(|i| self.face(new[i], oc[i], oc[(i + 1) % 4]));
        Ok((of, oc))
    }

    #[allow(clippy::too_many_arguments)]
    fn room(
        &mut self,
        ln: usize,
        name: &str,
        w: &Expr,
        h: &Expr,
        place: &Place,
        walls: &Option<(Expr, WallSide)>,
        fixed: bool,
    ) -> R<()> {
        let (wv, hv) = (self.len(w)?, self.len(h)?);
        // Beside another room: (shared side here, the face it's on, the other room's sw corner, side).
        let mut beside = None;
        let corner = match place {
            Place::At(p) => self.xy(p)?,
            Place::Beside(side, other) => {
                let Some(Obj::Rect {
                    faces,
                    corners,
                    outer,
                    ..
                }) = self.objs.get(&other.root).cloned()
                else {
                    return Err(format!("{other} isn't a room"));
                };
                let (k, here) = match side {
                    Side::Right => (1, 3),
                    Side::Left => (3, 1),
                    Side::Above => (2, 0),
                    Side::Below => (0, 2),
                };
                let base = match outer {
                    Some((of, _)) => of[k],
                    None => faces[k],
                };
                let b = self.at(self.faces[base].start);
                let sw = self.at(corners[0]);
                beside = Some((here, base, corners[0], *side));
                match side {
                    Side::Right => DVec2::new(b.x, sw.y),
                    Side::Left => DVec2::new(b.x - wv, sw.y),
                    Side::Above => DVec2::new(sw.x, b.y),
                    Side::Below => DVec2::new(sw.x, b.y - hv),
                }
            }
        };
        let (mut faces, mut corners) = self.rectangle(corner, wv, hv, false, false)?;
        self.size_dims(&faces, w, h)?;
        if let Place::At(Pos::Ref(p)) = place {
            let p = self.point(p)?;
            self.relations(
                ln,
                vec![json!({ "type": "coincident", "a": s(p), "b": s(corners[0]) })],
            )?;
        }
        let mut outer = None;
        if let Some((t, side)) = walls {
            let tv = self.len(t)?;
            let text = |half: bool| match (t.plain(), half) {
                (Some(_), _) => None,
                (None, false) => Some(t.text.clone()),
                (None, true) => Some(format!("({}) / 2", t.text)),
            };
            match side {
                WallSide::Inside => {
                    let (inner, ic) =
                        self.offset_rect(&faces, &corners, tv, false, text(false), false)?;
                    outer = Some((faces, corners));
                    faces = inner;
                    corners = ic;
                }
                WallSide::Centre => {
                    let lines: Vec<Value> =
                        faces.iter().map(|f| s(self.faces[*f].pieces[0])).collect();
                    self.exec(
                        "set_construction",
                        json!({ "ids": lines, "style": "centre_line" }),
                    )?;
                    let (inner, ic) =
                        self.offset_rect(&faces, &corners, tv / 2.0, false, text(true), true)?;
                    let (out, oc) =
                        self.offset_rect(&faces, &corners, tv / 2.0, true, text(true), true)?;
                    outer = Some((out, oc));
                    faces = inner;
                    corners = ic;
                }
                _ => {
                    let (out, oc) =
                        self.offset_rect(&faces, &corners, tv, true, text(false), false)?;
                    outer = Some((out, oc));
                }
            }
            if let Some((of, _)) = &outer {
                for i in 0..4 {
                    self.pair(faces[i], of[i]);
                }
            }
        }
        if let Some((here, base, other_sw, side)) = beside {
            let base_line = self.faces[base].pieces[0];
            let align = if matches!(side, Side::Right | Side::Left) {
                "horizontal"
            } else {
                "vertical"
            };
            let mut gone = vec![s(self.faces[faces[here]].pieces[0])];
            let mut on = vec![corners[here], corners[(here + 1) % 4]];
            if let Some((of, oc)) = &outer {
                gone.push(s(self.faces[of[here]].pieces[0]));
                on.extend([oc[here], oc[(here + 1) % 4]]);
            }
            self.exec("delete", json!({ "ids": gone }))?;
            let mut rels = vec![json!({ "type": align, "a": s(corners[0]), "b": s(other_sw) })];
            rels.extend(
                on.iter()
                    .map(|p| json!({ "type": "on_curve", "point": s(*p), "curve": s(base_line) })),
            );
            self.relations(ln, rels)?;
            // That side is now the other room's face, and its partner the other room's inside.
            let partner = self.faces[base].partner;
            faces[here] = base;
            if let (Some((of, _)), Some(p)) = (&mut outer, partner) {
                of[here] = p;
            }
        } else if fixed {
            self.fix(ln, corners[0])?;
        }
        self.rooms.push(name.to_owned());
        self.define(
            name,
            ln,
            Obj::Rect {
                faces,
                corners,
                outer,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn wall(
        &mut self,
        ln: usize,
        name: &str,
        from: &Pos,
        to: &Pos,
        thick: &Expr,
        side: WallSide,
        length: Option<&Expr>,
        fixed: bool,
    ) -> R<()> {
        let tv = self.len(thick)?;
        // Not joined to other geometry by its coordinates: the offset would follow it.
        let v = self.exec(
            "add_geometry",
            json!({ "items": [{ "type": "line", "from": self.pos_arg(from)?, "to": self.pos_arg(to)? }], "join": false }),
        )?;
        let c = &v["created"][0];
        let (line, a, b) = (eid(&c["id"])?, eid(&c["a"])?, eid(&c["b"])?);
        let (pa, pb) = (self.at(a), self.at(b));
        let dir = (pb - pa).normalize_or_zero();
        let left = dir.perp();
        let mid = (pa + pb) / 2.0;
        let span = pa.distance(pb).max(tv) + tv;
        let text = |half: bool| match (thick.plain(), half) {
            (Some(_), _) => None,
            (None, false) => Some(thick.text.clone()),
            (None, true) => Some(format!("({}) / 2", thick.text)),
        };
        // Offset `line` towards `towards`; the new face's ends, nearest a then b.
        let offset = |r: &mut Self,
                      d: f64,
                      towards: DVec2,
                      text: Option<String>,
                      restyle: bool|
         -> R<(EntityId, EntityId, EntityId)> {
            let before = r.constraint_ids();
            let v = r.exec(
                "offset",
                json!({ "id": s(line), "distance": d, "side": xy(towards) }),
            )?;
            let made = ids(&v["created"])?;
            let [f] = made.as_slice() else {
                return Err(format!(
                    "offsetting the wall's line made {} curves, not 1 (is it joined to other lines?)",
                    made.len()
                ));
            };
            if restyle {
                r.exec(
                    "set_construction",
                    json!({ "ids": [s(*f)], "style": "solid" }),
                )?;
            }
            r.link_offsets(&before, text)?;
            let (x, y) = r.ends(*f)?;
            Ok(if r.at(x).distance(pa) <= r.at(y).distance(pa) {
                (*f, x, y)
            } else {
                (*f, y, x)
            })
        };
        let (faces, axis, caps);
        if side == WallSide::Centre {
            self.exec(
                "set_construction",
                json!({ "ids": [s(line)], "style": "centre_line" }),
            )?;
            let (lf, ls, le) = offset(self, tv / 2.0, mid + left * span, text(true), true)?;
            let (rf, rs, re) = offset(self, tv / 2.0, mid - left * span, text(true), true)?;
            let v = self.exec(
                "add_geometry",
                json!({ "items": [{ "type": "line", "from": s(ls), "to": s(rs) }, { "type": "line", "from": s(le), "to": s(re) }], "auto_relations": false }),
            )?;
            let (c1, c2) = (eid(&v["created"][0]["id"])?, eid(&v["created"][1]["id"])?);
            self.relations(
                ln,
                vec![
                    json!({ "type": "perpendicular", "a": s(c1), "b": s(line) }),
                    json!({ "type": "on_curve", "point": s(a), "curve": s(c1) }),
                    json!({ "type": "perpendicular", "a": s(c2), "b": s(line) }),
                    json!({ "type": "on_curve", "point": s(b), "curve": s(c2) }),
                ],
            )?;
            let fl = self.face(lf, ls, le);
            let fr = self.face(rf, rs, re);
            faces = [fl, fr];
            axis = Some(line);
            caps = [c1, c2];
        } else {
            // The line drawn is one face; the wall lies to its left or right.
            let towards = if side == WallSide::Left {
                mid + left * span
            } else {
                mid - left * span
            };
            let (of, os, oe) = offset(self, tv, towards, text(false), false)?;
            let v = self.exec(
                "add_geometry",
                json!({ "items": [{ "type": "line", "from": s(a), "to": s(os) }, { "type": "line", "from": s(b), "to": s(oe) }], "auto_relations": false }),
            )?;
            let (c1, c2) = (eid(&v["created"][0]["id"])?, eid(&v["created"][1]["id"])?);
            self.relations(
                ln,
                vec![
                    json!({ "type": "perpendicular", "a": s(c1), "b": s(line) }),
                    json!({ "type": "perpendicular", "a": s(c2), "b": s(line) }),
                ],
            )?;
            let drawn = self.face(line, a, b);
            let other = self.face(of, os, oe);
            faces = if side == WallSide::Left {
                [other, drawn]
            } else {
                [drawn, other]
            };
            axis = None;
            caps = [c1, c2];
        }
        self.pair(faces[0], faces[1]);
        if let Some(l) = length {
            self.dims(vec![
                json!({ "type": "distance", "a": s(a), "b": s(b), "value": self.val(l) }),
            ])?;
        }
        if fixed {
            self.fix(ln, a)?;
        }
        self.define(name, ln, Obj::Wall { faces, axis, caps });
        Ok(())
    }

    /// Cut the stretch of face `f` between `p1` and `p2` (which are on it, `p1` nearer its
    /// start) out of it; returns the new ends at `p1` and `p2`.
    fn cut(&mut self, ln: usize, f: usize, p1: DVec2, p2: DVec2) -> R<(EntityId, EntityId)> {
        let within = |r: &Self, id: EntityId, p: DVec2| -> bool {
            let Ok((a, b)) = r.ends(id) else { return false };
            let (pa, pb) = (r.at(a), r.at(b));
            let d = pb - pa;
            let t = (p - pa).dot(d) / d.length_squared().max(1e-12);
            let margin = 1e-6 * d.length().max(1.0);
            t * d.length() > margin && (1.0 - t) * d.length() > margin
        };
        let pieces = self.faces[f].pieces.clone();
        let Some(k) = pieces.iter().position(|id| within(self, *id, p1)) else {
            return Err(
                "the opening runs past the end of the wall, or into another opening".into(),
            );
        };
        let piece = pieces[k];
        if !within(self, piece, p2) {
            return Err(
                "the opening runs past the end of the wall, or into another opening".into(),
            );
        }
        let (a, _) = self.ends(piece)?;
        let pa = self.at(a);
        let (n1, n2) = if pa.distance(p1) <= pa.distance(p2) {
            (p1, p2)
        } else {
            (p2, p1)
        };
        let v = self.exec(
            "trim_extend_split",
            json!({ "id": s(piece), "at": xy(n1), "mode": "split" }),
        )?;
        let rest = eid(&v["pieces"][1])?;
        let v = self.exec(
            "trim_extend_split",
            json!({ "id": s(rest), "at": xy(n2), "mode": "split" }),
        )?;
        let after = eid(&v["pieces"][1])?;
        let (m1, m2) = self.ends(rest)?;
        self.exec("delete", json!({ "ids": [s(rest)] }))?;
        self.relations(
            ln,
            vec![json!({ "type": "collinear", "a": s(piece), "b": s(after) })],
        )?;
        self.faces[f].pieces.insert(k + 1, after);
        Ok(if n1 == p1 { (m1, m2) } else { (m2, m1) })
    }

    fn opening(
        &mut self,
        ln: usize,
        name: &str,
        width: &Expr,
        host: &Path,
        along: &Along,
    ) -> R<()> {
        let f = match self.resolve(host)? {
            Target::Face(f) => f,
            t => {
                return Err(format!(
                    "{host} is {}; an opening goes in a wall's face, like bed.south or w1.left",
                    Self::kind_words(&t)
                ));
            }
        };
        let Some(g) = self.faces[f].partner else {
            return Err(format!(
                "{host} has no wall behind it (give the room walls, or use a wall)"
            ));
        };
        let (p0, p1) = (self.at(self.faces[f].start), self.at(self.faces[f].end));
        let length = p0.distance(p1);
        let u = (p1 - p0).normalize_or_zero();
        let w = self.len(width)?;
        // Smaller than any real door or window: most likely a unit slip.
        if w < 300.0 {
            let units = self.s.editor.doc.units();
            let mut warning = format!("{name} is only {} wide", units.format_length(w));
            if width.plain().is_some() {
                warning.push_str(&format!(
                    "; a plain number is in the drawing's unit ({}), so for inches write {}\" and for feet {}'",
                    self.unit().symbol(),
                    width.text,
                    width.text
                ));
            }
            self.warnings
                .push(json!({ "line": ln, "warning": warning }));
        }
        let t1 = match along {
            Along::Centred => (length - w) / 2.0,
            Along::Offset(by, from) => {
                let d = self.len(by)?;
                let q = match self.resolve(from)? {
                    Target::Point(p) => self.at(p),
                    Target::Line(_, a, b) => {
                        let (qa, qb) = (self.at(a), self.at(b));
                        let Some(x) = drift_geom::intersect::line_line(p0, u, qa, qb - qa) else {
                            return Err(format!(
                                "{from} runs along {host}, so a distance from it says nothing about where the opening is"
                            ));
                        };
                        x.at
                    }
                    t => {
                        return Err(format!(
                            "{from} is {}; measure from a point or a line",
                            Self::kind_words(&t)
                        ));
                    }
                };
                let tq = (q - p0).dot(u);
                if tq <= length / 2.0 {
                    tq + d
                } else {
                    tq - d - w
                }
            }
        };
        if w <= 0.0 || t1 <= 0.0 || t1 + w >= length {
            return Err(format!(
                "a {} opening there doesn't fit: {host} is {} long",
                self.s.editor.doc.units().format_length(w),
                self.s.editor.doc.units().format_length(length)
            ));
        }
        let (s1, s2) = (p0 + u * t1, p0 + u * (t1 + w));
        let (gs, ge) = (self.at(self.faces[g].start), self.at(self.faces[g].end));
        let (o1, o2) = (foot(s1, gs, ge), foot(s2, gs, ge));
        let (i1, i2) = self.cut(ln, f, s1, s2)?;
        let (x1, x2) = self.cut(ln, g, o1, o2)?;
        let v = self.exec(
            "add_geometry",
            json!({ "items": [{ "type": "line", "from": s(i1), "to": s(x1) }, { "type": "line", "from": s(i2), "to": s(x2) }], "auto_relations": false }),
        )?;
        let (j1, j2) = (eid(&v["created"][0]["id"])?, eid(&v["created"][1]["id"])?);
        let face_line = self.faces[f].pieces[0];
        self.relations(
            ln,
            vec![
                json!({ "type": "perpendicular", "a": s(j1), "b": s(face_line) }),
                json!({ "type": "perpendicular", "a": s(j2), "b": s(face_line) }),
            ],
        )?;
        self.dims(vec![
            json!({ "type": "distance", "a": s(i1), "b": s(i2), "value": self.val(width) }),
        ])?;
        match along {
            Along::Centred => {
                // Equal construction lines from each end of the face to the far end of the
                // nearer jamb: equal only when the gaps either side are, and (unlike lines
                // along the face) not lying on top of it.
                let (fs, fe) = (self.faces[f].start, self.faces[f].end);
                let v = self.exec(
                    "add_geometry",
                    json!({ "items": [{ "type": "line", "from": s(fs), "to": s(x1) }, { "type": "line", "from": s(x2), "to": s(fe) }], "construction": true, "auto_relations": false }),
                )?;
                let (c1, c2) = (eid(&v["created"][0]["id"])?, eid(&v["created"][1]["id"])?);
                self.relations(ln, vec![json!({ "type": "equal", "a": s(c1), "b": s(c2) })])?;
            }
            Along::Offset(by, from) => {
                let near = |r: &Self, q: DVec2| {
                    if r.at(i1).distance(q) <= r.at(i2).distance(q) {
                        (i1, j1)
                    } else {
                        (i2, j2)
                    }
                };
                let dim = match self.resolve(from)? {
                    Target::Point(p) => {
                        let (_, jamb) = near(self, self.at(p));
                        json!({ "type": "distance", "a": s(p), "b": s(jamb), "value": self.val(by) })
                    }
                    Target::Line(l, a, _) => {
                        let (pt, _) = near(self, self.at(a));
                        json!({ "type": "distance", "a": s(pt), "b": s(l), "value": self.val(by) })
                    }
                    _ => unreachable!("checked above"),
                };
                self.dims(vec![dim])?;
            }
        }
        self.define(name, ln, Obj::Opening([j1, j2]));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn grid(
        &mut self,
        ln: usize,
        name: &str,
        axis: Axis,
        labels: &[String],
        spacing: &Spacing,
        length: &Expr,
        at: Option<&Pos>,
        fixed: bool,
    ) -> R<()> {
        let origin = match at {
            Some(p) => self.xy(p)?,
            None => DVec2::ZERO,
        };
        let len = self.len(length)?;
        let gaps: Vec<&Expr> = match spacing {
            Spacing::Every(e) => vec![e; labels.len().saturating_sub(1)],
            Spacing::List(l) => l.iter().collect(),
        };
        let mut offsets = vec![0.0];
        for g in &gaps {
            let last = *offsets.last().unwrap_or(&0.0);
            offsets.push(last + self.len(g)?);
        }
        let cols = axis == Axis::Cols;
        let items: Vec<Value> = offsets
            .iter()
            .map(|o| {
                let (a, b) = if cols {
                    (origin + DVec2::new(*o, 0.0), origin + DVec2::new(*o, len))
                } else {
                    (origin + DVec2::new(0.0, *o), origin + DVec2::new(len, *o))
                };
                json!({ "type": "line", "from": xy(a), "to": xy(b) })
            })
            .collect();
        let v = self.exec("add_geometry", json!({ "items": items, "join": false }))?;
        let made: Vec<(EntityId, EntityId)> = v["created"]
            .as_array()
            .ok_or("no grid lines")?
            .iter()
            .map(|c| Ok((eid(&c["id"])?, eid(&c["a"])?)))
            .collect::<R<_>>()?;
        self.exec(
            "set_construction",
            json!({ "ids": made.iter().map(|(l, _)| s(*l)).collect::<Vec<_>>(), "style": "centre_line" }),
        )?;
        let (first, start) = made[0];
        let mut dims =
            vec![json!({ "type": "length", "line": s(first), "value": self.val(length) })];
        let mut rels = Vec::new();
        for (k, (l, a)) in made.iter().enumerate().skip(1) {
            rels.push(json!({ "type": "equal", "a": s(first), "b": s(*l) }));
            rels.push(json!({ "type": if cols { "horizontal" } else { "vertical" }, "a": s(start), "b": s(*a) }));
            dims.push(json!({ "type": if cols { "horizontal" } else { "vertical" }, "a": s(made[k - 1].1), "b": s(*a), "value": self.val(gaps[k - 1]) }));
        }
        self.relations(ln, rels)?;
        self.dims(dims)?;
        if fixed {
            self.fix(ln, start)?;
        }
        // Labels just past the lines' far (or near) ends.
        let style = self.doc().draft_style();
        let h = style.text_height * style.scale;
        for (o, label) in offsets.iter().zip(labels) {
            let (at, align) = if cols {
                (origin + DVec2::new(*o, len + h), "center")
            } else {
                (origin + DVec2::new(-h, *o - h / 2.0), "right")
            };
            self.exec(
                "add_text",
                json!({ "text": label, "at": xy(at), "align": align }),
            )?;
        }
        let axes: Vec<(String, EntityId)> = labels
            .iter()
            .cloned()
            .zip(made.iter().map(|(l, _)| *l))
            .collect();
        match self.objs.get_mut(name) {
            Some(Obj::Grid(have)) => have.extend(axes),
            _ => self.define(name, ln, Obj::Grid(axes)),
        }
        Ok(())
    }

    /// The point that places a single circle or point (its centre), for keeping copies in line.
    fn anchor(&self, curves: &[EntityId]) -> Option<EntityId> {
        match curves {
            [one] => match self.doc().get(*one).map(|e| e.kind) {
                Some(EntityKind::Circle { center, .. }) => Some(center),
                Some(EntityKind::Point(_)) => Some(*one),
                _ => None,
            },
            _ => None,
        }
    }

    fn array(&mut self, ln: usize, name: &str, of: &Path, kind: &ArrayKind) -> R<()> {
        let src = self.curves(of)?;
        let n = src.len();
        let chunk = |made: &[EntityId]| -> Vec<Vec<EntityId>> {
            made.chunks(n.max(1)).map(<[EntityId]>::to_vec).collect()
        };
        let list = |v: &[EntityId]| v.iter().map(|i| s(*i)).collect::<Vec<_>>();
        let copies = match kind {
            ArrayKind::Grid { first, second, up } => {
                let (c1, d1) = (self.count(&first.0)?, self.len(&first.1)?);
                let step = if *up {
                    DVec2::new(0.0, d1)
                } else {
                    DVec2::new(d1, 0.0)
                };
                let mut copies = vec![src.clone()];
                if c1 > 1 {
                    let v = self.exec("transform", json!({ "op": "linear_array", "ids": list(&src), "by": xy(step), "count": c1 }))?;
                    copies.extend(chunk(&ids(&v["created"])?));
                }
                // Keep single circles in line and at their spacing.
                let anchors: Option<Vec<EntityId>> =
                    copies.iter().map(|c| self.anchor(c)).collect();
                if let Some(anchors) = &anchors {
                    let along = if *up { "vertical" } else { "horizontal" };
                    let mut rels = Vec::new();
                    let mut dims = Vec::new();
                    for w in anchors.windows(2) {
                        rels.push(json!({ "type": along, "a": s(w[0]), "b": s(w[1]) }));
                        dims.push(json!({ "type": if *up { "vertical" } else { "horizontal" }, "a": s(w[0]), "b": s(w[1]), "value": self.val(&first.1) }));
                    }
                    self.relations(ln, rels)?;
                    if !dims.is_empty() {
                        self.dims(dims)?;
                    }
                }
                if let Some((c2e, d2e)) = second {
                    let (c2, d2) = (self.count(c2e)?, self.len(d2e)?);
                    if c2 > 1 {
                        let row = copies.concat();
                        let v = self.exec("transform", json!({ "op": "linear_array", "ids": list(&row), "by": xy(DVec2::new(0.0, d2)), "count": c2 }))?;
                        let made = ids(&v["created"])?;
                        let per_row = copies.len();
                        let rows: Vec<Vec<EntityId>> = made
                            .chunks(row.len().max(1))
                            .map(<[EntityId]>::to_vec)
                            .collect();
                        for r in rows {
                            copies.extend(chunk(&r));
                        }
                        if let Some(anchors) = anchors {
                            let all: Option<Vec<EntityId>> =
                                copies.iter().map(|c| self.anchor(c)).collect();
                            if let Some(all) = all {
                                let mut rels = Vec::new();
                                let mut dims = Vec::new();
                                // Each row copies the one before with its spacing; its first
                                // copy is tied to the one below.
                                for k in (per_row..all.len()).step_by(per_row) {
                                    let below = all[k - per_row];
                                    rels.push(json!({ "type": "vertical", "a": s(below), "b": s(all[k]) }));
                                    dims.push(json!({ "type": "vertical", "a": s(below), "b": s(all[k]), "value": self.val(d2e) }));
                                }
                                let _ = anchors;
                                self.relations(ln, rels)?;
                                self.dims(dims)?;
                            }
                        }
                    }
                }
                copies
            }
            ArrayKind::By(c, by) => {
                let count = self.count(c)?;
                let step = self.xy(by)?;
                let v = self.exec("transform", json!({ "op": "linear_array", "ids": list(&src), "by": xy(step), "count": count }))?;
                let mut copies = vec![src.clone()];
                copies.extend(chunk(&ids(&v["created"])?));
                copies
            }
            ArrayKind::Around(c, about, angle) => {
                let count = self.count(c)?;
                let mut args = json!({ "op": "circular_array", "ids": list(&src), "about": xy(self.xy(about)?), "count": count });
                if let Some(a) = angle {
                    args["angle"] = json!(self.eval(a, VKind::Angle)?);
                }
                let v = self.exec("transform", args)?;
                let mut copies = vec![src.clone()];
                copies.extend(chunk(&ids(&v["created"])?));
                copies
            }
        };
        self.define(name, ln, Obj::Array(copies));
        Ok(())
    }

    fn relation(&mut self, ln: usize, rel: Rel, args: &[Path], at: Option<&Pos>) -> R<()> {
        let pairs = |r: &Self, t: &str| -> R<Vec<Value>> {
            let es: Vec<EntityId> = args.iter().map(|p| r.entity(p)).collect::<R<_>>()?;
            Ok(es
                .windows(2)
                .map(|w| json!({ "type": t, "a": s(w[0]), "b": s(w[1]) }))
                .collect())
        };
        let rels = match rel {
            Rel::Parallel => pairs(self, "parallel")?,
            Rel::Perpendicular => pairs(self, "perpendicular")?,
            Rel::Equal => pairs(self, "equal")?,
            Rel::Collinear => pairs(self, "collinear")?,
            Rel::Tangent => pairs(self, "tangent")?,
            Rel::Concentric => pairs(self, "concentric")?,
            Rel::Coincident => vec![
                json!({ "type": "coincident", "a": s(self.point(&args[0])?), "b": s(self.point(&args[1])?) }),
            ],
            Rel::Horizontal | Rel::Vertical => {
                let t = if rel == Rel::Horizontal {
                    "horizontal"
                } else {
                    "vertical"
                };
                if args.len() == 2 {
                    vec![
                        json!({ "type": t, "a": s(self.point(&args[0])?), "b": s(self.point(&args[1])?) }),
                    ]
                } else {
                    vec![json!({ "type": t, "line": s(self.line(&args[0])?.0) })]
                }
            }
            Rel::Symmetric => vec![json!({
                "type": "symmetric", "a": s(self.point(&args[0])?), "b": s(self.point(&args[1])?), "axis": s(self.line(&args[2])?.0)
            })],
            Rel::Midpoint => vec![
                json!({ "type": "midpoint", "point": s(self.point(&args[0])?), "line": s(self.line(&args[1])?.0) }),
            ],
            Rel::On => vec![
                json!({ "type": "on_curve", "point": s(self.point(&args[0])?), "curve": s(self.entity(&args[1])?) }),
            ],
            Rel::Fix => {
                let mut r = json!({ "type": "fix", "point": s(self.point(&args[0])?) });
                if let Some(at) = at {
                    r["at"] = xy(self.xy(at)?);
                }
                vec![r]
            }
        };
        self.relations(ln, rels)
    }

    fn dimension(
        &mut self,
        kind: DimKind,
        a: &Path,
        b: Option<&Path>,
        dir: Option<Dir>,
        value: Option<&Expr>,
        reference: bool,
    ) -> R<()> {
        let mut d = match (kind, b) {
            (DimKind::Radius | DimKind::Diameter, _) => {
                let curve = match self.resolve(a)? {
                    Target::Curve(id, _) => id,
                    t => {
                        return Err(format!(
                            "{a} is {}, not a circle or arc",
                            Self::kind_words(&t)
                        ));
                    }
                };
                json!({ "type": if kind == DimKind::Radius { "radius" } else { "diameter" }, "curve": s(curve) })
            }
            (DimKind::Angle, Some(b)) => {
                json!({ "type": "angle", "a": s(self.line(a)?.0), "b": s(self.line(b)?.0) })
            }
            (DimKind::Angle, None) => {
                return Err("an angle is between two lines: dim angle a b = 30°".into());
            }
            (DimKind::Auto, None) => match self.resolve(a)? {
                Target::Curve(id, _) => json!({ "type": "radius", "curve": s(id) }),
                Target::Line(..) | Target::Face(_) => {
                    let (_, p, q) = self.line(a)?;
                    let t = match dir {
                        Some(Dir::Horizontal) => "horizontal",
                        Some(Dir::Vertical) => "vertical",
                        None => "distance",
                    };
                    json!({ "type": t, "a": s(p), "b": s(q) })
                }
                t => {
                    return Err(format!(
                        "{a} is {}; dimension a line or a circle, or write `dim {a} to <something> = …`",
                        Self::kind_words(&t)
                    ));
                }
            },
            (DimKind::Auto, Some(b)) => {
                let operand = |r: &Self, p: &Path| -> R<(EntityId, bool)> {
                    match r.resolve(p)? {
                        Target::Point(id) => Ok((id, false)),
                        Target::Line(id, ..) => Ok((id, true)),
                        Target::Face(f) => Ok((r.faces[f].pieces[0], true)),
                        Target::Curve(_, c) => Ok((c, false)),
                        t => Err(format!(
                            "{p} is {}; measure between points or lines",
                            Self::kind_words(&t)
                        )),
                    }
                };
                let (x, xl) = operand(self, a)?;
                let (y, yl) = operand(self, b)?;
                match dir {
                    Some(Dir::Horizontal) => json!({ "type": "horizontal", "a": s(x), "b": s(y) }),
                    Some(Dir::Vertical) => json!({ "type": "vertical", "a": s(x), "b": s(y) }),
                    None if xl && yl => {
                        let (p, _) = self.ends(x)?;
                        json!({ "type": "point_line", "point": s(p), "line": s(y) })
                    }
                    None => json!({ "type": "distance", "a": s(x), "b": s(y) }),
                }
            }
        };
        if reference {
            d["driven"] = json!(true);
        } else if let Some(v) = value {
            let k = if kind == DimKind::Angle {
                VKind::Angle
            } else {
                VKind::Length
            };
            d["value"] = self.val_as(v, k);
        }
        self.dims(vec![d])?;
        Ok(())
    }

    /// Where a label goes: the middle of a room or rectangle, a circle's centre.
    fn centre_of(&self, of: &Path) -> R<DVec2> {
        Ok(match self.resolve(of)? {
            Target::Area(corners) => {
                let pts: Vec<DVec2> = corners.iter().map(|c| self.at(*c)).collect();
                pts.iter().copied().sum::<DVec2>() / pts.len() as f64
            }
            Target::Curve(_, c) | Target::Point(c) => self.at(c),
            Target::Line(_, a, b) => (self.at(a) + self.at(b)) / 2.0,
            Target::Face(f) => (self.at(self.faces[f].start) + self.at(self.faces[f].end)) / 2.0,
            Target::Band(_) | Target::Group(_) => {
                let doc = self.doc();
                self.curves(of)?
                    .into_iter()
                    .filter_map(|id| doc.shape(id).map(|sh| sh.bounds()))
                    .reduce(drift_core::Bounds::union)
                    .ok_or_else(|| format!("{of} has nothing to measure"))?
                    .center()
            }
        })
    }

    /// Fill or hatch an area, or every stretch of a set of walls.
    fn fill_at(&mut self, of: &Path, base: Value) -> R<()> {
        let points: Vec<DVec2> = match self.resolve(of)? {
            Target::Band(faces) => {
                let mut pts = Vec::new();
                for f in faces {
                    let Some(g) = self.faces[f].partner else {
                        continue;
                    };
                    let (gs, ge) = (self.at(self.faces[g].start), self.at(self.faces[g].end));
                    for piece in self.faces[f].pieces.clone() {
                        let (a, b) = self.ends(piece)?;
                        let m = (self.at(a) + self.at(b)) / 2.0;
                        pts.push((m + foot(m, gs, ge)) / 2.0);
                    }
                }
                pts
            }
            _ => vec![self.centre_of(of)?],
        };
        let mut done = BTreeSet::new();
        for p in points {
            let mut args = base.clone();
            args["at"] = xy(p);
            let v = self.exec("add_fill", args)?;
            done.insert(v["id"].as_str().unwrap_or_default().to_owned());
        }
        Ok(())
    }

    /// Every name with its parts, as ids.
    pub fn names(&self) -> Map<String, Value> {
        let mut out = Map::new();
        let mut put = |k: String, id: EntityId| {
            out.insert(k, s(id));
        };
        let mut names: Vec<(&String, &Obj)> = self.objs.iter().collect();
        names.sort_by_key(|(n, _)| self.lines_of.get(*n).copied().unwrap_or(0));
        for (name, obj) in names {
            let face = |put: &mut dyn FnMut(String, EntityId), key: &str, f: usize| {
                let face = &self.faces[f];
                if face.pieces.len() == 1 {
                    put(key.to_owned(), face.pieces[0]);
                } else {
                    for (k, p) in face.pieces.iter().enumerate() {
                        put(format!("{key}[{}]", k + 1), *p);
                    }
                }
            };
            match obj {
                Obj::Point(p) => put(name.clone(), *p),
                Obj::Line(id, a, b) => {
                    put(name.clone(), *id);
                    put(format!("{name}.start"), *a);
                    put(format!("{name}.end"), *b);
                }
                Obj::Poly { lines, .. } => {
                    for (k, l) in lines.iter().enumerate() {
                        put(format!("{name}[{}]", k + 1), *l);
                    }
                }
                Obj::Rect {
                    faces,
                    corners,
                    outer,
                    ..
                } => {
                    for (i, side) in ["south", "east", "north", "west"].iter().enumerate() {
                        face(&mut put, &format!("{name}.{side}"), faces[i]);
                    }
                    for (i, c) in ["sw", "se", "ne", "nw"].iter().enumerate() {
                        put(format!("{name}.{c}"), corners[i]);
                    }
                    if let Some((of, oc)) = outer {
                        for (i, side) in ["south", "east", "north", "west"].iter().enumerate() {
                            face(&mut put, &format!("{name}.outer.{side}"), of[i]);
                        }
                        for (i, c) in ["sw", "se", "ne", "nw"].iter().enumerate() {
                            put(format!("{name}.outer.{c}"), oc[i]);
                        }
                    }
                }
                Obj::Wall { faces, axis, caps } => {
                    face(&mut put, &format!("{name}.left"), faces[0]);
                    face(&mut put, &format!("{name}.right"), faces[1]);
                    put(format!("{name}.start"), caps[0]);
                    put(format!("{name}.end"), caps[1]);
                    if let Some(a) = axis {
                        put(format!("{name}.axis"), *a);
                    }
                }
                Obj::Opening(j) => {
                    put(format!("{name}.start"), j[0]);
                    put(format!("{name}.end"), j[1]);
                }
                Obj::Grid(axes) => {
                    for (l, id) in axes {
                        put(format!("{name}.{l}"), *id);
                    }
                }
                Obj::Circle(id, c) => {
                    put(name.clone(), *id);
                    put(format!("{name}.center"), *c);
                }
                Obj::Arc {
                    id,
                    center,
                    start,
                    end,
                } => {
                    put(name.clone(), *id);
                    put(format!("{name}.center"), *center);
                    put(format!("{name}.start"), *start);
                    put(format!("{name}.end"), *end);
                }
                Obj::Array(copies) => {
                    for (k, c) in copies.iter().enumerate() {
                        if let [one] = c.as_slice() {
                            put(format!("{name}[{}]", k + 1), *one);
                        }
                    }
                }
                Obj::Used(ids) => {
                    if let [one] = ids.as_slice() {
                        put(name.clone(), *one);
                    } else {
                        for (k, id) in ids.iter().enumerate() {
                            put(format!("{name}[{}]", k + 1), *id);
                        }
                    }
                }
            }
        }
        out
    }

    /// Each room's inside area.
    pub fn rooms(&self) -> Vec<Value> {
        self.rooms
            .iter()
            .filter_map(|name| match self.objs.get(name) {
                Some(Obj::Rect { corners, .. }) => {
                    let pts: Vec<DVec2> = corners.iter().map(|c| self.at(*c)).collect();
                    let area = drift_core::polygon::area(&pts).abs();
                    Some(json!({ "room": name, "area_m2": crate::agent::args::num(area / 1e6) }))
                }
                _ => None,
            })
            .collect()
    }

    /// An error on statement `st`, with what it meant and the lines it involves.
    pub fn error(&self, st: &Statement, message: String) -> ScriptError {
        let mut e =
            ScriptError::new(st.line, &st.source, message.clone()).meant(Some(meant(&st.stmt)));
        let mut related: Vec<usize> = paths_of(&st.stmt)
            .iter()
            .filter_map(|p| self.lines_of.get(&p.root).copied())
            .filter(|l| *l != st.line)
            .collect();
        related.sort_unstable();
        related.dedup();
        e.related = related
            .into_iter()
            .map(|l| (l, self.sources.get(&l).cloned().unwrap_or_default()))
            .collect();
        let lower = message.to_lowercase();
        if matches!(st.stmt, Stmt::Dim { .. })
            && (lower.contains("already defined")
                || lower.contains("conflict")
                || lower.contains("redundant"))
        {
            e = e.hint("something earlier already sets this size: change that dimension or its variable instead, or add `ref` to show this one only");
        } else if lower.contains("conflict") {
            e = e.hint("an earlier relation or dimension already holds this; remove one of them");
        }
        e
    }
}
