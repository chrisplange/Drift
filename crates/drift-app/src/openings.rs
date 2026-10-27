//! Openings in the editor: placing doors, windows and cased openings in walls, keeping their
//! symbols generated as their walls change ([`Editor::follow_openings`], run with the walls'
//! regeneration), picking, sliding along the wall, flipping, sizing, and what happens to
//! them when their wall is split, joined, exploded or deleted.
//!
//! The cut is the walls' generator's (`drift_geom::walls::openings`); the record is
//! `drift_core::Opening`; the symbols are the library's hosted families
//! (`library::hosted`), in the sizes of the drawing's size set (`library::sizes`). See
//! `docs/walls-design.md` §6.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use drift_core::{
    Appearance, Constraint, DVec2, EntityId, Hand, Kind, LayerId, Opening, OpeningAt, OpeningId,
    Part, SegEnd, Shape, Station, Symbol, WallFace, WallId,
};
use drift_geom::walls::{Frame, Seg};

use crate::editor::{EditError, Editor};
use crate::library::{hosted, sizes};

/// How far an opening keeps from the corners either side (mm): a jamb on a corner would make
/// a sliver of face.
pub const CLEARANCE: f64 = 1.0;

/// The Opening tool bar's settings, kept for the next drawing too.
#[derive(Clone, Debug, PartialEq)]
pub struct OpeningSettings {
    /// The family placed (`door.single`).
    pub family: String,
    /// The type picked from the size set (its name), if one was.
    pub type_name: String,
    /// The cut, the leaf and the height (mm): 0 until set (then the set's usual size).
    pub width: f64,
    pub leaf: f64,
    pub height: f64,
    pub hand: Hand,
    pub facing: WallFace,
}

impl Default for OpeningSettings {
    fn default() -> Self {
        Self {
            family: "door.single".into(),
            type_name: String::new(),
            width: 0.0,
            leaf: 0.0,
            height: 0.0,
            hand: Hand::Left,
            facing: WallFace::Right,
        }
    }
}

/// Which of an opening's two flips a canvas arrow does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flip {
    /// The hinge to the other jamb.
    Hand,
    /// The swing to the other face.
    Facing,
}

/// A segment of a wall and where its faces lie, for an opening on it.
#[derive(Clone, Copy, Debug)]
pub struct Host {
    pub seg: Seg,
    pub offsets: (f64, f64),
    pub thickness: f64,
}

impl Editor {
    // ----- The tool bar's settings ------------------------------------------------------------

