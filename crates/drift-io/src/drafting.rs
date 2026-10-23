//! File records for drafting content (format 7): fills and hatches, annotations, dimension
//! prefixes / suffixes / tolerances, and the drafting style.

use drift_core::{
    Anchor, Annotation, AnnotationId, AnnotationRef, ArrowStyle, DVec2, DimNote, Document,
    DocumentError, DraftStyle, EntityId, Fill, FillId, FillKind, Hatch, LayerId, Leader, LeaderEnd,
    LeaderShape, LineWeight, NoteFrame, NoteStyle, OrdinateAxis, Rgb, Shown, Standard,
    StyleOverride, TextAlign, Tolerance,
};
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "demo"))]
use crate::id_string;
use crate::{LENGTH_FORMATS, LoadError, parse_id, parse_layer_id, parse_prefixed};

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
    /// (A block's fills have none.)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    /// An edit left it covering more than one area (v37).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    divided: bool,
    /// The path (a shape drawn as one) it fills the inside of (v38).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn fill_record(doc: &Document, id: FillId, f: &Fill) -> FillRecord {
    let mut r = style_record(f);
    r.id = id.to_string();
    // Curves deleted since are dropped: their ids could be reused.
    r.boundary = f
        .boundary
        .iter()
        .filter(|e| doc.contains(**e))
        .map(|e| id_string(*e))
        .collect();
    r.path = f
        .path
        .filter(|p| doc.path(*p).is_some())
        .map(|p| p.to_string());
    r
}

#[cfg(not(feature = "demo"))]
/// A block's fill: its seed and style.
pub(crate) fn block_fill_record(f: &Fill) -> FillRecord {
    let mut r = style_record(f);
    r.layer = None;
    r.order = 0;
    r.divided = false;
    // It filled a shape (v41).
    r.path = f.path.map(|_| "shape".to_owned());
    r
}

