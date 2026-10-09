//! File records for sketch text (format 15).

use drift_core::{DVec2, LayerId, Rgb, SketchText, TextFont, TextId};
use serde::{Deserialize, Serialize};

use crate::{LoadError, parse_layer_id, parse_prefixed};

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn is_sans(f: &String) -> bool {
    f == TextFont::Sans.name()
}

fn sans() -> String {
    TextFont::Sans.name().to_owned()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextRecord {
    pub(crate) id: String,
    text: String,
    at: [f64; 2],
    height: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    angle: f64,
    #[serde(default = "sans", skip_serializing_if = "is_sans")]
    font: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

pub(crate) fn text_record(id: TextId, t: &SketchText) -> TextRecord {
    TextRecord {
        id: id.to_string(),
        text: t.text.clone(),
        at: t.at.to_array(),
        height: t.height,
        angle: t.angle,
        font: t.font.name().to_owned(),
        layer: (t.layer != LayerId::DEFAULT).then(|| t.layer.to_string()),
        color: t.appearance.color.map(Rgb::hex),
    }
}

pub(crate) fn parse_text(r: TextRecord) -> Result<(TextId, SketchText), LoadError> {
    let id = TextId(parse_prefixed(&r.id, 't')?);
    let bad = |field: &'static str| LoadError::MissingField {
        id: r.id.clone(),
        field,
    };
    if !(r.height > 0.0 && r.height.is_finite()) {
        return Err(bad("height"));
    }
    let at = DVec2::from_array(r.at);
    if !at.is_finite() || !r.angle.is_finite() {
        return Err(LoadError::NonFinite(r.id.clone()));
    }
    let font = TextFont::from_name(&r.font).ok_or_else(|| bad("font"))?;
    let layer = match &r.layer {
        Some(l) => parse_layer_id(l)?,
        None => LayerId::DEFAULT,
    };
    let mut text = SketchText::new(r.text.clone(), at, r.height);
    text.angle = r.angle;
    text.font = font;
    text.layer = layer;
    text.appearance.color = match &r.color {
        Some(c) => Some(Rgb::parse(c).ok_or_else(|| bad("color"))?),
        None => None,
    };
    Ok((id, text))
}
