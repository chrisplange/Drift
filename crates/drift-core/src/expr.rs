//! A small expression language for dimension values and variables, with units.
//!
//! ```text
//! width / 2 + 3 mm        sqrt(a^2 + b^2)        1'-6 1/2"        atan2(rise, run)
//! ```
//!
//! - Arithmetic `+ - * / ^`, parentheses, and functions `sqrt abs min max hypot round floor
//!   ceil sin cos tan asin acos atan atan2`, plus the constant `pi`.
//! - Units after a term apply to the whole product before them (`100/2 mm` is 50 mm, `pi/4 rad`
//!   is 45°), except right before `^` (`3mm^2` is (3 mm)²): `mm cm m in ft deg rad`, and the
//!   marks `' " °`.
//!   Architectural forms like `1'-6"`, `1' 6 1/2"`, `6-1/2"` and `3/8"` are single literals,
//!   so `1'-6"` means one foot six inches rather than a subtraction.
//! - Lengths, angles and plain numbers are checked: adding a length to an angle is an error.
//!   Trigonometry takes angles, or plain numbers as degrees (the CAD convention).
//! - A result with no units is read in the context's unit (see [`evaluate_input`]).

use std::f64::consts::PI;
use std::fmt;

use crate::units::{Dim, Kind, LengthFormat, MM_PER_INCH, Quantity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError(pub String);

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExprError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError(msg.into()))
}

/// Unit names usable after a number: (name, base value, dimension).
const UNITS: &[(&str, f64, Dim)] = &[
    ("mm", 1.0, Dim::LENGTH),
    ("cm", 10.0, Dim::LENGTH),
    ("m", 1000.0, Dim::LENGTH),
    ("in", MM_PER_INCH, Dim::LENGTH),
    ("inch", MM_PER_INCH, Dim::LENGTH),
    ("ft", 12.0 * MM_PER_INCH, Dim::LENGTH),
    ("deg", 1.0, Dim::ANGLE),
    ("rad", 180.0 / PI, Dim::ANGLE),
];

const FUNCTIONS: &[&str] = &[
    "sqrt", "abs", "min", "max", "hypot", "round", "floor", "ceil", "sin", "cos", "tan", "asin",
    "acos", "atan", "atan2",
];

const CONSTANTS: &[&str] = &["pi"];

fn unit(name: &str) -> Option<Quantity> {
    UNITS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|&(_, value, dim)| Quantity { value, dim })
}

/// Check a variable name: letters, digits and `_`, not starting with a digit, and not a unit,
/// function or constant.
pub fn valid_name(name: &str) -> Result<(), ExprError> {
    let mut chars = name.chars();
    let ok_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !ok_start || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return err(format!(
            "`{name}` isn't a valid name: use letters, digits and _"
        ));
    }
    if unit(name).is_some() || FUNCTIONS.contains(&name) || CONSTANTS.contains(&name) {
        return err(format!("`{name}` is reserved"));
    }
    Ok(())
}

// ----- Lexer ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    /// An imperial literal such as `1'-6 1/2"` (already in mm).
    Literal(Quantity),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
    /// `'`, `"` or `°` after a value.
    Mark(char),
}

struct Lexer<'a> {
    chars: Vec<char>,
    text: &'a str,
}

