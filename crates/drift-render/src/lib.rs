//! GPU canvas renderer for Drift.
//!
//! Lines, circles and arcs are drawn as analytic signed-distance fields on instanced quads, so
//! they stay perfectly crisp at any zoom. The renderer only uses features available in WebGL2,
//! so the same code runs natively and in every browser.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

mod camera;
mod frame;
mod palette;
mod renderer;

pub use camera::Camera;
pub use frame::{
    ArcInstance, DrawGroup, FillVertex, Frame, FrameImage, Globals, GridInfo, GroupRanges,
    ImageVertex, SegmentInstance, Viewport, build_frame, clip_polygon, grid_info,
};
pub use palette::Palette;
pub use renderer::CanvasRenderer;
