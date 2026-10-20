//! Blender profiles: the curves of the blocks marked as profiles, for the Drift profiles
//! add-on to sweep (as a bevel object) or lathe (with a Screw modifier) in Blender.
//!
//! The file sits next to the drawing as `<name>.drift-profiles.json` and has its own schema,
//! versioned apart from `.drift`, so the add-on never reads a drawing:
//!
//! ```json
//! {
//!   "format": "drift-profiles",
//!   "version": 1,
//!   "units": "m",
//!   "source": "Trim.drift",
//!   "profiles": [
//!     {
//!       "name": "Crown",
//!       "id": "b2",
//!       "loops": [
//!         {"closed": true, "knots": [
//!           {"co":[0.0,0.0],"left":[0.0,-0.001],"right":[0.001,0.0]}
//!         ]}
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! Each profile is one block, in its own coordinates (its base point is the origin), in
//! metres with y up. Its curves are joined end to end into loops of cubic Bézier knots
//! (arcs, circles and ellipses become Béziers; lines have their handles at their thirds).
//! A loop that comes back to its start is `closed`; an inner loop (a hole) is a loop of its
//! own. Construction and centre lines and points are left out. Profiles are in block id
//! order and loops in the order of their curves, so the file changes only where the
//! drawing did.

use drift_core::{Block, BlockId, DVec2, LineKind};
use serde::{Deserialize, Serialize};

/// The file's format tag.
pub const FORMAT_TAG: &str = "drift-profiles";
/// The schema version written. Readers accept this and older.
pub const VERSION: u32 = 1;
/// What follows the drawing's name: `Trim.drift` → `Trim.drift-profiles.json`.
pub const EXTENSION: &str = "drift-profiles.json";

/// Curves within this distance (mm) of each other's ends join.
const JOIN_MM: f64 = 1e-4;
/// The longest arc one Bézier spans: 45° keeps a circle within 5 parts per million.
const MAX_STEP: f64 = std::f64::consts::FRAC_PI_4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileFile {
    pub format: String,
    pub version: u32,
    pub units: String,
    /// The drawing's file name.
    pub source: String,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The block's name.
    pub name: String,
    /// The block's id (`b3`), which stays the same when the block is renamed.
    pub id: String,
    pub loops: Vec<Loop>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub closed: bool,
    pub knots: Vec<Knot>,
}

/// A Bézier knot: where the curve passes, and the handles into it (`left`) and out of it
/// (`right`). An open loop's end handles point straight on, away from it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knot {
    pub co: [f64; 2],
    pub left: [f64; 2],
    pub right: [f64; 2],
}

/// The profiles of the blocks marked as profiles, or `None` when none is.
pub fn profiles<'a>(
    blocks: impl IntoIterator<Item = (BlockId, &'a Block)>,
    source: &str,
) -> Option<ProfileFile> {
    let profiles: Vec<Profile> = blocks
        .into_iter()
        .filter(|(_, b)| b.profile)
        .map(|(id, b)| Profile {
            name: b.name.clone(),
            id: id.to_string(),
            loops: loops(b),
        })
        .collect();
    (!profiles.is_empty()).then(|| ProfileFile {
        format: FORMAT_TAG.into(),
        version: VERSION,
        units: "m".into(),
        source: source.into(),
        profiles,
    })
}

/// The profiles file's text for `blocks`, or `None` when no block is a profile.
pub fn write<'a>(
    blocks: impl IntoIterator<Item = (BlockId, &'a Block)>,
    source: &str,
) -> Option<String> {
    profiles(blocks, source).map(|f| to_json(&f))
}

/// A profiles file as text: one knot to a line, so it diffs cleanly.
pub fn to_json(file: &ProfileFile) -> String {
    let s = |v: &str| serde_json::to_string(v).expect("a string serializes");
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"format\": {},\n", s(&file.format)));
    out.push_str(&format!("  \"version\": {},\n", file.version));
    out.push_str(&format!("  \"units\": {},\n", s(&file.units)));
    out.push_str(&format!("  \"source\": {},\n", s(&file.source)));
    out.push_str("  \"profiles\": [");
    for (i, p) in file.profiles.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": {},\n", s(&p.name)));
        out.push_str(&format!("      \"id\": {},\n", s(&p.id)));
        out.push_str("      \"loops\": [");
        for (j, l) in p.loops.iter().enumerate() {
            out.push_str(if j == 0 { "\n" } else { ",\n" });
            out.push_str(&format!(
                "        {{\"closed\": {}, \"knots\": [\n",
                l.closed
            ));
            for (k, knot) in l.knots.iter().enumerate() {
                out.push_str("          ");
                out.push_str(&serde_json::to_string(knot).expect("a knot serializes"));
                out.push_str(if k + 1 < l.knots.len() { ",\n" } else { "\n" });
            }
            out.push_str("        ]}");
        }
        out.push_str(if p.loops.is_empty() {
            "]\n"
        } else {
            "\n      ]\n"
        });
        out.push_str("    }");
    }
    out.push_str(if file.profiles.is_empty() {
        "]\n"
    } else {
        "\n  ]\n"
    });
    out.push_str("}\n");
    out
}

