//! File records for sheets (format 8).

use drift_core::{DVec2, PaperSize, Revision, Sheet, SheetId, TitleBlock, TitleFields};
use drift_core::{SheetView, ViewFrame};
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

/// A view of the sketch on a sheet.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewRecord {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    views: Option<Vec<ViewRecord>>,
    title_block: String,
    #[serde(default)]
    fields: FieldsRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revisions: Vec<RevisionRecord>,
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
        views: Some(
            s.views
                .iter()
                .map(|v| ViewRecord {
                    name: v.name.clone(),
                    scale: v.scale,
                    center: v.center.to_array(),
                    at: v.frame.map(|f| f.at.to_array()),
                    size: v.frame.map(|f| f.size.to_array()),
                    dimensions: v.dimensions,
                    notes: v.notes,
                    title: v.title,
                    border: v.border,
                })
                .collect(),
        ),
        title_block: s.title_block.key().into(),
        fields: FieldsRecord {
            title: f.title.clone(),
            number: f.number.clone(),
            author: f.author.clone(),
            checked: f.checked.clone(),
            date: f.date.clone(),
            company: f.company.clone(),
            material: f.material.clone(),
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
    }
}

pub(crate) fn parse_sheet(r: SheetRecord) -> Result<(SheetId, Sheet), LoadError> {
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
    let view = |scale: f64, center: [f64; 2]| -> Result<SheetView, LoadError> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(bad("scale"));
        }
        let center = DVec2::from_array(center);
        if !center.is_finite() {
            return Err(LoadError::NonFinite(r.id.clone()));
        }
        Ok(SheetView::new(scale, center))
    };
    let views = match &r.views {
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
    let f = r.fields;
    let sheet = Sheet {
        name: r.name,
        paper,
        portrait: r.portrait,
        views,
        title_block,
        fields: TitleFields {
            title: f.title,
            number: f.number,
            author: f.author,
            checked: f.checked,
            date: f.date,
            company: f.company,
            material: f.material,
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
    };
    Ok((SheetId(parse_prefixed(&r.id, 's')?), sheet))
}
