//! Editing a Drift drawing without a window: the [`editor::Editor`] (one editing session on one
//! document, with its undo history and caches), the edits that hang off it, and the drawing
//! [`tools`], which turn gestures into edits. Nothing here depends on egui, eframe or the GPU;
//! the app (drift-app) draws what the editor holds and feeds the tools its input.

#[cfg(test)]
mod areas_props;
pub mod blocks;
pub mod checker;
pub mod clipboard;
pub mod curves;
#[cfg(test)]
mod drag_props;
pub mod editor;
#[cfg(test)]
mod fill_tests;
pub mod fills;
pub mod fonts;
pub mod frame_items;
pub mod labels;
pub mod library;
pub mod modify;
pub mod openings;
#[cfg(test)]
mod openings_props;
#[cfg(test)]
mod perf_big;
#[cfg(test)]
mod perf_tests;
pub mod picks;
pub mod prefs;
pub mod stack;
pub mod temp_dims;
#[cfg(any(test, feature = "test-support"))]
pub mod test_sketches;
pub mod texts;
pub mod tools;
pub mod transform;
pub mod underlays;
pub mod walls;
pub mod walls_from_lines;
#[cfg(test)]
mod walls_props;
