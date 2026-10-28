//! Copy, cut and paste. The selection is copied as a small Drift document (text, so it
//! travels on the system clipboard between windows and sessions): the curves with their
//! points, the relations and dimensions among them, fills of copied areas, block copies
//! (with their blocks), sketch text, and groups copied whole (as new groups). Layers and
//! blocks are matched by name where the copy lands, and made there if missing.

use std::collections::{BTreeMap, BTreeSet};

use drift_core::{
    AnnotationId, AnnotationRef, BlockId, Bounds, Constraint, ConstraintId, DVec2, Document,
    EntityId, EntityKind, FillId, Group, GroupId, InstanceId, LayerId, TextId,
};

use crate::editor::Editor;

/// What to copy out of a document.
#[derive(Default)]
pub struct Picked {
    pub curves: Vec<EntityId>,
    pub instances: Vec<InstanceId>,
    pub texts: Vec<TextId>,
    /// Fills to copy even if their boundary isn't (they still need it copied to follow).
    pub fills: BTreeSet<FillId>,
    /// Notes, marks and callouts (a callout only with its geometry; a note's leader to
    /// geometry left behind keeps its tip where it is).
    pub annotations: Vec<AnnotationId>,
}

/// What a copy made.
#[derive(Default)]
pub struct Copied {
    /// Source entity → its copy.
    pub entities: BTreeMap<EntityId, EntityId>,
    pub instances: Vec<InstanceId>,
    pub texts: Vec<TextId>,
    /// Copies of the fills named in [`Picked::fills`].
    pub fills: BTreeSet<FillId>,
    pub annotations: Vec<AnnotationId>,
}

/// The layer in `dst` named like `layer` of `src` (made, with the same look, if missing).
fn layer_in(
    src: &Document,
    dst: &mut Document,
    layer: LayerId,
    map: &mut BTreeMap<LayerId, LayerId>,
) -> LayerId {
    if let Some(l) = map.get(&layer) {
        return *l;
    }
    let found = src.layer(layer).map(|source| {
        let existing = dst
            .layers()
            .find(|(_, l)| l.name == source.name)
            .map(|(id, _)| id);
        existing.unwrap_or_else(|| dst.add_layer(source.clone()))
    });
    let to = found.unwrap_or(LayerId::DEFAULT);
    map.insert(layer, to);
    to
}

/// `name`, or `name 2`, `name 3`… (counting on from a number it ends with) if one of `taken`
/// has it.
fn numbered_name<'a>(name: &str, taken: impl Iterator<Item = &'a str>) -> String {
    let taken: BTreeSet<&str> = taken.collect();
    let name = name.trim();
    if !taken.contains(name) {
        return name.to_owned();
    }
    let (stem, from) = match name.rsplit_once(' ') {
        Some((stem, n)) if !stem.is_empty() => match n.parse::<u32>() {
            Ok(n) => (stem, n + 1),
            Err(_) => (name, 2),
        },
        _ => (name, 2),
    };
    (from..)
        .map(|n| format!("{stem} {n}"))
        .find(|n| !taken.contains(n.as_str()))
        .expect("a free name")
}

/// New groups in `dst` for those of `groups` whose members were all copied (`copy` gives
/// each one's copy), named like the original with the next free number ("Panel" → "Panel 2").
/// With `same` (copies in the same document), a member that is its own copy, such as a line
/// on a mirror's axis, stays only in the original's group. Returns the new groups.
pub fn copy_groups(
    groups: impl IntoIterator<Item = Group>,
    dst: &mut Document,
    copy: impl Fn(EntityId) -> Option<EntityId>,
    same: bool,
) -> Vec<GroupId> {
    let mut made = Vec::new();
    for g in groups {
        let Some(copies) = g
            .members
            .iter()
            .map(|&e| copy(e).map(|c| (e, c)))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let members: BTreeSet<EntityId> = copies
            .into_iter()
            .filter(|(e, c)| !same || e != c)
            .map(|(_, c)| c)
            .collect();
        if members.is_empty() {
            continue;
        }
        let name = numbered_name(&g.name, dst.groups().map(|(_, g)| g.name.as_str()));
        if let Ok(id) = dst.add_group(Group { name, members }) {
            made.push(id);
        }
    }
    made
}

