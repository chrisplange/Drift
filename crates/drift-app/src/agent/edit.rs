//! Changing geometry: move, rotate, scale, mirror, arrays, offset, trim, fillet, delete;
//! and layers, fills and text.

use std::collections::BTreeSet;

use drift_core::{
    DVec2, EntityId, EntityKind, FillId, FillKind, Hatch, Layer, LineKind, LineType, LineWeight,
    Rgb, SketchText, TextAlign,
};
use drift_solver::Param;
use serde_json::{Value, json};

use super::args::{self, AnyId};
use super::{Res, Session};
use drift_edit::fills::FillStyle;
use drift_edit::modify::{Area, CuttingSet};

fn ids_json(ids: &[EntityId]) -> Value {
    json!(ids.iter().map(|i| args::id(*i)).collect::<Vec<_>>())
}

/// Curves (and lone points) to act on: points that define curves are left to follow them.
fn targets(s: &Session, a: &Value) -> Result<Vec<EntityId>, String> {
    let ids = args::entities_at(&s.editor.doc, a, "ids")?;
    if ids.is_empty() {
        return Err("`ids` is empty".into());
    }
    if let Some(id) = ids.iter().find(|id| s.editor.is_locked(**id)) {
        return Err(format!("{id} is on a locked layer"));
    }
    Ok(ids)
}

pub(super) fn move_point(s: &mut Session, a: &Value) -> Res {
    let p = args::point_at(&s.editor.doc, a, "point")?;
    let to = args::vec2_at(a, "to")?;
    if !s.editor.move_point_to(p, to) {
        return Err(format!(
            "{p} can't go there: its relations and dimensions hold it elsewhere"
        ));
    }
    Ok(json!({ "point": args::id(p), "at": args::pt(s.editor.doc.point(p).unwrap_or_default()) }))
}

/// Move `ids` by `delta`, their constraints pulling what's attached.
fn shift(s: &mut Session, ids: &[EntityId], delta: DVec2) -> Result<(), String> {
    let ed = &mut s.editor;
    ed.doc.begin();
    let dragged: Vec<Param> = ed
        .doc
        .defining_points(ids.iter().copied())
        .iter()
        .copied()
        .flat_map(Param::point)
        .collect();
    if !ed.drag(ids, delta, &dragged) {
        ed.doc.rollback();
        ed.sync();
        return Err(
            "its relations and dimensions hold it in place (remove or change them first)".into(),
        );
    }
    if !ed.commit("Move") {
        return Err(ed.out.notice.take().unwrap_or("the move conflicts".into()));
    }
    Ok(())
}

/// Copy `ids` in place with the relations among them; the copies, in order.
fn copy(s: &mut Session, ids: &[EntityId]) -> Vec<EntityId> {
    let picked = drift_edit::clipboard::Picked {
        curves: ids.to_vec(),
        instances: Vec::new(),
        texts: Vec::new(),
        fills: Default::default(),
        annotations: Vec::new(),
    };
    let ed = &mut s.editor;
    let source = ed.doc.clone();
    ed.doc.begin();
    let made = drift_edit::clipboard::copy_into(&source, &mut ed.doc, &picked, DVec2::ZERO, true);
    ed.commit("Copy");
    ids.iter()
        .filter_map(|id| made.entities.get(id).copied())
        .collect()
}

