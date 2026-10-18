//! Names and their parts, checked before anything runs: unknown names and parts, names used
//! twice, and unknown variables are errors that leave the drawing untouched.

use std::collections::HashMap;

use drift_core::Document;

use super::errors::ScriptError;
use super::parse::{
    Along, ArrayKind, Expr, Part, Path, Place, Pos, Spacing, Statement, Stmt, WallSide,
};

/// What a name is, for checking its parts.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Point,
    Line,
    Polyline(usize),
    Rect,
    Room {
        walls: bool,
    },
    Wall {
        axis: bool,
    },
    Opening,
    Grid(Vec<String>),
    Circle,
    Arc,
    Array(Box<Kind>, Option<usize>),
    /// Existing entities, by id (`use`); their kind is known only when the script runs.
    Used(usize),
}

impl Kind {
    pub fn noun(&self) -> &'static str {
        match self {
            Kind::Point => "point",
            Kind::Line => "line",
            Kind::Polyline(_) => "polyline",
            Kind::Rect => "rect",
            Kind::Room { .. } => "room",
            Kind::Wall { .. } => "wall",
            Kind::Opening => "opening",
            Kind::Grid(_) => "grid",
            Kind::Circle => "circle",
            Kind::Arc => "arc",
            Kind::Array(..) => "array",
            Kind::Used(_) => "set of existing entities",
        }
    }
}

const SIDES: &[&str] = &[
    "south", "east", "north", "west", "bottom", "right", "top", "left",
];
const CORNERS: &[&str] = &["sw", "se", "ne", "nw"];

/// The parts a kind has (first level only), for messages.
pub(super) fn parts(kind: &Kind) -> Vec<String> {
    let s = |l: &[&str]| l.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    match kind {
        Kind::Point | Kind::Used(_) => Vec::new(),
        Kind::Line => s(&["start", "end"]),
        Kind::Polyline(n) => {
            let mut v = s(&["start", "end", "inside"]);
            v.push(format!("[1] … [{n}] (its segments)"));
            v
        }
        Kind::Rect => [SIDES, CORNERS, &["width", "depth", "inside"]]
            .concat()
            .iter()
            .map(|x| (*x).to_owned())
            .collect(),
        Kind::Room { walls } => {
            let mut v: Vec<String> = [SIDES, CORNERS, &["width", "depth", "inside"]]
                .concat()
                .iter()
                .map(|x| (*x).to_owned())
                .collect();
            if *walls {
                v.extend(s(&["outer.<side or corner>", "walls"]));
            }
            v
        }
        Kind::Wall { axis } => {
            let mut v = s(&["left", "right", "start", "end"]);
            if *axis {
                v.push("axis".into());
            }
            v
        }
        Kind::Opening => s(&["start", "end"]),
        Kind::Grid(labels) => labels.clone(),
        Kind::Circle => s(&["center"]),
        Kind::Arc => s(&["center", "start", "end"]),
        Kind::Array(_, n) => vec![match n {
            Some(n) => format!("[1] … [{n}]"),
            None => "[1] … (its copies)".into(),
        }],
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The closest of `options` to `word`, if it's close enough to be a typo.
pub(super) fn closest<'a>(
    word: &str,
    options: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    options
        .into_iter()
        .map(|o| (levenshtein(&word.to_lowercase(), &o.to_lowercase()), o))
        .filter(|(d, o)| *d <= 2.max(o.len() / 3) && *d < o.len())
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| o)
}

