//! File records for sheets (format 8), views (format 27) and a sheet's own annotations
//! (format 45).

#[cfg(not(feature = "demo"))]
use drift_core::LayerId;
use drift_core::{
    Bounds, DVec2, PaperSize, Revision, Sheet, SheetColours, SheetId, TitleBlock, TitleFields,
};
use drift_core::{
    Constraint, NoteFrame, NoteStyle, PortId, SheetAnnotation, SheetAnnotationId, SheetLeader,
    StyleOverride, TextAlign,
};
use drift_core::{FieldSet, Project, View, ViewFrame, ViewId, Viewport};
use serde::{Deserialize, Serialize};

use crate::drafting::{LeaderRecord, StyleRecord, ToleranceRecord};
use crate::{LoadError, parse_prefixed};

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct FieldsRecord {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    checked: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    company: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    material: String,
    /// What the sheet is issued for (v30).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    status: String,
}

/// The project the sheets share (v30).
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct ProjectRecord {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    client: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    company: String,
    /// `architectural`; left out for mechanical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields: Option<String>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn project_record(p: &Project) -> Option<ProjectRecord> {
    (*p != Project::default()).then(|| ProjectRecord {
        name: p.name.clone(),
        number: p.number.clone(),
        client: p.client.clone(),
        address: p.address.clone(),
        company: p.company.clone(),
        fields: (p.fields != FieldSet::Mechanical).then(|| p.fields.key().into()),
    })
}

pub(crate) fn parse_project(r: ProjectRecord) -> Result<Project, LoadError> {
    let fields = match r.fields.as_deref() {
        None => FieldSet::Mechanical,
        Some(k) => FieldSet::parse(k).ok_or_else(|| LoadError::MissingField {
            id: "project".into(),
            field: "fields",
        })?,
    };
    Ok(Project {
        name: r.name,
        number: r.number,
        client: r.client,
        address: r.address,
        company: r.company,
        fields,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RevisionRecord {
    rev: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    by: String,
}

/// A view of the model: a named crop at a scale.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ViewRecord {
    pub(crate) id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    scale: f64,
    center: [f64; 2],
    /// The crop's width and height (world mm).
    size: [f64; 2],
    /// Written only when hidden in the view.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    dimensions: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    notes: bool,
    /// Construction geometry shown in the view; written only when shown (format 32).
    #[serde(default, skip_serializing_if = "is_false")]
    construction: bool,
    /// The crop shown on the canvas; written only when hidden.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    crop: bool,
    /// Layers left out of the view (format 28).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden_layers: Vec<String>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn view_record(id: ViewId, v: &View) -> ViewRecord {
    ViewRecord {
        id: id.to_string(),
        name: v.name.clone(),
        scale: v.scale,
        center: v.center.to_array(),
        size: v.size.to_array(),
        dimensions: v.dimensions,
        notes: v.notes,
        construction: v.construction,
        crop: v.crop_visible,
        hidden_layers: v.hidden_layers.iter().map(LayerId::to_string).collect(),
    }
}

pub(crate) fn parse_view(r: ViewRecord) -> Result<(ViewId, View), LoadError> {
    let bad = |field| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    if !(r.scale.is_finite() && r.scale > 0.0) {
        return Err(bad("scale"));
    }
    let (center, size) = (DVec2::from_array(r.center), DVec2::from_array(r.size));
    if !center.is_finite() || !size.is_finite() {
        return Err(LoadError::NonFinite(r.id.clone()));
    }
    if size.min_element() <= 0.0 {
        return Err(bad("size"));
    }
    let id = ViewId(parse_prefixed(&r.id, 'w')?);
    let hidden_layers = r
        .hidden_layers
        .iter()
        .map(|l| crate::parse_layer_id(l))
        .collect::<Result<_, _>>()?;
    let view = View {
        name: r.name,
        scale: r.scale,
        center,
        size,
        dimensions: r.dimensions,
        notes: r.notes,
        construction: r.construction,
        crop_visible: r.crop,
        hidden_layers,
    };
    Ok((id, view))
}

/// A view placed on a sheet.
#[derive(Debug, Serialize, Deserialize)]
struct ViewportRecord {
    view: String,
    /// Its middle on the paper (mm).
    at: [f64; 2],
    #[serde(default, skip_serializing_if = "is_false")]
    title: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    border: bool,
    /// What the sheet's annotations name it by (`r5`), once one is tied to it (v45).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

/// A leader of a sheet note (v45): the leader as a model note's is written, and the
/// viewport its tip is seen through. Without `port`, its `to` is a paper point (`at`).
#[derive(Debug, Serialize, Deserialize)]
struct SheetLeaderRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<String>,
    leader: LeaderRecord,
}

/// What a sheet dimension measures (v45): a dimension's `type` and the entities it is on,
/// as a constraint names them, without a value.
#[derive(Debug, Serialize, Deserialize)]
struct MeasureRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<String>,
}

/// The kinds of dimension a sheet dimension can be.
const MEASURES: [&str; 8] = [
    "distance",
    "horizontal_distance",
    "vertical_distance",
    "point_line_distance",
    "radius",
    "diameter",
    "radial_gap",
    "angle",
];

/// An annotation placed on a sheet (v45): a note (text at a paper point, with leaders) or
/// a dimension of model geometry through one of the sheet's viewports.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SheetAnnotationRecord {
    Note {
        id: String,
        text: String,
        /// The text's middle on the paper (mm).
        at: [f64; 2],
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        leaders: Vec<SheetLeaderRecord>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wrap: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        mask: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
        /// Its own settings over the drafting style.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<StyleRecord>,
    },
    Dimension {
        id: String,
        /// The viewport it measures through.
        port: String,
        measure: MeasureRecord,
        /// The label's offset from the dimension's anchor (paper mm); absent while it sits
        /// where a new dimension's does.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suffix: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<ToleranceRecord>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        below: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<StyleRecord>,
    },
}

