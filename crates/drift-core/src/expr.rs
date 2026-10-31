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
//!   A plain number added to or taken from a length (or set beside one in `min`, `max`,
//!   `hypot` and `atan2`) is read in the context's unit, as a plain number alone is; a plain
//!   number in a product is a factor (`width * 2`). Angles take plain numbers as degrees.
//!   Trigonometry takes angles, or plain numbers as degrees (the CAD convention).
//! - A result with no units is read in the context's unit (see [`evaluate_input`], which also
//!   writes that unit into the stored text).

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

    /// A whole number and a fraction with no mark, `35 1/2` (in the context's unit, as a
    /// bare number is). Spaces, not `-`, join them: `10-1/2` stays arithmetic.
    fn mixed_at(&self, i: usize) -> Option<(f64, usize)> {
        let c = &self.chars;
        let (n, j) = self.integer_at(i)?;
        let mut k = j;
        while c.get(k) == Some(&' ') {
            k += 1;
        }
        if k == j {
            return None;
        }
        let (f, end) = self.fraction_at(k)?;
        Some((n + f, end))
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
                } else if let Some((v, end)) = self.mixed_at(i) {
                    out.push((Tok::Num(v), start, end));
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
enum NodeKind {
    Value(Quantity),
    Var(String),
    Neg(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// A node of the tree and where it came from: `start..end` are character offsets into the
/// parsed text, so [`evaluate_input`] can write a unit after an operand it read in the
/// context's unit. A parenthesised expression is the inner node with the parentheses in its
/// span and `paren` set.
#[derive(Clone, Debug, PartialEq)]
struct Node {
    kind: NodeKind,
    start: usize,
    end: usize,
    paren: bool,
}

/// A parsed expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    root: Node,
}

struct Parser {
    /// Tokens with their character spans.
    toks: Vec<(Tok, usize, usize)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, ..)| t)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|(t, ..)| t.clone());
        self.pos += 1;
        t
    }

    /// A node spanning the tokens from `first` to the last one consumed.
    fn node(&self, kind: NodeKind, first: usize) -> Node {
        Node {
            kind,
            start: self.toks[first].1,
            end: self.toks[self.pos - 1].2,
            paren: false,
        }
    }

    /// A node for the single token at `at`.
    fn leaf(&self, kind: NodeKind, at: usize) -> Node {
        Node {
            kind,
            start: self.toks[at].1,
            end: self.toks[at].2,
            paren: false,
        }
    }

    fn sum(&mut self) -> Result<Node, ExprError> {
        let first = self.pos;
        let mut left = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let right = self.product()?;
            left = self.node(NodeKind::Bin(op, Box::new(left), Box::new(right)), first);
        }
        Ok(left)
    }

    /// The unit at `pos`, if any.
    fn unit_at(&self, pos: usize) -> Option<Quantity> {
        match self.toks.get(pos).map(|(t, ..)| t) {
            Some(Tok::Ident(name)) => unit(name),
            Some(Tok::Mark('\'')) => unit("ft"),
            Some(Tok::Mark('"')) => unit("in"),
            Some(Tok::Mark('°')) => unit("deg"),
            _ => None,
        }
    }

    /// `left` times the unit `u` at `pos` (consumed here), spanning from token `first`.
    fn times_unit(&mut self, left: Node, u: Quantity, first: usize) -> Node {
        let unit = self.leaf(NodeKind::Value(u), self.pos);
        self.pos += 1;
        self.node(NodeKind::Bin('*', Box::new(left), Box::new(unit)), first)
    }

    fn product(&mut self) -> Result<Node, ExprError> {
        let first = self.pos;
        let mut left = self.unary()?;
        loop {
            // A unit applies to the product so far.
            if let Some(u) = self.unit_at(self.pos) {
                left = self.times_unit(left, u, first);
                continue;
            }
            let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() else {
                break;
            };
            self.pos += 1;
            let right = self.unary()?;
            left = self.node(NodeKind::Bin(op, Box::new(left), Box::new(right)), first);
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        let first = self.pos;
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                let inner = self.unary()?;
                Ok(self.node(NodeKind::Neg(Box::new(inner)), first))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Node, ExprError> {
        let first = self.pos;
        let mut base = self.primary()?;
        // `3mm^2`: a unit right before `^` belongs to the base.
        if let Some(u) = self.unit_at(self.pos)
            && matches!(self.toks.get(self.pos + 1), Some((Tok::Op('^'), ..)))
        {
            base = self.times_unit(base, u, first);
        }
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let exp = self.unary()?;
            return Ok(self.node(NodeKind::Bin('^', Box::new(base), Box::new(exp)), first));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        let first = self.pos;
        match self.next() {
            Some(Tok::Num(v)) => Ok(self.leaf(NodeKind::Value(Quantity::number(v)), first)),
            Some(Tok::Literal(q)) => Ok(self.leaf(NodeKind::Value(q), first)),
            Some(Tok::LParen) => {
                let inner = self.sum()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(Node {
                        paren: true,
                        ..self.node(inner.kind, first)
                    }),
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
                    return Ok(self.node(NodeKind::Call(name, args), first));
                }
                if name == "pi" {
                    return Ok(self.leaf(NodeKind::Value(Quantity::number(PI)), first));
                }
                if unit(&name).is_some() {
                    return err(format!("`{name}` needs a number before it"));
                }
                Ok(self.leaf(NodeKind::Var(name), first))
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
    let toks = lexer.tokens()?;
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

/// What an evaluation reads bare numbers in, and the operands it did that to.
struct EvalCtx {
    length: LengthFormat,
    /// Operands read in the context's unit, in text order.
    coerced: Vec<Coerced>,
}

/// An operand with no unit that was read in the context's unit, by its span in the text.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coerced {
    start: usize,
    end: usize,
    /// The dimension it was read as (length or angle).
    dim: Dim,
    /// An operand under a binary operator (`60 + 10`, `2*30`) is parenthesised before the
    /// unit, as a whole result is (`(10*3) mm`), so the text reads as it evaluates.
    binary: bool,
}

/// Read a unitless operand in the context's unit when it meets a length or an angle: lengths
/// in `ctx.length`, angles in degrees (as a bare number alone is read). Records the operand
/// so the stored text can carry the unit. `None` when `q` keeps its dimension.
fn coerce(ctx: &mut EvalCtx, q: Quantity, node: &Node, to: Dim) -> Option<Quantity> {
    if !q.dim.is_none() {
        return None;
    }
    let value = if to == Dim::LENGTH {
        q.value * ctx.length.mm_per_unit()
    } else if to == Dim::ANGLE {
        q.value
    } else {
        return None;
    };
    ctx.coerced.push(Coerced {
        start: node.start,
        end: node.end,
        dim: to,
        binary: matches!(node.kind, NodeKind::Bin(..)) && !node.paren,
    });
    Some(Quantity { value, dim: to })
}

/// Functions whose arguments are like quantities (compared or combined, not multiplied), so
/// a bare number among lengths or angles is read in the context's unit.
const LIKE_ARGS: &[&str] = &["min", "max", "hypot", "atan2"];

impl Node {
    fn eval(&self, vars: &Lookup<'_>, ctx: &mut EvalCtx) -> Result<Quantity, ExprError> {
        Ok(match &self.kind {
            NodeKind::Value(q) => *q,
            NodeKind::Var(name) => match vars(name) {
                Some(r) => r?,
                None => return err(format!("unknown variable `{name}`")),
            },
            NodeKind::Neg(a) => {
                let q = a.eval(vars, ctx)?;
                Quantity {
                    value: -q.value,
                    ..q
                }
            }
            NodeKind::Bin(op, a, b) => {
                let (mut x, mut y) = (a.eval(vars, ctx)?, b.eval(vars, ctx)?);
                match op {
                    '+' | '-' => {
                        // A bare number added to a length or an angle is in the context's unit.
                        if x.dim != y.dim {
                            if let Some(q) = coerce(ctx, x, a, y.dim) {
                                x = q;
                            } else if let Some(q) = coerce(ctx, y, b, x.dim) {
                                y = q;
                            }
                        }
                        let adding = *op == '+';
                        same_dim(if adding { "add" } else { "subtract" }, x, y)?;
                        Quantity {
                            value: if adding {
                                x.value + y.value
                            } else {
                                x.value - y.value
                            },
                            dim: x.dim,
                        }
                    }
                    '*' => Quantity {
                        value: x.value * y.value,
                        dim: Dim {
                            length: x.dim.length + y.dim.length,
                            angle: x.dim.angle + y.dim.angle,
                        },
                    },
                    '/' => {
                        if y.value == 0.0 {
                            return err("division by zero");
                        }
                        Quantity {
                            value: x.value / y.value,
                            dim: Dim {
                                length: x.dim.length - y.dim.length,
                                angle: x.dim.angle - y.dim.angle,
                            },
                        }
                    }
                    _ => {
                        if !y.dim.is_none() {
                            return err("an exponent must be a plain number");
                        }
                        let Some(dim) = scale_dim(x.dim, y.value) else {
                            return err(format!(
                                "can't raise {} to the power {}",
                                x.dim.name(),
                                y.value
                            ));
                        };
                        Quantity {
                            value: x.value.powf(y.value),
                            dim,
                        }
                    }
                }
            }
            NodeKind::Call(f, args) => {
                let mut values: Vec<Quantity> = args
                    .iter()
                    .map(|a| a.eval(vars, ctx))
                    .collect::<Result<_, _>>()?;
                // Like quantities: a bare number among lengths or angles is in the context's unit.
                if LIKE_ARGS.contains(&f.as_str())
                    && let Some(dim) = values.iter().map(|q| q.dim).find(|d| !d.is_none())
                {
                    for (q, node) in values.iter_mut().zip(args) {
                        if let Some(c) = coerce(ctx, *q, node, dim) {
                            *q = c;
                        }
                    }
                }
                call(f, &values)?
            }
        })
    }

    fn identifiers(&self, out: &mut Vec<String>) {
        match &self.kind {
            NodeKind::Value(_) => {}
            NodeKind::Var(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            NodeKind::Neg(a) => a.identifiers(out),
            NodeKind::Bin(_, a, b) => {
                a.identifiers(out);
                b.identifiers(out);
            }
            NodeKind::Call(_, args) => args.iter().for_each(|a| a.identifiers(out)),
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
    /// Evaluate with variables from `vars`, reading a bare number added to a length in
    /// `length` (the module doc has the rule).
    pub fn eval(&self, vars: &Lookup<'_>, length: LengthFormat) -> Result<Quantity, ExprError> {
        self.eval_recording(vars, length).map(|(q, _)| q)
    }

    /// [`Expr::eval`], also returning the operands read in the context's unit.
    fn eval_recording(
        &self,
        vars: &Lookup<'_>,
        length: LengthFormat,
    ) -> Result<(Quantity, Vec<Coerced>), ExprError> {
        let mut ctx = EvalCtx {
            length,
            coerced: Vec::new(),
        };
        let q = self.root.eval(vars, &mut ctx)?;
        if q.value.is_finite() {
            Ok((q, ctx.coerced))
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
        matches!(self.root.kind, NodeKind::Value(q) if q.dim.is_none())
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
    let (q, coerced) = expr.eval_recording(vars, default)?;
    let value = to_kind(q, kind, default)?;
    let text = with_units(text, &coerced, default);
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
        text
    };
    Ok(Evaluated {
        value,
        text,
        references: expr.identifiers(),
    })
}

/// `text` with the unit written after each operand that was read in the context's unit
/// (`spacing + 60` → `spacing + 60 mm`), so the stored text keeps its value whatever the
/// display unit becomes. An operand under a binary operator is parenthesised first, as a whole
/// result is: `60 + 10 + spacing` → `(60 + 10) mm + spacing`, `spacing + 2*30` →
/// `spacing + (2*30) mm`. Spans can nest (`spacing + (spacing + 60) / spacing` reads `60` and
/// then the whole quotient), so they are written as a tree.
fn with_units(text: &str, coerced: &[Coerced], length: LengthFormat) -> String {
    if coerced.is_empty() {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut spans = coerced.to_vec();
    // By position, an outer span before the spans inside it.
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut out = String::new();
    write_spans(&chars, &spans, &mut 0, 0, chars.len(), length, &mut out);
    out
}

/// Write `chars[from..to]` to `out`, with every span from `spans[*next..]` that lies inside
/// it (they are sorted outer first) wrapped and followed by its unit.
fn write_spans(
    chars: &[char],
    spans: &[Coerced],
    next: &mut usize,
    from: usize,
    to: usize,
    length: LengthFormat,
    out: &mut String,
) {
    let mut last = from;
    while let Some(c) = spans.get(*next).copied()
        && c.start < to
    {
        *next += 1;
        out.extend(&chars[last..c.start]);
        if c.binary {
            out.push('(');
        }
        write_spans(chars, spans, next, c.start, c.end, length, out);
        if c.binary {
            out.push(')');
        }
        out.push(' ');
        out.push_str(if c.dim == Dim::LENGTH {
            length.symbol()
        } else {
            "deg"
        });
        last = c.end;
    }
    out.extend(&chars[last..to]);
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

    fn eval_in(text: &str, length: LengthFormat) -> Result<Quantity, ExprError> {
        let vars = |name: &str| match name {
            "width" => Some(Ok(Quantity::length(120.0))),
            "spacing" => Some(Ok(Quantity::length(60.0))),
            "tilt" => Some(Ok(Quantity::angle(30.0))),
            "n" => Some(Ok(Quantity::number(4.0))),
            "broken" => Some(err("broken")),
            _ => None,
        };
        parse(text)?.eval(&vars, length)
    }

    fn eval(text: &str) -> Result<Quantity, ExprError> {
        eval_in(text, LengthFormat::Millimetre)
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
        // A whole number and a fraction, no mark: one number (inches, in an inch drawing).
        assert_eq!(eval("35 1/2").unwrap(), Quantity::number(35.5));
        assert_eq!(eval("2 * 35 1/2").unwrap(), Quantity::number(71.0));
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
        assert_eq!(msg("3mm^2 + 5"), "can't add an area and a number");
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

    /// A bare number added to or taken from a length is in the context's unit (#241), as it
    /// is alone; beside an angle it is degrees; in a product it is a factor.
    #[test]
    fn bare_numbers_beside_lengths_follow_the_context_unit() {
        let inch = MM_PER_INCH;
        let in_inches = |t: &str| eval_in(t, LengthFormat::Inch).unwrap();
        assert_eq!(mm("spacing + 60"), 120.0);
        assert_eq!(
            in_inches("spacing + 60"),
            Quantity::length(60.0 + 60.0 * inch)
        );
        assert_eq!(in_inches("width + 2"), Quantity::length(120.0 + 2.0 * inch));
        assert_eq!(mm("10 + 5mm"), 15.0);
        assert_eq!(in_inches("10 + 5mm"), Quantity::length(10.0 * inch + 5.0));
        assert_eq!(mm("spacing - 10"), 50.0);
        assert_eq!(mm("60 - spacing"), 0.0);
        assert_eq!(mm("spacing + 2*5"), 70.0);
        assert_eq!(mm("spacing + n"), 64.0);
        // Products stay plain arithmetic.
        assert_eq!(eval("2 * 3").unwrap(), Quantity::number(6.0));
        assert_eq!(mm("spacing * 2"), 120.0);
        assert_eq!(mm("spacing * n"), 240.0);
        // Functions of like quantities read a bare number the same way.
        assert_eq!(mm("min(spacing, 50)"), 50.0);
        assert_eq!(mm("max(50, spacing)"), 60.0);
        assert_eq!(mm("hypot(spacing, 80)"), 100.0);
        let a = eval("atan2(spacing, 60)").unwrap();
        assert_eq!(a.dim, Dim::ANGLE);
        assert!((a.value - 45.0).abs() < 1e-12);
        // Angles: a bare number is degrees.
        assert_eq!(eval("tilt + 5").unwrap(), Quantity::angle(35.0));
        assert!((eval("sin(tilt + 60)").unwrap().value - 1.0).abs() < 1e-15);
        // Real mismatches still fail.
        let msg = |t: &str| eval(t).unwrap_err().0;
        assert_eq!(msg("5mm + 2deg"), "can't add a length and an angle");
        assert_eq!(msg("3mm^2 + 5"), "can't add an area and a number");
        assert_eq!(
            msg("spacing - 2deg"),
            "can't subtract a length and an angle"
        );
        assert_eq!(
            msg("min(spacing, 2deg)"),
            "can't use min() on a length and an angle"
        );
    }

    /// The stored text carries the unit a bare operand was read in, so it keeps its value
    /// whatever the display unit becomes, and evaluating it again changes nothing.
    #[test]
    fn coerced_operands_are_stored_with_their_unit() {
        let vars = |name: &str| match name {
            "spacing" => Some(Ok(Quantity::length(60.0))),
            "tilt" => Some(Ok(Quantity::angle(30.0))),
            "n" => Some(Ok(Quantity::number(4.0))),
            "a" | "b" => Some(Ok(Quantity::length(1.0))),
            _ => None,
        };
        let cases: &[(&str, &str, f64)] = &[
            ("spacing + 60", "spacing + 60 mm", 120.0),
            ("60 + spacing", "60 mm + spacing", 120.0),
            ("spacing - 2*5", "spacing - (2*5) mm", 50.0),
            ("spacing + 2*30", "spacing + (2*30) mm", 120.0),
            ("spacing + n", "spacing + n mm", 64.0),
            ("spacing + (10 + 20)", "spacing + (10 + 20) mm", 90.0),
            ("spacing + max(10, 20)", "spacing + max(10, 20) mm", 80.0),
            ("spacing + -5", "spacing + -5 mm", 55.0),
            ("spacing + 2^3", "spacing + (2^3) mm", 68.0),
            ("spacing + 35 1/2", "spacing + 35 1/2 mm", 95.5),
            ("max(spacing, 50)", "max(spacing, 50 mm)", 60.0),
            ("a + (b + 60)", "a + (b + 60 mm)", 62.0),
            ("(60 + 2) + spacing", "(60 + 2) mm + spacing", 122.0),
            // Operands under a binary operator are parenthesised, as a whole result is.
            ("60 + 10 + spacing", "(60 + 10) mm + spacing", 130.0),
            ("max(10 + 20, spacing)", "max((10 + 20) mm, spacing)", 60.0),
            // Nested: a sum read in the unit, inside a quotient read in it too.
            (
                "spacing + (spacing + 60) / spacing",
                "spacing + ((spacing + 60 mm) / spacing) mm",
                62.0,
            ),
            (
                "spacing + n * ((spacing + 60) / spacing)",
                "spacing + (n * ((spacing + 60 mm) / spacing)) mm",
                68.0,
            ),
            // Untouched: a factor, and text that already has its units.
            ("spacing * n", "spacing * n", 240.0),
            ("spacing + 60 mm", "spacing + 60 mm", 120.0),
        ];
        for (typed, stored, value) in cases {
            let r = evaluate_input(typed, Kind::Length, LengthFormat::Millimetre, &vars)
                .unwrap_or_else(|e| panic!("{typed}: {e}"));
            assert_eq!((r.text.as_str(), r.value), (*stored, *value), "{typed}");
            // Round trip: the same value in another display unit, and stable in this one.
            let again = evaluate_input(stored, Kind::Length, LengthFormat::Inch, &vars).unwrap();
            assert_eq!(
                (again.text.as_str(), again.value),
                (*stored, *value),
                "{stored} in inches"
            );
            let same =
                evaluate_input(stored, Kind::Length, LengthFormat::Millimetre, &vars).unwrap();
            assert_eq!(same.text, *stored, "{stored} again");
        }
        // Every case one level deeper, so its spans nest inside a read quotient: no panic,
        // the right value, and a stable text.
        for (typed, _, value) in cases {
            let deeper = format!("spacing + ({typed}) / spacing");
            let r = evaluate_input(&deeper, Kind::Length, LengthFormat::Millimetre, &vars)
                .unwrap_or_else(|e| panic!("{deeper}: {e}"));
            assert_eq!(r.value, 60.0 + value / 60.0, "{deeper}");
            let same = evaluate_input(&r.text, Kind::Length, LengthFormat::Inch, &vars)
                .unwrap_or_else(|e| panic!("{}: {e}", r.text));
            assert_eq!((same.text.as_str(), same.value), (r.text.as_str(), r.value));
        }
        // Inches, angles, and a bare operand inside a result that is itself bare.
        let r = evaluate_input("spacing + 2", Kind::Length, LengthFormat::Inch, &vars).unwrap();
        assert_eq!(
            (r.text.as_str(), r.value),
            ("spacing + 2 in", 60.0 + 2.0 * MM_PER_INCH)
        );
        let r = evaluate_input("tilt + 5", Kind::Angle, LengthFormat::Millimetre, &vars).unwrap();
        assert_eq!((r.text.as_str(), r.value), ("tilt + 5 deg", 35.0));
        let r = evaluate_input(
            "(spacing + 60) / spacing",
            Kind::Length,
            LengthFormat::Millimetre,
            &vars,
        )
        .unwrap();
        assert_eq!(
            (r.text.as_str(), r.value),
            ("((spacing + 60 mm) / spacing) mm", 2.0)
        );
        let same = evaluate_input(&r.text, Kind::Length, LengthFormat::Inch, &vars).unwrap();
        assert_eq!((same.text.as_str(), same.value), (r.text.as_str(), 2.0));
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
