//! Statements from a script's lines.

use super::errors::ScriptError;
use super::lex::{Line, Tok, Token};

/// A value as written: Drift expression text, with units and variables.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Expr {
    pub text: String,
    pub span: (usize, usize),
}

impl Expr {
    /// The value in mm (or degrees) when it's a bare number.
    pub fn plain(&self) -> Option<f64> {
        self.text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Part {
    Field(String),
    Index(usize),
}

/// A name and its parts: `bed`, `bed.outer.south`, `cols[3].center`, `e12`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Path {
    pub root: String,
    pub parts: Vec<Part>,
    pub span: (usize, usize),
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.root)?;
        for p in &self.parts {
            match p {
                Part::Field(s) => write!(f, ".{s}")?,
                Part::Index(i) => write!(f, "[{i}]")?,
            }
        }
        Ok(())
    }
}

/// A position: rough coordinates, or a named point.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pos {
    At(Expr, Expr),
    Ref(Path),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum WallSide {
    Outside,
    Inside,
    Centre,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    Right,
    Left,
    Above,
    Below,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Place {
    At(Pos),
    Beside(Side, Path),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum OpeningKind {
    Door,
    Window,
    Opening,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Along {
    Centred,
    Offset(Expr, Path),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Axis {
    Cols,
    Rows,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Spacing {
    Every(Expr),
    List(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ArrayKind {
    /// `count x spacing` along x (or y with `up`), and optionally a second along y.
    Grid {
        first: (Expr, Expr),
        second: Option<(Expr, Expr)>,
        up: bool,
    },
    By(Expr, Pos),
    Around(Expr, Pos, Option<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DimKind {
    Auto,
    Radius,
    Diameter,
    Angle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Dir {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Rel {
    Parallel,
    Perpendicular,
    Equal,
    Collinear,
    Tangent,
    Concentric,
    Coincident,
    Horizontal,
    Vertical,
    Symmetric,
    Midpoint,
    On,
    Fix,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum VarKind {
    Length,
    Angle,
    Number,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Stmt {
    Let {
        name: String,
        value: Expr,
        kind: Option<VarKind>,
    },
    Layer {
        name: String,
        color: Option<String>,
        weight: Option<Expr>,
        line: Option<String>,
        hidden: bool,
        locked: bool,
    },
    On(String),
    Point {
        name: String,
        at: Pos,
        fixed: bool,
    },
    Line {
        name: String,
        from: Pos,
        to: Pos,
        length: Option<Expr>,
        fixed: bool,
        construction: bool,
    },
    Polyline {
        name: String,
        points: Vec<Pos>,
        closed: bool,
        construction: bool,
    },
    Rect {
        name: String,
        w: Expr,
        h: Expr,
        at: Pos,
        centred: bool,
        about: bool,
        fixed: bool,
        construction: bool,
    },
    Circle {
        name: String,
        size: Expr,
        diameter: bool,
        at: Pos,
        fixed: bool,
        construction: bool,
    },
    Arc {
        name: String,
        centre: Pos,
        from: Pos,
        to: Pos,
        construction: bool,
    },
    Room {
        name: String,
        w: Expr,
        h: Expr,
        place: Place,
        walls: Option<(Expr, WallSide)>,
        fixed: bool,
    },
    Wall {
        name: String,
        from: Pos,
        to: Pos,
        thick: Expr,
        side: WallSide,
        length: Option<Expr>,
        fixed: bool,
    },
    Opening {
        kind: OpeningKind,
        name: String,
        width: Expr,
        host: Path,
        along: Along,
    },
    Grid {
        name: String,
        axis: Axis,
        labels: Vec<String>,
        spacing: Spacing,
        length: Expr,
        at: Option<Pos>,
        fixed: bool,
    },
    Array {
        name: String,
        of: Path,
        kind: ArrayKind,
    },
    Relation {
        rel: Rel,
        args: Vec<Path>,
        at: Option<Pos>,
    },
    Dim {
        kind: DimKind,
        a: Path,
        b: Option<Path>,
        dir: Option<Dir>,
        value: Option<Expr>,
        reference: bool,
    },
    Label {
        of: Path,
        text: String,
        height: Option<Expr>,
    },
    Text {
        text: String,
        at: Pos,
        height: Option<Expr>,
        align: Option<String>,
        angle: Option<Expr>,
    },
    Hatch {
        of: Path,
        pattern: Option<String>,
        scale: Option<Expr>,
        angle: Option<Expr>,
        color: Option<String>,
    },
    Fill {
        of: Path,
        color: Option<String>,
        opacity: Option<Expr>,
    },
    Use {
        ids: Vec<Path>,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Statement {
    pub stmt: Stmt,
    pub line: usize,
    pub source: String,
    /// `… on Walls`: the layer for this statement only.
    pub layer: Option<String>,
    /// Where the statement's own name is, for errors about it.
    pub name_span: Option<(usize, usize)>,
}

/// Words that end a value or a list of names.
const KEYWORDS: &[&str] = &[
    "x",
    "at",
    "to",
    "from",
    "in",
    "of",
    "by",
    "walls",
    "thick",
    "offset",
    "centred",
    "centered",
    "centre",
    "center",
    "fixed",
    "closed",
    "about",
    "length",
    "every",
    "spacing",
    "around",
    "angle",
    "ref",
    "horizontal",
    "vertical",
    "on",
    "as",
    "height",
    "align",
    "scale",
    "color",
    "colour",
    "opacity",
    "outside",
    "inside",
    "left",
    "right",
    "above",
    "below",
    "up",
    "number",
    "r",
    "d",
    "radius",
    "diameter",
    "solid",
    "weight",
    "line",
    "hidden",
    "locked",
    "cols",
    "rows",
];

/// Units Drift's expressions take after a number.
const UNITS: &[&str] = &["mm", "cm", "m", "in", "ft", "deg", "rad"];

pub(super) fn is_keyword(w: &str) -> bool {
    KEYWORDS.contains(&w)
}

/// Every statement's keyword, for errors and help.
pub(super) const STATEMENTS: &[&str] = &[
    "let",
    "layer",
    "on",
    "point",
    "line",
    "polyline",
    "rect",
    "circle",
    "arc",
    "construction",
    "room",
    "wall",
    "door",
    "window",
    "opening",
    "grid",
    "array",
    "parallel",
    "perpendicular",
    "equal",
    "collinear",
    "tangent",
    "concentric",
    "coincident",
    "horizontal",
    "vertical",
    "symmetric",
    "midpoint",
    "fix",
    "dim",
    "label",
    "text",
    "hatch",
    "fill",
    "use",
];

type R<T> = Result<T, ScriptError>;

struct P<'a> {
    line: &'a Line,
    toks: &'a [Token],
    k: usize,
}

impl<'a> P<'a> {
    fn peek_at(&self, n: usize) -> Option<&'a Tok> {
        self.toks.get(self.k + n).map(|t| &t.tok)
    }

    fn peek(&self) -> Option<&'a Tok> {
        self.peek_at(0)
    }

    fn word_at(&self, n: usize) -> Option<&'a str> {
        match self.peek_at(n) {
            Some(Tok::Word(w)) => Some(w),
            _ => None,
        }
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let t = self.toks.get(self.k);
        self.k += 1;
        t
    }

    fn eat(&mut self, w: &str) -> bool {
        if self.word_at(0) == Some(w) {
            self.k += 1;
            true
        } else {
            false
        }
    }

    fn eat_any(&mut self, ws: &[&str]) -> bool {
        ws.iter().any(|w| self.eat(w))
    }

    fn sym_at(&self, n: usize, c: char) -> bool {
        self.peek_at(n) == Some(&Tok::Sym(c))
    }

    fn eat_sym(&mut self, c: char) -> bool {
        if self.sym_at(0, c) {
            self.k += 1;
            true
        } else {
            false
        }
    }

    fn span_here(&self) -> (usize, usize) {
        match self.toks.get(self.k) {
            Some(t) => (t.start, t.end),
            None => {
                let n = self.line.text.trim_end().len();
                (n, n + 1)
            }
        }
    }

    /// The text of the next token, for messages.
    fn found(&self) -> String {
        match self.toks.get(self.k) {
            Some(t) => format!("`{}`", &self.line.text[t.start..t.end]),
            None => "the end of the line".into(),
        }
    }

    fn err(&self, msg: impl Into<String>) -> ScriptError {
        let (s, e) = self.span_here();
        ScriptError::new(self.line.number, &self.line.text, msg).span(s, e)
    }

    fn expected(&self, what: &str) -> ScriptError {
        self.err(format!("expected {what}, found {}", self.found()))
    }

    fn expect(&mut self, w: &str, what: &str) -> R<()> {
        if self.eat(w) {
            Ok(())
        } else {
            Err(self.expected(what))
        }
    }

    fn expect_sym(&mut self, c: char) -> R<()> {
        if self.eat_sym(c) {
            Ok(())
        } else {
            Err(self.expected(&format!("`{c}`")))
        }
    }

    /// A new name.
    fn name(&mut self, what: &str) -> R<String> {
        match self.peek() {
            Some(Tok::Word(w)) => {
                self.k += 1;
                Ok(w.clone())
            }
            _ => Err(self.expected(what)),
        }
    }

    /// A name or text in quotes (layer names may have spaces).
    fn name_or_text(&mut self, what: &str) -> R<String> {
        match self.peek() {
            Some(Tok::Word(w)) => {
                self.k += 1;
                Ok(w.clone())
            }
            Some(Tok::Str(s)) => {
                self.k += 1;
                Ok(s.clone())
            }
            _ => Err(self.expected(what)),
        }
    }

    fn text(&mut self, what: &str) -> R<String> {
        match self.peek() {
            Some(Tok::Str(s)) => {
                self.k += 1;
                Ok(s.clone())
            }
            _ => Err(self.expected(what)),
        }
    }

    fn color(&mut self) -> Option<String> {
        self.eat_any(&["color", "colour"]);
        match self.peek() {
            Some(Tok::Color(c)) => {
                self.k += 1;
                Some(c.clone())
            }
            _ => None,
        }
    }

    fn done(&self, takes: &str) -> R<()> {
        if self.k >= self.toks.len() {
            Ok(())
        } else {
            Err(self
                .err(format!("unexpected {} here", self.found()))
                .hint(takes))
        }
    }

    /// The length of an inch part (`6"`, `1/2"`, `6 1/2"`) starting `n` tokens ahead.
    fn inches_at(&self, n: usize) -> Option<usize> {
        let num = |i: usize| matches!(self.peek_at(n + i), Some(Tok::Num(_)));
        let inch = |i: usize| self.sym_at(n + i, '"');
        if num(0) && inch(1) {
            Some(2)
        } else if num(0) && self.sym_at(n + 1, '/') && num(2) && inch(3) {
            Some(4)
        } else if num(0) && num(1) && self.sym_at(n + 2, '/') && num(3) && inch(4) {
            Some(5)
        } else {
            None
        }
    }

    /// Units and marks after a term.
    fn units(&mut self) {
        loop {
            match self.peek() {
                Some(Tok::Word(u)) if UNITS.contains(&u.as_str()) => {
                    // `in` is inches only where it can't start `in <name>`.
                    if u == "in"
                        && self
                            .word_at(1)
                            .is_some_and(|w| !is_keyword(w) && !UNITS.contains(&w))
                    {
                        return;
                    }
                    self.k += 1;
                }
                Some(Tok::Sym('\'')) => {
                    self.k += 1;
                    if self.sym_at(0, '-')
                        && let Some(n) = self.inches_at(1)
                    {
                        self.k += 1 + n;
                    } else if let Some(n) = self.inches_at(0) {
                        self.k += n;
                    }
                }
                Some(Tok::Sym('"' | '°')) => self.k += 1,
                // `6 1/2"`: the fraction after the whole inches.
                Some(Tok::Num(_)) if self.inches_at(0).is_some_and(|n| n == 4) => self.k += 4,
                _ => return,
            }
        }
    }

    fn term(&mut self, what: &str) -> R<()> {
        while self.sym_at(0, '-') || self.sym_at(0, '+') {
            self.k += 1;
        }
        match self.peek() {
            Some(Tok::Num(_)) => {
                self.k += 1;
                // `6 1/2"` or `1/2"` as one literal.
                if self.sym_at(0, '/')
                    && matches!(self.peek_at(1), Some(Tok::Num(_)))
                    && self.sym_at(2, '"')
                {
                    self.k += 3;
                }
            }
            Some(Tok::Word(w)) if !is_keyword(w) => {
                self.k += 1;
                if self.eat_sym('(') && !self.eat_sym(')') {
                    loop {
                        self.expr(what)?;
                        if self.eat_sym(',') {
                            continue;
                        }
                        self.expect_sym(')')?;
                        break;
                    }
                }
            }
            Some(Tok::Sym('(')) => {
                self.k += 1;
                self.expr(what)?;
                self.expect_sym(')')?;
            }
            _ => return Err(self.expected(what)),
        }
        self.units();
        Ok(())
    }

    fn expr(&mut self, what: &str) -> R<Expr> {
        let first = self.k;
        self.term(what)?;
        while matches!(self.peek(), Some(Tok::Sym('+' | '-' | '*' | '/' | '^'))) {
            self.k += 1;
            self.term(what)?;
        }
        let (s, e) = (self.toks[first].start, self.toks[self.k - 1].end);
        let text = self.line.text[s..e].to_owned();
        if let Err(err) = drift_core::expr::parse(&text) {
            return Err(ScriptError::new(
                self.line.number,
                &self.line.text,
                format!("`{text}` isn't a value: {}", err.0),
            )
            .span(s, e));
        }
        Ok(Expr { text, span: (s, e) })
    }

    fn path(&mut self, what: &str) -> R<Path> {
        let start = self.span_here().0;
        let root = match self.peek() {
            Some(Tok::Word(w)) => w.clone(),
            _ => return Err(self.expected(what)),
        };
        self.k += 1;
        let mut parts = Vec::new();
        loop {
            if self.eat_sym('.') {
                match self.bump().map(|t| &t.tok) {
                    Some(Tok::Word(f) | Tok::Num(f)) => parts.push(Part::Field(f.clone())),
                    _ => {
                        self.k -= 1;
                        return Err(self.expected("a part name after `.`"));
                    }
                }
            } else if self.eat_sym('[') {
                let n = match self.peek() {
                    Some(Tok::Num(n)) => n.parse::<usize>().ok(),
                    _ => None,
                };
                let Some(n) = n.filter(|n| *n >= 1) else {
                    return Err(self.expected("a number from 1, as in `cols[2]`"));
                };
                self.k += 1;
                self.expect_sym(']')?;
                parts.push(Part::Index(n));
            } else {
                break;
            }
        }
        let end = self.toks[self.k - 1].end;
        Ok(Path {
            root,
            parts,
            span: (start, end),
        })
    }

    /// Names until the end of the line or a keyword.
    fn paths(&mut self, what: &str) -> R<Vec<Path>> {
        let mut out = vec![self.path(what)?];
        loop {
            self.eat_sym(',');
            match self.peek() {
                Some(Tok::Word(w)) if !is_keyword(w) => out.push(self.path(what)?),
                _ => return Ok(out),
            }
        }
    }

    fn pos(&mut self) -> R<Pos> {
        if self.sym_at(0, '(') {
            let save = self.k;
            self.k += 1;
            if self.expr("x").is_ok() && self.eat_sym(',') {
                let x = {
                    let end = self.k - 1;
                    let s = self.toks[save + 1].start;
                    let e = self.toks[end - 1].end;
                    Expr {
                        text: self.line.text[s..e].to_owned(),
                        span: (s, e),
                    }
                };
                let y = self.expr("a y coordinate")?;
                self.expect_sym(')')?;
                return Ok(Pos::At(x, y));
            }
            self.k = save;
        }
        if self.word_at(0).is_some() {
            let calc = matches!(
                self.peek_at(1),
                Some(Tok::Sym(
                    ',' | '+' | '-' | '*' | '/' | '^' | '(' | '\'' | '"' | '°'
                ))
            ) || self
                .word_at(1)
                .is_some_and(|u| UNITS.contains(&u) && u != "in");
            if !calc {
                return Ok(Pos::Ref(self.path("a point")?));
            }
        }
        let x = self.expr("a position like `0, 0` or a point's name")?;
        if !self.eat_sym(',') {
            return Err(self.expected("`,` and a y coordinate"));
        }
        let y = self.expr("a y coordinate")?;
        Ok(Pos::At(x, y))
    }

    fn label(&mut self) -> R<String> {
        match self.bump().map(|t| &t.tok) {
            Some(Tok::Word(w) | Tok::Num(w)) => Ok(w.clone()),
            _ => {
                self.k -= 1;
                Err(self.expected("a grid label such as `A` or `1`"))
            }
        }
    }

    fn labels(&mut self) -> R<Vec<String>> {
        let first = self.label()?;
        if self.eat_sym('~') {
            let span = self.span_here();
            let last = self.label()?;
            return range(&first, &last).ok_or_else(|| {
                ScriptError::new(
                    self.line.number,
                    &self.line.text,
                    format!("`{first}..{last}` isn't a range of letters (A..F) or numbers (1..8)"),
                )
                .span(span.0, span.1)
            });
        }
        let mut out = vec![first];
        while self.eat_sym(',') {
            out.push(self.label()?);
        }
        Ok(out)
    }
}

/// `A..E` or `1..4`.
fn range(a: &str, b: &str) -> Option<Vec<String>> {
    if let (Ok(x), Ok(y)) = (a.parse::<u32>(), b.parse::<u32>()) {
        return (x <= y && y - x < 200).then(|| (x..=y).map(|n| n.to_string()).collect());
    }
    let (mut ca, mut cb) = (a.chars(), b.chars());
    match (ca.next(), ca.next(), cb.next(), cb.next()) {
        (Some(x), None, Some(y), None)
            if x.is_ascii_alphabetic()
                && y.is_ascii_alphabetic()
                && x.is_ascii_uppercase() == y.is_ascii_uppercase()
                && x <= y =>
        {
            Some((x..=y).map(String::from).collect())
        }
        _ => None,
    }
}

/// Every statement in a script.
pub(super) fn parse(script: &str) -> R<Vec<Statement>> {
    super::lex::lines(script)?.iter().map(statement).collect()
}

fn statement(line: &Line) -> R<Statement> {
    let mut toks: &[Token] = &line.tokens;
    // `… on Layer` at the end: that statement's layer.
    let mut layer = None;
    let n = toks.len();
    if n > 2
        && toks[n - 2].tok == Tok::Word("on".into())
        && toks[0].tok != Tok::Word("on".into())
        && let Tok::Word(l) | Tok::Str(l) = &toks[n - 1].tok
    {
        layer = Some(l.clone());
        toks = &toks[..n - 2];
    }
    let mut p = P { line, toks, k: 0 };
    let construction = p.eat("construction");
    let Some(kw) = p.word_at(0) else {
        return Err(p.expected("a statement such as `room`, `line` or `dim`"));
    };
    p.k += 1;
    let name_span = p.toks.get(p.k).map(|t| (t.start, t.end));
    let stmt = match kw {
        "let" => {
            let name = p.name("a variable name")?;
            p.expect_sym('=')?;
            let value = p.expr("a value")?;
            let kind = if p.eat("number") {
                Some(VarKind::Number)
            } else if p.eat("length") {
                Some(VarKind::Length)
            } else if p.eat("angle") {
                Some(VarKind::Angle)
            } else {
                None
            };
            p.done("write it as: let W = 3.6 m (add `number` or `angle` for other kinds)")?;
            Stmt::Let { name, value, kind }
        }
        "layer" => {
            let name = p.name_or_text("a layer name")?;
            let (mut color, mut weight, mut ltype, mut hidden, mut locked) =
                (None, None, None, false, false);
            while p.k < p.toks.len() {
                if let Some(c) = p.color() {
                    color = Some(c);
                } else if p.eat("weight") {
                    weight = Some(p.expr("a line weight in mm")?);
                } else if p.eat("line") {
                    let t = p.name_or_text("a line type")?;
                    ltype = Some(t);
                } else if p.eat("hidden") {
                    hidden = true;
                } else if p.eat("locked") {
                    locked = true;
                } else {
                    p.done("a layer takes: color #rrggbb, weight 0.5, line continuous|dashed|hidden|center|phantom, hidden, locked")?;
                }
            }
            Stmt::Layer {
                name,
                color,
                weight,
                line: ltype,
                hidden,
                locked,
            }
        }
        "on" if p.toks.len() == 2 => Stmt::On(p.name_or_text("a layer name")?),
        "point" => {
            let name = p.name("a name for the point")?;
            p.expect("at", "`at` and where it goes")?;
            let at = p.pos()?;
            let fixed = p.eat("fixed");
            p.done("write it as: point p at 0, 0 [fixed]")?;
            Stmt::Point { name, at, fixed }
        }
        "line" => {
            let name = p.name("a name for the line")?;
            p.expect("from", "`from`")?;
            let from = p.pos()?;
            p.expect("to", "`to`")?;
            let to = p.pos()?;
            let (mut length, mut fixed) = (None, false);
            while p.k < p.toks.len() {
                if p.eat("length") {
                    length = Some(p.expr("a length")?);
                } else if p.eat("fixed") {
                    fixed = true;
                } else {
                    p.done("write it as: line l from 0,0 to 4200,0 [length 4.2 m] [fixed]")?;
                }
            }
            Stmt::Line {
                name,
                from,
                to,
                length,
                fixed,
                construction,
            }
        }
        "polyline" => {
            let name = p.name("a name for the polyline")?;
            let mut points = vec![p.pos()?];
            while p.k < p.toks.len() && !p.word_at(0).is_some_and(is_keyword) {
                points.push(p.pos()?);
            }
            let closed = p.eat("closed");
            p.done("write it as: polyline site (0,0) (30 m,0) (30 m,20 m) closed")?;
            if points.len() < 2 {
                return Err(p.err("a polyline needs at least two points"));
            }
            Stmt::Polyline {
                name,
                points,
                closed,
                construction,
            }
        }
        "rect" | "rectangle" => {
            let name = p.name("a name for the rectangle")?;
            let about = p.eat("about");
            let w = p.expr("a width")?;
            p.expect("x", "`x` and a height, as in `4200 x 3600`")?;
            let h = p.expr("a height")?;
            let centred = p.eat_any(&["centred", "centered"]);
            p.expect("at", "`at` and where its lower-left corner goes")?;
            let at = p.pos()?;
            let fixed = p.eat("fixed");
            p.done("write it as: rect r 4200 x 3600 at 0,0 [fixed] (or `centred at`)")?;
            Stmt::Rect {
                name,
                w,
                h,
                at,
                centred,
                about,
                fixed,
                construction,
            }
        }
        "circle" => {
            let name = p.name("a name for the circle")?;
            let diameter = if p.eat_any(&["r", "radius"]) {
                false
            } else if p.eat_any(&["d", "diameter"]) {
                true
            } else {
                return Err(p.expected("`r` and a radius or `d` and a diameter"));
            };
            let size = p.expr("a size")?;
            p.expect("at", "`at` and its centre")?;
            let at = p.pos()?;
            let fixed = p.eat("fixed");
            p.done("write it as: circle c r 225 at 0,0 [fixed]")?;
            Stmt::Circle {
                name,
                size,
                diameter,
                at,
                fixed,
                construction,
            }
        }
        "arc" => {
            let name = p.name("a name for the arc")?;
            if !p.eat_any(&["centre", "center"]) {
                return Err(p.expected("`centre`"));
            }
            let centre = p.pos()?;
            p.expect("from", "`from` and where it starts")?;
            let from = p.pos()?;
            p.expect("to", "`to` and where it ends")?;
            let to = p.pos()?;
            p.done("write it as: arc a centre 0,0 from 1000,0 to 0,1000 (counter-clockwise)")?;
            Stmt::Arc {
                name,
                centre,
                from,
                to,
                construction,
            }
        }
        "room" => {
            let name = p.name("a name for the room")?;
            let w = p.expr("a width")?;
            p.expect("x", "`x` and a depth, as in `4200 x 3600`")?;
            let h = p.expr("a depth")?;
            let (mut place, mut walls, mut fixed) = (None, None, false);
            while p.k < p.toks.len() {
                if p.eat("at") {
                    place = Some(Place::At(p.pos()?));
                } else if let Some(side) = [
                    ("right", Side::Right),
                    ("left", Side::Left),
                    ("above", Side::Above),
                    ("below", Side::Below),
                ]
                .iter()
                .find_map(|(w, s)| p.eat(w).then_some(*s))
                {
                    if matches!(side, Side::Right | Side::Left) {
                        p.expect("of", "`of` and the room it's next to")?;
                    }
                    place = Some(Place::Beside(side, p.path("a room")?));
                } else if p.eat("walls") {
                    let t = p.expr("a wall thickness")?;
                    let side = if p.eat("inside") {
                        WallSide::Inside
                    } else if p.eat_any(&["centre", "center"]) {
                        WallSide::Centre
                    } else {
                        p.eat("outside");
                        WallSide::Outside
                    };
                    walls = Some((t, side));
                } else if p.eat("fixed") {
                    fixed = true;
                } else {
                    p.done("a room takes: at 0,0 | right of / left of / above / below <room>, walls 200 [outside|inside|centre], fixed")?;
                }
            }
            let Some(place) = place else {
                return Err(p
                    .err("a room needs a place: `at 0,0`, or `right of <room>`")
                    .hint("write it as: room bed 3.6 m x 4.5 m at 0,0 walls 200"));
            };
            Stmt::Room {
                name,
                w,
                h,
                place,
                walls,
                fixed,
            }
        }
        "wall" => {
            let name = p.name("a name for the wall")?;
            p.expect("from", "`from`")?;
            let from = p.pos()?;
            p.expect("to", "`to`")?;
            let to = p.pos()?;
            let (mut thick, mut side, mut length, mut fixed) =
                (None, WallSide::Centre, None, false);
            while p.k < p.toks.len() {
                if p.eat("thick") {
                    thick = Some(p.expr("a thickness")?);
                } else if p.eat("left") {
                    side = WallSide::Left;
                } else if p.eat("right") {
                    side = WallSide::Right;
                } else if p.eat_any(&["centre", "center"]) {
                    side = WallSide::Centre;
                } else if p.eat("length") {
                    length = Some(p.expr("a length")?);
                } else if p.eat("fixed") {
                    fixed = true;
                } else {
                    p.done("a wall takes: thick 200, left|right|centre, length 8 m, fixed")?;
                }
            }
            let Some(thick) = thick else {
                return Err(p
                    .err("a wall needs a thickness")
                    .hint("write it as: wall w1 from 0,0 to 8 m,0 thick 200"));
            };
            Stmt::Wall {
                name,
                from,
                to,
                thick,
                side,
                length,
                fixed,
            }
        }
        "door" | "window" | "opening" => {
            let kind = match kw {
                "door" => OpeningKind::Door,
                "window" => OpeningKind::Window,
                _ => OpeningKind::Opening,
            };
            let name = p.name(&format!("a name for the {kw}"))?;
            let width = p.expr(&format!("the {kw}'s width, like `900`"))?;
            p.expect("in", "`in` and the wall it's in, like `in bed.south`")?;
            let host = p.path("a wall, like `bed.south`")?;
            let along = if p.eat("offset") {
                let by = p.expr("a distance")?;
                p.expect("from", "`from` and a corner or line, like `from bed.sw`")?;
                Along::Offset(by, p.path("a corner or line")?)
            } else {
                p.eat_any(&["centred", "centered"]);
                Along::Centred
            };
            p.done(&format!(
                "write it as: {kw} {name} 900 in bed.south centred (or `offset 600 from bed.sw`)"
            ))?;
            Stmt::Opening {
                kind,
                name,
                width,
                host,
                along,
            }
        }
        "grid" => {
            let name = if p.word_at(0).is_some_and(|w| w != "cols" && w != "rows") {
                p.name("a name for the grid")?
            } else {
                "grid".to_owned()
            };
            let axis = if p.eat("cols") {
                Axis::Cols
            } else if p.eat("rows") {
                Axis::Rows
            } else {
                return Err(p.expected("`cols` (vertical axes) or `rows` (horizontal axes)"));
            };
            let labels = p.labels()?;
            let (mut spacing, mut length, mut at, mut fixed) = (None, None, None, false);
            while p.k < p.toks.len() {
                if p.eat("every") {
                    spacing = Some(Spacing::Every(p.expr("a spacing")?));
                } else if p.eat("spacing") {
                    let mut list = vec![p.expr("a spacing")?];
                    while p.eat_sym(',') {
                        list.push(p.expr("a spacing")?);
                    }
                    spacing = Some(Spacing::List(list));
                } else if p.eat("length") {
                    length = Some(p.expr("a length")?);
                } else if p.eat("at") {
                    at = Some(p.pos()?);
                } else if p.eat("fixed") {
                    fixed = true;
                } else {
                    p.done("a grid takes: every 6 m (or spacing 6 m, 7.5 m), length 30 m, at 0,0, fixed")?;
                }
            }
            let hint = "write it as: grid cols A..E every 6 m length 30 m at 0,0";
            let Some(spacing) = spacing else {
                return Err(p
                    .err("a grid needs `every <spacing>` or `spacing <a>, <b>, …`")
                    .hint(hint));
            };
            let Some(length) = length else {
                return Err(p
                    .err("a grid needs `length`: how long its lines are")
                    .hint(hint));
            };
            if let Spacing::List(l) = &spacing
                && l.len() + 1 != labels.len()
            {
                return Err(p.err(format!(
                    "{} axes need {} spacings, not {}",
                    labels.len(),
                    labels.len() - 1,
                    l.len()
                )));
            }
            Stmt::Grid {
                name,
                axis,
                labels,
                spacing,
                length,
                at,
                fixed,
            }
        }
        "array" => {
            let name = p.name("a name for the array")?;
            p.expect("of", "`of` and what to copy")?;
            let of = p.path("what to copy")?;
            let count = p.expr("a count")?;
            let kind = if p.eat("x") {
                let step = p.expr("a spacing")?;
                let second = if p.eat_sym(',') {
                    let c = p.expr("a count")?;
                    p.expect("x", "`x` and a spacing")?;
                    Some((c, p.expr("a spacing")?))
                } else {
                    None
                };
                let up = p.eat("up");
                ArrayKind::Grid {
                    first: (count, step),
                    second,
                    up,
                }
            } else if p.eat("by") {
                ArrayKind::By(count, p.pos()?)
            } else if p.eat("around") {
                let about = p.pos()?;
                let angle = if p.eat("angle") {
                    Some(p.expr("an angle")?)
                } else {
                    None
                };
                ArrayKind::Around(count, about, angle)
            } else {
                return Err(p.expected("`x` and a spacing, `by dx, dy`, or `around` a centre"));
            };
            p.done("write it as: array cols of col 5 x 6 m [, 4 x 7.2 m] | 5 by 6000, 0 | 8 around 0,0")?;
            Stmt::Array { name, of, kind }
        }
        "parallel" | "perpendicular" | "equal" | "collinear" | "tangent" | "concentric"
        | "coincident" | "horizontal" | "vertical" | "symmetric" | "midpoint" | "on" | "fix" => {
            let rel = match kw {
                "parallel" => Rel::Parallel,
                "perpendicular" => Rel::Perpendicular,
                "equal" => Rel::Equal,
                "collinear" => Rel::Collinear,
                "tangent" => Rel::Tangent,
                "concentric" => Rel::Concentric,
                "coincident" => Rel::Coincident,
                "horizontal" => Rel::Horizontal,
                "vertical" => Rel::Vertical,
                "symmetric" => Rel::Symmetric,
                "midpoint" => Rel::Midpoint,
                "on" => Rel::On,
                _ => Rel::Fix,
            };
            let mut args = p.paths("a name")?;
            let mut at = None;
            match rel {
                Rel::Symmetric => {
                    p.expect("about", "`about` and the axis line")?;
                    args.push(p.path("an axis line")?);
                }
                Rel::Midpoint => {
                    p.expect("of", "`of` and a line")?;
                    args.push(p.path("a line")?);
                }
                Rel::Fix if p.eat("at") => at = Some(p.pos()?),
                _ => {}
            }
            let (min, max, usage) = match rel {
                Rel::Equal | Rel::Collinear => (2, 99, "equal a b [c …]"),
                Rel::Horizontal | Rel::Vertical => (1, 2, "horizontal line (or two points)"),
                Rel::Fix => (1, 1, "fix p [at 0,0]"),
                Rel::Symmetric => (3, 3, "symmetric p q about axis"),
                Rel::Midpoint => (2, 2, "midpoint p of line"),
                Rel::On => (2, 2, "on point curve"),
                _ => (2, 2, "parallel a b"),
            };
            p.done(&format!("write it as: {usage}"))?;
            if args.len() < min || args.len() > max {
                return Err(p.err(format!(
                    "`{kw}` takes {}",
                    usage.trim_start_matches(kw).trim()
                )));
            }
            Stmt::Relation { rel, args, at }
        }
        "dim" => {
            let kind = match p.word_at(0) {
                Some(w @ ("r" | "d" | "radius" | "diameter" | "angle"))
                    if p.word_at(1).is_some_and(|n| !is_keyword(n)) =>
                {
                    p.k += 1;
                    match w {
                        "r" | "radius" => DimKind::Radius,
                        "d" | "diameter" => DimKind::Diameter,
                        _ => DimKind::Angle,
                    }
                }
                _ => DimKind::Auto,
            };
            let a = p.path("what to dimension")?;
            let b = if kind == DimKind::Angle {
                p.eat("to");
                Some(p.path("a second line")?)
            } else if p.eat("to") {
                Some(p.path("what to measure to")?)
            } else {
                None
            };
            let direction = |p: &mut P<'_>| {
                if p.eat("horizontal") {
                    Some(Dir::Horizontal)
                } else if p.eat("vertical") {
                    Some(Dir::Vertical)
                } else {
                    None
                }
            };
            let mut dir = direction(&mut p);
            let value = if p.eat_sym('=') {
                Some(p.expr("a value, like `3.6 m` or `W/2`")?)
            } else {
                None
            };
            dir = dir.or_else(|| direction(&mut p));
            let reference = p.eat("ref");
            p.done("write it as: dim bed.south = 3.6 m | dim a to b = 1200 [horizontal|vertical] | dim r c = 225 | dim angle a b = 30° | dim bed.south ref")?;
            if value.is_none() && !reference {
                return Err(p
                    .err("a dimension needs `= <value>`, or `ref` to show it only")
                    .hint("write it as: dim bed.south = 3.6 m"));
            }
            Stmt::Dim {
                kind,
                a,
                b,
                dir,
                value,
                reference,
            }
        }
        "label" => {
            let of = p.path("a room or shape to label")?;
            let text = p.text("the label in quotes, like \"Bedroom 1\"")?;
            let height = if p.eat("height") {
                Some(p.expr("a text height")?)
            } else {
                None
            };
            p.done("write it as: label bed \"Bedroom 1\" [height 250]")?;
            Stmt::Label { of, text, height }
        }
        "text" => {
            let text = p.text("the text in quotes")?;
            p.expect("at", "`at` and where it goes")?;
            let at = p.pos()?;
            let (mut height, mut align, mut angle) = (None, None, None);
            while p.k < p.toks.len() {
                if p.eat("height") {
                    height = Some(p.expr("a height")?);
                } else if p.eat("align") {
                    let a = p.name_or_text("left, centre or right")?;
                    align = Some(a);
                } else if p.eat("angle") {
                    angle = Some(p.expr("an angle")?);
                } else {
                    p.done("text takes: height 250, align left|centre|right, angle 90")?;
                }
            }
            Stmt::Text {
                text,
                at,
                height,
                align,
                angle,
            }
        }
        "hatch" => {
            let of = p.path("what to hatch, like `bed.walls`")?;
            let pattern = match p.peek() {
                Some(Tok::Word(w)) if !is_keyword(w) => {
                    p.k += 1;
                    Some(w.clone())
                }
                Some(Tok::Str(s)) => {
                    p.k += 1;
                    Some(s.clone())
                }
                _ => None,
            };
            let (mut scale, mut angle, mut color) = (None, None, None);
            while p.k < p.toks.len() {
                if p.eat("scale") {
                    scale = Some(p.expr("a scale")?);
                } else if p.eat("angle") {
                    angle = Some(p.expr("an angle")?);
                } else if let Some(c) = p.color() {
                    color = Some(c);
                } else {
                    p.done("hatch takes: a pattern (ANSI31), scale 2, angle 45, color #rrggbb")?;
                }
            }
            Stmt::Hatch {
                of,
                pattern,
                scale,
                angle,
                color,
            }
        }
        "fill" => {
            let of = p.path("what to fill, like `bed`")?;
            p.eat("solid");
            let (mut color, mut opacity) = (None, None);
            while p.k < p.toks.len() {
                if let Some(c) = p.color() {
                    color = Some(c);
                } else if p.eat("opacity") {
                    opacity = Some(p.expr("an opacity from 0 to 1")?);
                } else {
                    p.done("fill takes: #rrggbb, opacity 0.5")?;
                }
            }
            Stmt::Fill { of, color, opacity }
        }
        "use" => {
            let ids = p.paths("an id like `e12`")?;
            p.expect("as", "`as` and a name")?;
            let name = p.name("a name")?;
            p.done("write it as: use e12 as north_wall")?;
            Stmt::Use { ids, name }
        }
        other => {
            p.k -= 1;
            let guess = super::names::closest(other, STATEMENTS.iter().copied());
            let mut e = p.err(format!("unknown statement `{other}`"));
            if let Some(g) = guess {
                e = e.hint(format!("did you mean `{g}`?"));
            }
            return Err(e);
        }
    };
    if construction
        && !matches!(
            stmt,
            Stmt::Line { .. }
                | Stmt::Polyline { .. }
                | Stmt::Rect { .. }
                | Stmt::Circle { .. }
                | Stmt::Arc { .. }
        )
    {
        return Err(ScriptError::new(
            line.number,
            &line.text,
            "`construction` goes before line, polyline, rect, circle or arc",
        )
        .span(line.tokens[0].start, line.tokens[0].end));
    }
    Ok(Statement {
        stmt,
        line: line.number,
        source: line.text.clone(),
        layer,
        name_span,
    })
}
