//! File records for underlays (format 12): placement and look, and the curves themselves in
//! a compact form (`{"l": [x1, y1, x2, y2]}`, `{"p": [x, y, x, y, …]}`, `{"c": [x, y, r]}`,
//! `{"a": [x, y, r, start°, sweep°]}`).

use std::sync::Arc;

use drift_core::{DVec2, RefPrim, Rgb, Underlay, UnderlayId};
use serde::{Deserialize, Serialize};

use crate::{LoadError, parse_prefixed};

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum PrimRecord {
    #[serde(rename = "l")]
    Line([f64; 4]),
    #[serde(rename = "p")]
    Polyline(Vec<f64>),
    #[serde(rename = "c")]
    Circle([f64; 3]),
    #[serde(rename = "a")]
    Arc([f64; 5]),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnderlayRecord {
    pub(crate) id: String,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    source: String,
    at: [f64; 2],
    scale: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    opacity: f64,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    snap: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    locked: bool,
    prims: Vec<PrimRecord>,
}

pub(crate) fn underlay_record(id: UnderlayId, u: &Underlay) -> UnderlayRecord {
    let prims = u
        .prims
        .iter()
        .map(|p| match p {
            RefPrim::Line(a, b) => PrimRecord::Line([a.x, a.y, b.x, b.y]),
            RefPrim::Polyline(points) => {
                PrimRecord::Polyline(points.iter().flat_map(|p| [p.x, p.y]).collect())
            }
            RefPrim::Circle { center, radius } => PrimRecord::Circle([center.x, center.y, *radius]),
            RefPrim::Arc {
                center,
                radius,
                start,
                sweep,
            } => PrimRecord::Arc([
                center.x,
                center.y,
                *radius,
                start.to_degrees(),
                sweep.to_degrees(),
            ]),
        })
        .collect();
    UnderlayRecord {
        id: id.to_string(),
        name: u.name.clone(),
        source: u.source.clone(),
        at: u.at.to_array(),
        scale: u.scale,
        angle: u.angle,
        color: u.color.map(Rgb::hex),
        opacity: u.opacity,
        visible: u.visible,
        snap: u.snap,
        locked: u.locked,
        prims,
    }
}

pub(crate) fn parse_underlay(r: UnderlayRecord) -> Result<(UnderlayId, Underlay), LoadError> {
    let id = UnderlayId(parse_prefixed(&r.id, 'u')?);
    let bad = |field: &'static str| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    if !(r.scale > 0.0 && r.scale.is_finite()) {
        return Err(bad("scale"));
    }
    let color = match &r.color {
        Some(c) => Some(Rgb::parse(c).ok_or_else(|| bad("color"))?),
        None => None,
    };
    let mut prims = Vec::with_capacity(r.prims.len());
    for p in &r.prims {
        prims.push(match p {
            PrimRecord::Line([x1, y1, x2, y2]) => {
                RefPrim::Line(DVec2::new(*x1, *y1), DVec2::new(*x2, *y2))
            }
            PrimRecord::Polyline(xy) => {
                if xy.len() % 2 != 0 || xy.len() < 4 {
                    return Err(bad("prims"));
                }
                RefPrim::Polyline(xy.chunks(2).map(|c| DVec2::new(c[0], c[1])).collect())
            }
            PrimRecord::Circle([x, y, r]) => RefPrim::Circle {
                center: DVec2::new(*x, *y),
                radius: *r,
            },
            PrimRecord::Arc([x, y, r, start, sweep]) => RefPrim::Arc {
                center: DVec2::new(*x, *y),
                radius: *r,
                start: start.to_radians(),
                sweep: sweep.to_radians(),
            },
        });
    }
    Ok((
        id,
        Underlay {
            name: r.name,
            source: r.source,
            prims: Arc::new(prims),
            at: DVec2::from_array(r.at),
            scale: r.scale,
            angle: r.angle,
            color,
            opacity: r.opacity.clamp(0.0, 1.0),
            visible: r.visible,
            snap: r.snap,
            locked: r.locked,
        },
    ))
}