impl Lexer<'_> {
    fn number_at(&self, mut i: usize) -> Option<(f64, usize)> {
        let start = i;
        let c = &self.chars;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
        }
        if i < c.len() && c[i] == '.' {
            i += 1;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == start || (i == start + 1 && c[start] == '.') {
            return None;
        }
        // Exponent, only if digits follow (so `2em` isn't misread).
        if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
            let mut j = i + 1;
            if j < c.len() && (c[j] == '+' || c[j] == '-') {
                j += 1;
            }
            if j < c.len() && c[j].is_ascii_digit() {
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        let s: String = c[start..i].iter().collect();
        s.parse().ok().map(|v| (v, i))
    }

    fn integer_at(&self, i: usize) -> Option<(f64, usize)> {
        let c = &self.chars;
        let mut j = i;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        (j > i).then(|| {
            let s: String = c[i..j].iter().collect();
            (s.parse().unwrap_or(0.0), j)
        })
    }

    /// `a/b` with integers.
    fn fraction_at(&self, i: usize) -> Option<(f64, usize)> {
        let (n, j) = self.integer_at(i)?;
        if self.chars.get(j) != Some(&'/') {
            return None;
        }
        let (d, k) = self.integer_at(j + 1)?;
        (d != 0.0).then_some((n / d, k))
    }

    /// Inches: `6"`, `6.5"`, `3/8"`, `6-1/2"`, `6 1/2"`. Returns inches and the end.
    fn inches_at(&self, i: usize) -> Option<(f64, usize)> {
        let c = &self.chars;
        let close = |j: usize| (c.get(j) == Some(&'"')).then_some(j + 1);
        if let Some((f, j)) = self.fraction_at(i)
            && let Some(end) = close(j)
        {
            return Some((f, end));
        }
        let (n, j) = self.number_at(i)?;
        if let Some(end) = close(j) {
            return Some((n, end));
        }
        // Whole inches then a fraction, joined by `-` or spaces.
        let mut k = j;
        if c.get(k) == Some(&'-') {
            k += 1;
        } else {
            while c.get(k) == Some(&' ') {
                k += 1;
            }
            if k == j {
                return None;
            }
        }
        let (f, m) = self.fraction_at(k)?;
        close(m).map(|end| (n + f, end))
    }

    /// Feet with optional inches: `2'`, `1'-6"`, `1' 6 1/2"`, or inches alone.
    fn imperial_at(&self, i: usize) -> Option<(Quantity, usize)> {
        let c = &self.chars;
        if let Some((feet, j)) = self.number_at(i)
            && c.get(j) == Some(&'\'')
        {
            let after = j + 1;
            let mut k = after;
            if c.get(k) == Some(&'-') {
                k += 1;
            } else {
                while c.get(k) == Some(&' ') {
                    k += 1;
                }
            }
            if let Some((inches, end)) = self.inches_at(k) {
                let total = feet * 12.0 + inches;
                return Some((Quantity::length(total * MM_PER_INCH), end));
            }
            return Some((Quantity::length(feet * 12.0 * MM_PER_INCH), after));
        }
        self.inches_at(i)
            .map(|(inches, end)| (Quantity::length(inches * MM_PER_INCH), end))
    }

    fn tokens(&self) -> Result<Vec<(Tok, usize, usize)>, ExprError> {
        let c = &self.chars;
        let mut out = Vec::new();
        let mut i = 0;
        while i < c.len() {
            let ch = c[i];
            let start = i;
            if ch.is_whitespace() {
                i += 1;
                continue;
            }
            if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit))
            {
                if let Some((q, end)) = self.imperial_at(i) {
                    out.push((Tok::Literal(q), start, end));
                    i = end;
                } else if let Some((v, end)) = self.number_at(i) {
                    out.push((Tok::Num(v), start, end));
                    i = end;
                } else {
                    return err(format!("bad number at `{}`", self.rest(i)));
                }
                continue;
            }
            if ch.is_ascii_alphabetic() || ch == '_' {
                while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
                out.push((Tok::Ident(c[start..i].iter().collect()), start, i));
                continue;
            }
            let tok = match ch {
                '+' | '-' | '*' | '/' | '^' => Tok::Op(ch),
                '×' => Tok::Op('*'),
                '÷' => Tok::Op('/'),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                '\'' | '"' | '°' => Tok::Mark(ch),
                _ => return err(format!("unexpected `{ch}`")),
            };
            out.push((tok, start, i + 1));
            i += 1;
        }
        Ok(out)
    }

    fn rest(&self, i: usize) -> String {
        let s: String = self.chars[i..].iter().collect();
        if s.is_empty() { self.text.into() } else { s }
    }
}