/// Copy `picked` from `src` into `dst`, moved by `offset`. Call inside a transaction on
/// `dst`. `same` says the two are the same document (layers and blocks keep their ids).
pub fn copy_into(
    src: &Document,
    dst: &mut Document,
    picked: &Picked,
    offset: DVec2,
    same: bool,
) -> Copied {
    let mut out = Copied::default();
    let mut layers: BTreeMap<LayerId, LayerId> = BTreeMap::new();
    let mut layer = |dst: &mut Document, l: LayerId| {
        if same {
            l
        } else {
            layer_in(src, dst, l, &mut layers)
        }
    };
    // Everything copied: the picked entities and the points they're defined by.
    let mut sources: BTreeSet<EntityId> = BTreeSet::new();
    // (Walls' faces aren't copied: the walls are, and make them again.)
    let generated = src.generated_entities();
    let wall_owners = src.wall_owners();
    for &id in picked.curves.iter().filter(|id| !generated.contains(id)) {
        if let Some(e) = src.get(id) {
            sources.insert(id);
            sources.extend(e.kind.points());
        }
    }
    // Points first, then the curves that use them.
    let (points, others): (Vec<EntityId>, Vec<EntityId>) = sources
        .iter()
        .partition(|id| src.get(**id).is_some_and(|e| e.kind.is_point()));
    for id in points.into_iter().chain(others) {
        let Some(e) = src.get(id).cloned() else {
            continue;
        };
        let map = &out.entities;
        let at = |id: &EntityId| map.get(id).copied();
        let made = (|| match e.kind {
            EntityKind::Point(p) => Some(Ok(dst.add_point(p + offset))),
            EntityKind::Line { a, b } => Some(dst.add_line(at(&a)?, at(&b)?)),
            EntityKind::Circle { center, radius } => Some(dst.add_circle(at(&center)?, radius)),
            EntityKind::Arc { center, start, end } => {
                Some(dst.add_arc(at(&center)?, at(&start)?, at(&end)?))
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => Some(dst.add_ellipse(at(&center)?, at(&major)?, at(&minor)?)),
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => Some(dst.add_ellipse_arc(
                at(&center)?,
                at(&major)?,
                at(&minor)?,
                at(&start)?,
                at(&end)?,
            )),
            EntityKind::Bezier { a, c1, c2, b } => {
                Some(dst.add_bezier(at(&a)?, at(&c1)?, at(&c2)?, at(&b)?))
            }
        })();
        if let Some(Ok(copy)) = made {
            let to = layer(dst, e.layer);
            let _ = dst.set_entity_layer(copy, to);
            dst.set_style(copy, e.style);
            let _ = dst.set_appearance(copy, e.appearance);
            out.entities.insert(id, copy);
        }
    }
    // Groups copied whole are groups of the copies.
    copy_groups(
        src.groups().map(|(_, g)| g.clone()),
        dst,
        |e| out.entities.get(&e).copied(),
        same,
    );
    // Relations and dimensions entirely within the copied geometry come along, except
    // fixed positions (the copy would be pinned where the original is).
    let map = &out.entities;
    let constraints: Vec<(ConstraintId, Constraint)> = src
        .constraints()
        .filter(|(_, c)| !c.is_point_fix())
        .filter(|(_, c)| c.entities().all(|e| map.contains_key(&e)))
        .map(|(id, c)| (id, *c))
        .collect();
    for (id, c) in constraints {
        let Ok(copy) = dst.add_constraint(c.map_entities(|e| map[&e])) else {
            continue;
        };
        if let Some(offset) = src.placement(id) {
            let _ = dst.set_placement(copy, offset);
        }
        for (end, start) in src.extension_starts(id).into_iter().enumerate() {
            let _ = dst.set_extension_start(copy, end, start);
        }
        if let Some(note) = src.dim_note(id) {
            dst.set_dim_note(copy, Some(note.clone()));
        }
        dst.set_shown(
            drift_core::AnnotationRef::Dimension(copy),
            src.shown(drift_core::AnnotationRef::Dimension(id)),
        );
        dst.set_style_override(
            drift_core::AnnotationRef::Dimension(copy),
            src.style_override(drift_core::AnnotationRef::Dimension(id)),
        );
        if src.is_driven(id) {
            let _ = dst.set_driven(copy, true);
        }
    }
    // Fills and hatches whose whole boundary was copied follow onto the copy.
    let fills: Vec<(FillId, drift_core::Fill)> = src
        .fills()
        .filter(|(id, _)| !wall_owners.fills.contains_key(id))
        .filter(|(_, f)| !f.boundary.is_empty() && f.boundary.iter().all(|e| map.contains_key(e)))
        .map(|(id, f)| (id, f.clone()))
        .collect();
    // Shapes (paths) whose curves were all copied are shapes of the copies too, so their
    // fills stay theirs.
    let mut paths = BTreeMap::new();
    for (id, p) in src.paths() {
        if !wall_owners.paths.contains_key(&id) && p.curves.iter().all(|c| map.contains_key(c)) {
            let copy = drift_core::Path {
                curves: p.curves.iter().map(|c| map[c]).collect(),
                closed: p.closed,
            };
            if let Ok(new) = dst.add_path(copy) {
                paths.insert(id, new);
            }
        }
    }
    for (id, mut fill) in fills {
        fill.path = fill.path.and_then(|p| paths.get(&p).copied());
        fill.boundary = fill.boundary.iter().map(|e| map[e]).collect();
        fill.boundary.sort_unstable();
        fill.seed += offset;
        fill.layer = layer(dst, fill.layer);
        // On the original's layer and in its place in the draw order.
        let copy = dst.add_fill(fill.clone());
        dst.set_fill(copy, fill);
        if picked.fills.contains(&id) {
            out.fills.insert(copy);
        }
    }
    // Walls whose centrelines were copied: copies along the copies (they make their faces
    // when the copy is committed). Tees on walls copied too point at their copies.
    let mut walls: BTreeMap<drift_core::WallId, drift_core::WallId> = BTreeMap::new();
    for (id, w) in src.walls() {
        let Some(&centre) = paths.get(&w.centre) else {
            continue;
        };
        let mut copy = w.clone();
        copy.centre = centre;
        copy.made = Default::default();
        copy.layer = layer(dst, w.layer);
        for e in &mut copy.ends {
            e.against = e.against.and_then(|c| map.get(&c).copied());
        }
        if let Ok(new) = dst.add_wall(copy) {
            walls.insert(id, new);
        }
    }
    for &new in walls.values() {
        if let Some(mut w) = dst.wall(new).cloned() {
            for e in &mut w.ends {
                e.tee = e.tee.and_then(|(h, s)| Some((*walls.get(&h)?, s)));
            }
            let _ = dst.set_wall(new, w);
        }
    }
    // Their openings: copies in the copies (they make their symbols when committed).
    let openings: Vec<drift_core::Opening> = src
        .openings()
        .filter(|(_, o)| walls.contains_key(&o.wall))
        .map(|(_, o)| o.clone())
        .collect();
    for mut o in openings {
        o.wall = walls[&o.wall];
        o.made = Default::default();
        o.layer = o.layer.map(|l| layer(dst, l));
        let _ = dst.add_opening(o);
    }
    // Fills bounded by their faces (a room's floor): onto the copies' faces, made now.
    let floors = crate::walls::face_fills(src, &walls, &|e| map.get(&e).copied(), &|p| p + offset);
    if !floors.is_empty() {
        drift_geom::walls::regenerate(dst);
        let mut placed = Vec::with_capacity(floors.len());
        for mut f in floors {
            f.fill.layer = layer(dst, f.fill.layer);
            placed.push(f);
        }
        for (from, copy) in crate::walls::add_face_fills(dst, placed) {
            if picked.fills.contains(&from) {
                out.fills.insert(copy);
            }
        }
    }
    // Block copies, bringing their blocks along (matched by name) into another document.
    let mut blocks: BTreeMap<BlockId, BlockId> = BTreeMap::new();
    for &id in &picked.instances {
        let Some(mut i) = src.instance(id).copied() else {
            continue;
        };
        if !same {
            let block = match blocks.get(&i.block) {
                Some(b) => Some(*b),
                None => src.block(i.block).map(|source| {
                    let existing = dst
                        .blocks()
                        .find(|(_, b)| b.name == source.name)
                        .map(|(id, _)| id);
                    let to = existing.unwrap_or_else(|| dst.add_block(source.clone()));
                    blocks.insert(i.block, to);
                    to
                }),
            };
            let Some(block) = block else { continue };
            i.block = block;
        }
        i.at += offset;
        i.layer = layer(dst, i.layer);
        if let Ok(copy) = dst.add_instance(i) {
            let _ = dst.set_instance(copy, i);
            out.instances.push(copy);
        }
    }
    for &id in &picked.texts {
        let Some(mut t) = src.text(id).cloned() else {
            continue;
        };
        t.at += offset;
        t.layer = layer(dst, t.layer);
        let copy = dst.add_text(t.clone());
        dst.set_text(copy, t);
        out.texts.push(copy);
    }
    for &id in &picked.annotations {
        let Some(a) = src.annotation(id) else {
            continue;
        };
        let Some(on) = a.with_entities(&|e| map.get(&e).copied(), &|anchor| src.anchor_pos(anchor))
        else {
            continue;
        };
        let mut copy = on.mapped(&|p| p + offset);
        // A copy of a section or elevation mark is a mark of its own, not the view's.
        copy.set_view(None);
        let Ok(made) = dst.add_annotation(copy) else {
            continue;
        };
        let (from, to) = (
            AnnotationRef::Annotation(id),
            AnnotationRef::Annotation(made),
        );
        dst.set_shown(to, src.shown(from));
        dst.set_style_override(to, src.style_override(from));
        out.annotations.push(made);
    }
    out
}

/// Everything in a document, as [`Picked`] (for pasting a copied fragment whole).
fn everything(doc: &Document) -> Picked {
    let referenced = doc.referenced_points();
    Picked {
        curves: doc
            .iter()
            .filter(|(id, _)| !referenced.contains(id))
            .map(|(id, _)| id)
            .collect(),
        instances: doc.instances().map(|(id, _)| id).collect(),
        texts: doc.texts().map(|(id, _)| id).collect(),
        fills: doc.fills().map(|(id, _)| id).collect(),
        annotations: doc.annotations().map(|(id, _)| id).collect(),
    }
}

/// The extent of what a fragment holds (for placing it under the pointer).
fn extent(doc: &Document) -> Bounds {
    let mut b: Bounds = doc
        .iter()
        .filter_map(|(id, _)| doc.shape(id))
        .map(|s| s.bounds())
        .collect();
    for (_, i) in doc.instances() {
        b.include(i.at);
    }
    for (_, t) in doc.texts() {
        b.include(t.at);
    }
    for (_, a) in doc.annotations() {
        if let Some(p) = crate::notes::label_world(doc, a) {
            b.include(p);
        }
    }
    b
}

/// Whether clipboard text is a copied piece of a sketch.
pub fn is_fragment(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('{') && t.contains("\"format\": \"drift\"")
}

impl Editor {
    /// What is selected, for copying.
    fn picked(&self) -> Picked {
        Picked {
            curves: self.selection.iter().copied().collect(),
            instances: self.selected_instances.iter().copied().collect(),
            texts: self.selected_texts.iter().copied().collect(),
            fills: self.selected_fills.iter().copied().collect(),
            annotations: self.picked_notes(),
        }
    }

    /// The selection as a small document of its own (`None` with nothing to copy).
    pub fn copy_fragment(&self) -> Option<Document> {
        let picked = self.picked();
        if picked.curves.is_empty()
            && picked.instances.is_empty()
            && picked.texts.is_empty()
            && picked.annotations.is_empty()
        {
            return None;
        }
        let mut fragment = Document::new();
        fragment.set_units(self.doc.units());
        fragment.begin();
        copy_into(&self.doc, &mut fragment, &picked, DVec2::ZERO, false);
        fragment.commit("Copy");
        Some(fragment)
    }

    /// The selection as clipboard text (`None` with nothing to copy).
    #[cfg(not(feature = "demo"))]
    pub fn copy_selection(&self) -> Option<String> {
        let fragment = self.copy_fragment()?;
        Some(drift_io::save(&fragment, &drift_io::Metadata::default()))
    }

    /// Copy the selection and delete it.
    pub fn cut_fragment(&mut self) -> Option<Document> {
        let fragment = self.copy_fragment()?;
        self.delete_selection();
        Some(fragment)
    }

    /// Copy the selection as clipboard text and delete it.
    #[cfg(all(test, not(feature = "demo")))]
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.copy_selection()?;
        self.delete_selection();
        Some(text)
    }

    /// Paste copied geometry: centred on `at`, or where it was copied from (`None`). The
    /// pasted things become the selection. Returns how many were pasted.
    pub fn paste(&mut self, text: &str, at: Option<DVec2>) -> Result<usize, String> {
        let (fragment, _) = drift_io::load(text).map_err(|e| e.to_string())?;
        self.paste_fragment(&fragment, at)
    }

    /// Paste a copied piece of a sketch (see [`Editor::copy_fragment`]), as
    /// [`Editor::paste`] does.
    pub fn paste_fragment(
        &mut self,
        fragment: &Document,
        at: Option<DVec2>,
    ) -> Result<usize, String> {
        let picked = everything(fragment);
        let offset = match at {
            Some(at) => {
                let b = extent(fragment);
                if b.is_empty() {
                    DVec2::ZERO
                } else {
                    at - (b.min + b.max) * 0.5
                }
            }
            None => DVec2::ZERO,
        };
        self.doc.begin();
        let made = copy_into(fragment, &mut self.doc, &picked, offset, false);
        if !self.commit("Paste") {
            return Err("the pasted geometry could not be solved".into());
        }
        self.selection = picked
            .curves
            .iter()
            .filter_map(|id| made.entities.get(id).copied())
            .collect();
        self.selected_instances = made.instances.iter().copied().collect();
        self.selected_fills = made.fills;
        self.selected_texts = made.texts.iter().copied().collect();
        self.selected_region = None;
        self.selected_annotation = None;
        self.annotation_picks = made
            .annotations
            .iter()
            .map(|id| drift_core::AnnotationRef::Annotation(*id))
            .collect();
        Ok(self.selection.len()
            + self.selected_instances.len()
            + made.texts.len()
            + made.annotations.len())
    }
}