    /// The size set the drawing uses.
    pub fn size_set(&self) -> &'static sizes::SizeSet {
        sizes::for_doc(&self.doc)
    }

    /// The type placed now: the tool bar's, else the set's usual one for the family (a
    /// door's 3′0″ or 900 mm leaf; a window or opening's nearest to 3′0″ by 4′0″).
    pub fn opening_type_now(&self) -> sizes::SizeRow {
        let s = &self.opening;
        let set = self.size_set();
        if s.width > 0.0 {
            return sizes::SizeRow {
                family: hosted::family(&s.family).map_or("door.single", |f| f.id),
                name: s.type_name.clone(),
                leaf: (if s.leaf > 0.0 { s.leaf } else { s.width }, s.height),
                opening: (s.width, s.height),
            };
        }
        let usual = |r: &&sizes::SizeRow| {
            let (w, h) = r.leaf;
            ((w - 900.0).abs() + 0.1 * (h - 1200.0).abs()).round() as i64
        };
        set.rows_of(&s.family)
            .min_by_key(usual)
            .or_else(|| set.doors.first())
            .cloned()
            .unwrap_or(sizes::SizeRow {
                family: "door.single",
                name: String::new(),
                leaf: (900.0, 2040.0),
                opening: (970.0, 2075.0),
            })
    }

    /// Pick a type from the size set for the tool bar.
    pub fn set_opening_type(&mut self, row: &sizes::SizeRow) {
        self.opening.family = row.family.to_owned();
        self.opening.type_name = row.name.clone();
        self.opening.width = row.opening.0;
        self.opening.leaf = row.leaf.0;
        self.opening.height = row.opening.1;
    }

    /// Pick a family for the tool bar: its usual type.
    pub fn set_opening_family(&mut self, family: &str) {
        self.opening.family = family.to_owned();
        self.opening.width = 0.0;
        let row = self.opening_type_now();
        self.set_opening_type(&row);
    }

    // ----- Where openings are ---------------------------------------------------------------

    /// The segment an opening is on, and its wall's faces.
    pub fn opening_host(&self, w: WallId, seg: u16) -> Option<Host> {
        let wall = self.doc.wall(w)?;
        let (segs, ..) = drift_geom::walls::regen::centreline(&self.doc, wall).ok()?;
        Some(Host {
            seg: *segs.get(usize::from(seg))?,
            offsets: wall.offsets(),
            thickness: wall.thickness,
        })
    }

    /// Where an opening's jambs are along its segment.
    pub fn opening_span(&self, o: OpeningId) -> Option<(f64, f64)> {
        let op = self.doc.opening(o)?;
        let host = self.opening_host(op.wall, op.seg)?;
        Some(op.span_on(host.seg.length()))
    }

    /// An opening's symbol frame.
    pub fn opening_frame(&self, o: OpeningId) -> Option<Frame> {
        let op = self.doc.opening(o)?;
        let host = self.opening_host(op.wall, op.seg)?;
        Frame::of(
            &host.seg,
            host.offsets,
            op.span_on(host.seg.length()),
            op.hand,
            op.facing,
        )
    }

    /// An opening's symbol in the drawing: lines and arcs, with their look.
    pub fn opening_symbol(&self, op: &Opening, frame: &Frame) -> Vec<(Shape, Appearance)> {
        let Symbol::Library { family, .. } = &op.symbol else {
            return Vec::new();
        };
        hosted::symbol(family, frame.width, op.leaf, frame.thickness)
            .into_iter()
            .filter_map(|(s, look)| Some((frame.shape(&s)?, look)))
            .collect()
    }

    /// Where along segment `seg` of wall `w` an opening `width` wide can go near `u` (along
    /// the centreline): the stretch between corners it would sit in, kept `CLEARANCE` clear
    /// of them. None if none is long enough.
    pub fn opening_room(
        &self,
        w: WallId,
        seg: u16,
        u: f64,
        width: f64,
        but: Option<OpeningId>,
    ) -> Option<(f64, f64)> {
        let spans = drift_geom::walls::free_spans(&self.doc, w, seg, but);
        spans
            .into_iter()
            .map(|(a, b)| (a + CLEARANCE, b - CLEARANCE))
            .filter(|(a, b)| b - a >= width)
            .min_by(|x, y| {
                let d = |(a, b): (f64, f64)| {
                    if u < a {
                        a - u
                    } else if u > b {
                        u - b
                    } else {
                        0.0
                    }
                };
                d(*x).total_cmp(&d(*y))
            })
    }

    /// Where an opening `width` wide centred on `u` goes: its first jamb, clamped into the
    /// room there is.
    pub fn opening_start_near(
        &self,
        w: WallId,
        seg: u16,
        u: f64,
        width: f64,
        but: Option<OpeningId>,
    ) -> Option<f64> {
        let (a, b) = self.opening_room(w, seg, u, width, but)?;
        Some((u - width / 2.0).clamp(a, b - width))
    }

    // ----- Picking ------------------------------------------------------------------------

    /// What picking an opening selects: its symbol and its jambs.
    pub fn opening_pick_set(&self, o: OpeningId) -> BTreeSet<EntityId> {
        let owners = self.wall_owners();
        owners
            .of
            .iter()
            .filter(|(_, ow)| matches!(ow, drift_core::Owner::Opening(x, _) if *x == o))
            .map(|(id, _)| *id)
            .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect()
    }

    /// The opening an entity is part of (its symbol, or a jamb of its cut).
    pub fn opening_of(&self, id: EntityId) -> Option<OpeningId> {
        if !self.doc.has_openings() {
            return None;
        }
        self.wall_owners().opening(id)
    }

    /// The openings picked: those whose symbol and jambs are all in the selection.
    pub fn selected_openings(&self) -> BTreeSet<OpeningId> {
        if !self.doc.has_openings() {
            return BTreeSet::new();
        }
        let owners = self.wall_owners();
        let candidates: BTreeSet<OpeningId> = self
            .selection
            .iter()
            .filter_map(|id| owners.opening(*id))
            .collect();
        candidates
            .into_iter()
            .filter(|o| {
                let set = self.opening_pick_set(*o);
                !set.is_empty() && set.iter().all(|e| self.selection.contains(e))
            })
            .collect()
    }

    // ----- Keeping them generated -----------------------------------------------------------

    /// Write the symbols of openings on the segments in `on` (every opening when None),
    /// inside the open transaction. Openings whose wall is gone were removed with it (the
    /// notice says so).
    pub(crate) fn follow_openings(&mut self, on: Option<&BTreeSet<(WallId, u16)>>) {
        if !self.doc.in_transaction() {
            return;
        }
        if !self.doc.has_openings() {
            return;
        }
        let touched: BTreeSet<OpeningId> = self
            .doc
            .openings_touched_in_transaction()
            .into_iter()
            .collect();
        let todo: Vec<(OpeningId, Opening)> = self
            .doc
            .openings()
            .filter(|(id, o)| {
                touched.contains(id) || on.is_none_or(|s| s.contains(&(o.wall, o.seg)))
            })
            .map(|(id, o)| (id, o.clone()))
            .collect();
        let mut hosts: HashMap<(WallId, u16), Option<Host>> = HashMap::new();
        for (id, op) in todo {
            let host = *hosts
                .entry((op.wall, op.seg))
                .or_insert_with(|| self.opening_host(op.wall, op.seg));
            let items = match host.and_then(|h| {
                Frame::of(
                    &h.seg,
                    h.offsets,
                    op.span_on(h.seg.length()),
                    op.hand,
                    op.facing,
                )
            }) {
                Some(frame) => self.opening_symbol(&op, &frame),
                None => Vec::new(),
            };
            let layer = op
                .layer
                .or_else(|| self.doc.wall(op.wall).map(|w| w.layer))
                .unwrap_or(LayerId::DEFAULT);
            drift_geom::walls::write_symbol(&mut self.doc, id, &items, layer);
        }
    }

    /// Openings removed in the open transaction with their wall: say so.
    pub(crate) fn note_gone_openings(&mut self) {
        if !self.doc.in_transaction() || self.notice.is_some() {
            return;
        }
        let gone: Vec<OpeningId> = self
            .doc
            .openings_touched_in_transaction()
            .into_iter()
            .filter(|o| self.doc.opening(*o).is_none())
            .filter(|o| {
                // (Not exploded: its symbol would still be there.)
                self.doc.opening_before_transaction(*o).is_some_and(|b| {
                    self.doc.wall(b.wall).is_none()
                        && b.made.entities().all(|e| !self.doc.contains(e))
                })
            })
            .collect();
        if !gone.is_empty() && self.notice.is_none() {
            self.notice = Some(match gone.len() {
                1 => format!("Opening {} went with its wall", gone[0]),
                n => format!("{n} openings went with their walls"),
            });
        }
    }

    /// Before every wall is made again (a structural edit): openings whose segment changed
    /// under them go where it went. A segment is its centreline curve: renumbered (a split, a
    /// join), the opening follows its curve to its new wall and segment; cut or trimmed, it
    /// goes to the segment that now runs through where its jambs were. One left with no wall
    /// there is removed, with a notice.
    pub(crate) fn reseat_openings(&mut self) {
        if !self.doc.has_openings() {
            return;
        }
        let list: Vec<(OpeningId, Opening)> =
            self.doc.openings().map(|(id, o)| (id, o.clone())).collect();
        // Where each centreline curve is now: wall and segment.
        let mut now: HashMap<EntityId, (WallId, u16)> = HashMap::new();
        for (w, wall) in self.doc.walls() {
            if let Some(p) = self.doc.path(wall.centre) {
                for (k, c) in p.curves.iter().enumerate() {
                    now.insert(*c, (w, k as u16));
                }
            }
        }
        let mut removed = 0;
        for (id, op) in list {
            // The curve it was on when the edit began.
            let was = self
                .doc
                .wall_before_transaction(op.wall)
                .and_then(|w| self.doc.path_before_transaction(w.centre))
                .and_then(|p| p.curves.get(usize::from(op.seg)).copied());
            let here = was.and_then(|c| now.get(&c).copied());
            if here == Some((op.wall, op.seg)) {
                continue;
            }
            // Where its jambs were (as the walls last made them), on what runs there now.
            let jambs = self.jamb_middles(op.wall, id);
            let host = match (here, jambs) {
                (Some(h), _) => Some(h),
                (None, Some((p0, p1))) => self.segment_through((p0 + p1) / 2.0, op.wall),
                (None, None) => None,
            };
            let Some((w, k)) = host else {
                if here.is_none() && jambs.is_none() && self.doc.wall(op.wall).is_some() {
                    // (Not cut and its curve gone: left for the regeneration to judge.)
                    continue;
                }
                let made: Vec<EntityId> = op.made.entities().collect();
                self.doc.remove_opening(id);
                self.doc.delete(made);
                removed += 1;
                continue;
            };
            let mut moved = op.clone();
            moved.wall = w;
            moved.seg = k;
            // At the same place: from its jambs where it was cut.
            if let (Some((p0, p1)), Some(h)) = (jambs, self.opening_host(w, k)) {
                let (a, b) = (h.seg.param(p0), h.seg.param(p1));
                moved.at = OpeningAt::from_start(a.min(b));
            }
            let _ = self.doc.set_opening(id, moved);
        }
        if removed > 0 && self.notice.is_none() {
            self.notice = Some(match removed {
                1 => "An opening was removed: no wall runs where it was".to_owned(),
                n => format!("{n} openings were removed: no wall runs where they were"),
            });
        }
    }

    /// The wall segment `p` lies on (within its wall's thickness), `prefer`'s first.
    fn segment_through(&self, p: DVec2, prefer: WallId) -> Option<(WallId, u16)> {
        let mut best: Option<(bool, f64, WallId, u16)> = None;
        for (w, wall) in self.doc.walls() {
            let Ok((segs, ..)) = drift_geom::walls::regen::centreline(&self.doc, wall) else {
                continue;
            };
            for (k, s) in segs.iter().enumerate() {
                let u = s.param(p);
                if !(0.0..=s.length()).contains(&u) {
                    continue;
                }
                let d = s.at(u).distance(p);
                if d > wall.thickness.max(1.0) {
                    continue;
                }
                let key = (w != prefer, d, w, k as u16);
                if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, _, w, k)| (w, k))
    }

    /// The middles of an opening's two jambs, as its wall last made them.
    fn jamb_middles(&self, w: WallId, o: OpeningId) -> Option<(DVec2, DVec2)> {
        let made = &self.doc.wall(w)?.made;
        let mid = |n: u8| {
            let (_, id) = made
                .curves
                .iter()
                .find(|(k, _)| k.part == Part::Jamb && k.from == Station::Jamb(o, n))?;
            match self.doc.shape(*id)? {
                Shape::Line { a, b } => Some((a + b) / 2.0),
                _ => None,
            }
        };
        Some((mid(0)?, mid(1)?))
    }

    /// Re-evaluate openings' width and offset expressions (inside a transaction, before a
    /// solve).
    pub(crate) fn apply_opening_sizes(&mut self) -> Result<(), EditError> {
        let list: Vec<(OpeningId, Opening)> = self
            .doc
            .openings()
            .filter(|(_, o)| {
                o.width_expr.is_some() || matches!(&o.at, OpeningAt::From { expr: Some(_), .. })
            })
            .map(|(id, o)| (id, o.clone()))
            .collect();
        if list.is_empty() {
            return Ok(());
        }
        let eval = drift_core::params::evaluate(&self.doc);
        for (id, mut o) in list {
            let value = |e: &str, what: &str| {
                drift_core::params::evaluate_text(&self.doc, &eval, e, Kind::Length)
                    .map(|v| v.value)
                    .map_err(|err| EditError::Invalid(format!("Opening {id}'s {what}: {}", err.0)))
            };
            if let Some(e) = o.width_expr.clone() {
                let v = value(&e, "width")?;
                if v > 0.0 && v.is_finite() {
                    o.leaf = (o.leaf - (o.width - v)).clamp(0.0, v);
                    o.width = v;
                }
            }
            if let OpeningAt::From {
                offset,
                expr: Some(e),
                ..
            } = &mut o.at
            {
                let v = value(e, "offset")?;
                if v.is_finite() {
                    *offset = v;
                }
            }
            let _ = self.doc.set_opening(id, o);
        }
        Ok(())
    }

    // ----- Commands -----------------------------------------------------------------------

    /// Place an opening of the tool bar's type in wall `w`, segment `seg`, its first jamb `u0`
    /// along the centreline (one undo step).
    pub fn place_opening(&mut self, w: WallId, seg: u16, u0: f64) -> Option<OpeningId> {
        let row = self.opening_type_now();
        let width = row.opening.0;
        let Some((a, b)) = self.opening_room(w, seg, u0 + width / 2.0, width, None) else {
            self.notice = Some(format!(
                "No room for a {} opening there",
                self.doc.units().format_readout(width)
            ));
            return None;
        };
        let u0 = u0.clamp(a, b - width);
        let len = self.opening_host(w, seg)?.seg.length();
        let mut op = Opening::new(w, seg, width);
        // Measured from the nearer end of the segment.
        op.at = if u0 <= len - u0 - width {
            OpeningAt::from_start(u0)
        } else {
            OpeningAt::From {
                end: SegEnd::End,
                offset: len - u0 - width,
                expr: None,
            }
        };
        op.leaf = row.leaf.0.min(width);
        op.height = row.opening.1;
        op.symbol = Symbol::Library {
            family: row.family.to_owned(),
            type_name: row.name.clone(),
        };
        op.hand = self.opening.hand;
        op.facing = self.opening.facing;
        let label = match hosted::family(row.family).map(|f| f.kind) {
            Some(hosted::FamilyKind::Window) => "Window",
            Some(hosted::FamilyKind::Door) => "Door",
            _ => "Opening",
        };
        self.doc.begin();
        let id = match self.doc.add_opening(op) {
            Ok(id) => id,
            Err(e) => {
                self.doc.rollback();
                self.notice = Some(format!("No opening: {e}"));
                self.sync();
                return None;
            }
        };
        if !self.commit(label) {
            return None;
        }
        Some(id)
    }

    /// Change openings' records in one undo step.
    pub fn edit_openings(
        &mut self,
        openings: &[OpeningId],
        label: &str,
        mut f: impl FnMut(&mut Opening),
    ) -> bool {
        self.doc.begin();
        let mut any = false;
        for o in openings {
            if let Some(mut op) = self.doc.opening(*o).cloned() {
                f(&mut op);
                if self.doc.opening(*o) != Some(&op) && self.doc.set_opening(*o, op).is_ok() {
                    any = true;
                }
            }
        }
        if !any {
            self.doc.rollback();
            self.sync();
            return false;
        }
        self.commit(label)
    }

    /// Flip openings' hinge or swing.
    pub fn flip_openings(&mut self, openings: &[OpeningId], flip: Flip) -> bool {
        self.edit_openings(openings, "Flip opening", |o| match flip {
            Flip::Hand => o.hand = o.hand.flipped(),
            Flip::Facing => o.facing = o.facing.flipped(),
        })
    }

    /// Slide an opening along its segment so its first jamb is at `u0` (kept in the room
    /// there is), inside the open transaction (a drag), keeping the end it is measured from.
    pub fn slide_opening(&mut self, o: OpeningId, u0: f64) -> bool {
        let Some(op) = self.doc.opening(o).cloned() else {
            return false;
        };
        let Some(host) = self.opening_host(op.wall, op.seg) else {
            return false;
        };
        let len = host.seg.length();
        let Some((a, b)) =
            self.opening_room(op.wall, op.seg, u0 + op.width / 2.0, op.width, Some(o))
        else {
            return false;
        };
        let u0 = u0.clamp(a, b - op.width);
        let mut moved = op.clone();
        moved.at = match op.at {
            OpeningAt::From {
                end: SegEnd::End, ..
            } => OpeningAt::From {
                end: SegEnd::End,
                offset: len - u0 - op.width,
                expr: None,
            },
            _ => OpeningAt::from_start(u0),
        };
        if moved == op {
            return true;
        }
        let _ = self.doc.set_opening(o, moved);
        self.follow_walls();
        true
    }

    /// The snaps a slid opening's first jamb takes: centred on its segment, and the offset
    /// from either corner its room there starts with being a round number is left to typing.
    pub fn opening_snaps(&self, o: OpeningId) -> Vec<f64> {
        let Some(op) = self.doc.opening(o) else {
            return Vec::new();
        };
        let Some(h) = self.opening_host(op.wall, op.seg) else {
            return Vec::new();
        };
        vec![(h.seg.length() - op.width) / 2.0]
    }

    /// Set an opening's offset from typed text: the distance from the nearer face corner to
    /// its near jamb (a length, or an expression of variables).
    pub fn set_opening_offset(&mut self, o: OpeningId, text: &str) -> Result<(), EditError> {
        let v = self.evaluate_text(text, Kind::Length)?;
        let op = self
            .doc
            .opening(o)
            .cloned()
            .ok_or_else(|| EditError::Invalid("No such opening".into()))?;
        let (lo, hi, from_start) = self
            .opening_offsets(o)
            .ok_or_else(|| EditError::Invalid("The opening isn't in its wall".into()))?;
        let len = self
            .opening_host(op.wall, op.seg)
            .map_or(0.0, |h| h.seg.length());
        let (u0, _) = op.span_on(len);
        // From the corner it is measured from now: the nearer one.
        let target = if from_start {
            lo + v.value
        } else {
            hi - v.value - op.width
        };
        let _ = u0;
        self.doc.begin();
        if !self.slide_opening(o, target) {
            self.doc.rollback();
            self.sync();
            return Err(EditError::Invalid("There is no room for it there".into()));
        }
        self.commit("Opening offset");
        Ok(())
    }

    /// The corners either side of an opening (along its centreline, from the faces), and
    /// whether the start's is the nearer.
    pub fn opening_offsets(&self, o: OpeningId) -> Option<(f64, f64, bool)> {
        let op = self.doc.opening(o)?;
        let host = self.opening_host(op.wall, op.seg)?;
        let (u0, u1) = op.span_on(host.seg.length());
        let (a, b) = drift_geom::walls::free_spans(&self.doc, op.wall, op.seg, Some(o))
            .into_iter()
            .find(|(a, b)| *a <= u0 + 1e-6 && *b >= u1 - 1e-6)?;
        Some((a, b, u0 - a <= b - u1))
    }

    /// Set an opening's width from typed text (a length or an expression): the cut grows
    /// about its middle, the leaf with it.
    pub fn set_opening_width(&mut self, o: OpeningId, text: &str) -> Result<(), EditError> {
        let v = self.evaluate_text(text, Kind::Length)?;
        let width = v.value;
        if !(width > 0.0 && width.is_finite()) {
            return Err(EditError::Invalid(
                "An opening's width must be a length".into(),
            ));
        }
        let op = self
            .doc
            .opening(o)
            .cloned()
            .ok_or_else(|| EditError::Invalid("No such opening".into()))?;
        let host = self
            .opening_host(op.wall, op.seg)
            .ok_or_else(|| EditError::Invalid("The opening isn't in its wall".into()))?;
        let (u0, u1) = op.span_on(host.seg.length());
        let mid = (u0 + u1) / 2.0;
        let Some(start) = self.opening_start_near(op.wall, op.seg, mid, width, Some(o)) else {
            return Err(EditError::Invalid(format!(
                "No room for a {} opening there",
                self.doc.units().format_readout(width)
            )));
        };
        let len = host.seg.length();
        let mut sized = op.clone();
        sized.leaf = (op.leaf - (op.width - width)).clamp(0.0, width);
        sized.width = width;
        sized.width_expr = (!v.references.is_empty()).then(|| v.text.clone());
        sized.at = match op.at {
            OpeningAt::Centred => OpeningAt::Centred,
            OpeningAt::From {
                end: SegEnd::End, ..
            } => OpeningAt::From {
                end: SegEnd::End,
                offset: len - start - width,
                expr: None,
            },
            OpeningAt::From { .. } => OpeningAt::from_start(start),
        };
        self.doc.begin();
        if self.doc.set_opening(o, sized).is_err() {
            self.doc.rollback();
            self.sync();
            return Err(EditError::Invalid("That width can't be".into()));
        }
        self.commit("Opening width");
        Ok(())
    }

    /// Centre an opening on its segment.
    pub fn centre_openings(&mut self, openings: &[OpeningId]) -> bool {
        self.edit_openings(openings, "Centre opening", |o| o.at = OpeningAt::Centred)
    }

    /// Delete openings (and their symbols): their walls close up (one undo step).
    pub fn delete_openings(&mut self, openings: &[OpeningId]) -> bool {
        self.doc.begin();
        for o in openings {
            if let Some(op) = self.doc.opening(*o).cloned() {
                self.doc.remove_opening(*o);
                self.doc.delete(op.made.entities().collect::<Vec<_>>());
            }
        }
        let ok = self.commit("Delete opening");
        self.selection.clear();
        ok
    }

    /// Exploding walls: their openings' records go, their symbols staying as plain lines and
    /// arcs (inside the open transaction).
    pub(crate) fn explode_openings(&mut self, walls: &[WallId]) {
        let list: Vec<OpeningId> = self
            .doc
            .openings()
            .filter(|(_, o)| walls.contains(&o.wall))
            .map(|(id, _)| id)
            .collect();
        for o in list {
            self.doc.remove_opening(o);
        }
    }

    /// Copies of openings for copied walls (`copies`: each wall's copy), inside the open
    /// transaction: the copies make their own symbols.
    pub(crate) fn copy_openings_onto(&mut self, copies: &BTreeMap<WallId, WallId>) {
        let list: Vec<Opening> = self
            .doc
            .openings()
            .filter(|(_, o)| copies.contains_key(&o.wall))
            .map(|(_, o)| o.clone())
            .collect();
        for mut o in list {
            o.wall = copies[&o.wall];
            o.made = Default::default();
            let _ = self.doc.add_opening(o);
        }
    }

    // ----- On the canvas ------------------------------------------------------------------

    /// A picked opening's flip arrows: where each is, the way it points, and what it flips.
    pub fn opening_flip_arrows(&self, o: OpeningId) -> Vec<(DVec2, DVec2, Flip)> {
        let Some(f) = self.opening_frame(o) else {
            return Vec::new();
        };
        let reach = f.width.max(f.thickness) * 0.25 + f.thickness;
        // Facing: across the wall from its middle, pointing to the other face; hand: past
        // the hinge jamb, pointing along to the other jamb.
        let middle = f.point(DVec2::new(f.width / 2.0, f.thickness / 2.0));
        vec![
            (middle + f.y * reach, f.y, Flip::Facing),
            (
                f.point(DVec2::new(-reach * 0.6, f.thickness / 2.0)),
                f.x,
                Flip::Hand,
            ),
        ]
    }

    /// The flip arrow of a picked opening under `p` (within `tol`, a pick's reach).
    pub fn opening_arrow_at(&self, p: DVec2, tol: f64) -> Option<(OpeningId, Flip)> {
        let picked = self.selected_openings();
        if picked.len() != 1 {
            return None;
        }
        let o = *picked.iter().next()?;
        self.opening_flip_arrows(o)
            .into_iter()
            .filter(|(at, ..)| at.distance(p) <= 2.0 * tol)
            .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
            .map(|(.., f)| (o, f))
    }

    /// A picked opening's dimensions, as the dimension renderer draws them: its width
    /// between its jambs on the face it swings to, and its offset from the nearer corner of
    /// that face.
    pub fn opening_dimensions(&self, o: OpeningId) -> Vec<(Constraint, DVec2)> {
        let mut out = Vec::new();
        let (Some(op), Some(frame)) = (self.doc.opening(o), self.opening_frame(o)) else {
            return out;
        };
        let Some(wall) = self.doc.wall(op.wall) else {
            return out;
        };
        // The jamb corners on that face (see `drift_geom::walls::openings`: at the first
        // jamb the wall leaves back along the segment, at the second forward).
        let end = |n: u8, dir| drift_core::EndKey {
            wall: op.wall,
            seg: op.seg,
            at: Station::Jamb(o, n),
            dir,
        };
        let (e0, e1) = (end(0, drift_core::Dir::Back), end(1, drift_core::Dir::Fwd));
        let (k0, k1) = match op.facing {
            WallFace::Left => (
                drift_core::CornerKey::Own(e0, false),
                drift_core::CornerKey::Own(e1, true),
            ),
            WallFace::Right => (
                drift_core::CornerKey::Own(e0, true),
                drift_core::CornerKey::Own(e1, false),
            ),
        };
        let (Some(&j0), Some(&j1)) = (wall.made.points.get(&k0), wall.made.points.get(&k1)) else {
            return out;
        };
        let part = match op.facing {
            WallFace::Left => Part::Left,
            WallFace::Right => Part::Right,
        };
        // The corners either side: the far ends of the face pieces that end at the jambs.
        let far_end = |j: EntityId| {
            wall.made
                .curves
                .iter()
                .filter(|(k, _)| k.seg == op.seg && k.part == part)
                .find_map(|(_, c)| {
                    let [a, b] = self.doc.get(*c)?.kind.ends()?;
                    if a == j {
                        Some(b)
                    } else if b == j {
                        Some(a)
                    } else {
                        None
                    }
                })
        };
        let at = |p: EntityId| self.doc.point(p).ok();
        let out_by = frame.thickness * 0.5 + 12.0 * drift_core::units::MM_PER_INCH;
        let mut push = |a: EntityId, b: EntityId| {
            let (Some(pa), Some(pb)) = (at(a), at(b)) else {
                return;
            };
            let c = Constraint::Distance {
                a,
                b,
                value: pa.distance(pb),
            };
            if let Some(anchor) = crate::annotations::dimension_anchor(&self.doc, &c) {
                out.push((c, (pa + pb) / 2.0 - frame.y * out_by - anchor));
            }
        };
        push(j0, j1);
        let sides: Vec<(f64, EntityId, EntityId)> = [(j0, far_end(j0)), (j1, far_end(j1))]
            .into_iter()
            .filter_map(|(j, c)| {
                let c = c?;
                Some((at(j)?.distance(at(c)?), c, j))
            })
            .collect();
        if let Some(&(_, c, j)) = sides.iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
            push(c, j);
        }
        out
    }
}
