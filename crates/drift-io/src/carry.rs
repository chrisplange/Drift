//! Carrying through what a newer Drift wrote that this build doesn't know.
//!
//! Reading passes over fields and tables this build doesn't know (a newer Drift may add them
//! without raising `reader`). So that saving here doesn't strip them, a file that has any is
//! kept as it was read ([`Kept::original`]), together with what this build writes for the
//! drawing as it was read ([`Kept::baseline`]). Saving compares what it writes now with the
//! baseline, record by record:
//!
//! - a record (or table) that didn't change is written as the file had it, byte for byte,
//!   unknown fields and all;
//! - a record that changed is written as now, with the unknown fields it had put back after
//!   the field that came before them;
//! - an unknown table is written back where it was;
//! - unknown data is lost only where the thing that held it is gone: a record that was
//!   deleted, a field that was cleared, a record replaced by one of another `type`, or a list
//!   whose items have no `id` and whose length changed. [`Kept::merge`] says so, and the app
//!   warns before saving.
//!
//! Records in a list are matched by their `id` (or `of`, for settings of a dimension or
//! annotation), or by position when the list has no ids and kept its length. Everything is
//! compared as text, as written: this build's writer is deterministic, so an unchanged
//! drawing writes the baseline exactly.

use std::collections::{HashMap, HashSet};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

/// What a file had that this build doesn't know, kept so that saving writes it back.
#[derive(Debug, PartialEq, Eq)]
pub struct Kept {
    /// The file as it was read.
    original: String,
    /// What this build wrote for the drawing as it was read.
    baseline: String,
}

impl Kept {
    /// `Some` when `original` has anything `baseline` (this build's own writing of it)
    /// doesn't.
    pub(crate) fn find(original: &str, baseline: String) -> Option<Self> {
        (original != baseline && has_extra(original, &baseline)).then(|| Self {
            original: original.to_owned(),
            baseline,
        })
    }

    /// The file to write for `now` (what this build writes for the drawing now), with what
    /// it didn't know put back, and whether any of that had to be left out.
    pub(crate) fn merge(&self, now: &str) -> (String, bool) {
        if now == self.baseline {
            return (self.original.clone(), false);
        }
        match (node(&self.original), node(&self.baseline), node(now)) {
            (Node::Object(o), Node::Object(b), Node::Object(n)) => {
                let (entries, lost) = merge_entries(&o, &b, &n);
                (layout(&entries), lost)
            }
            _ => (now.to_owned(), true),
        }
    }
}

/// An object's entries, in the order written.
struct Object<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Object<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> Visitor<'de> for Entries {
            type Value = Object<'de>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, &'de RawValue>()? {
                    entries.push(entry);
                }
                Ok(Object(entries))
            }
        }
        deserializer.deserialize_map(Entries)
    }
}

type Entries<'a> = Vec<(String, &'a RawValue)>;

enum Node<'a> {
    Object(Entries<'a>),
    Array(Vec<&'a RawValue>),
    Other,
}

fn node(text: &str) -> Node<'_> {
    match text.trim_start().as_bytes().first() {
        Some(b'{') => {
            serde_json::from_str::<Object>(text).map_or(Node::Other, |o| Node::Object(o.0))
        }
        Some(b'[') => serde_json::from_str(text).map_or(Node::Other, Node::Array),
        _ => Node::Other,
    }
}

fn get<'a>(entries: &[(String, &'a RawValue)], key: &str) -> Option<&'a RawValue> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
}

/// Whether `original` has anything `base` doesn't: a field or table it lacks, anywhere.
fn has_extra(original: &str, base: &str) -> bool {
    if original == base {
        return false;
    }
    match (node(original), node(base)) {
        (Node::Object(o), Node::Object(b)) => entries_have_extra(&o, &b),
        (Node::Array(o), Node::Array(b)) => pairs(&o, &b)
            .into_iter()
            .any(|(o, b)| has_extra(o.get(), b.get())),
        _ => false,
    }
}

fn entries_have_extra(o: &Entries, b: &Entries) -> bool {
    o.iter().any(|(k, ov)| match get(b, k) {
        None => true,
        Some(bv) => has_extra(ov.get(), bv.get()),
    })
}

/// The fields a list's records are known by, in order of preference.
const KEYS: [&str; 2] = ["id", "of"];

/// Each item's `key`, as written, when every item is a record that has one and no two share
/// it.
fn keys(items: &[&RawValue], key: &str) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    items
        .iter()
        .map(|item| match node(item.get()) {
            Node::Object(entries) => get(&entries, key)
                .map(|v| v.get().to_owned())
                .filter(|k| seen.insert(k.clone())),
            _ => None,
        })
        .collect()
}