/// `e12`: an entity id.
pub(super) fn is_entity_id(s: &str) -> bool {
    s.len() > 1 && s.starts_with('e') && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Checks the parts after a name of `kind`; `Err((part index, message))`.
fn check_parts(name: &str, kind: &Kind, parts: &[Part]) -> Result<(), (usize, String)> {
    let Some(first) = parts.first() else {
        return Ok(());
    };
    let rest = &parts[1..];
    let unknown = |p: &str| {
        let have = self::parts(kind);
        let guess = closest(p, have.iter().map(String::as_str));
        let mut m = format!("{} {name} has no part `{p}`", kind.noun());
        if let Some(g) = guess {
            m.push_str(&format!(" (did you mean `{g}`?)"));
        }
        if have.is_empty() {
            m.push_str("; it has no parts");
        } else {
            m.push_str(&format!(". Its parts: {}", have.join(", ")));
        }
        m
    };
    // What may follow a side: `.start`, `.end`, or a piece `[2]` once openings split it.
    let side_rest = |rest: &[Part]| -> Result<(), (usize, String)> {
        match rest {
            [] | [Part::Index(_)] => Ok(()),
            [Part::Field(f)] if f == "start" || f == "end" => Ok(()),
            [Part::Index(_), Part::Field(f)] if f == "start" || f == "end" => Ok(()),
            _ => Err((
                1,
                format!("after a side, write `.start`, `.end` or a piece like `[2]` ({name})"),
            )),
        }
    };
    let no_more = |rest: &[Part]| {
        if rest.is_empty() {
            Ok(())
        } else {
            Err((1, format!("`{name}` has no parts below that")))
        }
    };
    match (kind, first) {
        (Kind::Point, _) => Err((0, unknown(&part_text(first)))),
        (Kind::Line, Part::Field(f)) if f == "start" || f == "end" => no_more(rest),
        (Kind::Polyline(_), Part::Field(f)) if f == "start" || f == "end" || f == "inside" => {
            no_more(rest)
        }
        (Kind::Polyline(n), Part::Index(i)) => {
            if *i > *n {
                Err((
                    0,
                    format!("polyline {name} has {n} segments, so [{i}] is past its end"),
                ))
            } else {
                no_more(rest)
            }
        }
        (Kind::Rect | Kind::Room { .. }, Part::Field(f))
            if SIDES.contains(&f.as_str()) || f == "width" || f == "depth" =>
        {
            side_rest(rest)
        }
        (Kind::Rect | Kind::Room { .. }, Part::Field(f))
            if CORNERS.contains(&f.as_str()) || f == "inside" =>
        {
            no_more(rest)
        }
        (Kind::Room { walls: true }, Part::Field(f)) if f == "walls" => no_more(rest),
        (Kind::Room { walls: true }, Part::Field(f)) if f == "outer" => match rest.first() {
            Some(Part::Field(g)) if SIDES.contains(&g.as_str()) => {
                side_rest(&rest[1..]).map_err(|(i, m)| (i + 1, m))
            }
            Some(Part::Field(g)) if CORNERS.contains(&g.as_str()) => no_more(&rest[1..]),
            _ => Err((
                1,
                format!(
                    "after `outer`, write a side (north, east, south, west) or a corner (nw, ne, se, sw) of {name}"
                ),
            )),
        },
        (Kind::Wall { .. }, Part::Field(f)) if f == "left" || f == "right" => side_rest(rest),
        (Kind::Wall { .. }, Part::Field(f)) if f == "start" || f == "end" => no_more(rest),
        (Kind::Wall { axis: true }, Part::Field(f)) if f == "axis" => no_more(rest),
        (Kind::Opening, Part::Field(f)) if f == "start" || f == "end" => no_more(rest),
        (Kind::Grid(labels), Part::Field(f)) if labels.contains(f) => no_more(rest),
        (Kind::Circle, Part::Field(f)) if f == "center" || f == "centre" => no_more(rest),
        (Kind::Arc, Part::Field(f))
            if ["center", "centre", "start", "end"].contains(&f.as_str()) =>
        {
            no_more(rest)
        }
        (Kind::Array(of, n), Part::Index(i)) => {
            if n.is_some_and(|n| *i > n) {
                return Err((
                    0,
                    format!(
                        "array {name} has {} copies, so [{i}] is past its end",
                        n.unwrap_or(0)
                    ),
                ));
            }
            check_parts(name, of, rest).map_err(|(k, m)| (k + 1, m))
        }
        (Kind::Used(n), Part::Index(i)) if *n > 1 => {
            if *i > *n {
                Err((
                    0,
                    format!("{name} has {n} entities, so [{i}] is past its end"),
                ))
            } else {
                no_more(rest)
            }
        }
        (_, p) => Err((0, unknown(&part_text(p)))),
    }
}

fn part_text(p: &Part) -> String {
    match p {
        Part::Field(f) => f.clone(),
        Part::Index(i) => format!("[{i}]"),
    }
}

struct Checker<'a> {
    doc: &'a Document,
    kinds: HashMap<String, (Kind, usize)>,
    vars: Vec<String>,
}