// ----- Parser --------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Value(Quantity),
    Var(String),
    Neg(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// A parsed expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    root: Node,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn sum(&mut self) -> Result<Node, ExprError> {
        let mut left = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let right = self.product()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// The unit at `pos`, if any.
    fn unit_at(&self, pos: usize) -> Option<Quantity> {
        match self.toks.get(pos) {
            Some(Tok::Ident(name)) => unit(name),
            Some(Tok::Mark('\'')) => unit("ft"),
            Some(Tok::Mark('"')) => unit("in"),
            Some(Tok::Mark('°')) => unit("deg"),
            _ => None,
        }
    }

    fn product(&mut self) -> Result<Node, ExprError> {
        let mut left = self.unary()?;
        loop {
            // A unit applies to the product so far.
            if let Some(u) = self.unit_at(self.pos) {
                self.pos += 1;
                left = Node::Bin('*', Box::new(left), Box::new(Node::Value(u)));
                continue;
            }
            let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() else {
                break;
            };
            self.pos += 1;
            let right = self.unary()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(Node::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Node, ExprError> {
        let mut base = self.primary()?;
        // `3mm^2`: a unit right before `^` belongs to the base.
        if let Some(u) = self.unit_at(self.pos)
            && self.toks.get(self.pos + 1) == Some(&Tok::Op('^'))
        {
            self.pos += 1;
            base = Node::Bin('*', Box::new(base), Box::new(Node::Value(u)));
        }
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let exp = self.unary()?;
            return Ok(Node::Bin('^', Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Node::Value(Quantity::number(v))),
            Some(Tok::Literal(q)) => Ok(Node::Value(q)),
            Some(Tok::LParen) => {
                let inner = self.sum()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(inner),
                    _ => err("missing `)`"),
                }
            }
            Some(Tok::Ident(name)) => {
                if let Some(Tok::LParen) = self.peek() {
                    if !FUNCTIONS.contains(&name.as_str()) {
                        return err(format!("unknown function `{name}`"));
                    }
                    self.pos += 1;
                    let mut args = Vec::new();
                    if let Some(Tok::RParen) = self.peek() {
                        self.pos += 1;
                    } else {
                        loop {
                            args.push(self.sum()?);
                            match self.next() {
                                Some(Tok::Comma) => {}
                                Some(Tok::RParen) => break,
                                _ => return err(format!("missing `)` after {name}(…")),
                            }
                        }
                    }
                    return Ok(Node::Call(name, args));
                }
                if name == "pi" {
                    return Ok(Node::Value(Quantity::number(PI)));
                }
                if unit(&name).is_some() {
                    return err(format!("`{name}` needs a number before it"));
                }
                Ok(Node::Var(name))
            }
            Some(Tok::Op(op)) => err(format!("unexpected `{op}`")),
            Some(Tok::Mark(m)) => err(format!("unexpected `{m}`")),
            Some(Tok::RParen) => err("unexpected `)`"),
            Some(Tok::Comma) => err("unexpected `,`"),
            None => err("expression ends too soon"),
        }
    }
}

/// Parse an expression.
pub fn parse(text: &str) -> Result<Expr, ExprError> {
    let lexer = Lexer {
        chars: text.chars().collect(),
        text,
    };
    let toks: Vec<Tok> = lexer.tokens()?.into_iter().map(|(t, ..)| t).collect();
    if toks.is_empty() {
        return err("empty expression");
    }
    let mut p = Parser { toks, pos: 0 };
    let root = p.sum()?;
    if let Some(t) = p.peek() {
        return err(match t {
            Tok::Ident(name) => format!("unexpected `{name}`"),
            Tok::RParen => "unexpected `)`".into(),
            _ => "unexpected text after the expression".into(),
        });
    }
    Ok(Expr { root })
}

/// Look up a variable's value: `None` if there is no such variable.
pub type Lookup<'a> = dyn Fn(&str) -> Option<Result<Quantity, ExprError>> + 'a;

fn same_dim(op: &str, a: Quantity, b: Quantity) -> Result<(), ExprError> {
    if a.dim == b.dim {
        Ok(())
    } else {
        err(format!("can't {op} {} and {}", a.dim.name(), b.dim.name()))
    }
}

fn scale_dim(d: Dim, k: f64) -> Option<Dim> {
    let l = f64::from(d.length) * k;
    let a = f64::from(d.angle) * k;
    let int = |x: f64| ((x - x.round()).abs() < 1e-9).then_some(x.round() as i8);
    Some(Dim {
        length: int(l)?,
        angle: int(a)?,
    })
}

/// Radians for a trigonometric argument: angles, or plain numbers taken as degrees.
fn radians(q: Quantity, f: &str) -> Result<f64, ExprError> {
    match q.dim {
        d if d == Dim::ANGLE || d.is_none() => Ok(q.value.to_radians()),
        d => err(format!("{f}() takes an angle, not {}", d.name())),
    }
}

impl Node {
    fn eval(&self, vars: &Lookup<'_>) -> Result<Quantity, ExprError> {
        Ok(match self {
            Node::Value(q) => *q,
            Node::Var(name) => match vars(name) {
                Some(r) => r?,
                None => return err(format!("unknown variable `{name}`")),
            },
            Node::Neg(a) => {
                let q = a.eval(vars)?;
                Quantity {
                    value: -q.value,
                    ..q
                }
            }
            Node::Bin(op, a, b) => {
                let (a, b) = (a.eval(vars)?, b.eval(vars)?);
                match op {
                    '+' => {
                        same_dim("add", a, b)?;
                        Quantity {
                            value: a.value + b.value,
                            dim: a.dim,
                        }
                    }
                    '-' => {
                        same_dim("subtract", a, b)?;
                        Quantity {
                            value: a.value - b.value,
                            dim: a.dim,
                        }
                    }
                    '*' => Quantity {
                        value: a.value * b.value,
                        dim: Dim {
                            length: a.dim.length + b.dim.length,
                            angle: a.dim.angle + b.dim.angle,
                        },
                    },
                    '/' => {
                        if b.value == 0.0 {
                            return err("division by zero");
                        }
                        Quantity {
                            value: a.value / b.value,
                            dim: Dim {
                                length: a.dim.length - b.dim.length,
                                angle: a.dim.angle - b.dim.angle,
                            },
                        }
                    }
                    _ => {
                        if !b.dim.is_none() {
                            return err("an exponent must be a plain number");
                        }
                        let Some(dim) = scale_dim(a.dim, b.value) else {
                            return err(format!(
                                "can't raise {} to the power {}",
                                a.dim.name(),
                                b.value
                            ));
                        };
                        Quantity {
                            value: a.value.powf(b.value),
                            dim,
                        }
                    }
                }
            }
            Node::Call(f, args) => {
                let args: Vec<Quantity> = args
                    .iter()
                    .map(|a| a.eval(vars))
                    .collect::<Result<_, _>>()?;
                call(f, &args)?
            }
        })
    }

    fn identifiers(&self, out: &mut Vec<String>) {
        match self {
            Node::Value(_) => {}
            Node::Var(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Node::Neg(a) => a.identifiers(out),
            Node::Bin(_, a, b) => {
                a.identifiers(out);
                b.identifiers(out);
            }
            Node::Call(_, args) => args.iter().for_each(|a| a.identifiers(out)),
        }
    }
}

fn call(f: &str, args: &[Quantity]) -> Result<Quantity, ExprError> {
    let arity = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            err(format!(
                "{f}() takes {n} argument{}",
                if n == 1 { "" } else { "s" }
            ))
        }
    };
    let number = |q: Quantity| {
        if q.dim.is_none() {
            Ok(q.value)
        } else {
            err(format!("{f}() takes a plain number, not {}", q.dim.name()))
        }
    };
    Ok(match f {
        "sqrt" => {
            arity(1)?;
            let q = args[0];
            if q.value < 0.0 {
                return err("sqrt() of a negative value");
            }
            let Some(dim) = scale_dim(q.dim, 0.5) else {
                return err(format!("can't take the square root of {}", q.dim.name()));
            };
            Quantity {
                value: q.value.sqrt(),
                dim,
            }
        }
        "abs" => {
            arity(1)?;
            Quantity {
                value: args[0].value.abs(),
                ..args[0]
            }
        }
        "round" | "floor" | "ceil" => {
            arity(1)?;
            let v = number(args[0])?;
            Quantity::number(match f {
                "round" => v.round(),
                "floor" => v.floor(),
                _ => v.ceil(),
            })
        }
        "min" | "max" | "hypot" => {
            if args.is_empty() {
                return err(format!("{f}() needs arguments"));
            }
            for q in &args[1..] {
                same_dim(&format!("use {f}() on"), args[0], *q)?;
            }
            let values = args.iter().map(|q| q.value);
            let value = match f {
                "min" => values.fold(f64::INFINITY, f64::min),
                "max" => values.fold(f64::NEG_INFINITY, f64::max),
                _ => values.map(|v| v * v).sum::<f64>().sqrt(),
            };
            Quantity {
                value,
                dim: args[0].dim,
            }
        }
        "sin" | "cos" | "tan" => {
            arity(1)?;
            let r = radians(args[0], f)?;
            Quantity::number(match f {
                "sin" => r.sin(),
                "cos" => r.cos(),
                _ => r.tan(),
            })
        }
        "asin" | "acos" | "atan" => {
            arity(1)?;
            let v = number(args[0])?;
            if f != "atan" && !(-1.0..=1.0).contains(&v) {
                return err(format!("{f}() needs a value between -1 and 1"));
            }
            Quantity::angle(
                match f {
                    "asin" => v.asin(),
                    "acos" => v.acos(),
                    _ => v.atan(),
                }
                .to_degrees(),
            )
        }
        "atan2" => {
            arity(2)?;
            same_dim("use atan2() on", args[0], args[1])?;
            Quantity::angle(args[0].value.atan2(args[1].value).to_degrees())
        }
        _ => return err(format!("unknown function `{f}`")),
    })
}

impl Expr {
    /// Evaluate with variables from `vars`.
    pub fn eval(&self, vars: &Lookup<'_>) -> Result<Quantity, ExprError> {
        let q = self.root.eval(vars)?;
        if q.value.is_finite() {
            Ok(q)
        } else {
            err("the result isn't a finite number")
        }
    }

    /// Variables the expression refers to, in order of first use.
    pub fn identifiers(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.root.identifiers(&mut out);
        out
    }

    /// True for a single number with no units (e.g. `12.5`).
    pub fn is_bare_number(&self) -> bool {
        matches!(self.root, Node::Value(q) if q.dim.is_none())
    }
}

/// Convert a quantity to a value of `kind` as dimensions store it (mm, degrees, plain). A
/// result without units is read in the context's unit: `default` for lengths, degrees for
/// angles.
pub fn to_kind(q: Quantity, kind: Kind, default: LengthFormat) -> Result<f64, ExprError> {
    match (kind, q.dim) {
        (Kind::Length, d) if d == Dim::LENGTH => Ok(q.value),
        (Kind::Length, d) if d.is_none() => Ok(q.value * default.mm_per_unit()),
        (Kind::Angle, d) if d == Dim::ANGLE => Ok(q.value),
        (Kind::Angle, d) if d.is_none() => Ok(q.value),
        (Kind::Number, d) if d.is_none() => Ok(q.value),
        (kind, d) => err(format!("expected {}, got {}", kind.dim().name(), d.name())),
    }
}

/// A value of `kind` (mm, degrees, plain) as a quantity in base units.
pub fn from_kind(value: f64, kind: Kind) -> Quantity {
    match kind {
        Kind::Length => Quantity::length(value),
        Kind::Angle => Quantity::angle(value),
        Kind::Number => Quantity::number(value),
    }
}

/// The result of evaluating typed input in a context.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluated {
    /// The value as stored for the kind (mm, degrees, plain).
    pub value: f64,
    /// The text to store: with the context's unit made explicit when the input had none, so
    /// changing the display unit later never changes the value.
    pub text: String,
    /// Variables the expression uses (empty for a constant).
    pub references: Vec<String>,
}

