//! Walls from lines (W1.5): the editor command that finds the walls in a plan drawn as pairs
//! of face lines (an imported DWG or DXF plan, or walls an older script drew as lines) and
//! makes them walls, as one undo step. The finding is `drift_geom::walls::from_lines`.

use std::collections::{BTreeMap, HashMap};

use drift_core::{Document, EntityId, EntityKind, LayerId, LineKind, Path, Wall, WallId};
use drift_geom::walls::Seg;
use drift_geom::walls::from_lines::{LineIn, Settings, Why, find};

use crate::editor::Editor;
/// What Walls from lines does with the lines it made into walls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UsedLines {
    /// Delete them (the walls' faces lie where they were; Undo brings them back).
    #[default]
    Remove,
    /// Leave them where they are, over the walls.
    Keep,
    /// Move them to a hidden layer, "Wall lines".
    Layer,
}

impl UsedLines {
    pub const ALL: [UsedLines; 3] = [UsedLines::Remove, UsedLines::Keep, UsedLines::Layer];

    pub fn label(self) -> &'static str {
        match self {
            UsedLines::Remove => "Removed",
            UsedLines::Keep => "Kept",
            UsedLines::Layer => "Moved to a hidden layer",
        }
    }

    /// As kept in the preferences.
    pub fn key(self) -> &'static str {
        match self {
            UsedLines::Remove => "r",
            UsedLines::Keep => "k",
            UsedLines::Layer => "l",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.key() == key)
    }
}

/// The layer Walls from lines moves the lines it used to, when asked to.
pub const USED_LINES_LAYER: &str = "Wall lines";

/// What Walls from lines made and skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct FromLinesReport {
    pub walls: Vec<WallId>,
    /// How many lines went into them.
    pub used: usize,
    /// Lines skipped: why, how many pairs or groups, and the lines.
    pub skipped: Vec<(Why, usize, Vec<EntityId>)>,
    /// Lines the walls cover only in part (kept).
    pub partly: Vec<EntityId>,
    /// Gaps left open between walls in line (doors and windows).
    pub openings: usize,
    /// What was picked, to run it again.
    pub input: Vec<EntityId>,
    /// The drawing's revision just after: the report is current until it changes.
    pub revision: u64,
    /// What was done with the lines used.
    pub lines: UsedLines,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl FromLinesReport {
    /// "Made 24 walls from 61 lines; 3 line pairs skipped: spacing varies".
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Made {} from {}",
            plural(self.walls.len(), "wall", "walls"),
            plural(self.used, "line", "lines")
        );
        if self.openings > 0 {
            text += &format!(
                "; {} left open",
                plural(self.openings, "opening", "openings")
            );
        }
        for (why, count, _) in &self.skipped {
            text += &format!("; {} skipped: {}", Self::what(*why, *count), why.text());
        }
        text
    }

    /// "3 line pairs", "1 group of lines".
    pub fn what(why: Why, count: usize) -> String {
        match why {
            Why::SpacingVaries => plural(count, "line pair", "line pairs"),
            Why::Pattern => plural(count, "group of lines", "groups of lines"),
            Why::Object => plural(count, "short piece", "short pieces"),
        }
    }
}

