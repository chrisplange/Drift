//! Door / window: hover a wall and click to place the tool bar's door, window or cased
//! opening in it. It shows the cut and the symbol live, centred on the pointer and kept clear
//! of the corners; Space steps through the four ways it can open (right-in, left-in,
//! left-out, right-out). Typing a value sets its offset from the nearer corner, and Tab
//! switches to its width. Each opening is one undo step; the tool stays on for the next.

use drift_core::{Opening, OpeningAt, Symbol, WallId};

use super::*;

/// Where the opening would go: its wall, segment and first jamb along the centreline, and
/// which corner (the start's, or the end's) the pointer is nearer.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    wall: WallId,
    seg: u16,
    u0: f64,
    width: f64,
    /// The room there is (from corner to corner, along the centreline).
    room: (f64, f64),
}

#[derive(Default)]
pub struct OpeningTool {
    hover: Option<Pointer>,
    spot: Option<Spot>,
    /// Offset from the nearer corner, and width.
    typed: [Option<f64>; 2],
    /// Over a wall with no room for it: how wide it is.
    no_room: Option<f64>,
}

impl OpeningTool {
    /// Where an opening goes for the pointer at `p` (and what is typed).
    fn spot_for(&mut self, ed: &mut Editor, p: &Pointer) -> Option<Spot> {
        self.no_room = None;
        let (w, seg, on) = ed.wall_under(p.raw, p.tolerance, None)?;
        let host = ed.opening_host(w, seg)?;
        let u = host.seg.param(on);
        let width = self.typed[1].unwrap_or_else(|| ed.opening_type_now().opening.0);
        let Some(room) = ed.opening_room(w, seg, u, width, None) else {
            self.no_room = Some(width);
            return None;
        };
        let u0 = match self.typed[0] {
            Some(d) => {
                let nearer_start = u - room.0 <= room.1 - u;
                if nearer_start {
                    room.0 + d
                } else {
                    room.1 - d - width
                }
            }
            None => u - width / 2.0,
        }
        .clamp(room.0, room.1 - width);
        Some(Spot {
            wall: w,
            seg,
            u0,
            width,
            room,
        })
    }

    fn place(&mut self, ed: &mut Editor) {
        let Some(s) = self.spot else {
            ed.notice = Some(match self.no_room {
                Some(w) => format!(
                    "No room for a {} opening there",
                    ed.doc.units().format_readout(w)
                ),
                None => "Hover a wall to place an opening in it".into(),
            });
            return;
        };
        let was = ed.opening.clone();
        if let Some(w) = self.typed[1] {
            // A typed width: the leaf keeps its frame allowance.
            let row = ed.opening_type_now();
            ed.opening.leaf = (w - (row.opening.0 - row.leaf.0)).clamp(0.0, w);
            ed.opening.width = w;
            ed.opening.height = row.opening.1;
            ed.opening.family = row.family.to_owned();
            ed.opening.type_name.clear();
        }
        ed.place_opening(s.wall, s.seg, s.u0);
        if self.typed[1].is_none() {
            ed.opening = was;
        }
        self.typed = [None; 2];
    }
}

impl Tool for OpeningTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Opening
    }

    fn hint(&self) -> String {
        if self.spot.is_some() {
            "Click to place it · Space: the way it opens · type the offset from the nearer \
             corner, Tab: width"
                .into()
        } else {
            "Hover a wall to place a door, window or opening in it (pick the type on the bar)"
                .into()
        }
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => {
                self.hover = Some(p);
                self.spot = self.spot_for(ed, &p);
            }
            Input::Click(p, _) | Input::DragEnd(p, _) => {
                self.hover = Some(p);
                self.spot = self.spot_for(ed, &p);
                self.place(ed);
                self.spot = self.spot_for(ed, &p);
            }
            Input::DragStart(p, _) => self.hover = Some(p),
            Input::DoubleClick(_) | Input::Finish => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let busy = self.typed.iter().any(Option::is_some);
        self.typed = [None; 2];
        busy
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        &["Offset", "Width"]
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values.map(|v| v.filter(|x| x.is_finite() && *x >= 0.0));
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        self.spot = self.spot_for(ed, p);
        self.place(ed);
        self.spot = self.spot_for(ed, p);
    }

    fn key(&mut self, key: ToolKey, pointer: Option<&Pointer>, ed: &mut Editor) -> bool {
        match key {
            ToolKey::Space => {
                let (h, f) = Opening::next_swing(ed.opening.hand, ed.opening.facing);
                ed.opening.hand = h;
                ed.opening.facing = f;
                if let Some(p) = pointer {
                    self.spot = self.spot_for(ed, p);
                }
                true
            }
            _ => false,
        }
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let Some(s) = self.spot else {
            return Vec::new();
        };
        let Some(host) = ed.opening_host(s.wall, s.seg) else {
            return Vec::new();
        };
        let row = ed.opening_type_now();
        let mut op = Opening::new(s.wall, s.seg, s.width);
        op.at = OpeningAt::from_start(s.u0);
        op.leaf = (row.leaf.0 - (row.opening.0 - s.width)).clamp(0.0, s.width);
        op.symbol = Symbol::Library {
            family: row.family.to_owned(),
            type_name: row.name.clone(),
        };
        op.hand = ed.opening.hand;
        op.facing = ed.opening.facing;
        let Some(frame) = drift_geom::walls::Frame::of(
            &host.seg,
            host.offsets,
            (s.u0, s.u0 + s.width),
            op.hand,
            op.facing,
        ) else {
            return Vec::new();
        };
        let mut out: Vec<SketchItem> = ed
            .opening_symbol(&op, &frame)
            .into_iter()
            .map(|(shape, _)| preview(shape))
            .collect();
        // The jambs: where the wall is cut.
        let (l, r) = host.offsets;
        for u in [s.u0, s.u0 + s.width] {
            let at = host.seg.at(u);
            let n = host.seg.tangent(u).perp();
            out.push(preview(Shape::Line {
                a: at + n * l,
                b: at + n * r,
            }));
        }
        out
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let s = self.spot?;
        let (a, b) = s.room;
        let offset = (s.u0 - a).min(b - s.u0 - s.width).max(0.0);
        Some(format!(
            "Offset {}  Width {}",
            p.units.format_readout(offset),
            p.units.format_readout(s.width)
        ))
    }
}
