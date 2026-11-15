//! Blocks: make one from curves, place copies of it, explode copies, and turn a group into
//! a block or a copy into a group. Each calls what Drift's own commands call (Make block,
//! the Block tool, Explode, Explode to group, Make block from group).

use std::collections::BTreeSet;

use drift_core::{EntityId, InstanceId};
use serde_json::{Value, json};

use super::args::{self, num, pt};
use super::{Res, Session};
use drift_edit::editor::Editor;

/// The undo step's name for a `blocks` call.
pub(super) fn label(a: &Value) -> &'static str {
    match a.get("op").and_then(Value::as_str) {
        Some("make") => "make block",
        Some("place") => "place block",
        Some("explode") => "explode block",
        Some("from_group") => "block from group",
        _ => "blocks",
    }
}

pub(super) fn blocks(s: &mut Session, a: &Value) -> Res {
    let op = args::str_(a, "op")?;
    // (A copy made while a block is edited in place becomes that block's curves, and a
    // block can't be made inside one.)
    if s.editor.block_edit.is_some() {
        return Err("a block is being edited in place in Drift: finish that first".into());
    }
    match op {
        "make" => make(s, a),
        "place" => place(s, a),
        "explode" => explode(s, a),
        "from_group" => from_group(s, a),
        other => Err(format!(
            "unknown op `{other}` (make, place, explode or from_group)"
        )),
    }
}

/// A block copy: its block, where it is and how it is placed.
pub(super) fn describe_copy(ed: &Editor, id: InstanceId) -> Value {
    let doc = &ed.doc;
    let Some(i) = doc.instance(id) else {
        return json!({ "id": id.to_string(), "missing": true });
    };
    let b = ed.instance_bounds(id);
    let bounds =
        (!b.is_empty()).then(|| json!([num(b.min.x), num(b.min.y), num(b.max.x), num(b.max.y)]));
    json!({
        "id": id.to_string(),
        "block": i.block.to_string(),
        "name": doc.block(i.block).map(|b| b.name.clone()),
        "at": pt(i.at),
        "angle": num(i.angle),
        "scale": num(i.scale),
        "mirror": i.mirror,
        "layer": doc.layer(i.layer).map(|l| format!("{} ({})", i.layer, l.name)),
        "bounds": bounds,
    })
}

/// The block a new copy is of, and that copy.
fn made(ed: &mut Editor, copy: InstanceId) -> Res {
    let Some(i) = ed.doc.instance(copy).copied() else {
        return Err(ed
            .out
            .notice
            .take()
            .unwrap_or("the block could not be made".into()));
    };
    let block = ed.doc.block(i.block).ok_or("the block could not be made")?;
    Ok(json!({
        "block": i.block.to_string(),
        "name": block.name,
        "curves": block.items.len(),
        "fills": block.fills.len(),
        "copy": describe_copy(ed, copy),
    }))
}

fn locked_layer(ed: &Editor, copy: InstanceId) -> bool {
    ed.doc
        .instance(copy)
        .and_then(|i| ed.doc.layer(i.layer))
        .is_some_and(|l| l.locked)
}

/// Make block: the curves `ids` become a block, and a copy of it takes their place.
fn make(s: &mut Session, a: &Value) -> Res {
    let ids = args::entities_at(&s.editor.doc, a, "ids")?;
    if ids.is_empty() {
        return Err("`ids` is empty".into());
    }
    if let Some(id) = ids.iter().find(|id| s.editor.is_locked(**id)) {
        return Err(format!("{id} is on a locked layer"));
    }
    let name = args::opt_str(a, "name")?.unwrap_or_default();
    let base = args::opt_vec2(a, "base")?;
    let ed = &mut s.editor;
    // Drift makes a block of what is picked.
    let picked = std::mem::replace(&mut ed.selection, ids.into_iter().collect());
    match ed.make_block_at(name, base) {
        Ok(copy) => made(ed, copy),
        Err(e) => {
            ed.selection = picked;
            Err(if e.starts_with("Select the geometry") {
                "`ids` has no curves to make a block of".into()
            } else {
                e
            })
        }
    }
}

