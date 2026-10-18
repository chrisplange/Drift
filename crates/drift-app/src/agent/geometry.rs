//! `add_geometry`: points, lines, polylines, rectangles, polygons, circles, arcs, ellipses
//! and Bézier curves, joined where they share points.

use drift_core::{Constraint, DVec2, Document, EntityId, EntityKind, LineKind};
use serde_json::{Map, Value, json};

use super::args;
use super::{Res, Session};

/// Lines within this many degrees of level or plumb get a horizontal or vertical relation.
const AXIS_DEGREES: f64 = 1.0;

/// Where points go, sharing one point per location (within this batch, and with the
/// drawing's existing points when joining).
struct Points {
    made: Vec<(DVec2, EntityId)>,
    existing: Vec<(DVec2, EntityId)>,
}

impl Points {
    fn new(doc: &Document, join: bool) -> Self {
        let existing = if join {
            doc.iter()
                .filter_map(|(id, e)| match e.kind {
                    EntityKind::Point(p) if doc.users_of(id).next().is_some() => Some((p, id)),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            made: Vec::new(),
            existing,
        }
    }

    /// A point from a coordinate (shared with any point already there) or a point id.
    fn get(&mut self, doc: &mut Document, v: &Value) -> Result<EntityId, String> {
        if v.is_string() || v.is_u64() {
            let id = args::entity(doc, v)?;
            if !doc.get(id).is_some_and(|e| e.kind.is_point()) {
                return Err(format!("{id} is not a point"));
            }
            return Ok(id);
        }
        let p = args::vec2(v)?;
        let tol = 1e-6 * p.abs().max_element().max(1.0);
        if let Some((_, id)) = self
            .made
            .iter()
            .chain(&self.existing)
            .find(|(q, _)| q.distance(p) <= tol)
        {
            return Ok(*id);
        }
        let id = doc.add_point(p);
        self.made.push((p, id));
        Ok(id)
    }

    /// A new point that is never shared (centres, handles).
    fn fresh(&mut self, doc: &mut Document, p: DVec2) -> EntityId {
        doc.add_point(p)
    }

    fn at(&mut self, doc: &mut Document, item: &Value, key: &str) -> Result<EntityId, String> {
        self.get(doc, item.get(key).ok_or(format!("`{key}` is required"))?)
            .map_err(|e| format!("`{key}`: {e}"))
    }
}

/// Everything one call adds, and the relations it infers.
struct Build<'a> {
    doc: &'a mut Document,
    points: Points,
    curves: Vec<EntityId>,
    relations: Vec<Constraint>,
    auto: bool,
}

impl Build<'_> {
    fn line(&mut self, a: EntityId, b: EntityId) -> Result<EntityId, String> {
        let id = self.doc.add_line(a, b).map_err(|e| e.to_string())?;
        self.curves.push(id);
        if self.auto {
            let (pa, pb) = (
                self.doc.point(a).unwrap_or_default(),
                self.doc.point(b).unwrap_or_default(),
            );
            let d = pb - pa;
            let slope = AXIS_DEGREES.to_radians().tan();
            if d.y.abs() <= slope * d.x.abs() {
                self.relations.push(Constraint::Horizontal { line: id });
            } else if d.x.abs() <= slope * d.y.abs() {
                self.relations.push(Constraint::Vertical { line: id });
            }
        }
        Ok(id)
    }

