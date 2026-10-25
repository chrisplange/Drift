//! File records for blocks and their placed copies (format 10; a block's `fills`, 22; its
//! `profile` flag, 36).

#[cfg(not(feature = "demo"))]
use drift_core::LayerId;
use drift_core::{
    Appearance, Block, BlockId, BlockItem, DVec2, Instance, InstanceId, LineKind, LineType,
    LineWeight, Rgb, Shape,
};
use serde::{Deserialize, Serialize};

use crate::{LoadError, Style, parse_layer_id, parse_prefixed};

fn is_false(b: &bool) -> bool {
    !*b
}

/// One curve of a block definition (angles in degrees).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum ItemShape {
    Point {
        at: [f64; 2],
    },
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// A cubic Bézier: its four points.
    Bezier {
        p: [[f64; 2]; 4],
    },
    /// `major` is the first semi-axis as a vector; angles are parametric, in degrees.
    Ellipse {
        center: [f64; 2],
        major: [f64; 2],
        minor: f64,
        start: f64,
        sweep: f64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ItemRecord {
    #[serde(flatten)]
    shape: ItemShape,
    #[serde(default, skip_serializing_if = "Style::is_solid")]
    style: Style,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    linetype: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BlockRecord {
    pub(crate) id: String,
    name: String,
    items: Vec<ItemRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fills: Vec<crate::drafting::FillRecord>,
    /// Written to the Blender profiles file.
    #[serde(default, skip_serializing_if = "is_false")]
    profile: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct InstanceRecord {
    pub(crate) id: String,
    block: String,
    at: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    mirror: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<String>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn block_record(id: BlockId, b: &Block) -> BlockRecord {
    let items = b
        .items
        .iter()
        .map(|item| {
            let shape = match item.shape {
                Shape::Point(p) => ItemShape::Point { at: p.to_array() },
                Shape::Line { a, b } => ItemShape::Line {
                    a: a.to_array(),
                    b: b.to_array(),
                },
                Shape::Circle { center, radius } => ItemShape::Circle {
                    center: center.to_array(),
                    radius,
                },
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => ItemShape::Arc {
                    center: center.to_array(),
                    radius,
                    start: start.to_degrees(),
                    sweep: sweep.to_degrees(),
                },
                Shape::Bezier { p } => ItemShape::Bezier {
                    p: p.map(|q| q.to_array()),
                },
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    start,
                    sweep,
                } => ItemShape::Ellipse {
                    center: center.to_array(),
                    major: major.to_array(),
                    minor,
                    start: start.to_degrees(),
                    sweep: sweep.to_degrees(),
                },
            };
            let a = item.appearance;
            ItemRecord {
                shape,
                style: item.style.into(),
                color: a.color.map(Rgb::hex),
                weight: a.weight.map(LineWeight::mm),
                linetype: a.line_type.map(|t| t.name().to_owned()),
            }
        })
        .collect();
    BlockRecord {
        id: id.to_string(),
        name: b.name.clone(),
        items,
        fills: b
            .fills
            .iter()
            .map(crate::drafting::block_fill_record)
            .collect(),
        profile: b.profile,
    }
}

pub(crate) fn parse_block(r: BlockRecord) -> Result<(BlockId, Block), LoadError> {
    let bad = |field| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    let finite = |v: [f64; 2]| {
        let p = DVec2::from_array(v);
        p.is_finite()
            .then_some(p)
            .ok_or(LoadError::NonFinite(r.id.clone()))
    };
    let mut items = Vec::new();
    for item in &r.items {
        let shape = match item.shape {
            ItemShape::Point { at } => Shape::Point(finite(at)?),
            ItemShape::Line { a, b } => Shape::Line {
                a: finite(a)?,
                b: finite(b)?,
            },
            ItemShape::Circle { center, radius } if radius > 0.0 && radius.is_finite() => {
                Shape::Circle {
                    center: finite(center)?,
                    radius,
                }
            }
            ItemShape::Arc {
                center,
                radius,
                start,
                sweep,
            } if radius > 0.0 && radius.is_finite() && start.is_finite() && sweep.is_finite() => {
                Shape::Arc {
                    center: finite(center)?,
                    radius,
                    start: start.to_radians(),
                    sweep: sweep.to_radians(),
                }
            }
            ItemShape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } if minor > 0.0 && minor.is_finite() && start.is_finite() && sweep.is_finite() => {
                Shape::Ellipse {
                    center: finite(center)?,
                    major: finite(major)?,
                    minor,
                    start: start.to_radians(),
                    sweep: sweep.to_radians(),
                }
            }
            ItemShape::Bezier { p } => Shape::Bezier {
                p: [finite(p[0])?, finite(p[1])?, finite(p[2])?, finite(p[3])?],
            },
            _ => return Err(bad("radius")),
        };
        let appearance = Appearance {
            color: match &item.color {
                Some(c) => Some(Rgb::parse(c).ok_or_else(|| bad("color"))?),
                None => None,
            },
            weight: item.weight.map(LineWeight::nearest),
            line_type: match &item.linetype {
                Some(t) => Some(LineType::from_name(t).ok_or_else(|| bad("linetype"))?),
                None => None,
            },
        };
        items.push(BlockItem {
            shape,
            style: LineKind::from(item.style),
            appearance,
        });
    }
    let fills = r
        .fills
        .into_iter()
        .map(|f| crate::drafting::parse_block_fill(f, &r.id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        BlockId(parse_prefixed(&r.id, 'b')?),
        Block {
            name: r.name,
            items,
            fills,
            profile: r.profile,
        },
    ))
}

#[cfg(not(feature = "demo"))]
pub(crate) fn instance_record(id: InstanceId, i: &Instance) -> InstanceRecord {
    InstanceRecord {
        id: id.to_string(),
        block: i.block.to_string(),
        at: i.at.to_array(),
        angle: (i.angle != 0.0).then_some(i.angle),
        scale: (i.scale != 1.0).then_some(i.scale),
        mirror: i.mirror,
        layer: (i.layer != LayerId::DEFAULT).then(|| i.layer.to_string()),
    }
}

pub(crate) fn parse_instance(r: InstanceRecord) -> Result<(InstanceId, Instance), LoadError> {
    let mut i = Instance::new(
        BlockId(parse_prefixed(&r.block, 'b')?),
        DVec2::from_array(r.at),
    );
    i.angle = r.angle.unwrap_or(0.0);
    i.scale = r.scale.unwrap_or(1.0);
    i.mirror = r.mirror;
    if let Some(l) = &r.layer {
        i.layer = parse_layer_id(l)?;
    }
    Ok((InstanceId(parse_prefixed(&r.id, 'i')?), i))
}