/// The key the three lists' records are matched by, with each list's keys.
fn keyed(lists: [&[&RawValue]; 3]) -> Option<[Vec<String>; 3]> {
    KEYS.iter().find_map(|key| {
        let [a, b, c] = lists.map(|l| keys(l, key));
        Some([a?, b?, c?])
    })
}

/// Items of `o` and `b` that are the same record.
fn pairs<'a>(o: &[&'a RawValue], b: &[&'a RawValue]) -> Vec<(&'a RawValue, &'a RawValue)> {
    if let Some([ok, bk, _]) = keyed([o, b, &[]]) {
        let by_key: HashMap<&str, &RawValue> = bk
            .iter()
            .map(String::as_str)
            .zip(b.iter().copied())
            .collect();
        return ok
            .iter()
            .zip(o)
            .filter_map(|(k, ov)| Some((*ov, *by_key.get(k.as_str())?)))
            .collect();
    }
    if o.len() == b.len() {
        return o.iter().copied().zip(b.iter().copied()).collect();
    }
    Vec::new()
}

/// One value: `original` as read, `base` as this build wrote it then, `now` as it writes it
/// now. Returns the text to write and whether unknown data was left out.
fn merge(original: &str, base: &str, now: &str) -> (String, bool) {
    if now == base {
        return (original.to_owned(), false);
    }
    match (node(original), node(base), node(now)) {
        (Node::Object(o), Node::Object(b), Node::Object(n)) => {
            let (entries, lost) = merge_entries(&o, &b, &n);
            let mut out = String::from("{");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&key(k));
                out.push(':');
                out.push_str(v);
            }
            out.push('}');
            (out, lost)
        }
        (Node::Array(o), Node::Array(b), Node::Array(n)) => {
            let (items, lost) = merge_items(&o, &b, &n);
            (format!("[{}]", items.join(",")), lost)
        }
        _ => (now.to_owned(), has_extra(original, base)),
    }
}

fn key(k: &str) -> String {
    serde_json::to_string(k).unwrap_or_default()
}

fn merge_entries(o: &Entries, b: &Entries, n: &Entries) -> (Vec<(String, String)>, bool) {
    let owned = |entries: &Entries| -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(k, v)| (k.clone(), v.get().to_owned()))
            .collect()
    };
    // Another kind of record under the same id: a new record, not this one changed.
    if let (Some(bt), Some(nt)) = (get(b, "type"), get(n, "type"))
        && bt.get() != nt.get()
    {
        return (owned(n), entries_have_extra(o, b));
    }
    let mut lost = false;
    let mut out: Vec<(String, String)> = n
        .iter()
        .map(|(k, nv)| {
            let text = match (get(o, k), get(b, k)) {
                (Some(ov), Some(bv)) => {
                    let (text, l) = merge(ov.get(), bv.get(), nv.get());
                    lost |= l;
                    text
                }
                _ => nv.get().to_owned(),
            };
            (k.clone(), text)
        })
        .collect();
    // A field cleared since: what it held that this build didn't know goes with it.
    for (k, bv) in b {
        if get(n, k).is_none()
            && let Some(ov) = get(o, k)
        {
            lost |= has_extra(ov.get(), bv.get());
        }
    }
    // What this build doesn't know, after the field it followed.
    for (i, (k, ov)) in o.iter().enumerate() {
        if get(b, k).is_some() || out.iter().any(|(ok, _)| ok == k) {
            continue;
        }
        let at = o[..i]
            .iter()
            .rev()
            .find_map(|(pk, _)| out.iter().position(|(ok, _)| ok == pk))
            .map_or(0, |p| p + 1);
        out.insert(at, (k.clone(), ov.get().to_owned()));
    }
    (out, lost)
}

fn merge_items(o: &[&RawValue], b: &[&RawValue], n: &[&RawValue]) -> (Vec<String>, bool) {
    let mut lost = false;
    if let Some([ok, bk, nk]) = keyed([o, b, n]) {
        let by_key = |keys: &[String], items: &[&RawValue]| -> HashMap<String, String> {
            keys.iter()
                .cloned()
                .zip(items.iter().map(|v| v.get().to_owned()))
                .collect()
        };
        let (om, bm) = (by_key(&ok, o), by_key(&bk, b));
        let items = n
            .iter()
            .zip(&nk)
            .map(|(nv, k)| match (om.get(k), bm.get(k)) {
                (Some(ov), Some(bv)) => {
                    let (text, l) = merge(ov, bv, nv.get());
                    lost |= l;
                    text
                }
                _ => nv.get().to_owned(),
            })
            .collect();
        // Records deleted since.
        let now: HashSet<&String> = nk.iter().collect();
        for (k, bv) in &bm {
            if !now.contains(k)
                && let Some(ov) = om.get(k)
            {
                lost |= has_extra(ov, bv);
            }
        }
        return (items, lost);
    }
    if o.len() == b.len() && b.len() == n.len() {
        let items = o
            .iter()
            .zip(b)
            .zip(n)
            .map(|((ov, bv), nv)| {
                let (text, l) = merge(ov.get(), bv.get(), nv.get());
                lost |= l;
                text
            })
            .collect();
        return (items, lost);
    }
    let lost = pairs(o, b)
        .into_iter()
        .any(|(o, b)| has_extra(o.get(), b.get()));
    (n.iter().map(|v| v.get().to_owned()).collect(), lost)
}