#[cfg(not(feature = "demo"))]
fn sheet_annotation_record(id: SheetAnnotationId, a: &SheetAnnotation) -> SheetAnnotationRecord {
    let look = |look: &StyleOverride| {
        (!look.is_empty()).then(|| crate::drafting::look_record(String::new(), look))
    };
    match a {
        SheetAnnotation::Note {
            text,
            at,
            leaders,
            style,
            look: own,
        } => SheetAnnotationRecord::Note {
            id: id.to_string(),
            text: text.clone(),
            at: at.to_array(),
            leaders: leaders
                .iter()
                .map(|l| SheetLeaderRecord {
                    port: l.port.map(|p| p.to_string()),
                    leader: crate::drafting::leader_record(&l.leader),
                })
                .collect(),
            align: (style.align != TextAlign::Left).then(|| style.align.name().to_owned()),
            wrap: style.wrap,
            frame: (style.frame != NoteFrame::None).then(|| style.frame.key().to_owned()),
            mask: style.mask,
            bold: style.bold,
            italic: style.italic,
            height: style.height,
            style: look(own),
        },
        SheetAnnotation::Dimension {
            port,
            measure,
            label,
            note,
            look: own,
        } => {
            let r = crate::constraint_record(drift_core::ConstraintId(1), measure, None, false);
            let some = |s: &String| (!s.is_empty()).then(|| s.clone());
            SheetAnnotationRecord::Dimension {
                id: id.to_string(),
                port: port.to_string(),
                measure: MeasureRecord {
                    kind: r.kind,
                    a: r.a,
                    b: r.b,
                    line: r.line,
                    point: r.point,
                    curve: r.curve,
                },
                label: label.map(|l| l.to_array()),
                prefix: some(&note.prefix),
                suffix: some(&note.suffix),
                tol: crate::drafting::tolerance_record(&note.tolerance),
                below: some(&note.below),
                text: some(&note.text),
                style: look(own),
            }
        }
    }
}