/// Evaluate typed input for a value of `kind`, reading bare numbers in `default`.
pub fn evaluate_input(
    text: &str,
    kind: Kind,
    default: LengthFormat,
    vars: &Lookup<'_>,
) -> Result<Evaluated, ExprError> {
    let text = text.trim();
    let expr = parse(text)?;
    let q = expr.eval(vars)?;
    let value = to_kind(q, kind, default)?;
    let unit = match kind {
        Kind::Length => default.symbol(),
        Kind::Angle => "deg",
        Kind::Number => "",
    };
    let text = if q.dim.is_none() && !unit.is_empty() {
        if expr.is_bare_number() {
            format!("{text} {unit}")
        } else {
            format!("({text}) {unit}")
        }
    } else {
        text.to_owned()
    };
    Ok(Evaluated {
        value,
        text,
        references: expr.identifiers(),
    })
}

/// Replace whole-word uses of variable `old` with `new` (for renaming a variable).
pub fn rename(text: &str, old: &str, new: &str) -> String {
    let lexer = Lexer {
        chars: text.chars().collect(),
        text,
    };
    let Ok(tokens) = lexer.tokens() else {
        return text.to_owned();
    };
    let mut out = String::new();
    let mut last = 0;
    for (i, (tok, start, end)) in tokens.iter().enumerate() {
        let is_call = matches!(tokens.get(i + 1), Some((Tok::LParen, ..)));
        if let Tok::Ident(name) = tok
            && name == old
            && !is_call
        {
            out.extend(&lexer.chars[last..*start]);
            out.push_str(new);
            last = *end;
        }
    }
    out.extend(&lexer.chars[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> Result<Quantity, ExprError> {
        let vars = |name: &str| match name {
            "width" => Some(Ok(Quantity::length(120.0))),
            "n" => Some(Ok(Quantity::number(4.0))),
            "broken" => Some(err("broken")),
            _ => None,
        };
        parse(text)?.eval(&vars)
    }

    fn mm(text: &str) -> f64 {
        let q = eval(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(q.dim, Dim::LENGTH, "{text}");
        q.value
    }

    #[test]
    fn arithmetic_and_units() {
        assert_eq!(mm("1in + 3mm"), 28.4);
        assert_eq!(mm("width / 2 + 3 mm"), 63.0);
        assert_eq!(mm("2 * (10 mm + 5mm)"), 30.0);
        assert_eq!(mm("(1 + 2) cm"), 30.0);
        assert_eq!(mm("100/2 mm"), 50.0);
        assert_eq!(mm("1in + 100/2 mm"), 75.4);
        assert_eq!(mm("-2mm"), -2.0);
        assert_eq!(mm("sqrt(3mm^2 + 4mm^2)"), 5.0);
        assert_eq!(mm("hypot(3mm, 4mm)"), 5.0);
        assert_eq!(mm("max(width, 1m) - width"), 880.0);
        assert_eq!(mm("width / n"), 30.0);
        assert_eq!(mm("1.5e2 mm"), 150.0);
        assert_eq!(eval("2 + 3 * 4 ^ 2").unwrap(), Quantity::number(50.0));
        assert_eq!(eval("round(2.5)").unwrap(), Quantity::number(3.0));
    }

    #[test]
    fn imperial_literals() {
        let inch = MM_PER_INCH;
        assert_eq!(mm("2'"), 24.0 * inch);
        assert_eq!(mm("3\""), 3.0 * inch);
        assert_eq!(mm("1'6\""), 18.0 * inch);
        assert_eq!(mm("1'-6\""), 18.0 * inch);
        assert_eq!(mm("1' 6 1/2\""), 18.5 * inch);
        assert_eq!(mm("1'-6 1/2\""), 18.5 * inch);
        assert_eq!(mm("6-1/2\""), 6.5 * inch);
        assert_eq!(mm("6 1/2\""), 6.5 * inch);
        assert_eq!(mm("3/8\""), 0.375 * inch);
        assert_eq!(mm("2.5\""), 2.5 * inch);
        // With spaces around `-`, it's arithmetic.
        assert!((mm("2' - 6\"") - 18.0 * inch).abs() < 1e-12);
        // Not imperial: plain arithmetic.
        assert_eq!(eval("10-1/2").unwrap(), Quantity::number(9.5));
        assert!((mm("1' + 1\"") - 13.0 * inch).abs() < 1e-12);
    }

    #[test]
    fn angles_and_trigonometry() {
        let q = eval("30deg").unwrap();
        assert_eq!(q.dim, Dim::ANGLE);
        assert_eq!(q.value, 30.0);
        assert!((eval("sin(30)").unwrap().value - 0.5).abs() < 1e-15);
        assert!((eval("sin(30°)").unwrap().value - 0.5).abs() < 1e-15);
        assert!((eval("cos(pi rad)").unwrap().value + 1.0).abs() < 1e-15);
        let a = eval("atan2(1mm, 1mm)").unwrap();
        assert_eq!(a.dim, Dim::ANGLE);
        assert!((to_kind(a, Kind::Angle, LengthFormat::Millimetre).unwrap() - 45.0).abs() < 1e-12);
    }

    #[test]
    fn errors_explain_themselves() {
        let msg = |t: &str| eval(t).unwrap_err().0;
        assert_eq!(msg("10 + 5mm"), "can't add a number and a length");
        assert_eq!(msg("5mm + 2deg"), "can't add a length and an angle");
        assert_eq!(msg("height * 2"), "unknown variable `height`");
        assert_eq!(msg("broken + 1"), "broken");
        assert_eq!(msg("frob(2)"), "unknown function `frob`");
        assert_eq!(msg("(1 + 2"), "missing `)`");
        assert_eq!(msg("1 / 0"), "division by zero");
        assert_eq!(msg("sqrt(2mm)"), "can't take the square root of a length");
        assert_eq!(msg("2 width"), "unexpected `width`");
        assert_eq!(msg(""), "empty expression");
        assert_eq!(msg("mm"), "`mm` needs a number before it");
        assert_eq!(msg("1 $"), "unexpected `$`");
    }

    #[test]
    fn input_is_read_in_the_context_unit_and_normalised() {
        let none = |_: &str| None;
        let r = evaluate_input("2", Kind::Length, LengthFormat::Inch, &none).unwrap();
        assert_eq!((r.value, r.text.as_str()), (2.0 * MM_PER_INCH, "2 in"));
        let r = evaluate_input("10*3", Kind::Length, LengthFormat::Millimetre, &none).unwrap();
        assert_eq!((r.value, r.text.as_str()), (30.0, "(10*3) mm"));
        let r = evaluate_input("1'6\"", Kind::Length, LengthFormat::Millimetre, &none).unwrap();
        assert_eq!(r.text, "1'6\"");
        let r = evaluate_input("45", Kind::Angle, LengthFormat::Millimetre, &none).unwrap();
        assert_eq!((r.value, r.text.as_str()), (45.0, "45 deg"));
        let r = evaluate_input("pi/4 rad", Kind::Angle, LengthFormat::Millimetre, &none).unwrap();
        assert!((r.value - 45.0).abs() < 1e-12);
        assert!(evaluate_input("3mm", Kind::Angle, LengthFormat::Millimetre, &none).is_err());
        let vars = |n: &str| (n == "w").then_some(Ok(Quantity::length(10.0)));
        let r = evaluate_input("w/2", Kind::Length, LengthFormat::Inch, &vars).unwrap();
        assert_eq!((r.value, r.references), (5.0, vec!["w".to_owned()]));
    }

    #[test]
    fn rename_and_names() {
        assert_eq!(
            rename("w + w2*w - max(w, 1)", "w", "width"),
            "width + w2*width - max(width, 1)"
        );
        assert!(valid_name("hole_d2").is_ok());
        assert!(valid_name("2x").is_err());
        assert!(valid_name("in").is_err());
        assert!(valid_name("sqrt").is_err());
        assert!(valid_name("a-b").is_err());
    }
}