impl Editor {
    /// Walls found among `picked`: parallel lines a wall's thickness apart become walls at
    /// the measured thickness (snapped to a preset when close), welded where they meet, with
    /// the Wall tool's poché, as one undo step. What happens to the lines used is the
    /// `wall_lines` setting. The report (also kept in `from_lines`) says what was made and
    /// skipped.
    pub fn walls_from_lines(&mut self, picked: &[EntityId]) -> Result<FromLinesReport, String> {
        let owners = self.doc.has_walls().then(|| self.wall_owners());
        let locked = self.doc.locked_entities();
        let mut input = Vec::new();
        let mut lines = Vec::new();
        for &id in picked {
            if owners.as_ref().is_some_and(|o| o.get(id).is_some()) || locked.contains(&id) {
                continue;
            }
            let Some(e) = self.doc.get(id) else {
                continue;
            };
            let at = |p: EntityId| self.doc.point(p).unwrap_or_default();
            let seg = match e.kind {
                EntityKind::Line { a, b } => Seg::Line { a: at(a), b: at(b) },
                EntityKind::Arc { center, start, end } => {
                    let (c, a) = (at(center), at(start));
                    Seg::Arc {
                        center: c,
                        radius: a.distance(c),
                        a,
                        b: at(end),
                        ccw: true,
                    }
                }
                _ => continue,
            };
            input.push(id);
            lines.push(LineIn {
                id,
                group: e.layer.0,
                seg,
                guide: e.style == LineKind::Construction,
            });
        }
        if lines.iter().all(|l| l.guide) {
            return Err("Pick the plan's wall lines first (drag a box round them)".into());
        }
        let metric = self.doc.units().length.is_metric();
        // The drawing's presets (the Wall tool's list).
        let presets = self.wall_presets().iter().map(|p| p.mm).collect();
        let settings = Settings::new(
            drift_geom::walls::regen::snap(&self.doc),
            presets,
            if metric {
                1.0
            } else {
                drift_core::units::MM_PER_INCH / 16.0
            },
        );
        let found = find(&lines, &settings);
        if found.walls.is_empty() {
            return Err(match found.skipped.first() {
                Some(s) => format!("No walls found: {}", s.why.text()),
                None => {
                    "No walls found: pick pairs of parallel lines a wall's thickness apart".into()
                }
            });
        }
        self.doc.begin();
        let mut points: HashMap<usize, EntityId> = HashMap::new();
        let mut made = Vec::new();
        for fw in &found.walls {
            let layer = LayerId(fw.group);
            let mut new: Vec<EntityId> = Vec::new();
            let mut point = |doc: &mut Document, n: usize, new: &mut Vec<EntityId>| {
                *points.entry(n).or_insert_with(|| {
                    let id = doc.add_point(found.points[n]);
                    new.push(id);
                    id
                })
            };
            let mut curves = Vec::new();
            for &(seg, a, b) in &fw.segs {
                let pa = point(&mut self.doc, a, &mut new);
                let pb = point(&mut self.doc, b, &mut new);
                let curve = match seg {
                    Seg::Line { .. } => self.doc.add_line(pa, pb),
                    Seg::Arc { center, ccw, .. } => {
                        let c = self.doc.add_point(center);
                        new.push(c);
                        if ccw {
                            self.doc.add_arc(c, pa, pb)
                        } else {
                            self.doc.add_arc(c, pb, pa)
                        }
                    }
                };
                let Ok(curve) = curve else {
                    continue;
                };
                self.doc.set_style(curve, LineKind::Construction);
                new.push(curve);
                curves.push(curve);
            }
            for id in new {
                let _ = self.doc.set_entity_layer(id, layer);
            }
            if curves.len() != fw.segs.len() {
                continue;
            }
            let Ok(path) = self.doc.add_path(Path {
                curves,
                closed: fw.closed,
            }) else {
                continue;
            };
            let mut w = Wall::new(path, fw.thickness, layer);
            w.poche = self.wall_poche_now();
            if let Ok(id) = self.doc.add_wall(w) {
                made.push(id);
            }
        }
        if made.is_empty() {
            self.doc.rollback();
            self.sync();
            return Err("No walls could be made from those lines".into());
        }
        let used: Vec<EntityId> = found.used().into_iter().collect();
        self.put_away_used_lines(&used);
        self.commit("Walls from lines");
        self.selection = made.iter().flat_map(|w| self.wall_curves(*w)).collect();
        let mut skipped: BTreeMap<Why, (usize, Vec<EntityId>)> = BTreeMap::new();
        for s in &found.skipped {
            let e = skipped.entry(s.why).or_default();
            e.0 += 1;
            e.1.extend(&s.lines);
        }
        let report = FromLinesReport {
            walls: made,
            used: used.len(),
            skipped: skipped
                .into_iter()
                .map(|(why, (n, lines))| (why, n, lines))
                .collect(),
            partly: found.partly.clone(),
            openings: found.openings.len(),
            input,
            revision: self.doc.revision(),
            lines: self.wall_lines,
        };
        self.from_lines = Some(report.clone());
        Ok(report)
    }

    /// Remove, keep or move the lines made into walls, as `wall_lines` says.
    fn put_away_used_lines(&mut self, used: &[EntityId]) {
        match self.wall_lines {
            UsedLines::Remove => self.doc.delete(used.iter().copied()),
            UsedLines::Keep => {}
            UsedLines::Layer => {
                let named = self
                    .doc
                    .layers()
                    .find(|(_, l)| l.name == USED_LINES_LAYER)
                    .map(|(id, _)| id);
                let layer = match named {
                    Some(id) => id,
                    None => {
                        let mut l = drift_core::Layer::new(USED_LINES_LAYER);
                        l.visible = false;
                        self.doc.add_layer(l)
                    }
                };
                let set: std::collections::HashSet<EntityId> = used.iter().copied().collect();
                for &id in used {
                    let ends: Vec<EntityId> = self
                        .doc
                        .get(id)
                        .map(|e| e.kind.points().collect())
                        .unwrap_or_default();
                    let _ = self.doc.set_entity_layer(id, layer);
                    for p in ends {
                        // (A point another line still uses stays where it is.)
                        if self.doc.users_of(p).all(|u| set.contains(&u)) {
                            let _ = self.doc.set_entity_layer(p, layer);
                        }
                    }
                }
            }
        }
    }

    /// The last Walls from lines report, while the drawing is as it left it.
    pub fn from_lines_now(&self) -> Option<&FromLinesReport> {
        self.from_lines
            .as_ref()
            .filter(|r| r.revision == self.doc.revision())
    }

    /// Do the last Walls from lines again with the lines used put away another way (one
    /// undo step still: the first is undone).
    pub fn redo_walls_from_lines(&mut self, lines: UsedLines) -> Result<FromLinesReport, String> {
        self.wall_lines = lines;
        let Some(report) = self.from_lines_now().cloned() else {
            return Err("The drawing has changed since".into());
        };
        if report.lines == lines {
            return Ok(report);
        }
        self.undo();
        self.walls_from_lines(&report.input)
    }
}

#[cfg(test)]
mod tests;