#[cfg(test)]
mod tests {
    use drift_core::{LineKind, SketchText};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A 20 × 10 rectangle with relations and a width dimension on a "Walls" layer, plus a
    /// line of text.
    fn sketch() -> Editor {
        let mut ed = Editor::default();
        let walls = ed.add_layer("Walls");
        ed.set_current_layer(walls);
        ed.doc.begin();
        let c =
            [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)].map(|p| ed.doc.add_point(p));
        let l: Vec<EntityId> = (0..4)
            .map(|n| ed.doc.add_line(c[n], c[(n + 1) % 4]).unwrap())
            .collect();
        ed.doc.set_style(l[3], LineKind::Construction);
        for (n, line) in l.iter().enumerate() {
            let rel = if n % 2 == 0 {
                Constraint::Horizontal { line: *line }
            } else {
                Constraint::Vertical { line: *line }
            };
            ed.doc.add_constraint(rel).unwrap();
        }
        ed.doc
            .add_constraint(Constraint::Distance {
                a: c[0],
                b: c[1],
                value: 20.0,
            })
            .unwrap();
        ed.commit("Rectangle");
        ed.add_text(SketchText::new("Hall", v(2.0, 2.0), 3.0));
        ed.selected_texts.clear();
        ed
    }

    #[test]
    fn copy_and_paste_within_a_sketch() {
        let mut ed = sketch();
        // (Select all takes the text too.)
        ed.select_all();
        let lines = ed.selection.len() + ed.selected_texts.len();
        let (entities, relations) = (ed.doc.len(), ed.doc.constraint_count());
        let text = ed.copy_selection().expect("something to copy");
        assert!(is_fragment(&text));
        // Pasted centred on a point: the copy's middle lands there.
        let n = ed.paste(&text, Some(v(110.0, 55.0))).unwrap();
        assert_eq!(n, lines);
        assert_eq!(ed.doc.len(), 2 * entities);
        assert_eq!(ed.doc.constraint_count(), 2 * relations);
        let b: Bounds = ed
            .selection
            .iter()
            .filter_map(|id| ed.doc.shape(*id))
            .map(|s| s.bounds())
            .collect();
        assert!(((b.min + b.max) * 0.5 - v(110.0, 55.0)).length() < 1e-9);
        // The copies keep their layer and style, and the layer isn't duplicated.
        assert_eq!(
            ed.doc.layers().filter(|(_, l)| l.name == "Walls").count(),
            1
        );
        let styles = ed
            .selection
            .iter()
            .filter(|id| ed.doc.get(**id).unwrap().style == LineKind::Construction)
            .count();
        assert_eq!(styles, 1);
        // One undo step.
        ed.undo();
        assert_eq!(ed.doc.len(), entities);
    }

    #[test]
    fn paste_into_another_sketch_brings_layers_and_text() {
        let mut from = sketch();
        from.select_all();
        from.selected_texts = from.doc.texts().map(|(id, _)| id).collect();
        let text = from.copy_selection().unwrap();
        let mut to = Editor::default();
        let n = to.paste(&text, None).unwrap();
        assert_eq!(n, 5, "four lines and the text");
        assert!(to.doc.layers().any(|(_, l)| l.name == "Walls"));
        assert_eq!(to.doc.texts().count(), 1);
        assert_eq!(to.doc.constraint_count(), 5);
        // In place: the same coordinates as the original.
        assert_eq!(to.bounds().min, v(0.0, 0.0));
    }

    #[test]
    fn cut_removes_and_plain_text_is_not_a_fragment() {
        let mut ed = sketch();
        ed.select_all();
        let text = ed.cut_selection().unwrap();
        assert!(ed.doc.iter().next().is_none());
        assert!(ed.paste(&text, None).is_ok());
        assert!(!is_fragment("hello"));
        assert!(ed.paste("hello", None).is_err());
        assert!(Editor::default().copy_selection().is_none());
    }

    #[test]
    fn groups_copied_whole_are_new_groups_and_a_part_of_one_is_loose() {
        assert_eq!(numbered_name("Panel", ["Panel"].into_iter()), "Panel 2");
        assert_eq!(
            numbered_name("Panel 2", ["Panel", "Panel 2"].into_iter()),
            "Panel 3"
        );
        assert_eq!(numbered_name("Desk", ["Panel"].into_iter()), "Desk");
        let mut ed = sketch();
        let lines: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        ed.selection = lines[..2].iter().copied().collect();
        let a = ed.make_group().unwrap();
        ed.selection = lines[2..].iter().copied().collect();
        let b = ed.make_group().unwrap();
        // Both groups, copied and pasted: two new groups of the copies.
        ed.selection = lines.iter().copied().collect();
        let text = ed.copy_selection().unwrap();
        ed.paste(&text, Some(v(100.0, 0.0))).unwrap();
        let names: Vec<String> = ed.doc.groups().map(|(_, g)| g.name.clone()).collect();
        assert_eq!(names, ["Group 1", "Group 2", "Group 3", "Group 4"]);
        for (id, g) in ed.doc.groups() {
            assert_eq!(g.members.len(), 2);
            if id != a && id != b {
                assert!(g.members.iter().all(|m| ed.selection.contains(m)));
            }
        }
        assert_eq!(
            ed.doc.group(a).unwrap().members,
            lines[..2].iter().copied().collect()
        );
        // One undo step takes the copies and their groups.
        ed.undo();
        assert_eq!(ed.doc.groups().count(), 2);
        // Part of a group: loose copies.
        ed.selection = [lines[0]].into();
        ed.duplicate_selection();
        assert_eq!(ed.doc.groups().count(), 2);
        assert_eq!(ed.doc.group_of(*ed.selection.first().unwrap()), None);
    }
}
