//! File records for drafting content (format 7): fills and hatches, annotations, dimension
//! prefixes / suffixes / tolerances, and the drafting style.

use drift_core::{
    Anchor, Annotation, AnnotationId, ArrowStyle, DVec2, DimNote, Document, DocumentError,
    DraftStyle, EntityId, Fill, FillId, FillKind, Hatch, LayerId, Leader, LeaderEnd, LineWeight,
    OrdinateAxis, Rgb, Standard, Tolerance,
};
use serde::{Deserialize, Serialize};

use crate::{LENGTH_FORMATS, LoadError, id_string, parse_id, parse_layer_id, parse_prefixed};

fn is_zero(v: &i32) -> bool {
    *v == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn parse_color(id: &str, c: &Option<String>) -> Result<Option<Rgb>, LoadError> {
    match c {
        Some(c) => Rgb::parse(c).map(Some).ok_or(LoadError::MissingField {
            id: id.to_owned(),
            field: "color",
        }),
        None => Ok(None),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FillRecord {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    boundary: Vec<String>,
    seed: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "is_false")]
    model: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opacity: Option<f64>,
    /// Hatch line weight in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    order: i32,
}

pub(crate) fn fill_record(doc: &Document, id: FillId, f: &Fill) -> FillRecord {
    let default = Fill::new(Vec::new(), DVec2::ZERO, FillKind::Solid);
    let mut r = FillRecord {
        id: id.to_string(),
        kind: "solid".into(),
        // Curves deleted since are dropped: their ids could be reused.
        boundary: f
            .boundary
            .iter()
            .filter(|e| doc.contains(**e))
            .map(|e| id_string(*e))
            .collect(),
        seed: f.seed.to_array(),
        pattern: None,
        scale: None,
        angle: None,
        origin: None,
        model: false,
        background: None,
        layer: (f.layer != LayerId::DEFAULT).then(|| f.layer.to_string()),
        color: f.color.map(Rgb::hex),
        opacity: None,
        weight: f.weight.map(LineWeight::mm),
        order: f.order,
    };
    match &f.kind {
        FillKind::Solid => {
            r.opacity = (f.opacity != default.opacity).then_some(f.opacity);
        }
        FillKind::Hatch(h) => {
            r.kind = "hatch".into();
            r.pattern = Some(h.pattern.clone());
            r.scale = (h.scale != 1.0).then_some(h.scale);
            r.angle = (h.angle != 0.0).then_some(h.angle);
            r.origin = (h.origin != DVec2::ZERO).then(|| h.origin.to_array());
            r.model = h.model;
            r.background = h.background.map(Rgb::hex);
        }
    }
    r
}

pub(crate) fn parse_fill(r: FillRecord) -> Result<(FillId, Fill), LoadError> {
    let id = FillId(parse_prefixed(&r.id, 'f')?);
    let boundary: Result<Vec<EntityId>, _> = r.boundary.iter().map(|b| parse_id(b)).collect();
    let seed = DVec2::from_array(r.seed);
    if !seed.is_finite() {
        return Err(LoadError::NonFinite(r.id));
    }
    let kind = match r.kind.as_str() {
        "solid" => FillKind::Solid,
        "hatch" => FillKind::Hatch(Hatch {
            pattern: r.pattern.clone().ok_or(LoadError::MissingField {
                id: r.id.clone(),
                field: "pattern",
            })?,
            scale: r.scale.unwrap_or(1.0),
            angle: r.angle.unwrap_or(0.0),
            origin: r.origin.map_or(DVec2::ZERO, DVec2::from_array),
            model: r.model,
            background: parse_color(&r.id, &r.background)?,
        }),
        other => return Err(LoadError::UnknownConstraint(format!("fill {other}"))),
    };
    let mut fill = Fill::new(boundary?, seed, kind);
    if let Some(l) = &r.layer {
        fill.layer = parse_layer_id(l)?;
    }
    fill.color = parse_color(&r.id, &r.color)?;
    if let Some(o) = r.opacity {
        fill.opacity = o.clamp(0.0, 1.0);
    }
    fill.weight = r.weight.map(LineWeight::nearest);
    fill.order = r.order;
    Ok((id, fill))
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnchorRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LeaderRecord {
    to: AnchorRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AnnotationRecord {
    Note {
        id: String,
        text: String,
        at: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leader: Option<LeaderRecord>,
    },
    CentreMark {
        id: String,
        curve: String,
    },
    Centreline {
        id: String,
        a: String,
        b: String,
    },
    HoleCallout {
        id: String,
        curve: String,
        at: [f64; 2],
        #[serde(default, skip_serializing_if = "String::is_empty")]
        suffix: String,
    },
    ChamferCallout {
        id: String,
        line: String,
        at: [f64; 2],
    },
    Ordinate {
        id: String,
        origin: String,
        point: String,
        axis: String,
        at: f64,
    },
}

pub(crate) fn annotation_record(id: AnnotationId, a: &Annotation) -> AnnotationRecord {
    let id = id.to_string();
    match a {
        Annotation::Note { text, at, leader } => AnnotationRecord::Note {
            id,
            text: text.clone(),
            at: at.to_array(),
            leader: leader.map(|l| LeaderRecord {
                to: match l.to {
                    Anchor::Free(p) => AnchorRecord {
                        at: Some(p.to_array()),
                        ..Default::default()
                    },
                    Anchor::Point(e) => AnchorRecord {
                        point: Some(id_string(e)),
                        ..Default::default()
                    },
                    Anchor::Curve { curve, t } => AnchorRecord {
                        curve: Some(id_string(curve)),
                        t: Some(t),
                        ..Default::default()
                    },
                },
                end: (l.end != LeaderEnd::Arrow).then(|| l.end.key().to_owned()),
            }),
        },
        Annotation::CentreMark { curve } => AnnotationRecord::CentreMark {
            id,
            curve: id_string(*curve),
        },
        Annotation::Centreline { a, b } => AnnotationRecord::Centreline {
            id,
            a: id_string(*a),
            b: id_string(*b),
        },
        Annotation::HoleCallout { curve, at, suffix } => AnnotationRecord::HoleCallout {
            id,
            curve: id_string(*curve),
            at: at.to_array(),
            suffix: suffix.clone(),
        },
        Annotation::ChamferCallout { line, at } => AnnotationRecord::ChamferCallout {
            id,
            line: id_string(*line),
            at: at.to_array(),
        },
        Annotation::Ordinate {
            origin,
            point,
            axis,
            at,
        } => AnnotationRecord::Ordinate {
            id,
            origin: id_string(*origin),
            point: id_string(*point),
            axis: match axis {
                OrdinateAxis::X => "x",
                OrdinateAxis::Y => "y",
            }
            .into(),
            at: *at,
        },
    }
}

pub(crate) fn parse_annotation(
    r: AnnotationRecord,
) -> Result<(AnnotationId, Annotation), LoadError> {
    let missing = |id: &str, field| LoadError::MissingField {
        id: id.to_owned(),
        field,
    };
    let finite = |id: &str, v: [f64; 2]| {
        let p = DVec2::from_array(v);
        if p.is_finite() {
            Ok(p)
        } else {
            Err(LoadError::NonFinite(id.to_owned()))
        }
    };
    let (id, a) = match r {
        AnnotationRecord::Note {
            id,
            text,
            at,
            leader,
        } => {
            let leader = match leader {
                None => None,
                Some(l) => {
                    let to = match (l.to.at, &l.to.point, &l.to.curve) {
                        (Some(p), None, None) => Anchor::Free(finite(&id, p)?),
                        (None, Some(p), None) => Anchor::Point(parse_id(p)?),
                        (None, None, Some(c)) => Anchor::Curve {
                            curve: parse_id(c)?,
                            t: l.to.t.ok_or_else(|| missing(&id, "t"))?,
                        },
                        _ => return Err(missing(&id, "to")),
                    };
                    let end = match &l.end {
                        Some(e) => LeaderEnd::parse(e).ok_or_else(|| missing(&id, "end"))?,
                        None => LeaderEnd::Arrow,
                    };
                    Some(Leader { to, end })
                }
            };
            let at = finite(&id, at)?;
            (id, Annotation::Note { text, at, leader })
        }
        AnnotationRecord::CentreMark { id, curve } => {
            let curve = parse_id(&curve)?;
            (id, Annotation::CentreMark { curve })
        }
        AnnotationRecord::Centreline { id, a, b } => {
            let (a, b) = (parse_id(&a)?, parse_id(&b)?);
            (id, Annotation::Centreline { a, b })
        }
        AnnotationRecord::HoleCallout {
            id,
            curve,
            at,
            suffix,
        } => {
            let (curve, at) = (parse_id(&curve)?, finite(&id, at)?);
            (id, Annotation::HoleCallout { curve, at, suffix })
        }
        AnnotationRecord::ChamferCallout { id, line, at } => {
            let (line, at) = (parse_id(&line)?, finite(&id, at)?);
            (id, Annotation::ChamferCallout { line, at })
        }
        AnnotationRecord::Ordinate {
            id,
            origin,
            point,
            axis,
            at,
        } => {
            let axis = match axis.as_str() {
                "x" => OrdinateAxis::X,
                "y" => OrdinateAxis::Y,
                _ => return Err(missing(&id, "axis")),
            };
            if !at.is_finite() {
                return Err(LoadError::NonFinite(id));
            }
            let (origin, point) = (parse_id(&origin)?, parse_id(&point)?);
            (
                id,
                Annotation::Ordinate {
                    origin,
                    point,
                    axis,
                    at,
                },
            )
        }
    };
    Ok((AnnotationId(parse_prefixed(&id, 'n')?), a))
}

/// A dimension's tolerance.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ToleranceRecord {
    Symmetric { value: f64 },
    Deviation { upper: f64, lower: f64 },
    Limits { upper: f64, lower: f64 },
    Fit { fit: String },
}

pub(crate) fn tolerance_record(t: &Tolerance) -> Option<ToleranceRecord> {
    Some(match t {
        Tolerance::None => return None,
        Tolerance::Symmetric(v) => ToleranceRecord::Symmetric { value: *v },
        Tolerance::Deviation { upper, lower } => ToleranceRecord::Deviation {
            upper: *upper,
            lower: *lower,
        },
        Tolerance::Limits { upper, lower } => ToleranceRecord::Limits {
            upper: *upper,
            lower: *lower,
        },
        Tolerance::Fit(f) => ToleranceRecord::Fit { fit: f.clone() },
    })
}

pub(crate) fn parse_tolerance(t: Option<ToleranceRecord>) -> Tolerance {
    match t {
        None => Tolerance::None,
        Some(ToleranceRecord::Symmetric { value }) => Tolerance::Symmetric(value),
        Some(ToleranceRecord::Deviation { upper, lower }) => Tolerance::Deviation { upper, lower },
        Some(ToleranceRecord::Limits { upper, lower }) => Tolerance::Limits { upper, lower },
        Some(ToleranceRecord::Fit { fit }) => Tolerance::Fit(fit),
    }
}

pub(crate) fn dim_note(
    prefix: Option<String>,
    suffix: Option<String>,
    tol: Option<ToleranceRecord>,
) -> Option<DimNote> {
    let note = DimNote {
        prefix: prefix.unwrap_or_default(),
        suffix: suffix.unwrap_or_default(),
        tolerance: parse_tolerance(tol),
    };
    (!note.is_empty()).then_some(note)
}

/// The drafting style; written in full when it isn't the default.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DraftRecord {
    standard: String,
    text_height: f64,
    arrow: String,
    arrow_size: f64,
    extension_gap: f64,
    extension_overshoot: f64,
    aligned_text: bool,
    trailing_zeros: bool,
    leading_zero: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alt_units: Option<String>,
    scale: f64,
}

pub(crate) fn draft_record(s: &DraftStyle) -> Option<DraftRecord> {
    (*s != DraftStyle::default()).then(|| DraftRecord {
        standard: match s.standard {
            Standard::Iso => "iso",
            Standard::Asme => "asme",
        }
        .into(),
        text_height: s.text_height,
        arrow: s.arrow.key().into(),
        arrow_size: s.arrow_size,
        extension_gap: s.extension_gap,
        extension_overshoot: s.extension_overshoot,
        aligned_text: s.aligned_text,
        trailing_zeros: s.trailing_zeros,
        leading_zero: s.leading_zero,
        alt_units: s.alt_units.and_then(|u| {
            LENGTH_FORMATS
                .iter()
                .find(|(f, _)| *f == u)
                .map(|(_, n)| (*n).to_owned())
        }),
        scale: s.scale,
    })
}

pub(crate) fn parse_draft(r: DraftRecord) -> Result<DraftStyle, LoadError> {
    let bad = |field| LoadError::MissingField {
        id: "drafting".into(),
        field,
    };
    let positive = |v: f64, field| {
        if v.is_finite() && v > 0.0 {
            Ok(v)
        } else {
            Err(bad(field))
        }
    };
    let non_negative = |v: f64, field| {
        if v.is_finite() && v >= 0.0 {
            Ok(v)
        } else {
            Err(bad(field))
        }
    };
    Ok(DraftStyle {
        standard: match r.standard.as_str() {
            "iso" => Standard::Iso,
            "asme" => Standard::Asme,
            _ => return Err(bad("standard")),
        },
        text_height: positive(r.text_height, "text_height")?,
        arrow: ArrowStyle::parse(&r.arrow).ok_or_else(|| bad("arrow"))?,
        arrow_size: positive(r.arrow_size, "arrow_size")?,
        extension_gap: non_negative(r.extension_gap, "extension_gap")?,
        extension_overshoot: non_negative(r.extension_overshoot, "extension_overshoot")?,
        aligned_text: r.aligned_text,
        trailing_zeros: r.trailing_zeros,
        leading_zero: r.leading_zero,
        alt_units: match &r.alt_units {
            Some(u) => Some(
                LENGTH_FORMATS
                    .iter()
                    .find(|(_, n)| n == u)
                    .map(|(f, _)| *f)
                    .ok_or_else(|| bad("alt_units"))?,
            ),
            None => None,
        },
        scale: positive(r.scale, "scale")?,
    })
}

/// Add loaded fills and annotations to a document (after its entities and layers).
pub(crate) fn insert_all(
    doc: &mut Document,
    fills: Vec<FillRecord>,
    annotations: Vec<AnnotationRecord>,
) -> Result<(), LoadError> {
    for r in fills {
        let id_text = r.id.clone();
        let (id, fill) = parse_fill(r)?;
        if doc.fill(id).is_some() {
            return Err(LoadError::DuplicateId(id_text));
        }
        doc.insert_fill(id, fill).map_err(LoadError::Document)?;
    }
    for r in annotations {
        let (id, a) = parse_annotation(r)?;
        if doc.annotation(id).is_some() {
            return Err(LoadError::DuplicateId(id.to_string()));
        }
        doc.insert_annotation(id, a)
            .map_err(|e: DocumentError| LoadError::Document(e))?;
    }
    Ok(())
}
