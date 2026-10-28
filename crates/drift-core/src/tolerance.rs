//! Tolerances: how close is close enough, named once with what each is for.
//!
//! Lengths are millimetres unless a name says pixels. Where a tolerance has to grow with
//! the drawing (floating-point error grows with the size of the coordinates), it is a
//! factor of [`scale`], the size that bounds that error.

/// Curves flattened to points stay within this of the true curve: in the model (to decide
/// what is inside an area, so trimming and regions agree about it; to measure; for fill
/// meshes drawn at any zoom) and on paper (exports, print, underlays). A fifth of a
/// 1200-dpi dot, and about as fine as the old per-call segment counts were at usual sizes.
pub const FLATTEN_MM: f64 = 0.005;

/// Curves flattened on screen stay within this many physical pixels of the true curve.
pub const FLATTEN_PX: f64 = 0.1;
