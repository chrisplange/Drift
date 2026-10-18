//! Splitting a script into logical lines and tokens.

use super::errors::ScriptError;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok {
    /// A word: a keyword, a name, a variable, a unit or an id like `e12`.
    Word(String),
    Num(String),
    Str(String),
    /// `#rrggbb`.
    Color(String),
    /// `( ) , . [ ] = + - * / ^ ' " ° :`, and `..` as `~`.
    Sym(char),
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub tok: Tok,
    /// Byte range in the line's text.
    pub start: usize,
    pub end: usize,
}

/// A statement's line: its number (the first, if continued with `\`), its text and tokens.
#[derive(Clone, Debug)]
pub(super) struct Line {
    pub number: usize,
    pub text: String,
    pub tokens: Vec<Token>,
}

/// The script's statements' lines, without comments and blank lines.
pub(super) fn lines(script: &str) -> Result<Vec<Line>, ScriptError> {
    let mut out = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in script.lines().enumerate() {
        let number = i + 1;
        let (start, mut text) = pending.take().unwrap_or((number, String::new()));
        let code = strip_comment(raw);
        if let Some(rest) = code.trim_end().strip_suffix('\\') {
            text.push_str(rest);
            text.push(' ');
            pending = Some((start, text));
            continue;
        }
        text.push_str(code);
        if text.trim().is_empty() {
            continue;
        }
        let tokens = tokens(&text).map_err(|(col, msg)| ScriptError::at(start, &text, col, msg))?;
        out.push(Line {
            number: start,
            text,
            tokens,
        });
    }
    if let Some((start, text)) = pending
        && !text.trim().is_empty()
    {
        let tokens = tokens(&text).map_err(|(col, msg)| ScriptError::at(start, &text, col, msg))?;
        out.push(Line {
            number: start,
            text,
            tokens,
        });
    }
    Ok(out)
}

/// A colour `#rrggbb` at the start of `s`.
fn is_color(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 7
        && b[0] == b'#'
        && b[1..7].iter().all(u8::is_ascii_hexdigit)
        && b.get(7).is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// The line without its comment (`#` to the end, unless it starts a colour or is in a string).
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut prev_digit = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' if !in_str && prev_digit => {}
            '"' => in_str = !in_str,
            '#' if !in_str && !is_color(&line[i..]) => return &line[..i],
            _ => {}
        }
        prev_digit = c.is_ascii_digit();
    }
    line
}

fn tokens(text: &str) -> Result<Vec<Token>, (usize, String)> {
    let mut out: Vec<Token> = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |k: usize| chars.get(k).map(|&(_, c)| c);
    let pos = |k: usize| chars.get(k).map_or(text.len(), |&(i, _)| i);
    let mut k = 0;
    while k < chars.len() {
        let (start, c) = chars[k];
        // Straight after the previous token, with no space.
        let touching = out.last().is_some_and(|t| t.end == start);
        let prev_num = touching && out.last().is_some_and(|t| matches!(t.tok, Tok::Num(_)));
        let prev_wordish = touching
            && out
                .last()
                .is_some_and(|t| matches!(t.tok, Tok::Word(_) | Tok::Sym(']') | Tok::Sym(')')));
        if c.is_whitespace() {
            k += 1;
            continue;
        }
        let tok;
        if c.is_ascii_digit()
            || (c == '.' && at(k + 1).is_some_and(|d| d.is_ascii_digit()) && !prev_wordish)
        {
            let mut j = k;
            let mut dot = false;
            while let Some(d) = at(j) {
                if d.is_ascii_digit() {
                    j += 1;
                } else if d == '.' && !dot && at(j + 1).is_some_and(|e| e.is_ascii_digit()) {
                    dot = true;
                    j += 1;
                } else {
                    break;
                }
            }
            tok = Tok::Num(text[start..pos(j)].to_owned());
            k = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = k;
            while at(j).is_some_and(|d| d.is_alphanumeric() || d == '_') {
                j += 1;
            }
            tok = Tok::Word(text[start..pos(j)].to_owned());
            k = j;
        } else if c == '"' && !prev_num {
            let mut j = k + 1;
            while at(j).is_some_and(|d| d != '"') {
                j += 1;
            }
            if at(j).is_none() {
                return Err((start, "this text has no closing `\"`".into()));
            }
            tok = Tok::Str(text[pos(k + 1)..pos(j)].to_owned());
            k = j + 1;
        } else if c == '#' && is_color(&text[start..]) {
            tok = Tok::Color(text[start..start + 7].to_owned());
            k += 7;
        } else if c == '.' && at(k + 1) == Some('.') {
            tok = Tok::Sym('~');
            k += 2;
        } else if c == '×' {
            tok = Tok::Word("x".into());
            k += 1;
        } else if "(),.[]=+-*/^'\"°:".contains(c) {
            tok = Tok::Sym(c);
            k += 1;
        } else {
            return Err((start, format!("unexpected `{c}`")));
        }
        out.push(Token {
            tok,
            start,
            end: pos(k),
        });
    }
    Ok(out)
}

/// `text` with " mm" after each bare number that is added to or taken from something
/// (`W/2 + 100` gives `W/2 + 100 mm`), or `None` if it has no such number. Drift's
/// expressions won't add a plain number to a length.
pub(super) fn with_mm(text: &str) -> Option<String> {
    let toks = tokens(text).ok()?;
    let sym = |t: Option<&Token>, set: &str| {
        t.is_some_and(|t| matches!(t.tok, Tok::Sym(c) if set.contains(c)))
    };
    let mut ends = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if !matches!(t.tok, Tok::Num(_)) {
            continue;
        }
        let prev = i.checked_sub(1).and_then(|j| toks.get(j));
        let next = toks.get(i + 1);
        let bare = next.is_none() || sym(next, "+-),");
        let operand = prev.is_none() || sym(prev, "+-(,");
        if bare && operand && (sym(prev, "+-") || sym(next, "+-")) {
            ends.push(t.end);
        }
    }
    if ends.is_empty() {
        return None;
    }
    let mut out = text.to_owned();
    for end in ends.into_iter().rev() {
        out.insert_str(end, " mm");
    }
    Some(out)
}
