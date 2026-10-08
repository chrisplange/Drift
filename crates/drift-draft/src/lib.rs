//! Drafting for Drift: hatch patterns (AutoCAD `.pat` format, plus a built-in library) and
//! hatch lines clipped exactly to closed regions.

pub mod hatch;
pub mod pattern;

pub use hatch::{Hatching, Placement, hatch};
pub use pattern::{PatLine, Pattern, builtin, find, parse_pat, to_pat};