pub(super) fn transform(s: &mut Session, a: &Value) -> Res {
    let op = args::str_(a, "op")?;
    let ids = targets(s, a)?;
    let want_copy = args::opt_bool(a, "copy")?.unwrap_or(false);
    let ids = if want_copy && matches!(op, "move" | "rotate" | "scale") {
        copy(s, &ids)
    } else {
        ids
    };
    let mut result = json!({ "op": op, "ids": ids_json(&ids) });
    match op {
        "move" => shift(s, &ids, args::vec2_at(a, "by")?)?,
        "rotate" => {
            let degrees = args::f64_(a, "angle")?;
            let ed = &mut s.editor;
            let about = match args::opt_vec2(a, "about")? {
                Some(p) => p,
                None => ed.bounds_of(&ids).ok_or("nothing to rotate")?.center(),
            };
            let points: BTreeSet<EntityId> = ed.doc.defining_points(ids.iter().copied());
            ed.doc.begin();
            let (ok, removed) = ed.rotate_geometry(&ids, &points, about, degrees);
            if !ok {
                ed.doc.rollback();
                ed.sync();
                return Err("its constraints don't allow it to turn".into());
            }
            if drift_edit::editor::is_quarter_turn(degrees) {
                ed.restore_after_rotation(&removed, degrees);
            } else if !removed.is_empty() {
                result["relations_removed"] = json!(removed.len());
            }
            if !ed.commit("Rotate") {
                return Err(ed.out.notice.take().unwrap_or("the turn conflicts".into()));
            }
        }
        "scale" => {
            let factor = match a.get("factor") {
                Some(Value::Array(_)) => args::vec2_at(a, "factor")?,
                _ => DVec2::splat(args::f64_(a, "factor")?),
            };
            if factor.x <= 0.0 || factor.y <= 0.0 {
                return Err("`factor` must be positive".into());
            }
            let ed = &mut s.editor;
            let about = match args::opt_vec2(a, "about")? {
                Some(p) => p,
                None => ed.bounds_of(&ids).ok_or("nothing to scale")?.center(),
            };
            let points = ed.doc.defining_points(ids.iter().copied());
            ed.doc.begin();
            if !ed.scale_geometry(&ids, &points, about, factor, true) {
                ed.doc.rollback();
                ed.sync();
                return Err("its constraints don't allow that size".into());
            }
            if !ed.commit("Scale") {
                return Err(ed.out.notice.take().unwrap_or("the scale conflicts".into()));
            }
        }
        "mirror" => {
            let axis = args::entity_at(&s.editor.doc, a, "axis")?;
            let made = s.editor.mirror(&ids, axis).map_err(|e| e.to_string())?;
            if !args::opt_bool(a, "keep_original")?.unwrap_or(true) {
                let ed = &mut s.editor;
                ed.doc.begin();
                ed.doc.delete(ids.iter().copied().filter(|id| *id != axis));
                ed.commit("Delete originals");
            }
            result["created"] = ids_json(&made);
        }
        "linear_array" => {
            let count = args::f64_(a, "count")? as usize;
            let step = args::vec2_at(a, "by")?;
            let made = s
                .editor
                .linear_pattern(&ids, count, step)
                .map_err(|e| e.to_string())?;
            result["created"] = ids_json(&made);
        }
        "circular_array" => {
            let count = args::f64_(a, "count")? as usize;
            let about = args::vec2_at(a, "about")?;
            let total = args::opt_f64(a, "angle")?.unwrap_or(360.0);
            let made = s
                .editor
                .circular_pattern(&ids, count, about, total)
                .map_err(|e| e.to_string())?;
            result["created"] = ids_json(&made);
        }
        other => return Err(format!("unknown op `{other}`")),
    }
    Ok(result)
}

pub(super) fn offset(s: &mut Session, a: &Value) -> Res {
    let id = args::entity_at(&s.editor.doc, a, "id")?;
    let distance = args::f64_(a, "distance")?;
    let side = args::vec2_at(a, "side")?;
    let made = s
        .editor
        .offset(id, distance, side)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "created": ids_json(&made) }))
}

pub(super) fn trim(s: &mut Session, a: &Value) -> Res {
    let id = args::entity_at(&s.editor.doc, a, "id")?;
    if a.get("remove").is_some() {
        return trim_by(s, a, id);
    }
    let at = args::vec2_at(a, "at")?;
    // Only these curves cut (or are reached), when given.
    let mut against = match a.get("against") {
        Some(_) => Some(
            CuttingSet::of(
                &s.editor.doc,
                args::entities_at(&s.editor.doc, a, "against")?,
            )
            .ok_or("`against`: none of those is a curve")?,
        ),
        None => None,
    };
    let ed = &mut s.editor;
    match args::opt_str(a, "mode")?.unwrap_or("trim") {
        "trim" => ed
            .trim_at_against(id, at, against.as_mut())
            .map_err(|e| e.to_string())?,
        "extend" => ed
            .extend_at_against(id, at, against.as_ref())
            .map_err(|e| e.to_string())?,
        "split" => {
            // `split_at` gives the point at the cut; the new piece is the other curve there.
            let point = ed.split_at(id, at).map_err(|e| e.to_string())?;
            let rest = ed
                .doc
                .users_of(point)
                .find(|u| *u != id)
                .ok_or("the split made no second piece")?;
            return Ok(
                json!({ "pieces": [args::id(id), args::id(rest)], "point": args::id(point) }),
            );
        }
        other => return Err(format!("unknown mode `{other}` (trim, extend or split)")),
    }
    Ok(json!({ "done": true }))
}