/// The Block tool: a copy of `block` at `at`, turned, mirrored and scaled.
fn place(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let block = args::block(doc, a.get("block").ok_or("`block` is required")?)?;
    let at = args::vec2_at(a, "at")?;
    let angle = args::opt_f64(a, "angle")?.unwrap_or(0.0);
    let mirror = args::opt_bool(a, "mirror")?.unwrap_or(false);
    let scale = args::opt_f64(a, "scale")?.unwrap_or(1.0);
    if scale <= 0.0 {
        return Err("`scale` must be positive".into());
    }
    let layer = args::opt_layer(doc, a, "layer")?;
    let before: BTreeSet<InstanceId> = doc.instances().map(|(id, _)| id).collect();
    let ed = &mut s.editor;
    ed.place_instance(block, at, angle, mirror);
    let Some(copy) = ed
        .doc
        .instances()
        .map(|(id, _)| id)
        .find(|id| !before.contains(id))
    else {
        return Err(ed
            .out
            .notice
            .take()
            .unwrap_or("the copy could not be placed".into()));
    };
    // Its size and layer, as the copy's Placement panel sets them.
    let mut placed = ed.doc.instance(copy).copied().ok_or("the copy is gone")?;
    placed.scale = scale;
    if let Some(l) = layer {
        placed.layer = l;
    }
    if ed.doc.instance(copy) != Some(&placed) {
        ed.doc.begin();
        ed.doc
            .set_instance(copy, placed)
            .map_err(|e| e.to_string())?;
        ed.commit("Edit block copy");
    }
    Ok(json!({ "copy": describe_copy(ed, copy) }))
}

/// Explode (or Explode to group): copies become ordinary curves again.
fn explode(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let copies: BTreeSet<InstanceId> = args::array(a, "copies")?
        .iter()
        .map(|v| args::instance(doc, v).map_err(|e| format!("`copies`: {e}")))
        .collect::<Result<_, _>>()?;
    if copies.is_empty() {
        return Err("`copies` is empty".into());
    }
    if let Some(id) = copies.iter().find(|id| locked_layer(&s.editor, **id)) {
        return Err(format!("{id} is on a locked layer"));
    }
    let to_group = args::opt_bool(a, "to_group")?.unwrap_or(false);
    let ed = &mut s.editor;
    // Drift explodes the copies that are picked.
    ed.selection.clear();
    ed.selected_instances = copies.clone();
    let ids =
        |ids: &BTreeSet<EntityId>| json!(ids.iter().map(|e| args::id(*e)).collect::<Vec<_>>());
    let result = if to_group {
        let groups = ed.explode_to_groups();
        if groups.len() != copies.len() {
            return Err("a copy could not be made a group".into());
        }
        let groups: Vec<Value> = groups
            .iter()
            .filter_map(|g| {
                let group = ed.doc.group(*g)?;
                Some(json!({ "id": g.to_string(), "name": group.name, "members": ids(&group.members) }))
            })
            .collect();
        json!({ "groups": groups, "created": ids(&ed.selection) })
    } else {
        ed.explode_selected();
        json!({ "created": ids(&ed.selection) })
    };
    if let Some(left) = copies.iter().find(|id| ed.doc.instance(**id).is_some()) {
        return Err(ed
            .out
            .notice
            .take()
            .unwrap_or(format!("{left} could not be exploded")));
    }
    Ok(result)
}

/// Make block from group: the group's curves (and fills) become a block named after it.
fn from_group(s: &mut Session, a: &Value) -> Res {
    let doc = &s.editor.doc;
    let group = args::group(doc, a.get("group").ok_or("`group` is required")?)?;
    if let Some(id) = doc
        .group(group)
        .and_then(|g| g.members.iter().find(|id| s.editor.is_locked(**id)))
    {
        return Err(format!("{id} is on a locked layer"));
    }
    let ed = &mut s.editor;
    let copy = ed.block_from_group(group)?;
    made(ed, copy)
}
