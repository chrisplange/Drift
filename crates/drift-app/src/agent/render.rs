//! `render`: a PNG picture of the drawing, or part of it, for the assistant to look at.
//!
//! The drawing is drawn as it prints (fills, curves, dimensions, notes), then optionally
//! marked up: curves coloured by how well defined they are, end points, relation tags and
//! entity ids, so the picture can be checked against the drawing's data.

use std::collections::BTreeMap;

use drift_core::{Bounds, Constraint, DVec2, EntityId, EntityKind, LineKind, Rgb, TextFont};
use drift_draft::page::Align;
use drift_solver::Definition;
use serde_json::{Value, json};

use super::args;
use super::raster::{self, Canvas};
use super::{Res, Session};

/// Pixels per page millimetre: about 100 dpi, so annotations come out at a readable size.
const PX_PER_MM: f64 = 4.0;
const UNDER: Rgb = Rgb([30, 110, 220]);
const FULL: Rgb = Rgb([20, 20, 20]);
const OVER: Rgb = Rgb([220, 40, 40]);
const CONSTRUCTION: Rgb = Rgb([150, 150, 150]);
const TAG: Rgb = Rgb([0, 130, 70]);
const LABEL: Rgb = Rgb([150, 60, 160]);

/// The short tag drawn for a relation.
fn tag(c: &Constraint) -> Option<&'static str> {
    Some(match c {
        Constraint::Horizontal { .. } | Constraint::HorizontalPoints { .. } => "H",
        Constraint::Vertical { .. } | Constraint::VerticalPoints { .. } => "V",
        Constraint::Parallel { .. } => "//",
        Constraint::Perpendicular { .. } => "L",
        Constraint::Equal { .. } => "=",
        Constraint::Tangent { .. } => "T",
        Constraint::Concentric { .. } => "CC",
        Constraint::Coradial { .. } => "CR",
        Constraint::Collinear { .. } => "COL",
        Constraint::PointOnCurve { .. } => "ON",
        Constraint::Symmetric { .. } => "SYM",
        Constraint::Midpoint { .. } => "M",
        Constraint::Fix { .. } | Constraint::FixRadius { .. } => "FIX",
        Constraint::Smooth { .. } => "G1",
        _ => return None,
    })
}

const LEGEND: &str = "Curves: black = fully defined, blue = can still move, red = over-defined (conflict), \
grey dashed = construction. Green tags are relations: H horizontal, V vertical, // parallel, \
L perpendicular, = equal, T tangent, CC concentric, CR coradial, COL collinear, ON point on curve, \
SYM symmetric, M midpoint, FIX fixed, G1 smooth. Purple labels are entity ids.";

/// Where a curve's label goes: halfway along it.
fn middle(pts: &[DVec2]) -> Option<(DVec2, DVec2)> {
    let total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut left = total / 2.0;
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        if len >= left && len > 0.0 {
            let dir = (w[1] - w[0]) / len;
            return Some((w[0] + dir * left, dir));
        }
        left -= len;
    }
    pts.first().map(|p| (*p, DVec2::X))
}

/// What the picture should show by default: the drawing and its dimension labels, with air
/// around it.
fn extent(s: &mut Session) -> Bounds {
    let ed = &s.editor;
    let mut b = ed.bounds();
    for (_, u) in ed.doc.underlays().filter(|(_, u)| u.visible) {
        let ub = u.bounds();
        if !ub.is_empty() {
            b.include(ub.min);
            b.include(ub.max);
        }
    }
    for (_, t) in ed.doc.texts() {
        b.include(t.at);
    }
    if b.is_empty() {
        return Bounds {
            min: DVec2::splat(-500.0),
            max: DVec2::splat(500.0),
        };
    }
    let doc = &ed.doc;
    for (id, c) in doc.constraints() {
        if let (Some(anchor), Some(offset)) = (
            crate::annotations::dimension_anchor(doc, c),
            doc.placement(id),
        ) {
            b.include(anchor + offset);
        }
    }
    let size = (b.max - b.min).max(DVec2::splat(1.0));
    let pad = size.max_element() * 0.08;
    Bounds {
        min: b.min - DVec2::splat(pad),
        max: b.max + DVec2::splat(pad),
    }
}

