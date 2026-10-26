//! Size sets: the door and window types (and wall thickness presets) of one market, as data
//! (`docs/walls-design.md` §6). Each set is a JSON file in `sizes/`, compiled in and read
//! once; adding a market is one file and one line in [`SOURCES`]. The generators stay in
//! code ([`super::hosted`]): a set only says which sizes each family comes in.
//!
//! A type keeps two sizes apart: the **leaf** (what the symbol draws: a door's leaf, a
//! window's nominal size) and the **opening** (the cut in the wall: a door's rough opening).
//! A file's rows are written as groups (a family, its widths and heights, and the allowance
//! from leaf to opening), or as single rows where a type differs.
//!
//! Drawings store an opening's actual sizes, never a row, so a set can change without
//! changing a drawing.

use std::sync::OnceLock;

use drift_core::{Document, SizeSetId};
use serde::Deserialize;

use super::INCH;
use super::hosted::{self, FamilyKind};

/// The sets compiled in: (id, JSON).
const SOURCES: &[(&str, &str)] = &[
    ("us", include_str!("sizes/us.json")),
    ("metric", include_str!("sizes/metric.json")),
];

/// One type of a family.
#[derive(Clone, Debug, PartialEq)]
pub struct SizeRow {
    pub family: &'static str,
    /// Its name in the set's own terms: `3′0″ × 6′8″`, `3050`, `900 × 2040`.
    pub name: String,
    /// The leaf (a window's nominal size): width and height in mm.
    pub leaf: (f64, f64),
    /// The cut in the wall: width and height in mm.
    pub opening: (f64, f64),
}

impl SizeRow {
    /// Named as the library names its blocks: `Door – Single Swing – 3′0″ × 6′8″`.
    pub fn label(&self) -> String {
        let family = hosted::family(self.family).map_or(self.family, |f| f.name);
        format!("{family} – {}", self.name)
    }
}

/// A wall thickness preset.
#[derive(Clone, Debug, PartialEq)]
pub struct WallPreset {
    pub label: String,
    pub mm: f64,
}

/// A market's sizes.
#[derive(Clone, Debug)]
pub struct SizeSet {
    pub id: &'static str,
    pub name: String,
    /// Which drawings it suits by default.
    pub metric: bool,
    pub doors: Vec<SizeRow>,
    pub windows: Vec<SizeRow>,
    pub walls: Vec<WallPreset>,
    /// The thickness presets as the wall tool bar lists them (made once).
    presets: Vec<crate::walls::Preset>,
}

impl SizeSet {
    /// The types of a family, in the set's order.
    pub fn rows_of<'a>(&'a self, family: &'a str) -> impl Iterator<Item = &'a SizeRow> + 'a {
        self.doors
            .iter()
            .chain(&self.windows)
            .filter(move |r| r.family == family)
    }

    /// The families the set has types of, in the library's order.
    pub fn families(&self) -> Vec<&'static hosted::Family> {
        hosted::FAMILIES
            .iter()
            .filter(|f| self.rows_of(f.id).next().is_some())
            .collect()
    }

    /// The wall thickness presets.
    pub fn wall_presets(&self) -> &[crate::walls::Preset] {
        &self.presets
    }
}

#[derive(Deserialize)]
struct FileSet {
    id: String,
    name: String,
    units: String,
    /// `in` or `mm`: what the sizes are written in.
    unit: String,
    #[serde(default)]
    walls: Vec<FileWall>,
    #[serde(default)]
    door_groups: Vec<FileGroup>,
    #[serde(default)]
    doors: Vec<FileRow>,
    #[serde(default)]
    window_groups: Vec<FileGroup>,
    #[serde(default)]
    windows: Vec<FileRow>,
}

#[derive(Deserialize)]
struct FileWall {
    label: String,
    thickness: f64,
}

/// Every width by every height, each opening the leaf plus the allowance.
#[derive(Deserialize)]
struct FileGroup {
    family: String,
    widths: Vec<f64>,
    heights: Vec<f64>,
    allowance: [f64; 2],
    /// `us`: windows named the US way (`3050` for 3′0″ × 5′0″).
    #[serde(default)]
    names: Option<String>,
}

#[derive(Deserialize)]
struct FileRow {
    family: String,
    #[serde(default)]
    name: Option<String>,
    leaf: [f64; 2],
    opening: [f64; 2],
}

