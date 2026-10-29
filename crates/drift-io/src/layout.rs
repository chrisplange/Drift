//! How a `.drift` file is laid out (`docs/file-format.md`, policy rule 9): top-level entries one
//! per line, lists of records one record per line, and everything inside a record compact, as
//! `serde_json` writes it. [`save`](crate::save) writes through it, and so does carrying through
//! (`carry.rs`) when it puts a file back together.

use std::io::{self, Write};

use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter};

/// The file as text, laid out.
pub(crate) fn to_file<T: Serialize + ?Sized>(value: &T) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, FileLayout::default());
    value
        .serialize(&mut serializer)
        .expect("a drawing serializes");
    out.push(b'\n');
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

/// A [`Formatter`] that lays out the file's top level and its lists, and leaves everything
/// deeper to [`CompactFormatter`].
#[derive(Default)]
struct FileLayout {
    /// How many objects and lists are open.
    depth: usize,
    /// The top-level list being written has had an item.
    items: bool,
}

impl Formatter for FileLayout {
    fn begin_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.depth += 1;
        if self.depth == 2 {
            self.items = false;
        }
        CompactFormatter.begin_array(writer)
    }

    fn end_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.depth -= 1;
        if self.depth == 1 && self.items {
            writer.write_all(b"\n  ]")
        } else {
            CompactFormatter.end_array(writer)
        }
    }

    fn begin_array_value<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if self.depth == 2 {
            self.items = true;
            writer.write_all(if first { b"\n    " } else { b",\n    " })
        } else {
            CompactFormatter.begin_array_value(writer, first)
        }
    }

    fn begin_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.depth += 1;
        CompactFormatter.begin_object(writer)
    }

    fn end_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.depth -= 1;
        if self.depth == 0 {
            writer.write_all(b"\n}")
        } else {
            CompactFormatter.end_object(writer)
        }
    }

    fn begin_object_key<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if self.depth == 1 {
            writer.write_all(if first { b"\n  " } else { b",\n  " })
        } else {
            CompactFormatter.begin_object_key(writer, first)
        }
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        if self.depth == 1 {
            writer.write_all(b": ")
        } else {
            CompactFormatter.begin_object_value(writer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_level_and_its_lists_are_laid_out_and_the_rest_is_compact() {
        let value = serde_json::json!({
            "a": {"b": [1, {"c": [2, 3]}]},
            "empty": [],
            "list": [{"x": [1, [2]]}, "y"],
            "n": 1.5,
        });
        assert_eq!(
            to_file(&value),
            "{\n  \"a\": {\"b\":[1,{\"c\":[2,3]}]},\n  \"empty\": [],\n  \"list\": [\n    \
             {\"x\":[1,[2]]},\n    \"y\"\n  ],\n  \"n\": 1.5\n}\n"
        );
    }
}