    fn item(&mut self, item: &Value) -> Result<Value, String> {
        let kind = args::str_(item, "type")?;
        let doc_err = |e: drift_core::DocumentError| e.to_string();
        let mut out = Map::new();
        out.insert("type".into(), json!(kind));
        let mut put = |k: &str, v: Value| {
            out.insert(k.into(), v);
        };
        match kind {
            "point" => {
                let p = self.points.at(self.doc, item, "at")?;
                put("id", args::id(p));
            }
            "line" => {
                let a = self.points.at(self.doc, item, "from")?;
                let b = self.points.at(self.doc, item, "to")?;
                let id = self.line(a, b)?;
                put("id", args::id(id));
                put("a", args::id(a));
                put("b", args::id(b));
            }
            "polyline" => {
                let list = args::array(item, "points")?;
                if list.len() < 2 {
                    return Err("a polyline needs at least two points".into());
                }
                let closed = args::opt_bool(item, "closed")?.unwrap_or(false);
                let pts: Vec<EntityId> = list
                    .iter()
                    .map(|v| self.points.get(self.doc, v))
                    .collect::<Result<_, _>>()?;
                let mut lines = Vec::new();
                let n = pts.len();
                let segments = if closed { n } else { n - 1 };
                for i in 0..segments {
                    lines.push(self.line(pts[i], pts[(i + 1) % n])?);
                }
                put(
                    "lines",
                    json!(lines.iter().map(|l| args::id(*l)).collect::<Vec<_>>()),
                );
                put(
                    "points",
                    json!(pts.iter().map(|p| args::id(*p)).collect::<Vec<_>>()),
                );
            }
            "rectangle" => {
                let (p, q) = match (item.get("corner"), item.get("from")) {
                    (Some(c), _) => {
                        let c = args::vec2(c)?;
                        let w = args::f64_(item, "width")?;
                        let h = args::f64_(item, "height")?;
                        (c, c + DVec2::new(w, h))
                    }
                    (None, Some(_)) => (args::vec2_at(item, "from")?, args::vec2_at(item, "to")?),
                    _ => return Err("a rectangle needs `corner`, `width` and `height`, or `from` and `to` corners".into()),
                };
                let (lo, hi) = (p.min(q), p.max(q));
                if hi.x - lo.x <= 0.0 || hi.y - lo.y <= 0.0 {
                    return Err("a rectangle needs a width and a height".into());
                }
                let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
                let ids: Vec<EntityId> = corners
                    .iter()
                    .map(|c| self.points.get(self.doc, &json!([c.x, c.y])))
                    .collect::<Result<_, _>>()?;
                // Its sides are level and plumb however automatic relations are set.
                let auto = std::mem::replace(&mut self.auto, true);
                let sides: Vec<EntityId> = (0..4)
                    .map(|i| self.line(ids[i], ids[(i + 1) % 4]))
                    .collect::<Result<_, _>>()?;
                self.auto = auto;
                put(
                    "sides",
                    json!({ "bottom": args::id(sides[0]), "right": args::id(sides[1]), "top": args::id(sides[2]), "left": args::id(sides[3]) }),
                );
                put(
                    "corners",
                    json!({ "bottom_left": args::id(ids[0]), "bottom_right": args::id(ids[1]), "top_right": args::id(ids[2]), "top_left": args::id(ids[3]) }),
                );
            }
            "polygon" => {
                let center = args::vec2_at(item, "center")?;
                let radius = args::f64_(item, "radius")?;
                let sides = args::f64_(item, "sides")? as usize;
                if !(3..=200).contains(&sides) || radius <= 0.0 {
                    return Err("a polygon needs 3 to 200 sides and a positive radius".into());
                }
                let turn = args::opt_f64(item, "rotation")?
                    .unwrap_or(90.0)
                    .to_radians();
                let c = self.points.fresh(self.doc, center);
                let circle = self.doc.add_circle(c, radius).map_err(doc_err)?;
                self.doc.set_style(circle, LineKind::Construction);
                let pts: Vec<EntityId> = (0..sides)
                    .map(|k| {
                        let a = turn + std::f64::consts::TAU * k as f64 / sides as f64;
                        let p = center + DVec2::from_angle(a) * radius;
                        self.points.get(self.doc, &json!([p.x, p.y]))
                    })
                    .collect::<Result<_, _>>()?;
                let auto = std::mem::replace(&mut self.auto, false);
                let lines: Vec<EntityId> = (0..sides)
                    .map(|i| self.line(pts[i], pts[(i + 1) % sides]))
                    .collect::<Result<_, _>>()?;
                self.auto = auto;
                // Regular: every corner on the circle, every side the same.
                for p in &pts {
                    self.relations.push(Constraint::PointOnCurve {
                        point: *p,
                        curve: circle,
                    });
                }
                for w in lines.windows(2) {
                    self.relations.push(Constraint::Equal { a: w[0], b: w[1] });
                }
                put(
                    "lines",
                    json!(lines.iter().map(|l| args::id(*l)).collect::<Vec<_>>()),
                );
                put(
                    "points",
                    json!(pts.iter().map(|p| args::id(*p)).collect::<Vec<_>>()),
                );
                put("circle", args::id(circle));
                put("center", args::id(c));
            }
            "circle" => {
                let c = self.points.at(self.doc, item, "center")?;
                let r = args::f64_(item, "radius")?;
                let id = self.doc.add_circle(c, r).map_err(doc_err)?;
                self.curves.push(id);
                put("id", args::id(id));
                put("center", args::id(c));
            }
            "arc" => {
                let (c, s, e) = if item.get("through").is_some() {
                    let p0 = args::vec2_at(item, "start")?;
                    let p1 = args::vec2_at(item, "through")?;
                    let p2 = args::vec2_at(item, "end")?;
                    let center =
                        circumcenter(p0, p1, p2).ok_or("the three arc points are in a line")?;
                    // Arcs run counter-clockwise: one drawn clockwise is the same arc from its end.
                    let ccw = (p1 - p0).perp_dot(p2 - p0) > 0.0;
                    let (from, to) = if ccw {
                        ("start", "end")
                    } else {
                        ("end", "start")
                    };
                    let s = self.points.at(self.doc, item, from)?;
                    let e = self.points.at(self.doc, item, to)?;
                    (self.points.fresh(self.doc, center), s, e)
                } else {
                    let c = self.points.at(self.doc, item, "center")?;
                    let s = self.points.at(self.doc, item, "start")?;
                    let e = self.points.at(self.doc, item, "end")?;
                    (c, s, e)
                };
                let id = self.doc.add_arc(c, s, e).map_err(doc_err)?;
                self.curves.push(id);
                put("id", args::id(id));
                put("center", args::id(c));
                put("start", args::id(s));
                put("end", args::id(e));
            }
            "ellipse" => {
                let center = args::vec2_at(item, "center")?;
                let major_end = args::vec2_at(item, "major")?;
                let minor = args::f64_(item, "minor_radius")?;
                let axis = major_end - center;
                let c = self.points.fresh(self.doc, center);
                let m = self.points.fresh(self.doc, major_end);
                let n = self
                    .points
                    .fresh(self.doc, center + axis.perp().normalize_or_zero() * minor);
                let id = self.doc.add_ellipse(c, m, n).map_err(doc_err)?;
                self.curves.push(id);
                put("id", args::id(id));
                put("center", args::id(c));
            }
            "bezier" => {
                let a = self.points.at(self.doc, item, "from")?;
                let c1 = args::vec2_at(item, "c1")?;
                let c2 = args::vec2_at(item, "c2")?;
                let c1 = self.points.fresh(self.doc, c1);
                let c2 = self.points.fresh(self.doc, c2);
                let b = self.points.at(self.doc, item, "to")?;
                let id = self.doc.add_bezier(a, c1, c2, b).map_err(doc_err)?;
                self.curves.push(id);
                put("id", args::id(id));
                put("a", args::id(a));
                put("b", args::id(b));
            }
            other => return Err(format!("unknown geometry type `{other}`")),
        }
        Ok(Value::Object(out))
    }
}

