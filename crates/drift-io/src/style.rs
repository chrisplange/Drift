//! What the two CAD writers (R12 in `dxf`, R2018 and DWG in `cad`) share: how layers and
//! blocks are named, the line type table, and how text is escaped.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use drift_core::LineType;
use drift_draft::page::{Drawing, Item, LayerInfo, dash_pattern};

use crate::dxf::line_type_name;

/// A name unique (ignoring case) among the names in `taken` (Drift's names to those
/// written), for one written as `base`: `base` itself, else the first free one of
/// `numbered(base, 2)`, `numbered(base, 3)`, ... Both CAD writers name layers and blocks so,
/// each with its own `base` and `numbered` rule.
pub(crate) fn unique_name(
    base: String,
    taken: &BTreeMap<String, String>,
    numbered: impl Fn(&str, usize) -> String,
) -> String {
    let clash = |n: &str| taken.values().any(|t| t.eq_ignore_ascii_case(n));
    if !clash(&base) {
        return base;
    }
    (2..)
        .map(|k| numbered(&base, k))
        .find(|n| !clash(n))
        .unwrap_or(base)
}

/// A drawing's layers as a CAD file has them: each Drift layer under a name of its own
/// (unique ignoring case, as CAD needs; layer 0 stays 0) and each written layer's look, `L`
/// being what the format keeps of it. The name rule is the writer's: `base` is the name a
/// Drift name is written as, `numbered` the `k`th one for names that come out alike (see
/// [`unique_name`]).
pub(crate) struct LayerTable<L> {
    /// Drift names to the names written.
    pub names: BTreeMap<String, String>,
    /// Each layer's look, by the name written (the first look given wins).
    pub looks: BTreeMap<String, L>,
    base: fn(&str) -> String,
    numbered: fn(&str, usize) -> String,
}

impl<L: Copy> LayerTable<L> {
    pub fn new(base: fn(&str) -> String, numbered: fn(&str, usize) -> String) -> Self {
        LayerTable {
            names: BTreeMap::new(),
            looks: BTreeMap::new(),
            base,
            numbered,
        }
    }

    /// The name layer `name` is written as, adding it in `look` unless it is there already;
    /// `true` when it was added.
    pub fn add(&mut self, name: &str, look: L) -> (String, bool) {
        if let Some(n) = self.names.get(name) {
            return (n.clone(), false);
        }
        let written = if name == "0" {
            "0".to_owned()
        } else {
            unique_name((self.base)(name), &self.names, self.numbered)
        };
        self.names.insert(name.to_owned(), written.clone());
        self.looks.entry(written.clone()).or_insert(look);
        (written, true)
    }

    /// The drawing's own layers, each in `look`; `added` is told of each new one (its name
    /// written and look).
    pub fn add_declared(
        &mut self,
        d: &Drawing,
        look: impl Fn(&LayerInfo) -> L,
        mut added: impl FnMut(&str, L),
    ) {
        for l in &d.layers {
            let look = look(l);
            let (written, new) = self.add(&l.name, look);
            if new {
                added(&written, look);
            }
        }
    }

    /// Any other layers the drawing's items and its blocks' items are on, each in its first
    /// item's `look`; `added` is told of each new one.
    pub fn add_used(
        &mut self,
        d: &Drawing,
        look: impl Fn(&Item) -> L,
        mut added: impl FnMut(&str, L),
    ) {
        for item in d
            .items
            .iter()
            .chain(d.blocks.iter().flat_map(|b| b.items.iter()))
        {
            if !self.names.contains_key(&item.layer) {
                let look = look(item);
                let (written, _) = self.add(&item.layer, look);
                added(&written, look);
            }
        }
    }
}

/// A line type as CAD files define it (lengths in mm on paper, scaled by `$LTSCALE`).
pub(crate) struct LineTypeDef {
    pub kind: LineType,
    pub name: &'static str,
    pub description: &'static str,
    /// Dashes and gaps by turns.
    pub pattern: &'static [f64],
}

impl LineTypeDef {
    /// The length of one repeat of the pattern.
    pub fn length(&self) -> f64 {
        self.pattern.iter().sum()
    }
}

/// Drift's line types, Continuous first, as both CAD writers define them.
pub(crate) fn line_types() -> impl Iterator<Item = LineTypeDef> {
    [
        LineType::Continuous,
        LineType::Dashed,
        LineType::Hidden,
        LineType::Center,
        LineType::Phantom,
    ]
    .into_iter()
    .map(|kind| LineTypeDef {
        kind,
        name: line_type_name(kind),
        description: kind.label(),
        pattern: dash_pattern(kind),
    })
}

/// Text for a TEXT entity. `%%` would start a control code (`%%c` is Ø), so a percent sign
/// before another is written `%%%`; control characters are left out. For R12 (`r12`) Ø, °
/// and ± are its `%%c`, `%%d` and `%%p` codes and other characters outside ASCII are
/// `\U+XXXX` (AutoCAD and other readers decode it in every version; one beyond what four
/// digits name, emoji and the like, is `?`). R2018 files are UTF-8: other characters go as
/// they are.
pub(crate) fn text_value(s: &str, r12: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '%' if chars.peek() == Some(&'%') => out.push_str("%%%"),
            c if !r12 => {
                if !c.is_control() {
                    out.push(c);
                }
            }
            'Ø' => out.push_str("%%c"),
            '°' => out.push_str("%%d"),
            '±' => out.push_str("%%p"),
            c if c.is_ascii_control() => {}
            c if c.is_ascii() => out.push(c),
            c if u32::from(c) > 0xFFFF => out.push('?'),
            c => {
                let _ = write!(out, "\\U+{:04X}", u32::from(c));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_for_each_version() {
        assert_eq!(text_value("50%%c", false), "50%%%%c");
        assert_eq!(text_value("Ø10 ±0.1 €", false), "Ø10 ±0.1 €");
        assert_eq!(text_value("a\u{1}\u{85}b", false), "ab");
        assert_eq!(
            text_value("Ø10 ±0.1 45° 50%%", true),
            "%%c10 %%p0.1 45%%d 50%%%%"
        );
        assert_eq!(text_value("€ 😀 \u{1}\u{85}", true), "\\U+20AC ? \\U+0085");
    }

    #[test]
    fn layers_are_named_once_and_apart() {
        let mut t: LayerTable<u8> =
            LayerTable::new(|n| n.to_ascii_uppercase(), |b, k| format!("{b}_{k}"));
        assert_eq!(t.add("Walls", 1), ("WALLS".to_owned(), true));
        assert_eq!(t.add("walls", 2), ("WALLS_2".to_owned(), true));
        assert_eq!(t.add("Walls", 3), ("WALLS".to_owned(), false));
        assert_eq!(t.add("0", 4), ("0".to_owned(), true));
        assert_eq!(t.looks["WALLS"], 1);
    }
}