pub(super) fn render(s: &mut Session, a: &Value, image: &mut Option<Vec<u8>>) -> Res {
    let region = match a.get("region") {
        Some(Value::Array(v)) if v.len() == 4 => {
            let n: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
            if n.len() != 4 || n[0] == n[2] || n[1] == n[3] {
                return Err("`region` is [min_x, min_y, max_x, max_y], with a size".into());
            }
            Bounds {
                min: DVec2::new(n[0].min(n[2]), n[1].min(n[3])),
                max: DVec2::new(n[0].max(n[2]), n[1].max(n[3])),
            }
        }
        None | Some(Value::Null) => extent(s),
        _ => return Err("`region` is [min_x, min_y, max_x, max_y]".into()),
    };
    let max_px = args::opt_f64(a, "width")?
        .unwrap_or(1024.0)
        .clamp(64.0, 4096.0);
    let size = region.max - region.min;
    // The longer side of the region gets the size asked for.
    let mm_per_px = size.max_element() / max_px;
    let (w, h) = (
        (size.x / mm_per_px).round().max(16.0) as u32,
        (size.y / mm_per_px).round().max(16.0) as u32,
    );
    let page = DVec2::new(f64::from(w), f64::from(h)) / PX_PER_MM;
    let scale = size.x / page.x;
    let show_dims = args::opt_bool(a, "dimensions")?.unwrap_or(true);
    let show_dof = args::opt_bool(a, "dof_colors")?.unwrap_or(true);
    let show_tags = args::opt_bool(a, "relations")?.unwrap_or(true);
    let show_points = args::opt_bool(a, "points")?.unwrap_or(true);
    let labels = args::opt_str(a, "labels")?.unwrap_or("curves");
    if !matches!(labels, "none" | "curves" | "all") {
        return Err("`labels` is none, curves or all".into());
    }

    let painter = s.painter();
    s.patterns.update(s.editor.doc.custom_patterns());
    let center = region.center();
    let drawing = crate::output::view_drawing(
        &mut s.editor,
        &s.patterns,
        &painter,
        center,
        scale,
        page,
        show_dims,
    );
    let mut canvas = Canvas::new(w, h, page).ok_or("the picture is too big")?;
    raster::draw(&mut canvas, &drawing);

    // World millimetres onto the page.
    let to_page = |p: DVec2| page / 2.0 + (p - center) / scale;
    let px = 1.0 / PX_PER_MM; // a pixel, in page mm
    let ed = &s.editor;
    let doc = &ed.doc;
    let analysis = ed.analysis();
    let state = |id: EntityId| match analysis.definitions.get(&id) {
        Some(Definition::Over) => OVER,
        Some(Definition::Full) => FULL,
        _ => UNDER,
    };
    let mut curves = Vec::new();
    for (id, e) in doc.iter() {
        if e.kind.is_point() || !doc.is_visible(id) {
            continue;
        }
        let Some(shape) = doc.shape(id) else { continue };
        let pts: Vec<DVec2> = args::polyline(&shape).into_iter().map(to_page).collect();
        curves.push((id, e.kind, e.style, pts));
    }
    if show_dof {
        for (id, _, style, pts) in &curves {
            let (c, dash) = if *style == LineKind::Solid {
                (state(*id), None)
            } else {
                (CONSTRUCTION, Some(vec![6.0, 4.0]))
            };
            let c = if analysis.conflicts.iter().any(|k| {
                doc.constraint(*k)
                    .is_some_and(|k| k.entities().any(|x| x == *id))
            }) {
                OVER
            } else {
                c
            };
            canvas.stroke(pts, false, c, 1.6, dash);
        }
    }
    if show_points {
        for (id, kind, style, _) in &curves {
            if *style != LineKind::Solid {
                continue;
            }
            let ends: Vec<EntityId> = match kind {
                EntityKind::Circle { center, .. } | EntityKind::Ellipse { center, .. } => {
                    vec![*center]
                }
                k => k.ends().map(|e| e.to_vec()).unwrap_or_default(),
            };
            let _ = id;
            for p in ends {
                if let Ok(at) = doc.point(p) {
                    let c = if show_dof { state(p) } else { FULL };
                    canvas.dot(to_page(at), 2.5, c);
                }
            }
        }
    }
    let text_h = 9.0 * px * 0.7;
    if show_tags {
        // Tags beside the curve (or point) they apply to, stacked when there are several.
        let mut per: BTreeMap<EntityId, usize> = BTreeMap::new();
        for (_, c) in doc.constraints() {
            let Some(t) = tag(c) else { continue };
            let Some(owner) = c
                .entities()
                .find(|e| doc.get(*e).is_some_and(|x| !x.kind.is_point()))
                .or_else(|| c.entities().next())
            else {
                continue;
            };
            let Some((at, dir)) = curves
                .iter()
                .find(|(id, ..)| *id == owner)
                .and_then(|(_, _, _, pts)| middle(pts))
                .or_else(|| doc.point(owner).ok().map(|p| (to_page(p), DVec2::X)))
            else {
                continue;
            };
            let n = per.entry(owner).or_default();
            let normal = dir.perp();
            let along = dir * (*n as f64 * 14.0 * px);
            *n += 1;
            let spot = at + normal * (9.0 * px) + along;
            canvas.text(
                t,
                &TextFont::Sans,
                spot,
                text_h,
                0.0,
                Align::Center,
                TAG,
                2.5,
            );
        }
    }
    if labels != "none" {
        for (id, _, _, pts) in &curves {
            if let Some((at, dir)) = middle(pts) {
                let spot = at - dir.perp() * (12.0 * px);
                canvas.text(
                    &id.to_string(),
                    &TextFont::Sans,
                    spot,
                    text_h,
                    0.0,
                    Align::Center,
                    LABEL,
                    2.5,
                );
            }
        }
        if labels == "all" {
            for (id, e) in doc.iter() {
                if let EntityKind::Point(p) = e.kind
                    && doc.users_of(id).next().is_some()
                {
                    let spot = to_page(p) + DVec2::new(4.0, 4.0) * px;
                    canvas.text(
                        &id.to_string(),
                        &TextFont::Sans,
                        spot,
                        text_h * 0.85,
                        0.0,
                        Align::Left,
                        LABEL,
                        2.5,
                    );
                }
            }
        }
    }
    *image = Some(canvas.png()?);
    Ok(json!({
        "width": w, "height": h,
        "region": [args::num(region.min.x), args::num(region.min.y), args::num(region.max.x), args::num(region.max.y)],
        "mm_per_pixel": args::num(mm_per_px),
        "pixel_to_world": "x = region[0] + px * mm_per_pixel; y = region[3] - py * mm_per_pixel",
        "legend": LEGEND,
        "sketch": s.status(),
    }))
}