/// Whether a side word is `inside` (else `outside`).
fn side(a: &Value, key: &str) -> Result<Option<bool>, String> {
    match args::opt_str(a, key)? {
        None => Ok(None),
        Some("inside") => Ok(Some(true)),
        Some("outside") => Ok(Some(false)),
        Some(other) => Err(format!("`{key}` is inside or outside, not `{other}`")),
    }
}

/// Trim the shape of `id` (and `ids`) where it lies inside or outside the closed shape `by`,
/// cut by that shape alone; with `and_remove`, then the shape `by` where it lies inside or
/// outside the first shape as it was. Union: inside, inside; notch (the first minus `by`):
/// inside, outside; overlap: outside, outside.
fn trim_by(s: &mut Session, a: &Value, id: EntityId) -> Res {
    let doc = &s.editor.doc;
    let mut what = vec![id];
    if a.get("ids").is_some() {
        what.extend(args::entities_at(doc, a, "ids")?);
    }
    let by = args::entities_at(doc, a, "by")?;
    let first = side(a, "remove")?.ok_or("`remove` is required")?;
    let then = side(a, "and_remove")?;
    let ed = &mut s.editor;
    let (what, by) = (ed.shape_curves(&what), ed.shape_curves(&by));
    // The inside of `by`: as given (a shape trimmed open since keeps the inside it had), or
    // as its curves close it now.
    let by_area = match a.get("inside_of") {
        Some(_) => Area {
            loops: args::array(a, "inside_of")?
                .iter()
                .map(|lp| {
                    lp.as_array()
                        .ok_or("`inside_of`: each is a list of points")?
                        .iter()
                        .map(args::vec2)
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<_, String>>()?,
        },
        None => ed.closed_area(&by).ok_or("`by` isn't a closed shape")?,
    };
    // (The first shape's inside as it is now: it may be trimmed open below.)
    let what_area = match then {
        Some(_) => Some(
            ed.closed_area(&what)
                .ok_or("`id` isn't a closed shape, so `and_remove` can't tell its inside")?,
        ),
        None => None,
    };
    let done = ed
        .trim_by(&what, &by, &by_area, first)
        .map_err(|e| e.to_string())?;
    let (mut trimmed, mut made, mut skipped) = (done.trimmed, done.made, done.skipped);
    if let (Some(inside), Some(area)) = (then, what_area) {
        let mut now: Vec<EntityId> = what
            .iter()
            .copied()
            .filter(|c| ed.doc.contains(*c))
            .collect();
        now.extend(&made);
        let done = ed
            .trim_by(&by, &now, &area, inside)
            .map_err(|e| e.to_string())?;
        trimmed += done.trimmed;
        made.extend(done.made);
        for why in done.skipped {
            if !skipped.contains(&why) {
                skipped.push(why);
            }
        }
    }
    made.retain(|c| ed.doc.contains(*c));
    Ok(json!({ "trimmed": trimmed, "made": ids_json(&made), "skipped": skipped }))
}

/// The corner point: a point id, or the point at (or nearest to) a coordinate where two
/// curves meet.
fn corner(s: &Session, v: &Value) -> Result<EntityId, String> {
    let doc = &s.editor.doc;
    if !v.is_array() && !v.is_object() {
        return args::point_at(doc, &json!({ "corner": v }), "corner");
    }
    let at = args::vec2(v)?;
    doc.iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Point(p) if doc.users_of(id).count() >= 2 => Some((id, p.distance(at))),
            _ => None,
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(id, _)| id)
        .ok_or("no corner there".into())
}

/// The corners: one (an id or a coordinate) or a list of them.
fn corners(s: &Session, a: &Value) -> Result<Vec<EntityId>, String> {
    let v = a.get("corner").ok_or("`corner` is required")?;
    let many = v
        .as_array()
        .is_some_and(|items| items.first().is_some_and(|f| !f.is_number()));
    if !many {
        return Ok(vec![corner(s, v)?]);
    }
    let mut out = Vec::new();
    for item in v.as_array().into_iter().flatten() {
        let p = corner(s, item)?;
        if out.contains(&p) {
            return Err(format!("`corner`: {} is listed twice", args::id(p)));
        }
        out.push(p);
    }
    Ok(out)
}

pub(super) fn fillet(s: &mut Session, a: &Value) -> Res {
    let ps = corners(s, a)?;
    let many = a.get("corner").is_some_and(|v| {
        v.as_array()
            .is_some_and(|items| items.first().is_some_and(|f| !f.is_number()))
    });
    let ed = &mut s.editor;
    let made = if let Some(r) = args::opt_f64(a, "radius")? {
        ed.fillet_corners(&ps, r).map_err(|e| e.to_string())?
    } else {
        let (d1, d2) = match a.get("chamfer") {
            Some(Value::Array(v)) if v.len() == 2 => (
                v[0].as_f64().ok_or("`chamfer` is a distance or two")?,
                v[1].as_f64().ok_or("`chamfer` is a distance or two")?,
            ),
            Some(Value::Number(n)) => {
                let d = n.as_f64().unwrap_or_default();
                (d, d)
            }
            _ => return Err("give a `radius` (fillet) or a `chamfer` distance".into()),
        };
        ed.chamfer_corners(&ps, d1, d2).map_err(|e| e.to_string())?
    };
    Ok(if many {
        json!({ "created": ids_json(&made), "corner": ids_json(&ps) })
    } else {
        json!({ "created": args::id(made[0]), "corner": args::id(ps[0]) })
    })
}

pub(super) fn delete(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let ids: Vec<AnyId> = args::array(a, "ids")?
        .iter()
        .map(|v| args::any_id(doc, v))
        .collect::<Result<_, _>>()?;
    let ed = &mut s.editor;
    ed.doc.begin();
    let mut entities = Vec::new();
    for id in &ids {
        match *id {
            AnyId::Entity(e) => entities.push(e),
            AnyId::Constraint(c) => ed.doc.remove_constraint(c),
            AnyId::Fill(f) => ed.doc.remove_fill(f),
            AnyId::Text(t) => ed.doc.remove_text(t),
        }
    }
    ed.delete_curves(entities);
    ed.selection.clear();
    ed.commit("Delete");
    Ok(json!({ "deleted": ids.len() }))
}

pub(super) fn set_construction(s: &mut Session, a: &Value) -> Res {
    let ids = targets(s, a)?;
    let on = args::opt_bool(a, "construction")?.unwrap_or(true);
    let style = match args::opt_str(a, "style")? {
        Some("solid") => LineKind::Solid,
        Some("construction") => LineKind::Construction,
        Some("centre_line" | "center_line") => LineKind::Center,
        Some(other) => {
            return Err(format!(
                "unknown style `{other}` (solid, construction or centre_line)"
            ));
        }
        None if on => LineKind::Construction,
        None => LineKind::Solid,
    };
    let ed = &mut s.editor;
    ed.doc.begin();
    for id in &ids {
        ed.doc.set_style(*id, style);
    }
    ed.commit("Construction");
    Ok(json!({ "ids": ids_json(&ids), "style": args::style_name(style) }))
}

/// Apply a layer description's properties to `layer`.
fn layer_props(l: &mut Layer, v: &Value) -> Result<(), String> {
    if let Some(name) = args::opt_str(v, "name")? {
        l.name = name.to_owned();
    }
    if let Some(c) = args::opt_str(v, "color")? {
        l.color = Some(Rgb::parse(c).ok_or(format!("colour `{c}` isn't like #rrggbb"))?);
    }
    if let Some(w) = args::opt_f64(v, "weight_mm")? {
        l.weight = LineWeight::nearest_cad(w);
    }
    if let Some(t) = args::opt_str(v, "line_type")? {
        l.line_type = LineType::from_name(t).ok_or(format!(
            "line type `{t}` isn't one of continuous, dashed, hidden, center, phantom"
        ))?;
    }
    if let Some(b) = args::opt_bool(v, "visible")? {
        l.visible = b;
    }
    if let Some(b) = args::opt_bool(v, "locked")? {
        l.locked = b;
    }
    Ok(())
}

pub(super) fn layers(s: &mut Session, a: &Value) -> Res {
    let ed = &mut s.editor;
    ed.doc.begin();
    let mut created = Vec::new();
    let result = (|| -> Result<(), String> {
        if let Some(Value::Array(list)) = a.get("create") {
            for v in list {
                let name = args::str_(v, "name")?;
                let mut l = Layer::new(name);
                layer_props(&mut l, v)?;
                created.push(ed.doc.add_layer(l));
            }
        }
        if let Some(Value::Array(list)) = a.get("update") {
            for v in list {
                let id = args::layer(&ed.doc, v.get("layer").ok_or("update needs `layer`")?)?;
                let mut l = ed.doc.layer(id).cloned().ok_or("no such layer")?;
                layer_props(&mut l, v)?;
                ed.doc.set_layer(id, l);
            }
        }
        if let Some(assign) = a.get("assign") {
            let id = args::layer(&ed.doc, assign.get("layer").ok_or("assign needs `layer`")?)?;
            // A curve takes its points with it, a shape whose whole outline is named its
            // fill (#266).
            let what = drift_edit::editor::LayerMove {
                entities: args::entities_at(&ed.doc, assign, "ids")?
                    .into_iter()
                    .collect(),
                ..Default::default()
            };
            ed.put_on_layer(id, &what).map_err(|e| e.to_string())?;
        }
        if let Some(Value::Array(list)) = a.get("delete") {
            for v in list {
                let id = args::layer(&ed.doc, v)?;
                ed.doc.remove_layer(id).map_err(|e| e.to_string())?;
            }
        }
        if let Some(v) = a.get("current") {
            let id = args::layer(&ed.doc, v)?;
            ed.doc.set_current_layer(id).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        ed.doc.rollback();
        return Err(e);
    }
    ed.commit("Layers");
    let doc = &ed.doc;
    let list: Vec<Value> = doc
        .layers()
        .map(|(id, l)| json!({ "id": id.to_string(), "name": l.name, "current": id == doc.current_layer() }))
        .collect();
    Ok(
        json!({ "created": created.iter().map(|l| l.to_string()).collect::<Vec<_>>(), "layers": list }),
    )
}

pub(super) fn add_fill(s: &mut Session, a: &Value) -> Res {
    let at = args::vec2_at(a, "at")?;
    let layer = args::opt_layer(&s.editor.doc, a, "layer")?;
    let kind = match args::opt_str(a, "kind")?.unwrap_or("hatch") {
        "solid" => FillKind::Solid,
        "hatch" => {
            let mut h = Hatch::new(args::opt_str(a, "pattern")?.unwrap_or("ANSI31"));
            s.patterns.update(s.editor.doc.custom_patterns());
            if s.patterns.get(&h.pattern).is_none() {
                return Err(format!("no hatch pattern `{}`", h.pattern));
            }
            h.scale = args::opt_f64(a, "scale")?.unwrap_or(1.0);
            h.angle = args::opt_f64(a, "angle")?.unwrap_or(0.0);
            FillKind::Hatch(h)
        }
        other => return Err(format!("unknown fill kind `{other}` (solid or hatch)")),
    };
    let color = match args::opt_str(a, "color")? {
        Some(c) => Some(Rgb::parse(c).ok_or(format!("colour `{c}` isn't like #rrggbb"))?),
        None => None,
    };
    let ed = &mut s.editor;
    let style = std::mem::replace(
        &mut ed.prefs.fill_style,
        FillStyle {
            kind,
            color,
            opacity: args::opt_f64(a, "opacity")?.unwrap_or(0.35).clamp(0.0, 1.0),
            weight: None,
        },
    );
    let before: Vec<FillId> = ed.doc.fills().map(|(id, _)| id).collect();
    let result = ed.add_fill_at(at);
    ed.prefs.fill_style = style;
    let id = result?;
    // An area that is already filled isn't filled twice: its fill takes the new style.
    let restyled = before.contains(&id);
    if let Some(l) = layer
        && let Some(mut f) = ed.doc.fill(id).cloned()
    {
        f.layer = l;
        ed.update_fill(id, f);
    }
    let regions = ed.regions();
    let area = drift_geom::regions::region_at(&regions, at).map(|r| r.area);
    let mut out = json!({ "id": id.to_string(), "area_mm2": area.map(args::num) });
    if restyled {
        out["restyled"] = json!(true);
        out["note"] = json!(format!(
            "this area was already filled: {id} took the new style, no new fill was made"
        ));
    }
    Ok(out)
}

pub(super) fn add_text(s: &mut Session, a: &Value) -> Res {
    let text = args::str_(a, "text")?;
    let at = args::vec2_at(a, "at")?;
    let height = args::opt_f64(a, "height")?.unwrap_or_else(|| {
        // Readable when printed: the drafting text height on paper, at the drawing's scale.
        let style = s.editor.doc.draft_style();
        style.text_height * style.scale
    });
    let mut t = SketchText::new(text, at, height);
    t.angle = args::opt_f64(a, "angle")?.unwrap_or(0.0);
    t.align = match args::opt_str(a, "align")?.unwrap_or("left") {
        "left" => TextAlign::Left,
        "center" | "centre" => TextAlign::Center,
        "right" => TextAlign::Right,
        other => return Err(format!("unknown align `{other}`")),
    };
    let layer = args::opt_layer(&s.editor.doc, a, "layer")?;
    let ed = &mut s.editor;
    let id = ed.add_text(t);
    if let Some(l) = layer {
        ed.update_text(id, |t| t.layer = l);
    }
    Ok(json!({ "id": id.to_string(), "height": args::num(height) }))
}