/// Feet and inches the way door sizes are named: `3′0″`, `2′6″`, `2′10½″`.
fn feet_inches(inches: f64) -> String {
    let feet = (inches / 12.0).floor();
    let rest = inches - feet * 12.0;
    let whole = rest.trunc();
    let half = if rest - whole >= 0.25 { "½" } else { "" };
    format!("{}′{}{half}″", feet as i64, whole as i64)
}

/// A US window name: feet and inches of width then height, `3050` (3′0″ × 5′0″).
fn us_window(w: f64, h: f64) -> String {
    let part = |v: f64| {
        let feet = (v / 12.0).floor();
        format!("{}{}", feet as i64, (v - feet * 12.0).round() as i64)
    };
    format!("{}{}", part(w), part(h))
}

fn parse(id: &'static str, text: &str) -> Result<SizeSet, String> {
    let f: FileSet = serde_json::from_str(text).map_err(|e| format!("size set {id}: {e}"))?;
    if f.id != id {
        return Err(format!("size set {id} calls itself {}", f.id));
    }
    let scale = match f.unit.as_str() {
        "in" => INCH,
        "mm" => 1.0,
        u => return Err(format!("size set {id}: unknown unit {u}")),
    };
    let metric = match f.units.as_str() {
        "metric" => true,
        "imperial" => false,
        u => return Err(format!("size set {id}: unknown units {u}")),
    };
    let family = |name: &str| {
        hosted::family(name)
            .map(|f| f.id)
            .ok_or_else(|| format!("size set {id}: no family {name}"))
    };
    let name_of = |w: f64, h: f64, us_window_names: bool| {
        if us_window_names {
            us_window(w, h)
        } else if scale == INCH {
            format!("{} × {}", feet_inches(w), feet_inches(h))
        } else {
            format!("{w} × {h}")
        }
    };
    let mut doors = Vec::new();
    let mut windows = Vec::new();
    for (groups, rows, out) in [
        (&f.door_groups, &f.doors, &mut doors),
        (&f.window_groups, &f.windows, &mut windows),
    ] {
        for g in groups {
            let fam = family(&g.family)?;
            let us = g.names.as_deref() == Some("us");
            for &h in &g.heights {
                for &w in &g.widths {
                    out.push(SizeRow {
                        family: fam,
                        name: name_of(w, h, us),
                        leaf: (w * scale, h * scale),
                        opening: ((w + g.allowance[0]) * scale, (h + g.allowance[1]) * scale),
                    });
                }
            }
        }
        for r in rows {
            let fam = family(&r.family)?;
            out.push(SizeRow {
                family: fam,
                name: r
                    .name
                    .clone()
                    .unwrap_or_else(|| name_of(r.leaf[0], r.leaf[1], false)),
                leaf: (r.leaf[0] * scale, r.leaf[1] * scale),
                opening: (r.opening[0] * scale, r.opening[1] * scale),
            });
        }
    }
    // Doors by family, then width, then height (a group lists heights outermost).
    let order = |rows: &mut Vec<SizeRow>| {
        let rank = |f: &str| hosted::FAMILIES.iter().position(|x| x.id == f);
        rows.sort_by(|a, b| {
            rank(a.family)
                .cmp(&rank(b.family))
                .then(a.leaf.0.total_cmp(&b.leaf.0))
                .then(a.leaf.1.total_cmp(&b.leaf.1))
        });
    };
    order(&mut doors);
    order(&mut windows);
    for r in doors.iter().chain(&windows) {
        let kind = hosted::family(r.family).map(|f| f.kind);
        let is_window = windows.contains(r);
        if is_window != (kind == Some(FamilyKind::Window)) {
            return Err(format!("size set {id}: {} is in the wrong list", r.label()));
        }
    }
    let walls: Vec<WallPreset> = f
        .walls
        .iter()
        .map(|w| WallPreset {
            label: w.label.clone(),
            mm: w.thickness * scale,
        })
        .collect();
    // (The tool bar's presets borrow their labels for the life of the program: sets are
    // read once.)
    let presets = walls
        .iter()
        .map(|w| crate::walls::Preset {
            label: Box::leak(w.label.clone().into_boxed_str()),
            mm: w.mm,
        })
        .collect();
    Ok(SizeSet {
        id,
        name: f.name,
        metric,
        doors,
        windows,
        walls,
        presets,
    })
}

