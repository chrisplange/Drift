//! Tolerances: how close is close enough, named once with what each is for.
//!
//! Lengths are millimetres unless a name says otherwise. A tolerance against rounding has
//! to grow with the drawing, because floating-point error grows with the size of the
//! coordinates: those are factors of [`scale`], the size that bounds that error. The others
//! are absolute.

/// The size that bounds the rounding error of coordinates up to `extent` (their largest
/// magnitude): `extent`, at least 1 mm; 1 for a drawing with no finite size.
pub fn scale(extent: f64) -> f64 {
    if extent.is_finite() {
        extent.abs().max(1.0)
    } else {
        1.0
    }
}

/// Rounding noise, relative to [`scale`]: points and lengths this close are the same.
/// About 10⁷ ulps of the coordinates, far below anything drawn, far above rounding. Regions
/// snap ends together within it, crossings take a touch within it (with [`PARALLEL_SINE`],
/// #191's rule), walls join within it.
pub const SAME: f64 = 1e-9;

/// [`SAME`] for coordinates up to `extent`.
pub fn same(extent: f64) -> f64 {
    SAME * scale(extent)
}

/// Positions and numbers from files or measured off drawn geometry, relative to their size
/// ([`scale`], or the drawing's size where a reader says so): this close, they are the same.
/// Looser than [`SAME`]: what a writer rounded, not what arithmetic did. Imported ends join
/// within it ([`IMPORT_JOIN`]); hatch patterns match the library's within it; bounds are
/// padded by it.
pub const NEAR: f64 = 1e-6;

/// Imported drawings: points this close, relative to the drawing's size, are one point, and
/// an end this close to another curve is put on it. Files round their coordinates to a
/// precision relative to the sheet, so ends drawn to meet miss each other, or the curve
/// they end on, by that much. Measured on real PDFs at paper size (#184): every such miss is
/// under 0.1 µm, and real detail (dash ends, short segments, glyph corners) starts at about
/// 1 µm; this is about 1 µm on a 1 m sheet, and grows with the scale a PDF comes in at, as
/// its rounding does. A CAD drawing's coordinates aren't rounded to a sheet, so its join is
/// capped at [`IMPORT_JOIN_CAD_MM`].
pub const IMPORT_JOIN: f64 = NEAR;

/// The most (mm) a CAD drawing's (DXF, DWG) imported points can be apart and still be
/// joined, whatever [`IMPORT_JOIN`] of its size is (so it acts on drawings over 5 m). In the
/// samples measured (#184) every CAD join was within 1 nm, but [`IMPORT_JOIN`] alone joined
/// two ends 17.6 µm apart in a 21.6 m building drawing, and would join within 5 mm on a
/// site plan 5 km across: a real gap of 0.01 mm between two parts has to stay open, and
/// this is half that. It isn't smaller because not every CAD file writes full doubles: a
/// DXF written with 4 decimals in inches rounds to 2.54 µm, and its ends still join.
pub const IMPORT_JOIN_CAD_MM: f64 = 5e-3;

/// A length (mm) below anything drawn and above rounding at drawing sizes: the least a gap
/// tolerance or a lookup grid's cell can be.
pub const TINY_MM: f64 = 1e-6;

/// Areas below this times [`scale`]² are rounding noise, not faces.
pub const AREA_NOISE: f64 = 1e-12;

/// Two directions are parallel when the sine of the angle between them is at most this.
/// Scale-free: it compares directions, not positions.
pub const PARALLEL_SINE: f64 = 1e-12;

/// Two directions whose angles (radians, as `atan2` gives them) are this close are the same
/// up to rounding: for ordering curves round a point, where the turn of each then decides.
pub const SAME_ANGLE: f64 = 1e-9;

/// Two angles (radians) worked out from drawn geometry this close are the same: ends that
/// run straight on, a whole turn, a tangent touch.
pub const ANGLE: f64 = 1e-6;

/// A length (mm) or turn (radians) at or below this is none at all: a curve with no length,
/// a direction that can't be told, an arc with no sweep. For guards against dividing by
/// nothing, not for comparing positions (that is [`same`]).
pub const NONE: f64 = 1e-12;

/// A turn within this of a whole number of quarter turns (in quarter turns, so about 1e-7°)
/// is one: placed exactly (`exact_rotor`), and keeping horizontal and vertical relations.
pub const QUARTER_TURN: f64 = 1e-9;

/// Curves flattened to points stay within this of the true curve: in the model (to decide
/// what is inside an area, so trimming and regions agree about it; to measure; for fill
/// meshes drawn at any zoom) and on paper (exports, print, underlays). A fifth of a
/// 1200-dpi dot, and about as fine as the old per-call segment counts were at usual sizes.
pub const FLATTEN_MM: f64 = 0.005;

/// Curves flattened to measure their length stay within this of the true curve: a chord
/// that sags `s` over a piece `l` long is about `8s²/3l` short, so a length shown to a
/// thousandth needs chords far finer than a drawing does.
pub const MEASURE_MM: f64 = 1e-5;

/// Curves flattened on screen stay within this many physical pixels of the true curve.
pub const FLATTEN_PX: f64 = 0.1;
