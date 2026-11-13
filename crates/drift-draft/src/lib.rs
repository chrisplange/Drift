//! Drafting for Drift: hatch patterns (AutoCAD `.pat` format, plus a built-in library), hatch
//! lines clipped exactly to closed regions, the page display list that output formats share,
//! and sheet borders, title blocks and revision tables.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

pub mod crop;
pub mod font;
pub mod hatch;
pub mod logo;
pub mod outline;
pub mod page;
pub mod pattern;
pub mod sheet;

pub use hatch::{Hatching, Placement, hatch, tessellate};
pub use pattern::{PatLine, Pattern, builtin, find, parse_pat, to_pat};