/// The file's entries laid out as `save` lays them out: one per line, and lists with one item
/// per line.
fn layout(entries: &[(String, String)]) -> String {
    let mut out = String::from("{\n");
    for (i, (k, v)) in entries.iter().enumerate() {
        out.push_str("  ");
        out.push_str(&key(k));
        out.push_str(": ");
        match node(v) {
            Node::Array(items) if items.is_empty() => out.push_str("[]"),
            Node::Array(items) => {
                out.push('[');
                for (j, item) in items.iter().enumerate() {
                    out.push_str(if j == 0 { "\n    " } else { ",\n    " });
                    out.push_str(item.get());
                }
                out.push_str("\n  ]");
            }
            _ => out.push_str(v.trim()),
        }
        out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_what_save_writes() {
        let text = crate::save(&drift_core::demo::mounting_plate(), &Default::default());
        let Node::Object(entries) = node(&text) else {
            panic!("a file is an object")
        };
        let entries: Vec<(String, String)> = entries
            .iter()
            .map(|(k, v)| (k.clone(), v.get().to_owned()))
            .collect();
        assert_eq!(layout(&entries), text);
    }

    #[test]
    fn records_are_matched_by_id_and_unknown_fields_follow_their_neighbour() {
        let o = r#"[{"id":"a","x":1,"new":true,"y":2},{"id":"b","x":1,"more":[1]}]"#;
        let b = r#"[{"id":"a","x":1,"y":2},{"id":"b","x":1}]"#;
        // `b` moved first and changed; `a` unchanged.
        let n = r#"[{"id":"b","x":5},{"id":"a","x":1,"y":2}]"#;
        let (text, lost) = merge(o, b, n);
        assert_eq!(
            text,
            r#"[{"id":"b","x":5,"more":[1]},{"id":"a","x":1,"new":true,"y":2}]"#
        );
        assert!(!lost);
        // `a` deleted: its unknown field is lost.
        let (text, lost) = merge(o, b, r#"[{"id":"b","x":1}]"#);
        assert_eq!(text, r#"[{"id":"b","x":1,"more":[1]}]"#);
        assert!(lost);
        // A record of another type under `b`'s id doesn't take `b`'s fields.
        let o = r#"[{"type":"line","id":"b","new":1}]"#;
        let b = r#"[{"type":"line","id":"b"}]"#;
        let (text, lost) = merge(o, b, r#"[{"type":"arc","id":"b"}]"#);
        assert_eq!(text, r#"[{"type":"arc","id":"b"}]"#);
        assert!(lost);
    }

    #[test]
    fn a_list_without_ids_is_matched_by_position_while_its_length_holds() {
        let o = r#"{"items":[{"p":1,"new":1},{"p":2}]}"#;
        let b = r#"{"items":[{"p":1},{"p":2}]}"#;
        let (text, lost) = merge(o, b, r#"{"items":[{"p":3},{"p":2}]}"#);
        assert_eq!(text, r#"{"items":[{"p":3,"new":1},{"p":2}]}"#);
        assert!(!lost);
        let (text, lost) = merge(o, b, r#"{"items":[{"p":2}]}"#);
        assert_eq!(text, r#"{"items":[{"p":2}]}"#);
        assert!(lost);
        // Without unknown data nothing is lost.
        let (_, lost) = merge(b, b, r#"{"items":[{"p":2}]}"#);
        assert!(!lost);
    }

    #[test]
    fn a_cleared_field_loses_what_it_held() {
        let o = r#"{"id":"a","tol":{"type":"x","new":1}}"#;
        let b = r#"{"id":"a","tol":{"type":"x"}}"#;
        let (text, lost) = merge(o, b, r#"{"id":"a"}"#);
        assert_eq!(text, r#"{"id":"a"}"#);
        assert!(lost);
    }

    #[test]
    fn a_field_this_build_writes_now_wins_over_the_old_one() {
        // A newer build wrote `closed: false`, which this one leaves out; set since.
        let o = r#"{"id":"a","closed":false}"#;
        let b = r#"{"id":"a"}"#;
        let (text, lost) = merge(o, b, r#"{"id":"a","closed":true}"#);
        assert_eq!(text, r#"{"id":"a","closed":true}"#);
        assert!(!lost);
    }
}