#[cfg(not(feature = "demo"))]
fn style_record(f: &Fill) -> FillRecord {
    let default = Fill::new(Vec::new(), DVec2::ZERO, FillKind::Solid);
    let mut r = FillRecord {
        id: String::new(),
        kind: "solid".into(),
        boundary: Vec::new(),
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
        divided: f.divided,
        path: None,
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
    Ok((id, parse_fill_body(r)?))
}

/// A block's fill.
pub(crate) fn parse_block_fill(mut r: FillRecord, block: &str) -> Result<Fill, LoadError> {
    if r.id.is_empty() {
        r.id = block.to_owned();
    }
    let shape = r.path.take().is_some();
    let mut fill = parse_fill_body(r)?;
    fill.boundary.clear();
    fill.layer = LayerId::DEFAULT;
    fill.divided = false;
    fill.path = shape.then_some(drift_core::blocks::SHAPE_FILL);
    Ok(fill)
}

fn parse_fill_body(r: FillRecord) -> Result<Fill, LoadError> {
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
    fill.divided = r.divided;
    fill.path = match &r.path {
        Some(p) => Some(drift_core::PathId(crate::parse_prefixed(p, 'p')?)),
        None => None,
    };
    Ok(fill)
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
    /// A bent leader's bend, from its tip (v24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bend: Option<[f64; 2]>,
    /// A curved leader's bow, as a fraction of its length (v24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bulge: Option<f64>,
    /// Without a shoulder into the text (v24); absent has one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    no_shoulder: bool,
    /// The shoulder's length, paper mm (v24); absent is the usual length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shoulder: Option<f64>,
}

#[cfg(not(feature = "demo"))]
fn leader_record(l: &Leader) -> LeaderRecord {
    LeaderRecord {
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
        bend: match l.shape {
            LeaderShape::Bent(b) => Some(b.to_array()),
            _ => None,
        },
        bulge: match l.shape {
            LeaderShape::Curved(k) => Some(k),
            _ => None,
        },
        no_shoulder: !l.shoulder,
        shoulder: (l.shoulder_len != Leader::SHOULDER).then_some(l.shoulder_len),
    }
}

fn parse_leader(id: &str, l: &LeaderRecord) -> Result<Leader, LoadError> {
    let missing = |field| LoadError::MissingField {
        id: id.to_owned(),
        field,
    };
    let finite = |v: [f64; 2]| {
        let p = DVec2::from_array(v);
        p.is_finite()
            .then_some(p)
            .ok_or_else(|| LoadError::NonFinite(id.to_owned()))
    };
    let to = match (l.to.at, &l.to.point, &l.to.curve) {
        (Some(p), None, None) => Anchor::Free(finite(p)?),
        (None, Some(p), None) => Anchor::Point(parse_id(p)?),
        (None, None, Some(c)) => Anchor::Curve {
            curve: parse_id(c)?,
            t: l.to.t.ok_or_else(|| missing("t"))?,
        },
        _ => return Err(missing("to")),
    };
    let end = match &l.end {
        Some(e) => LeaderEnd::parse(e).ok_or_else(|| missing("end"))?,
        None => LeaderEnd::Arrow,
    };
    let shape = match (l.bend, l.bulge) {
        (Some(b), _) => LeaderShape::Bent(finite(b)?),
        (None, Some(k)) if k.is_finite() => LeaderShape::Curved(k.clamp(-1.0, 1.0)),
        (None, Some(_)) => return Err(LoadError::NonFinite(id.to_owned())),
        (None, None) => LeaderShape::Straight,
    };
    let shoulder_len = match l.shoulder {
        Some(len) if len.is_finite() && len >= 0.0 => len.min(100.0),
        Some(_) => return Err(missing("shoulder")),
        None => Leader::SHOULDER,
    };
    Ok(Leader {
        to,
        end,
        shape,
        shoulder: !l.no_shoulder,
        shoulder_len,
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AnnotationRecord {
    Note {
        id: String,
        text: String,
        at: [f64; 2],
        /// The one leader notes had before v24 (read, not written).
        #[serde(default, skip_serializing)]
        leader: Option<Box<LeaderRecord>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        leaders: Vec<LeaderRecord>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<String>,
        /// Wrap width, paper mm.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wrap: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        mask: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        bold: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        italic: bool,
        /// Text height, paper mm.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
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
    Section {
        id: String,
        a: [f64; 2],
        b: [f64; 2],
        label: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sheet: String,
        #[serde(default, skip_serializing_if = "is_false")]
        flip: bool,
        /// The view the mark points at (`w3`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<String>,
    },
    Elevation {
        id: String,
        at: [f64; 2],
        angle: f64,
        label: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sheet: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<String>,
    },
}

#[cfg(not(feature = "demo"))]
pub(crate) fn annotation_record(id: AnnotationId, a: &Annotation) -> AnnotationRecord {
    let id = id.to_string();
    match a {
        Annotation::Note {
            text,
            at,
            leaders,
            style,
        } => AnnotationRecord::Note {
            id,
            text: text.clone(),
            at: at.to_array(),
            leader: None,
            leaders: leaders.iter().map(leader_record).collect(),
            align: (style.align != TextAlign::Left).then(|| style.align.name().to_owned()),
            wrap: style.wrap,
            frame: (style.frame != NoteFrame::None).then(|| style.frame.key().to_owned()),
            mask: style.mask,
            bold: style.bold,
            italic: style.italic,
            height: style.height,
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
        Annotation::Section {
            a,
            b,
            label,
            sheet,
            flip,
            view,
        } => AnnotationRecord::Section {
            id,
            a: a.to_array(),
            b: b.to_array(),
            label: label.clone(),
            sheet: sheet.clone(),
            flip: *flip,
            view: view.map(|v| v.to_string()),
        },
        Annotation::Elevation {
            at,
            angle,
            label,
            sheet,
            view,
        } => AnnotationRecord::Elevation {
            id,
            at: at.to_array(),
            angle: *angle,
            label: label.clone(),
            sheet: sheet.clone(),
            view: view.map(|v| v.to_string()),
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
            leaders,
            align,
            wrap,
            frame,
            mask,
            bold,
            italic,
            height,
        } => {
            let leaders = leader
                .as_deref()
                .into_iter()
                .chain(&leaders)
                .map(|l| parse_leader(&id, l))
                .collect::<Result<Vec<_>, _>>()?;
            let positive = |v: Option<f64>, field| match v {
                Some(v) if v.is_finite() && v > 0.0 => Ok(Some(v)),
                Some(_) => Err(missing(&id, field)),
                None => Ok(None),
            };
            let style = NoteStyle {
                align: match &align {
                    Some(a) => TextAlign::ALL
                        .into_iter()
                        .find(|x| x.name() == a)
                        .ok_or_else(|| missing(&id, "align"))?,
                    None => TextAlign::Left,
                },
                wrap: positive(wrap, "wrap")?,
                frame: match &frame {
                    Some(f) => NoteFrame::parse(f).ok_or_else(|| missing(&id, "frame"))?,
                    None => NoteFrame::None,
                },
                mask,
                bold,
                italic,
                height: positive(height, "height")?,
            };
            let at = finite(&id, at)?;
            (
                id,
                Annotation::Note {
                    text,
                    at,
                    leaders,
                    style,
                },
            )
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
        AnnotationRecord::Section {
            id,
            a,
            b,
            label,
            sheet,
            flip,
            view,
        } => {
            let (a, b) = (finite(&id, a)?, finite(&id, b)?);
            let view = parse_view_link(view.as_deref())?;
            (
                id,
                Annotation::Section {
                    a,
                    b,
                    label,
                    sheet,
                    flip,
                    view,
                },
            )
        }
        AnnotationRecord::Elevation {
            id,
            at,
            angle,
            label,
            sheet,
            view,
        } => {
            let view = parse_view_link(view.as_deref())?;
            let at = finite(&id, at)?;
            if !angle.is_finite() {
                return Err(LoadError::NonFinite(id));
            }
            (
                id,
                Annotation::Elevation {
                    at,
                    angle,
                    label,
                    sheet,
                    view,
                },
            )
        }
    };
    Ok((AnnotationId(parse_prefixed(&id, 'n')?), a))
}

/// A mark's link to a view (`w3`); whether the view exists is checked once views are read.
fn parse_view_link(view: Option<&str>) -> Result<Option<drift_core::ViewId>, LoadError> {
    view.map(|v| parse_prefixed(v, 'w').map(drift_core::ViewId))
        .transpose()
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

#[cfg(not(feature = "demo"))]
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
    below: Option<String>,
    text: Option<String>,
) -> Option<DimNote> {
    let note = DimNote {
        prefix: prefix.unwrap_or_default(),
        suffix: suffix.unwrap_or_default(),
        tolerance: parse_tolerance(tol),
        below: below.unwrap_or_default(),
        text: text.unwrap_or_default(),
    };
    (!note.is_empty()).then_some(note)
}

/// Where a dimension (`c…`) or annotation (`n…`) is shown, when not everywhere.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShownRecord {
    of: String,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    canvas: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    sheets: bool,
    /// The views it is drawn in, if only some (v31).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    views: Vec<String>,
}

fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_yes(b: &bool) -> bool {
    *b
}

#[cfg(not(feature = "demo"))]
pub(crate) fn shown_record(
    r: AnnotationRef,
    s: Shown,
    views: Option<&std::collections::BTreeSet<drift_core::ViewId>>,
) -> ShownRecord {
    ShownRecord {
        of: match r {
            AnnotationRef::Dimension(id) => id.to_string(),
            AnnotationRef::Annotation(id) => id.to_string(),
        },
        canvas: s.canvas,
        sheets: s.sheets,
        views: views
            .into_iter()
            .flatten()
            .map(ToString::to_string)
            .collect(),
    }
}

/// Where a dimension or annotation is shown, and the views it is drawn in (empty: every
/// view).
pub(crate) type ParsedShown = (AnnotationRef, Shown, Vec<drift_core::ViewId>);

pub(crate) fn parse_shown(r: ShownRecord) -> Result<ParsedShown, LoadError> {
    let views = r
        .views
        .iter()
        .map(|v| parse_prefixed(v, 'w').map(drift_core::ViewId))
        .collect::<Result<Vec<_>, _>>()?;
    let of = if r.of.starts_with('c') {
        AnnotationRef::Dimension(drift_core::ConstraintId(parse_prefixed(&r.of, 'c')?))
    } else {
        AnnotationRef::Annotation(AnnotationId(parse_prefixed(&r.of, 'n')?))
    };
    Ok((
        of,
        Shown {
            canvas: r.canvas,
            sheets: r.sheets,
        },
        views,
    ))
}

/// What a dimension (`c…`) or annotation (`n…`) sets for itself over the drawing's style:
/// only the settings it overrides. `alt_units` and `color` take `"none"` for "none, whatever
/// the drawing has".
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StyleRecord {
    of: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arrow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arrow_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extension_gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extension_overshoot: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aligned_text: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_zeros: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    leading_zero: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alt_units: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn override_record(r: AnnotationRef, s: &StyleOverride) -> StyleRecord {
    StyleRecord {
        of: match r {
            AnnotationRef::Dimension(id) => id.to_string(),
            AnnotationRef::Annotation(id) => id.to_string(),
        },
        text_height: s.text_height,
        arrow: s.arrow.map(|a| a.key().to_owned()),
        arrow_size: s.arrow_size,
        extension_gap: s.extension_gap,
        extension_overshoot: s.extension_overshoot,
        aligned_text: s.aligned_text,
        trailing_zeros: s.trailing_zeros,
        leading_zero: s.leading_zero,
        alt_units: s.alt_units.map(|u| match u {
            None => "none".to_owned(),
            Some(u) => LENGTH_FORMATS
                .iter()
                .find(|(f, _)| *f == u)
                .map_or_else(|| "none".to_owned(), |(_, n)| (*n).to_owned()),
        }),
        color: s
            .color
            .map(|c| c.map_or_else(|| "none".to_owned(), Rgb::hex)),
    }
}

pub(crate) fn parse_override(r: StyleRecord) -> Result<(AnnotationRef, StyleOverride), LoadError> {
    let of = if r.of.starts_with('c') {
        AnnotationRef::Dimension(drift_core::ConstraintId(parse_prefixed(&r.of, 'c')?))
    } else {
        AnnotationRef::Annotation(AnnotationId(parse_prefixed(&r.of, 'n')?))
    };
    let bad = |field| LoadError::MissingField {
        id: r.of.clone(),
        field,
    };
    let size = |v: Option<f64>, field, min: f64| match v {
        Some(v) if !(v.is_finite() && v >= min) => Err(bad(field)),
        v => Ok(v),
    };
    Ok((
        of,
        StyleOverride {
            text_height: size(r.text_height, "text_height", f64::MIN_POSITIVE)?,
            arrow: match &r.arrow {
                Some(a) => Some(ArrowStyle::parse(a).ok_or_else(|| bad("arrow"))?),
                None => None,
            },
            arrow_size: size(r.arrow_size, "arrow_size", f64::MIN_POSITIVE)?,
            extension_gap: size(r.extension_gap, "extension_gap", 0.0)?,
            extension_overshoot: size(r.extension_overshoot, "extension_overshoot", 0.0)?,
            aligned_text: r.aligned_text,
            trailing_zeros: r.trailing_zeros,
            leading_zero: r.leading_zero,
            alt_units: match r.alt_units.as_deref() {
                None => None,
                Some("none") => Some(None),
                Some(u) => Some(Some(
                    LENGTH_FORMATS
                        .iter()
                        .find(|(_, n)| *n == u)
                        .map(|(f, _)| *f)
                        .ok_or_else(|| bad("alt_units"))?,
                )),
            },
            color: match r.color.as_deref() {
                None => None,
                Some("none") => Some(None),
                Some(c) => Some(Some(Rgb::parse(c).ok_or_else(|| bad("color"))?)),
            },
        },
    ))
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

#[cfg(not(feature = "demo"))]
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
        color: s.color.map(Rgb::hex),
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
        color: match &r.color {
            Some(c) => Some(Rgb::parse(c).ok_or_else(|| bad("color"))?),
            None => None,
        },
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