/// Read a profiles file (of this version or older).
pub fn parse(text: &str) -> Result<ProfileFile, String> {
    let file: ProfileFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.format != FORMAT_TAG {
        return Err(format!("not a Drift profiles file ({:?})", file.format));
    }
    if file.version == 0 || file.version > VERSION {
        return Err(format!(
            "profiles file version {} is not supported",
            file.version
        ));
    }
    Ok(file)
}

/// Where a drawing's profiles file goes: beside it, `Trim.drift` → `Trim.drift-profiles.json`.
pub fn path_beside(drawing: &std::path::Path) -> std::path::PathBuf {
    drawing.with_extension(EXTENSION)
}

/// What [`update_beside`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Updated {
    /// Wrote this many profiles.
    Written(usize),
    /// No block is a profile any more, so the old file was deleted.
    Removed,
    /// No block is a profile and there was no file.
    Nothing,
}

/// Write the profiles file beside the drawing at `drawing`, or delete a stale one when no
/// block is a profile. The file is written whole to a temporary name and then renamed over
/// the old one, so a reader never sees half of it. (Native only: the web has no paths.)
pub fn update_beside<'a>(
    drawing: &std::path::Path,
    blocks: impl IntoIterator<Item = (BlockId, &'a Block)>,
) -> std::io::Result<Updated> {
    let path = path_beside(drawing);
    let source = drawing
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(file) = profiles(blocks, &source) else {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(Updated::Removed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Updated::Nothing),
            Err(e) => Err(e),
        };
    };
    let mut temp = path.clone().into_os_string();
    temp.push(".tmp");
    let temp = std::path::PathBuf::from(temp);
    std::fs::write(&temp, to_json(&file))?;
    // A reader holding the old file open can block the rename on Windows for a moment.
    let mut tries = 0;
    loop {
        match std::fs::rename(&temp, &path) {
            Ok(()) => return Ok(Updated::Written(file.profiles.len())),
            Err(_) if tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(e);
            }
        }
    }
}

/// A run of joined curves as cubic Béziers in drawing order, in mm.
type Chain = Vec<[DVec2; 4]>;

/// A block's solid curves joined into loops.
fn loops(block: &Block) -> Vec<Loop> {
    // Each loop with the index of its first curve, to keep them in drawing order.
    let mut loops: Vec<(usize, Loop)> = Vec::new();
    let mut open: Vec<(usize, Chain)> = Vec::new();
    for (i, item) in block.items.iter().enumerate() {
        if item.style != LineKind::Solid {
            continue;
        }
        let cubics = item.shape.cubics(MAX_STEP);
        if cubics.is_empty() {
            continue;
        }
        if item.shape.is_closed() {
            loops.push((i, close(cubics)));
        } else {
            open.push((i, cubics));
        }
    }
    // Join the open curves end to end, whichever way round each was drawn.
    let near = |a: DVec2, b: DVec2| a.distance(b) <= JOIN_MM;
    let start = |c: &Chain| c[0][0];
    let end = |c: &Chain| c[c.len() - 1][3];
    let mut used = vec![false; open.len()];
    for first in 0..open.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut chain = open[first].1.clone();
        let mut closed = false;
        // The next unused curve with an end at `at`.
        let take = |at: DVec2, used: &mut Vec<bool>| {
            let k = (0..open.len()).find(|&k| {
                !used[k] && (near(start(&open[k].1), at) || near(end(&open[k].1), at))
            })?;
            used[k] = true;
            Some(&open[k].1)
        };
        // On from its end…
        while !closed {
            let at = end(&chain);
            let Some(next) = take(at, &mut used) else {
                break;
            };
            if near(start(next), at) {
                chain.extend(next.iter().copied());
            } else {
                chain.extend(reversed(next));
            }
            closed = near(end(&chain), start(&chain));
        }
        // …and back from its start.
        while !closed {
            let at = start(&chain);
            let Some(before) = take(at, &mut used) else {
                break;
            };
            let mut before = if near(end(before), at) {
                before.clone()
            } else {
                reversed(before)
            };
            before.extend(chain);
            chain = before;
            closed = near(end(&chain), start(&chain));
        }
        let l = if closed {
            close(chain)
        } else {
            knots_open(&chain)
        };
        loops.push((open[first].0, l));
    }
    loops.sort_by_key(|(at, _)| *at);
    loops.into_iter().map(|(_, l)| l).collect()
}

