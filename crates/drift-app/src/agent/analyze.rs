//! `analyze`: how well defined the sketch is, what's wrong with it, and its closed areas.

use std::collections::BTreeMap;

use drift_core::{Constraint, EntityId, EntityKind, LineKind};
use drift_solver::Definition;
use serde_json::{Value, json};

use super::args::{self, num, pt};
use super::{Res, Session};

/// Open ends: curve ends that meet no other curve (a gap in an outline that should close).
fn open_ends(s: &Session) -> Vec<Value> {
    let doc = &s.editor.doc;
    let mut ends: BTreeMap<EntityId, usize> = BTreeMap::new();
    for (_, e) in doc.iter() {
        if e.style != LineKind::Solid {
            continue;
        }
        if let Some([a, b]) = e.kind.ends() {
            *ends.entry(a).or_default() += 1;
            *ends.entry(b).or_default() += 1;
        }
    }
    // Points held onto other curves by relations aren't loose.
    let held = |p: EntityId| {
        doc.constraints_on(p).any(|c| {
            matches!(
                doc.constraint(c),
                Some(
                    Constraint::Coincident { .. }
                        | Constraint::PointOnCurve { .. }
                        | Constraint::Midpoint { .. }
                )
            )
        })
    };
    ends.into_iter()
        .filter(|(p, n)| *n == 1 && !held(*p))
        .map(|(p, _)| json!({ "point": args::id(p), "at": pt(doc.point(p).unwrap_or_default()) }))
        .collect()
}

pub(super) fn analyze(s: &mut Session) -> Res {
    s.editor.sync();
    let ed = &mut s.editor;
    let report = ed.analysis().clone();
    let doc = &ed.doc;
    let mut under = Vec::new();
    let mut over = Vec::new();
    for (id, e) in doc.iter() {
        if e.kind.is_point() && doc.users_of(id).next().is_some() {
            continue;
        }
        match report.definitions.get(&id) {
            Some(Definition::Under) => under.push(args::id(id)),
            Some(Definition::Over) => over.push(args::id(id)),
            _ => {}
        }
    }
    // Points that can still move, for dimensioning to (curves' points are listed by curve).
    let mut free_points = Vec::new();
    for (id, e) in doc.iter() {
        if let EntityKind::Point(p) = e.kind
            && doc.users_of(id).next().is_some()
            && matches!(report.definitions.get(&id), Some(Definition::Under))
        {
            free_points.push(json!({ "point": args::id(id), "at": pt(p) }));
        }
    }
    let describe = |ids: &[drift_core::ConstraintId]| -> Vec<Value> {
        ids.iter()
            .map(|c| super::inspect::describe_constraint(doc, *c))
            .collect()
    };
    let conflicts = describe(&report.conflicts);
    let redundant = describe(&report.redundant);
    let degenerate: Vec<Value> = drift_solver::degenerate_entities(doc)
        .into_iter()
        .map(args::id)
        .collect();
    let overlapping: Vec<Value> = ed.overlapping().iter().map(|id| args::id(*id)).collect();
    let open = open_ends(s);
    let ed = &mut s.editor;
    let units = ed.doc.units();
    let regions = ed.regions();
    let ids = ed.region_ids();
    let mut areas: Vec<Value> = regions
        .iter()
        .map(|r| {
            let mut boundary: Vec<EntityId> =
                r.outer.curves().into_iter().map(|i| ids[i]).collect();
            boundary.sort();
            boundary.dedup();
            let b = r.bounds;
            json!({
                "area_mm2": num(r.area),
                "area_m2": num(r.area / 1e6),
                "area": units.format_area(r.area),
                "perimeter": num(r.perimeter),
                "centroid": pt(r.centroid),
                "bounds": [num(b.min.x), num(b.min.y), num(b.max.x), num(b.max.y)],
                "boundary": boundary.into_iter().map(args::id).collect::<Vec<_>>(),
                "holes": r.holes.len(),
            })
        })
        .collect();
    areas.sort_by(|a, b| {
        b["area_mm2"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&a["area_mm2"].as_f64().unwrap_or_default())
    });
    let dof = report.dof;
    let mut advice = Vec::new();
    if !report.conflicts.is_empty() {
        advice.push("Some constraints conflict: remove or change one of `conflicts`.".to_owned());
    }
    if !open.is_empty() {
        advice.push(format!(
            "{} curve end(s) meet nothing: join them (add_relations coincident) to close outlines.",
            open.len()
        ));
    }
    if !overlapping.is_empty() {
        advice.push(
            "Some curves lie on top of others (`overlapping`): delete the duplicates.".to_owned(),
        );
    }
    match dof {
        Some(0) if report.conflicts.is_empty() => advice.push("The sketch is fully defined.".to_owned()),
        Some(n) => advice.push(format!(
            "{n} degree(s) of freedom remain: add relations or dimensions to the `under_defined` entities (or fix a point to place the whole sketch)."
        )),
        None => {}
    }
    Ok(json!({
        "dof": dof,
        "fully_defined": dof == Some(0) && report.conflicts.is_empty(),
        "converged": report.converged,
        "under_defined": under,
        "free_points": free_points,
        "over_defined": over,
        "conflicts": conflicts,
        "redundant": redundant,
        "degenerate": degenerate,
        "overlapping": overlapping,
        "open_ends": open,
        "regions": areas,
        "advice": advice,
    }))
}
