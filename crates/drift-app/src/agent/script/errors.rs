//! A script's errors: where, what was meant, and how to fix it.

use serde_json::{Value, json};

/// An error in a script (boxed: results carry it a long way up the parser).
#[derive(Clone, Debug, Default)]
pub(super) struct ScriptError(Box<Detail>);

impl std::ops::Deref for ScriptError {
    type Target = Detail;
    fn deref(&self) -> &Detail {
        &self.0
    }
}

impl std::ops::DerefMut for ScriptError {
    fn deref_mut(&mut self) -> &mut Detail {
        &mut self.0
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Detail {
    pub line: usize,
    /// Byte range in the source line, for the column and the marker under it.
    pub span: Option<(usize, usize)>,
    pub source: String,
    pub message: String,
    /// How the statement was read, in plain words.
    pub meant: Option<String>,
    /// Other lines it involves (line number, its source).
    pub related: Vec<(usize, String)>,
    pub hint: Option<String>,
}

impl ScriptError {
    pub fn new(line: usize, source: &str, message: impl Into<String>) -> Self {
        Self(Box::new(Detail {
            line,
            source: source.to_owned(),
            message: message.into(),
            ..Default::default()
        }))
    }

    /// An error at byte `col` of the line.
    pub fn at(line: usize, source: &str, col: usize, message: impl Into<String>) -> Self {
        let end = source[col..]
            .char_indices()
            .nth(1)
            .map_or(source.len(), |(i, _)| col + i);
        Self::new(line, source, message).span(col, end)
    }

    pub fn span(mut self, start: usize, end: usize) -> Self {
        self.span = Some((start, end.max(start)));
        self
    }

    pub fn meant(mut self, meant: Option<String>) -> Self {
        self.meant = meant;
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The 1-based column, in characters.
    pub fn column(&self) -> Option<usize> {
        self.span
            .map(|(s, _)| self.source[..s.min(self.source.len())].chars().count() + 1)
    }

    pub fn text(&self) -> String {
        let mut out = match self.column() {
            Some(c) => format!("line {}, column {c}: {}", self.line, self.message),
            None => format!("line {}: {}", self.line, self.message),
        };
        let gutter = self
            .related
            .iter()
            .map(|(n, _)| *n)
            .chain([self.line])
            .max()
            .unwrap_or(0)
            .to_string()
            .len();
        let mut quoted: Vec<(usize, &str)> = self
            .related
            .iter()
            .map(|(n, s)| (*n, s.as_str()))
            .chain([(self.line, self.source.trim_end())])
            .collect();
        quoted.sort_by_key(|(n, _)| *n);
        quoted.dedup_by_key(|(n, _)| *n);
        for (n, src) in quoted {
            out.push_str(&format!("\n  {n:>gutter$} | {}", src.trim_end()));
            if n == self.line
                && let Some((s, e)) = self.span
            {
                let s = s.min(self.source.len());
                let e = e.min(self.source.len());
                let pad = self.source[..s].chars().count();
                let width = self.source[s..e].chars().count().max(1);
                out.push_str(&format!(
                    "\n  {:gutter$} | {}{}",
                    "",
                    " ".repeat(pad),
                    "^".repeat(width)
                ));
            }
        }
        if let Some(m) = &self.meant {
            out.push_str(&format!("\n  meant: {m}"));
        }
        if let Some(h) = &self.hint {
            out.push_str(&format!("\n  {h}"));
        }
        out.push_str("\nNothing was changed.");
        out
    }

    pub fn json(&self) -> Value {
        let mut v = json!({
            "error": self.text(),
            "line": self.line,
            "source": self.source.trim(),
        });
        if let Some(c) = self.column() {
            v["column"] = json!(c);
        }
        if let Some(m) = &self.meant {
            v["meant"] = json!(m);
        }
        if !self.related.is_empty() {
            v["related_lines"] = json!(self.related.iter().map(|(n, _)| n).collect::<Vec<_>>());
        }
        if let Some(h) = &self.hint {
            v["hint"] = json!(h);
        }
        v
    }
}
