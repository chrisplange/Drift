//! File records for sheets (format 8).

use drift_core::{DVec2, PaperSize, Revision, Sheet, SheetId, TitleBlock, TitleFields};
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
    scale: f64,
    center: [f64; 2],
    title_block: String,
    #[serde(default)]
    fields: FieldsRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revisions: Vec<RevisionRecord>,
    /// Written only when hidden on the sheet.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    dimensions: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    notes: bool,
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
        scale: s.scale,
        center: s.center.to_array(),
        title_block: s.title_block.key().into(),
        dimensions: s.dimensions,
        notes: s.notes,
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
    if !(r.scale.is_finite() && r.scale > 0.0) {
        return Err(bad("scale"));
    }
    let center = DVec2::from_array(r.center);
    if !center.is_finite() {
        return Err(LoadError::NonFinite(r.id.clone()));
    }
    let title_block = TitleBlock::parse(&r.title_block).ok_or_else(|| bad("title_block"))?;
    let f = r.fields;
    let sheet = Sheet {
        name: r.name,
        paper,
        portrait: r.portrait,
        scale: r.scale,
        center,
        title_block,
        dimensions: r.dimensions,
        notes: r.notes,
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
