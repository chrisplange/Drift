//! File records for underlays (format 12): placement and look, and the curves themselves in
//! a compact form (`{"l": [x1, y1, x2, y2]}`, `{"p": [x, y, x, y, …]}`, `{"c": [x, y, r]}`,
//! `{"a": [x, y, r, start°, sweep°]}`). Their pictures (format 33) are PNGs in base64, each
//! with its top-left corner and the vectors along its top and down its left side.

use std::sync::Arc;

use drift_core::{DVec2, RefPrim, Rgb, Underlay, UnderlayId, UnderlayImage};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<ImageRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageRecord {
    at: [f64; 2],
    across: [f64; 2],
    down: [f64; 2],
    png: String,
}

#[cfg(not(feature = "demo"))]
/// A picture as a PNG.
fn encode_png(image: &UnderlayImage) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    let written = encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&image.rgba));
    written.expect("a picture's pixels fill it");
    out
}

/// A PNG's pixels as RGBA, with its size.
fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == (w as usize) * (h as usize) * 4).then_some((w, h, rgba))
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[cfg(not(feature = "demo"))]
fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(BASE64[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        let v = BASE64.iter().position(|b| *b == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(not(feature = "demo"))]
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
        images: u
            .images
            .iter()
            .map(|i| ImageRecord {
                at: i.at.to_array(),
                across: i.across.to_array(),
                down: i.down.to_array(),
                png: base64(&encode_png(i)),
            })
            .collect(),
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
    let mut images = Vec::with_capacity(r.images.len());
    for i in &r.images {
        let (width, height, rgba) = unbase64(&i.png)
            .and_then(|png| decode_png(&png))
            .ok_or_else(|| bad("images"))?;
        images.push(UnderlayImage {
            width,
            height,
            rgba: Arc::new(rgba),
            at: DVec2::from_array(i.at),
            across: DVec2::from_array(i.across),
            down: DVec2::from_array(i.down),
        });
    }
    Ok((
        id,
        Underlay {
            name: r.name,
            source: r.source,
            prims: Arc::new(prims),
            images: Arc::new(images),
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