/// Every size set, in the order the pickers list them.
pub fn all() -> &'static [SizeSet] {
    static SETS: OnceLock<Vec<SizeSet>> = OnceLock::new();
    SETS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(id, text)| parse(id, text).expect("the size sets compiled in are valid"))
            .collect()
    })
}

/// The set of an id.
pub fn get(id: &str) -> Option<&'static SizeSet> {
    all().iter().find(|s| s.id == id)
}

/// The set a drawing uses: the one its rules name, else by its units (US imperial for
/// imperial drawings, generic metric for metric ones).
pub fn for_doc(doc: &Document) -> &'static SizeSet {
    let named = doc.rules().size_set.and_then(|s| get(s.as_str()));
    named.unwrap_or_else(|| {
        let metric = doc.units().length.is_metric();
        all()
            .iter()
            .find(|s| s.metric == metric)
            .unwrap_or(&all()[0])
    })
}

/// The id to store for a set picked for a drawing (None: by its units).
pub fn id_for(set: Option<&str>) -> Option<SizeSetId> {
    set.and_then(SizeSetId::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_sets_name_real_families_and_openings_hold_their_leaves() {
        assert!(all().len() >= 2);
        for set in all() {
            assert!(
                !set.doors.is_empty() && !set.windows.is_empty(),
                "{}",
                set.id
            );
            assert!(!set.walls.is_empty(), "{}", set.id);
            assert!(SizeSetId::new(set.id).is_some(), "{}", set.id);
            for r in set.doors.iter().chain(&set.windows) {
                assert!(hosted::family(r.family).is_some(), "{}", r.label());
                assert!(
                    r.opening.0 >= r.leaf.0 && r.opening.1 >= r.leaf.1,
                    "{}: the opening is smaller than its leaf",
                    r.label()
                );
                assert!(r.leaf.0 > 0.0 && r.leaf.1 > 0.0, "{}", r.label());
            }
            let mut names: Vec<String> = set
                .doors
                .iter()
                .chain(&set.windows)
                .map(SizeRow::label)
                .collect();
            let n = names.len();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), n, "{}: two types share a name", set.id);
        }
    }

    #[test]
    fn us_doors_are_named_by_leaf_with_a_rough_opening() {
        let us = get("us").expect("the US set");
        let d = us
            .rows_of("door.single")
            .find(|r| r.name == "3′0″ × 6′8″")
            .expect("a 3′0″ × 6′8″ door");
        assert_eq!(d.leaf, (36.0 * INCH, 80.0 * INCH));
        assert_eq!(d.opening, (38.0 * INCH, 82.5 * INCH));
        // Leaves 2′0″ to 3′0″ in 2″ steps, then 3′6″ and 4′0″; 6′8″, 7′0″ and 8′0″ high.
        let widths: Vec<f64> = us
            .rows_of("door.single")
            .filter(|r| r.leaf.1 == 80.0 * INCH)
            .map(|r| (r.leaf.0 / INCH).round())
            .collect();
        assert_eq!(
            widths,
            [24.0, 26.0, 28.0, 30.0, 32.0, 34.0, 36.0, 42.0, 48.0]
        );
        assert!(us.rows_of("window.double-hung").any(|r| r.name == "3050"));
        assert_eq!(feet_inches(30.5), "2′6½″");
    }

    #[test]
    fn a_drawing_gets_its_set_by_its_units_unless_it_names_one() {
        let mut doc = Document::new();
        doc.set_units(drift_core::Units::architectural());
        let imperial = for_doc(&doc).id;
        assert_eq!(imperial, "us");
        doc.set_units(drift_core::Units::metric());
        let metric = for_doc(&doc).id;
        assert_ne!(imperial, metric);
        assert!(for_doc(&doc).metric);
        let mut rules = doc.rules();
        rules.size_set = id_for(Some("us"));
        doc.set_rules(rules);
        assert_eq!(for_doc(&doc).id, "us");
        // A set this build doesn't have: by the units.
        rules.size_set = id_for(Some("din-18101"));
        doc.set_rules(rules);
        assert_eq!(for_doc(&doc).id, metric);
    }
}