fn parse_sheet_annotation(
    r: SheetAnnotationRecord,
) -> Result<(SheetAnnotationId, SheetAnnotation), LoadError> {
    let missing = |id: &str, field| LoadError::MissingField {
        id: id.to_owned(),
        field,
    };
    let finite = |id: &str, v: [f64; 2]| {
        let p = DVec2::from_array(v);
        p.is_finite()
            .then_some(p)
            .ok_or_else(|| LoadError::NonFinite(id.to_owned()))
    };
    let port = |p: &str| parse_prefixed(p, 'r').map(PortId);
    let look = |id: &str, style: Option<StyleRecord>| match style {
        Some(s) => crate::drafting::parse_look(id, s),
        None => Ok(StyleOverride::default()),
    };
    let (id_text, a) = match r {
        SheetAnnotationRecord::Note {
            id,
            text,
            at,
            leaders,
            align,
            wrap,
            frame,
            mask,
            bold,
            italic,
            height,
            style,
        } => {
            let leaders = leaders
                .iter()
                .map(|l| {
                    Ok(SheetLeader {
                        leader: crate::drafting::parse_leader(&id, &l.leader)?,
                        port: l.port.as_deref().map(port).transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, LoadError>>()?;
            let positive = |v: Option<f64>, field| match v {
                Some(v) if v.is_finite() && v > 0.0 => Ok(Some(v)),
                Some(_) => Err(missing(&id, field)),
                None => Ok(None),
            };
            let note_style = NoteStyle {
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
            let a = SheetAnnotation::Note {
                text,
                at: finite(&id, at)?,
                leaders,
                style: note_style,
                look: look(&id, style)?,
            };
            (id, a)
        }
        SheetAnnotationRecord::Dimension {
            id,
            port: p,
            measure,
            label,
            prefix,
            suffix,
            tol,
            below,
            text,
            style,
        } => {
            if !MEASURES.contains(&measure.kind.as_str()) {
                return Err(missing(&id, "measure"));
            }
            // Parsed as a dimension with a placeholder value (it has none of its own).
            let angle = measure.kind == "angle";
            let rec = crate::ConstraintRecord {
                kind: measure.kind,
                id: "c1".to_owned(),
                a: measure.a,
                b: measure.b,
                line: measure.line,
                point: measure.point,
                curve: measure.curve,
                value: (!angle).then_some(1.0),
                degrees: angle.then_some(1.0),
                ..Default::default()
            };
            let (_, c, ..): (_, Constraint, _, _) =
                crate::parse_constraint(rec).map_err(|e| match e {
                    // (Named by the annotation, not the placeholder.)
                    LoadError::MissingField { field, .. } => missing(&id, field),
                    e => e,
                })?;
            let a = SheetAnnotation::Dimension {
                port: port(&p)?,
                measure: c,
                label: label.map(|l| finite(&id, l)).transpose()?,
                note: crate::drafting::dim_note(prefix, suffix, tol, below, text)
                    .unwrap_or_default(),
                look: look(&id, style)?,
            };
            (id, a)
        }
    };
    if !a.is_valid() {
        return Err(LoadError::NonFinite(id_text));
    }
    Ok((SheetAnnotationId(parse_prefixed(&id_text, 'a')?), a))
}

/// A view of the sketch on a sheet, as written from v18 to v25 (read only: each becomes a
/// model view and a viewport).
#[derive(Debug, Serialize, Deserialize)]
struct OldViewRecord {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    scale: f64,
    center: [f64; 2],
    /// Its middle and size on the paper (mm); left out while it fills the drawing area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<[f64; 2]>,
    /// Written only when hidden in the view.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    dimensions: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    notes: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    title: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    border: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SheetRecord {
    pub(crate) id: String,
    name: String,
    /// A standard size (`A3`, `ANSI-B`…), or `custom` with `size`.
    paper: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "is_false")]
    portrait: bool,
    /// Before v18 a sheet had one view, written as these four fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dimensions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes: Option<bool>,
    /// From v18 to v25, the sheet's own views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    views: Option<Vec<OldViewRecord>>,
    /// The views placed on it (v27).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    viewports: Vec<ViewportRecord>,
    title_block: String,
    #[serde(default)]
    fields: FieldsRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revisions: Vec<RevisionRecord>,
    /// `greyscale` or `mono` (black & white); left out for colour (and before v25).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colours: Option<String>,
    /// The notes, leaders and dimensions placed on the sheet (v45).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    annotations: Vec<SheetAnnotationRecord>,
}

#[cfg(not(feature = "demo"))]
pub(crate) fn sheet_record(id: SheetId, s: &Sheet) -> SheetRecord {
    let f = &s.fields;
    SheetRecord {
        id: id.to_string(),
        name: s.name.clone(),
        paper: s.paper.key(),
        size: match s.paper {
            PaperSize::Custom(w, h) => Some([w, h]),
            _ => None,
        },
        portrait: s.portrait,
        scale: None,
        center: None,
        dimensions: None,
        notes: None,
        views: None,
        viewports: s
            .viewports
            .iter()
            .map(|p| ViewportRecord {
                view: p.view.to_string(),
                at: p.at.to_array(),
                title: p.title,
                border: p.border,
                id: p.id.map(|id| id.to_string()),
            })
            .collect(),
        title_block: s.title_block.key().into(),
        fields: FieldsRecord {
            title: f.title.clone(),
            number: f.number.clone(),
            author: f.author.clone(),
            checked: f.checked.clone(),
            date: f.date.clone(),
            company: f.company.clone(),
            material: f.material.clone(),
            status: f.status.clone(),
        },
        revisions: s
            .revisions
            .iter()
            .map(|r| RevisionRecord {
                rev: r.rev.clone(),
                date: r.date.clone(),
                description: r.description.clone(),
                by: r.by.clone(),
            })
            .collect(),
        colours: (s.colours != SheetColours::Colour).then(|| s.colours.key().into()),
        annotations: s
            .annotations
            .iter()
            .map(|(id, a)| sheet_annotation_record(*id, a))
            .collect(),
    }
}

/// A view a sheet held itself before v27, to be made a model view when the file is read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OldView {
    pub name: String,
    pub scale: f64,
    pub center: DVec2,
    /// `None` filled the sheet's drawing area.
    pub frame: Option<ViewFrame>,
    pub dimensions: bool,
    pub notes: bool,
    pub title: bool,
    pub border: bool,
}

impl OldView {
    /// The model view and the viewport it becomes on `sheet`: the same crop of the model at
    /// the same place on the paper, so the sheet prints as it did. A view that filled the
    /// drawing area is cropped to it.
    pub(crate) fn upgrade(&self, sheet: &Sheet, name: String) -> (View, Viewport) {
        let frame = self.frame.unwrap_or_else(|| {
            let area = drift_draft::sheet::drawing_area(sheet);
            ViewFrame {
                at: area.center(),
                size: area.size(),
            }
        });
        let half = frame.size * self.scale * 0.5;
        let mut view = View::new(
            name,
            Bounds {
                min: self.center - half,
                max: self.center + half,
            },
            self.scale,
        );
        view.dimensions = self.dimensions;
        view.notes = self.notes;
        let port = Viewport {
            // Set when the view is added to the document.
            view: ViewId(0),
            at: frame.at,
            title: self.title,
            border: self.border,
            id: None,
        };
        (view, port)
    }
}

/// A sheet, and (from a file before v27) the views it held itself, to be made model views.
pub(crate) fn parse_sheet(r: SheetRecord) -> Result<(SheetId, Sheet, Vec<OldView>), LoadError> {
    let bad = |field| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    let paper = match (r.paper.as_str(), r.size) {
        ("custom", Some([w, h])) if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => {
            PaperSize::Custom(w, h)
        }
        (key, _) => PaperSize::parse(key).ok_or_else(|| bad("paper"))?,
    };
    let view = |scale: f64, center: [f64; 2]| -> Result<OldView, LoadError> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(bad("scale"));
        }
        let center = DVec2::from_array(center);
        if !center.is_finite() {
            return Err(LoadError::NonFinite(r.id.clone()));
        }
        Ok(OldView {
            name: String::new(),
            scale,
            center,
            frame: None,
            dimensions: true,
            notes: true,
            title: false,
            border: false,
        })
    };
    let viewports = r
        .viewports
        .iter()
        .map(|p| {
            let at = DVec2::from_array(p.at);
            if !at.is_finite() {
                return Err(LoadError::NonFinite(r.id.clone()));
            }
            Ok(Viewport {
                view: ViewId(parse_prefixed(&p.view, 'w')?),
                at,
                title: p.title,
                border: p.border,
                id: match &p.id {
                    Some(id) => Some(PortId(parse_prefixed(id, 'r')?)),
                    None => None,
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut annotations = std::collections::BTreeMap::new();
    for a in r.annotations {
        let (id, a) = parse_sheet_annotation(a)?;
        if annotations.insert(id, a).is_some() {
            return Err(LoadError::DuplicateId(id.to_string()));
        }
    }
    let old_views = match &r.views {
        Some(records) => records
            .iter()
            .map(|v| {
                let mut out = view(v.scale, v.center)?;
                out.name = v.name.clone();
                out.frame = match (v.at, v.size) {
                    (Some(at), Some(size)) => {
                        let (at, size) = (DVec2::from_array(at), DVec2::from_array(size));
                        if !at.is_finite() || !size.is_finite() || size.min_element() <= 0.0 {
                            return Err(bad("view frame"));
                        }
                        Some(ViewFrame { at, size })
                    }
                    _ => None,
                };
                out.dimensions = v.dimensions;
                out.notes = v.notes;
                out.title = v.title;
                out.border = v.border;
                Ok(out)
            })
            .collect::<Result<Vec<_>, _>>()?,
        // v27 on: its views are the model's.
        None if r.scale.is_none() && r.center.is_none() => Vec::new(),
        // Before v18: the sheet's one view, filling it.
        None => {
            let mut out = view(
                r.scale.ok_or_else(|| bad("scale"))?,
                r.center.ok_or_else(|| bad("center"))?,
            )?;
            out.dimensions = r.dimensions.unwrap_or(true);
            out.notes = r.notes.unwrap_or(true);
            vec![out]
        }
    };
    let title_block = TitleBlock::parse(&r.title_block).ok_or_else(|| bad("title_block"))?;
    let colours = match &r.colours {
        Some(key) => SheetColours::parse(key).ok_or_else(|| bad("colours"))?,
        None => SheetColours::Colour,
    };
    let f = r.fields;
    let sheet = Sheet {
        name: r.name,
        paper,
        portrait: r.portrait,
        viewports,
        title_block,
        fields: TitleFields {
            title: f.title,
            number: f.number,
            author: f.author,
            checked: f.checked,
            date: f.date,
            company: f.company,
            material: f.material,
            status: f.status,
        },
        revisions: r
            .revisions
            .into_iter()
            .map(|r| Revision {
                rev: r.rev,
                date: r.date,
                description: r.description,
                by: r.by,
            })
            .collect(),
        colours,
        annotations,
    };
    Ok((SheetId(parse_prefixed(&r.id, 's')?), sheet, old_views))
}
