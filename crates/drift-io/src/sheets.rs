//! File records for sheets (format 8) and views (format 27).

use drift_core::{
    Bounds, DVec2, PaperSize, Revision, Sheet, SheetColours, SheetId, TitleBlock, TitleFields,
};
use drift_core::{FieldSet, LayerId, Project, View, ViewFrame, ViewId, Viewport};
use serde::{Deserialize, Serialize};

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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    /// The crop shown on the canvas; written only when hidden.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    crop: bool,
    /// Layers left out of the view (format 28).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden_layers: Vec<String>,
}

pub(crate) fn view_record(id: ViewId, v: &View) -> ViewRecord {
    ViewRecord {
        id: id.to_string(),
        name: v.name.clone(),
        scale: v.scale,
        center: v.center.to_array(),
        size: v.size.to_array(),
        dimensions: v.dimensions,
        notes: v.notes,
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
        crop_visible: r.crop,
        hidden_layers,
    };
    Ok((id, view))
}

/// A view placed on a sheet.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportRecord {
    view: String,
    /// Its middle on the paper (mm).
    at: [f64; 2],
    #[serde(default, skip_serializing_if = "is_false")]
    title: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    border: bool,
}

/// A view of the sketch on a sheet, as written from v18 to v25 (read only: each becomes a
/// model view and a viewport).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
}

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
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
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
    };
    Ok((SheetId(parse_prefixed(&r.id, 's')?), sheet, old_views))
}