/// The centre of the circle through three points.
fn circumcenter(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let size = (b - a).length().max((c - a).length());
    if d.abs() <= 1e-12 * size * size {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    Some(DVec2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    ))
}

pub(super) fn add(s: &mut Session, a: &Value) -> Res {
    let items = args::array(a, "items")?;
    if items.is_empty() {
        return Err("`items` is empty".into());
    }
    let layer = args::opt_layer(&s.editor.doc, a, "layer")?;
    let construction = args::opt_bool(a, "construction")?.unwrap_or(false);
    let auto = args::opt_bool(a, "auto_relations")?.unwrap_or(true);
    let join = args::opt_bool(a, "join")?.unwrap_or(true);
    let ed = &mut s.editor;
    ed.doc.begin();
    let points = Points::new(&ed.doc, join);
    let mut build = Build {
        doc: &mut ed.doc,
        points,
        curves: Vec::new(),
        relations: Vec::new(),
        auto,
    };
    let mut created = Vec::new();
    let mut failed = None;
    for (i, item) in items.iter().enumerate() {
        match build.item(item) {
            Ok(v) => created.push(v),
            Err(e) => {
                failed = Some(format!("items[{i}]: {e}"));
                break;
            }
        }
    }
    let (curves, relations) = (build.curves, build.relations);
    if let Some(e) = failed {
        ed.doc.rollback();
        return Err(e);
    }
    // Everything made goes on the layer asked for; construction curves don't print or bound
    // areas.
    let all: Vec<EntityId> = ed.doc.created_in_transaction();
    for id in &all {
        if let Some(l) = layer {
            ed.doc.set_entity_layer(*id, l).map_err(|e| e.to_string())?;
        }
        if construction && curves.contains(id) {
            ed.doc.set_style(*id, LineKind::Construction);
        }
    }
    if !ed.commit("Add geometry") {
        return Err(ed
            .notice
            .take()
            .unwrap_or("the geometry could not be added".into()));
    }
    // Inferred relations (level and plumb lines, regular polygons), as far as they fit.
    let mut added = 0;
    let mut skipped = Vec::new();
    if !relations.is_empty() {
        match ed.add_constraints("Relations", &relations) {
            Ok(ids) => added = ids.len(),
            Err(_) => {
                for c in &relations {
                    match ed.add_constraints("Relation", std::slice::from_ref(c)) {
                        Ok(_) => added += 1,
                        Err(e) => skipped.push(format!(
                            "{} on {}: {e}",
                            super::inspect::type_name(c),
                            c.entities()
                                .map(|e| e.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    }
                }
            }
        }
    }
    let mut v = json!({ "created": created, "relations_added": added });
    if !skipped.is_empty() {
        v["relations_skipped"] = json!(skipped);
    }
    Ok(v)
}
