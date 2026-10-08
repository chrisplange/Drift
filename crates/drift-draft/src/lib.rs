//! Drafting for Drift: hatch patterns (AutoCAD `.pat` format, plus a built-in library), hatch
//! lines clipped exactly to closed regions, the page display list that output formats share,
//! and sheet borders, title blocks and revision tables.

pub mod hatch;
pub mod page;
pub mod pattern;
pub mod sheet;

pub use hatch::{Hatching, Placement, hatch};
pub use pattern::{PatLine, Pattern, builtin, find, parse_pat, to_pat};