impl Checker<'_> {
    fn err(st: &Statement, span: (usize, usize), msg: impl Into<String>) -> ScriptError {
        ScriptError::new(st.line, &st.source, msg).span(span.0, span.1)
    }

    fn path(&self, st: &Statement, p: &Path) -> Result<(), ScriptError> {
        let Some((kind, _)) = self.kinds.get(&p.root) else {
            if is_entity_id(&p.root) {
                let id = drift_core::EntityId(p.root[1..].parse().unwrap_or(u32::MAX));
                if !self.doc.contains(id) {
                    return Err(Self::err(
                        st,
                        p.span,
                        format!("{} doesn't exist in the drawing", p.root),
                    ));
                }
                if !p.parts.is_empty() {
                    return Err(Self::err(
                        st,
                        p.span,
                        format!(
                            "an id like {} has no parts; `use {} as <name>` doesn't either",
                            p.root, p.root
                        ),
                    ));
                }
                return Ok(());
            }
            let mut m = format!("`{}` isn't defined", p.root);
            if let Some(g) = closest(&p.root, self.kinds.keys().map(String::as_str)) {
                m.push_str(&format!(" (did you mean `{g}`?)"));
            } else if self.vars.contains(&p.root) {
                m.push_str(" as a shape: it's a variable");
            }
            return Err(Self::err(st, p.span, m)
                .hint("name shapes where you make them (`room bed …`), or `use e12 as name` for existing ones"));
        };
        check_parts(&p.root, kind, &p.parts).map_err(|(_, m)| Self::err(st, p.span, m))
    }

    fn pos(&self, st: &Statement, p: &Pos) -> Result<(), ScriptError> {
        match p {
            Pos::At(x, y) => {
                self.expr(st, x)?;
                self.expr(st, y)
            }
            Pos::Ref(path) => self.path(st, path),
        }
    }

    fn expr(&self, st: &Statement, e: &Expr) -> Result<(), ScriptError> {
        let Ok(parsed) = drift_core::expr::parse(&e.text) else {
            return Ok(());
        };
        for id in parsed.identifiers() {
            if !self.vars.contains(&id) && self.doc.variable_named(&id).is_none() {
                let mut m = format!("unknown variable `{id}`");
                if let Some(g) = closest(&id, self.vars.iter().map(String::as_str)) {
                    m.push_str(&format!(" (did you mean `{g}`?)"));
                }
                return Err(
                    Self::err(st, e.span, m).hint(format!("define it first: let {id} = 3.6 m"))
                );
            }
        }
        Ok(())
    }

    fn define(&mut self, st: &Statement, name: &str, kind: Kind) -> Result<(), ScriptError> {
        let span = st.name_span.unwrap_or((0, st.source.len()));
        if let Some((k, line)) = self.kinds.get(name) {
            return Err(Self::err(
                st,
                span,
                format!("`{name}` is already the {} on line {line}", k.noun()),
            ));
        }
        if is_entity_id(name) {
            return Err(Self::err(
                st,
                span,
                format!("`{name}` looks like an id; choose another name"),
            ));
        }
        if self.vars.iter().any(|v| v == name) {
            return Err(Self::err(
                st,
                span,
                format!("`{name}` is already a variable"),
            ));
        }
        self.kinds.insert(name.to_owned(), (kind, st.line));
        Ok(())
    }

    fn statement(&mut self, st: &Statement) -> Result<(), ScriptError> {
        let count = |e: &Expr| e.plain().map(|n| n.round().max(1.0) as usize);
        match &st.stmt {
            Stmt::Let { name, value, .. } => {
                self.expr(st, value)?;
                if self.kinds.contains_key(name) {
                    return Err(Self::err(
                        st,
                        st.name_span.unwrap_or_default(),
                        format!("`{name}` is already a shape's name"),
                    ));
                }
                if drift_core::expr::valid_name(name).is_err() {
                    return Err(Self::err(
                        st,
                        st.name_span.unwrap_or_default(),
                        format!("`{name}` can't be a variable's name"),
                    ));
                }
                self.vars.push(name.clone());
            }
            Stmt::Layer { .. } | Stmt::On(_) => {}
            Stmt::Point { name, at, .. } => {
                self.pos(st, at)?;
                self.define(st, name, Kind::Point)?;
            }
            Stmt::Line {
                name,
                from,
                to,
                length,
                ..
            } => {
                self.pos(st, from)?;
                self.pos(st, to)?;
                if let Some(l) = length {
                    self.expr(st, l)?;
                }
                self.define(st, name, Kind::Line)?;
            }
            Stmt::Polyline {
                name,
                points,
                closed,
                ..
            } => {
                for p in points {
                    self.pos(st, p)?;
                }
                let n = points.len() - usize::from(!closed);
                self.define(st, name, Kind::Polyline(n))?;
            }
            Stmt::Rect { name, w, h, at, .. } => {
                self.expr(st, w)?;
                self.expr(st, h)?;
                self.pos(st, at)?;
                self.define(st, name, Kind::Rect)?;
            }
            Stmt::Circle { name, size, at, .. } => {
                self.expr(st, size)?;
                self.pos(st, at)?;
                self.define(st, name, Kind::Circle)?;
            }
            Stmt::Arc {
                name,
                centre,
                from,
                to,
                ..
            } => {
                for p in [centre, from, to] {
                    self.pos(st, p)?;
                }
                self.define(st, name, Kind::Arc)?;
            }
            Stmt::Room {
                name,
                w,
                h,
                place,
                walls,
                ..
            } => {
                self.expr(st, w)?;
                self.expr(st, h)?;
                match place {
                    Place::At(p) => self.pos(st, p)?,
                    Place::Beside(_, other) => {
                        self.path(st, other)?;
                        if !other.parts.is_empty()
                            || !matches!(self.kinds.get(&other.root), Some((Kind::Room { .. }, _)))
                        {
                            return Err(Self::err(
                                st,
                                other.span,
                                format!("`{other}` isn't a room"),
                            ));
                        }
                    }
                }
                if let Some((t, _)) = walls {
                    self.expr(st, t)?;
                }
                self.define(
                    st,
                    name,
                    Kind::Room {
                        walls: walls.is_some(),
                    },
                )?;
            }
            Stmt::Wall {
                name,
                from,
                to,
                thick,
                side,
                length,
                ..
            } => {
                self.pos(st, from)?;
                self.pos(st, to)?;
                self.expr(st, thick)?;
                if let Some(l) = length {
                    self.expr(st, l)?;
                }
                self.define(
                    st,
                    name,
                    Kind::Wall {
                        axis: *side == WallSide::Centre,
                    },
                )?;
            }
            Stmt::Opening {
                name,
                width,
                host,
                along,
                ..
            } => {
                self.expr(st, width)?;
                self.path(st, host)?;
                if let Along::Offset(by, from) = along {
                    self.expr(st, by)?;
                    self.path(st, from)?;
                }
                self.define(st, name, Kind::Opening)?;
            }
            Stmt::Grid {
                name,
                labels,
                spacing,
                length,
                at,
                ..
            } => {
                match spacing {
                    Spacing::Every(e) => self.expr(st, e)?,
                    Spacing::List(l) => {
                        for e in l {
                            self.expr(st, e)?;
                        }
                    }
                }
                self.expr(st, length)?;
                if let Some(at) = at {
                    self.pos(st, at)?;
                }
                match self.kinds.get_mut(name) {
                    Some((Kind::Grid(have), _)) => {
                        if let Some(dup) = labels.iter().find(|l| have.contains(l)) {
                            return Err(Self::err(
                                st,
                                st.name_span.unwrap_or_default(),
                                format!("grid {name} already has an axis {dup}"),
                            ));
                        }
                        have.extend(labels.iter().cloned());
                    }
                    _ => self.define(st, name, Kind::Grid(labels.clone()))?,
                }
            }
            Stmt::Array { name, of, kind } => {
                self.path(st, of)?;
                let src = if of.parts.is_empty() {
                    self.kinds
                        .get(&of.root)
                        .map_or(Kind::Used(1), |(k, _)| k.clone())
                } else {
                    Kind::Used(1)
                };
                let n = match kind {
                    ArrayKind::Grid { first, second, .. } => {
                        self.expr(st, &first.0)?;
                        self.expr(st, &first.1)?;
                        let mut n = count(&first.0);
                        if let Some((c, s)) = second {
                            self.expr(st, c)?;
                            self.expr(st, s)?;
                            n = n.zip(count(c)).map(|(a, b)| a * b);
                        }
                        n
                    }
                    ArrayKind::By(c, by) => {
                        self.expr(st, c)?;
                        self.pos(st, by)?;
                        count(c)
                    }
                    ArrayKind::Around(c, about, angle) => {
                        self.expr(st, c)?;
                        self.pos(st, about)?;
                        if let Some(a) = angle {
                            self.expr(st, a)?;
                        }
                        count(c)
                    }
                };
                self.define(st, name, Kind::Array(Box::new(src), n))?;
            }
            Stmt::Relation { args, at, .. } => {
                for a in args {
                    self.path(st, a)?;
                }
                if let Some(at) = at {
                    self.pos(st, at)?;
                }
            }
            Stmt::Dim { a, b, value, .. } => {
                self.path(st, a)?;
                if let Some(b) = b {
                    self.path(st, b)?;
                }
                if let Some(v) = value {
                    self.expr(st, v)?;
                }
            }
            Stmt::Label { of, height, .. } => {
                self.path(st, of)?;
                if let Some(h) = height {
                    self.expr(st, h)?;
                }
            }
            Stmt::Text {
                at, height, angle, ..
            } => {
                self.pos(st, at)?;
                for e in [height, angle].into_iter().flatten() {
                    self.expr(st, e)?;
                }
            }
            Stmt::Hatch {
                of, scale, angle, ..
            } => {
                self.path(st, of)?;
                for e in [scale, angle].into_iter().flatten() {
                    self.expr(st, e)?;
                }
            }
            Stmt::Fill { of, opacity, .. } => {
                self.path(st, of)?;
                if let Some(o) = opacity {
                    self.expr(st, o)?;
                }
            }
            Stmt::Use { ids, name } => {
                for id in ids {
                    if !is_entity_id(&id.root) || !id.parts.is_empty() {
                        return Err(Self::err(
                            st,
                            id.span,
                            format!("`{id}` isn't an entity id like e12"),
                        ));
                    }
                    self.path(st, id)?;
                }
                self.define(st, name, Kind::Used(ids.len()))?;
            }
        }
        Ok(())
    }
}

/// Check every statement's names, parts and variables against the drawing.
pub(super) fn check(statements: &[Statement], doc: &Document) -> Result<(), ScriptError> {
    let mut c = Checker {
        doc,
        kinds: HashMap::new(),
        vars: Vec::new(),
    };
    for st in statements {
        c.statement(st)?;
    }
    Ok(())
}