/// A chain run the other way.
fn reversed(chain: &Chain) -> Chain {
    chain
        .iter()
        .rev()
        .map(|&[a, b, c, d]| [d, c, b, a])
        .collect()
}

/// Metres from mm, to the nanometre.
fn m(p: DVec2) -> [f64; 2] {
    // Adding 0.0 turns −0 into 0.
    [p.x, p.y].map(|v| (v * 1e6).round() / 1e9 + 0.0)
}

/// A chain that ends where it starts, as a closed loop: a knot where each Bézier starts.
fn close(mut chain: Chain) -> Loop {
    let n = chain.len();
    chain[n - 1][3] = chain[0][0];
    let knots = (0..n)
        .map(|i| Knot {
            co: m(chain[i][0]),
            left: m(chain[(i + n - 1) % n][2]),
            right: m(chain[i][1]),
        })
        .collect();
    Loop {
        closed: true,
        knots,
    }
}

/// An open chain as a loop: a knot at each end and where each Bézier meets the next.
fn knots_open(chain: &Chain) -> Loop {
    let n = chain.len();
    let mut knots: Vec<Knot> = (0..n)
        .map(|i| {
            let co = chain[i][0];
            let left = if i == 0 {
                co * 2.0 - chain[0][1]
            } else {
                chain[i - 1][2]
            };
            Knot {
                co: m(co),
                left: m(left),
                right: m(chain[i][1]),
            }
        })
        .collect();
    let [_, _, c2, end] = chain[n - 1];
    knots.push(Knot {
        co: m(end),
        left: m(c2),
        right: m(end * 2.0 - c2),
    });
    Loop {
        closed: false,
        knots,
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    use drift_core::shape::bezier_point;
    use drift_core::{Appearance, BlockItem, Shape};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn solid(shape: Shape) -> BlockItem {
        BlockItem {
            shape,
            style: LineKind::Solid,
            appearance: Appearance::default(),
        }
    }

    fn line(a: DVec2, b: DVec2) -> BlockItem {
        solid(Shape::Line { a, b })
    }

    fn profile(name: &str, items: Vec<BlockItem>) -> Block {
        Block {
            name: name.into(),
            items,
            profile: true,
            ..Default::default()
        }
    }

    fn mm(p: [f64; 2]) -> DVec2 {
        DVec2::from_array(p) * 1000.0
    }

    /// Every point along a loop's Béziers, in mm.
    fn samples(l: &Loop) -> Vec<DVec2> {
        let n = l.knots.len();
        let segments = if l.closed { n } else { n - 1 };
        let mut out = Vec::new();
        for i in 0..segments {
            let (a, b) = (l.knots[i], l.knots[(i + 1) % n]);
            let p = [mm(a.co), mm(a.right), mm(b.left), mm(b.co)];
            out.extend((0..=16).map(|k| bezier_point(&p, k as f64 / 16.0)));
        }
        out
    }

    /// A moulding: a rectangle with a rounded corner (drawn in a muddle of directions and
    /// orders), a round hole, a construction line, a point, and a centre line.
    fn moulding() -> Block {
        profile(
            "Crown",
            vec![
                line(v(0.0, 0.0), v(40.0, 0.0)),
                solid(Shape::Circle {
                    center: v(10.0, 10.0),
                    radius: 4.0,
                }),
                line(v(0.0, 30.0), v(0.0, 0.0)),
                solid(Shape::Arc {
                    center: v(30.0, 20.0),
                    radius: 10.0,
                    start: 0.0,
                    sweep: FRAC_PI_2,
                }),
                line(v(40.0, 20.0), v(40.0, 0.0)),
                line(v(30.0, 30.0), v(0.0, 30.0)),
                BlockItem {
                    style: LineKind::Construction,
                    ..line(v(-5.0, 15.0), v(45.0, 15.0))
                },
                BlockItem {
                    style: LineKind::Center,
                    ..line(v(20.0, -5.0), v(20.0, 35.0))
                },
                solid(Shape::Point(v(20.0, 15.0))),
            ],
        )
    }

    #[test]
    fn a_profile_round_trips_through_its_text() {
        let a = moulding();
        let file = profiles([(BlockId(2), &a)], "Trim.drift").unwrap();
        let text = to_json(&file);
        assert_eq!(parse(&text).unwrap(), file);
        // Plain JSON too, for any reader.
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], "drift-profiles");
        assert_eq!(value["units"], "m");
        assert_eq!(value["profiles"][0]["id"], "b2");
        assert!(parse(&text.replace("\"version\": 1", "\"version\": 2")).is_err());
        assert!(parse(&text.replace("drift-profiles", "other")).is_err());
    }

    #[test]
    fn curves_join_into_an_outline_and_a_hole() {
        let file = profiles([(BlockId(2), &moulding())], "Trim.drift").unwrap();
        let loops = &file.profiles[0].loops;
        assert_eq!(loops.len(), 2, "{loops:?}");
        // The outline comes first (its first curve is the block's first item).
        let outline = &loops[0];
        assert!(outline.closed);
        // Four lines and a quarter arc in two 45° pieces.
        assert_eq!(outline.knots.len(), 6);
        // Construction, centre lines and points are left out.
        for p in samples(outline) {
            assert!(
                (-1e-6..=40.0 + 1e-6).contains(&p.x) && (-1e-6..=30.0 + 1e-6).contains(&p.y),
                "{p}"
            );
        }
        let hole = &loops[1];
        assert!(hole.closed);
        assert_eq!(hole.knots.len(), 8);
    }

    #[test]
    fn arcs_and_ellipses_stay_within_tolerance() {
        let radius = 250.0;
        let arc = profile(
            "Arc",
            vec![solid(Shape::Arc {
                center: v(5.0, -3.0),
                radius,
                start: 0.3,
                sweep: 4.0,
            })],
        );
        let file = profiles([(BlockId(1), &arc)], "x.drift").unwrap();
        let l = &file.profiles[0].loops[0];
        assert!(!l.closed);
        let worst = samples(l)
            .iter()
            .map(|p| (p.distance(v(5.0, -3.0)) - radius).abs())
            .fold(0.0, f64::max);
        // 45° pieces: within about 4 parts per million of the radius.
        assert!(worst < 5e-6 * radius, "{worst}");

        let (center, major, minor) = (v(1.0, 2.0), DVec2::from_angle(0.4) * 80.0, 30.0);
        let ellipse = profile(
            "Ellipse",
            vec![solid(Shape::Ellipse {
                center,
                major,
                minor,
                start: 0.0,
                sweep: TAU,
            })],
        );
        let file = profiles([(BlockId(1), &ellipse)], "x.drift").unwrap();
        let l = &file.profiles[0].loops[0];
        assert!(l.closed);
        assert_eq!(l.knots.len(), 8);
        // On the ellipse: (x/a)² + (y/b)² = 1 in its own axes.
        let u = major.normalize();
        for p in samples(l) {
            let d = p - center;
            let (x, y) = (d.dot(u) / major.length(), d.dot(u.perp()) / minor);
            assert!((x * x + y * y - 1.0).abs() < 2e-5, "{p}");
        }
    }

    #[test]
    fn open_chains_stay_open_for_lathes() {
        // Half a vase: up from the axis, round a bulge, and back to the axis at the top.
        let half = profile(
            "Vase",
            vec![
                line(v(0.0, 0.0), v(40.0, 0.0)),
                solid(Shape::Arc {
                    center: v(40.0, 60.0),
                    radius: 60.0,
                    start: -FRAC_PI_2,
                    sweep: PI,
                }),
                // Drawn backwards: from the axis out to the lip.
                line(v(0.0, 120.0), v(40.0, 120.0)),
            ],
        );
        let file = profiles([(BlockId(1), &half)], "x.drift").unwrap();
        let loops = &file.profiles[0].loops;
        assert_eq!(loops.len(), 1);
        let l = &loops[0];
        assert!(!l.closed);
        // One line, the arc in four, the other line: six pieces, seven knots, from axis to
        // axis.
        assert_eq!(l.knots.len(), 7);
        assert_eq!(l.knots[0].co, [0.0, 0.0]);
        assert_eq!(l.knots[6].co, [0.0, 0.12]);
        // The open ends' outer handles point straight on.
        assert_eq!(l.knots[0].left[1], 0.0);
        assert!(l.knots[0].left[0] < 0.0);
    }

    #[test]
    fn coordinates_are_metres_from_the_base_point() {
        // A block's own coordinates already have its base point at the origin.
        let square = profile(
            "Square",
            vec![
                line(v(-10.0, -10.0), v(10.0, -10.0)),
                line(v(10.0, -10.0), v(10.0, 10.0)),
                line(v(10.0, 10.0), v(-10.0, 10.0)),
                line(v(-10.0, 10.0), v(-10.0, -10.0)),
            ],
        );
        let file = profiles([(BlockId(1), &square)], "x.drift").unwrap();
        let l = &file.profiles[0].loops[0];
        assert!(l.closed);
        let cos: Vec<[f64; 2]> = l.knots.iter().map(|k| k.co).collect();
        assert_eq!(
            cos,
            [[-0.01, -0.01], [0.01, -0.01], [0.01, 0.01], [-0.01, 0.01]]
        );
        // Straight sides: handles at their thirds.
        let third = 0.02 / 3.0 - 0.01;
        assert!((l.knots[0].right[0] - third).abs() < 1e-9);
    }

    #[test]
    fn only_marked_blocks_are_written_in_id_order() {
        let a = moulding();
        let mut b = moulding();
        b.name = "Base".into();
        let mut chair = moulding();
        chair.name = "Chair".into();
        chair.profile = false;
        assert!(profiles([(BlockId(1), &chair)], "x.drift").is_none());
        let file = profiles(
            [(BlockId(1), &chair), (BlockId(2), &a), (BlockId(5), &b)],
            "x.drift",
        )
        .unwrap();
        let names: Vec<(&str, &str)> = file
            .profiles
            .iter()
            .map(|p| (p.name.as_str(), p.id.as_str()))
            .collect();
        assert_eq!(names, [("Crown", "b2"), ("Base", "b5")]);
    }

    #[test]
    fn the_file_beside_a_drawing_is_written_and_a_stale_one_removed() {
        let dir = std::env::temp_dir().join(format!("drift-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let drawing = dir.join("Trim.drift");
        let beside = dir.join("Trim.drift-profiles.json");
        assert_eq!(path_beside(&drawing), beside);
        let _ = std::fs::remove_file(&beside);

        let mut a = moulding();
        assert_eq!(
            update_beside(&drawing, [(BlockId(2), &a)]).unwrap(),
            Updated::Written(1)
        );
        let read = parse(&std::fs::read_to_string(&beside).unwrap()).unwrap();
        assert_eq!(read.source, "Trim.drift");
        assert_eq!(read.profiles[0].name, "Crown");
        // No temporary file is left.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        a.profile = false;
        assert_eq!(
            update_beside(&drawing, [(BlockId(2), &a)]).unwrap(),
            Updated::Removed
        );
        assert!(!beside.exists());
        assert_eq!(
            update_beside(&drawing, [(BlockId(2), &a)]).unwrap(),
            Updated::Nothing
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The add-on's tests read this file: it must be exactly what Drift writes.
    #[test]
    fn the_blender_add_on_fixture_is_what_drift_writes() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../integrations/blender/tests/fixtures/Trim.drift-profiles.json"
        );
        let half = profile(
            "Vase",
            vec![
                line(v(0.0, 0.0), v(40.0, 0.0)),
                solid(Shape::Arc {
                    center: v(40.0, 60.0),
                    radius: 60.0,
                    start: -FRAC_PI_2,
                    sweep: PI,
                }),
                line(v(0.0, 120.0), v(40.0, 120.0)),
            ],
        );
        let text = write(
            [(BlockId(2), &moulding()), (BlockId(3), &half)],
            "Trim.drift",
        )
        .unwrap();
        if std::env::var_os("DRIFT_BLESS").is_some() {
            std::fs::write(fixture, &text).unwrap();
        }
        let on_disk = std::fs::read_to_string(fixture)
            .expect("the fixture (run with DRIFT_BLESS=1 to write it)")
            .replace("\r\n", "\n");
        assert_eq!(on_disk, text);
    }
}
