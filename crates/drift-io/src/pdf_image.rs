//! A PDF image XObject as pixels: its samples (raw or Flate-compressed through lopdf, JPEG
//! through zune-jpeg) in its colour space, with its soft mask, stencil mask or colour-key
//! mask as alpha. Images are cut down to at most [`MAX_SIDE`] pixels a side.

use std::collections::HashMap;

use drift_core::Rgb;
use lopdf::{Dictionary, Document, Object, Stream};

use crate::pdf_import::Space;

/// The longest side an image is kept at, in pixels.
pub(crate) const MAX_SIDE: u32 = 4096;
/// Images with more samples than this aren't read at all.
const MAX_SAMPLES: u64 = 400_000_000;

/// Pixels as RGBA, a byte a channel (not premultiplied), rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why an image couldn't be read: said to the user.
pub(crate) type Unread = &'static str;

pub(crate) const UNREADABLE: Unread = "raster image in a form Drift can't read (JPEG 2000, \
                                       JBIG2 or fax)";

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn numbers(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<Vec<f64>> {
    let a = dict.get_deref(key, doc).ok()?.as_array().ok()?;
    Some(
        a.iter()
            .filter_map(|o| num(doc.dereference(o).ok()?.1))
            .collect(),
    )
}

fn filter_names(doc: &Document, dict: &Dictionary) -> Vec<Vec<u8>> {
    match dict.get_deref(b"Filter", doc) {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(a)) => a
            .iter()
            .filter_map(|o| {
                doc.dereference(o)
                    .ok()?
                    .1
                    .as_name()
                    .ok()
                    .map(<[u8]>::to_vec)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// An image's samples, `components` a pixel, and its bits per component (JPEGs come out at
/// 8). `None` for what can't be read.
fn samples(doc: &Document, stream: &Stream, limit: usize) -> Option<(Vec<u8>, Option<usize>)> {
    let filters = filter_names(doc, &stream.dict);
    match filters.last().map(Vec::as_slice) {
        Some(b"DCTDecode" | b"DCT") => {
            // Any filters before the JPEG's own are undone first.
            let jpeg = if filters.len() == 1 {
                stream.content.clone()
            } else {
                let mut inner = stream.clone();
                let rest: Vec<Object> = filters[..filters.len() - 1]
                    .iter()
                    .map(|f| Object::Name(f.clone()))
                    .collect();
                inner.dict.set("Filter", Object::Array(rest));
                inner.decompressed_content_with_limit(limit).ok()?
            };
            let (pixels, components) = decode_jpeg(&jpeg)?;
            Some((pixels, Some(components)))
        }
        Some(b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode" | b"CCF") => None,
        _ => Some((stream.decompressed_content_with_limit(limit).ok()?, None)),
    }
}

/// A JPEG's samples (grey, RGB or CMYK, as the file has them) and how many a pixel.
fn decode_jpeg(bytes: &[u8]) -> Option<(Vec<u8>, usize)> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let mut probe = JpegDecoder::new(ZCursor::new(bytes));
    probe.decode_headers().ok()?;
    let out = match probe.input_colorspace()? {
        ColorSpace::Luma | ColorSpace::LumaA => ColorSpace::Luma,
        ColorSpace::CMYK | ColorSpace::YCCK => ColorSpace::CMYK,
        _ => ColorSpace::RGB,
    };
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(out)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().ok()?;
    Some((pixels, out.num_components()))
}

/// Unpacked sample values of a row-packed image (rows padded to whole bytes), as integers.
fn unpack(data: &[u8], width: usize, height: usize, components: usize, bits: usize) -> Vec<u16> {
    let per_row = width * components;
    let row_bytes = (per_row * bits).div_ceil(8);
    let mut out = Vec::with_capacity(per_row * height);
    for row in 0..height {
        let start = row * row_bytes;
        for i in 0..per_row {
            let v = match bits {
                8 => data.get(start + i).map_or(0, |b| u16::from(*b)),
                16 => {
                    let at = start + 2 * i;
                    match data.get(at..at + 2) {
                        Some(b) => u16::from_be_bytes([b[0], b[1]]),
                        None => 0,
                    }
                }
                1 | 2 | 4 => {
                    let bit = i * bits;
                    let byte = data.get(start + bit / 8).copied().unwrap_or(0);
                    let shift = 8 - bits - bit % 8;
                    u16::from((byte >> shift) & ((1u8 << bits) - 1))
                }
                _ => 0,
            };
            out.push(v);
        }
    }
    out
}

/// A stencil (an image mask): where it is painted, by its samples and `Decode`.
fn stencil(doc: &Document, stream: &Stream) -> Option<(usize, usize, Vec<bool>)> {
    let d = &stream.dict;
    let int = |k: &[u8]| d.get_deref(k, doc).ok().and_then(num).map(|v| v as usize);
    let (w, h) = (int(b"Width")?, int(b"Height")?);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_SAMPLES {
        return None;
    }
    let (data, _) = samples(doc, stream, w * h + h + 1024)?;
    // Samples of 0 are painted, unless `Decode` is [1 0].
    let flip = numbers(doc, d, b"Decode").is_some_and(|v| v.first() == Some(&1.0));
    let bits = unpack(&data, w, h, 1, 1);
    Some((w, h, bits.into_iter().map(|b| (b == 0) != flip).collect()))
}

/// A soft mask's alpha (its grey samples), `w` × `h`.
fn soft_mask(doc: &Document, stream: &Stream) -> Option<(usize, usize, Vec<u8>)> {
    let d = &stream.dict;
    let int = |k: &[u8]| d.get_deref(k, doc).ok().and_then(num).map(|v| v as usize);
    let (w, h) = (int(b"Width")?, int(b"Height")?);
    let bits = int(b"BitsPerComponent").unwrap_or(8);
    if w == 0
        || h == 0
        || (w as u64) * (h as u64) > MAX_SAMPLES
        || ![1, 2, 4, 8, 16].contains(&bits)
    {
        return None;
    }
    let (data, jpeg) = samples(doc, stream, (w * h * bits).div_ceil(8) + h + 1024)?;
    let bits = if jpeg.is_some() { 8 } else { bits };
    let top = ((1u32 << bits) - 1) as f64;
    let flip = numbers(doc, d, b"Decode").is_some_and(|v| v.first() == Some(&1.0));
    let alpha = unpack(&data, w, h, 1, bits)
        .into_iter()
        .map(|v| {
            let a = f64::from(v) / top;
            ((if flip { 1.0 - a } else { a }) * 255.0).round() as u8
        })
        .collect();
    Some((w, h, alpha))
}

/// An image XObject (or an inline image's dictionary and data) as pixels, its colours in
/// `space`. A stencil mask (`ImageMask`) is painted in `paint`. Every pixel's alpha is
/// multiplied by `opacity`.
pub(crate) fn decode(
    doc: &Document,
    stream: &Stream,
    space: &Space,
    paint: Rgb,
    opacity: f64,
) -> Result<Pixels, Unread> {
    let d = &stream.dict;
    let int = |k: &[u8]| d.get_deref(k, doc).ok().and_then(num).map(|v| v as usize);
    let (w, h) = match (int(b"Width"), int(b"Height")) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => return Err(UNREADABLE),
    };
    if (w as u64) * (h as u64) > MAX_SAMPLES {
        return Err("raster image too large to read");
    }
    let image_mask = d
        .get_deref(b"ImageMask", doc)
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    let alpha_scale = opacity.clamp(0.0, 1.0);
    let mut rgba = vec![0u8; w * h * 4];
    if image_mask {
        let (_, _, painted) = stencil(doc, stream).ok_or(UNREADABLE)?;
        let a = (alpha_scale * 255.0).round() as u8;
        for (i, on) in painted.iter().enumerate() {
            if *on {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[paint.0[0], paint.0[1], paint.0[2], a]);
            }
        }
        return Ok(shrink(Pixels {
            width: w as u32,
            height: h as u32,
            rgba,
        }));
    }
    let n = space.components();
    let bits = int(b"BitsPerComponent").unwrap_or(8);
    if ![1, 2, 4, 8, 16].contains(&bits) || n == 0 {
        return Err(UNREADABLE);
    }
    let limit = (w * h * n * bits).div_ceil(8) + h * n + 4096;
    let (data, jpeg) = samples(doc, stream, limit).ok_or(UNREADABLE)?;
    let (n, bits) = match jpeg {
        // A JPEG says its own components (an Adobe CMYK one in a Device space).
        Some(c) => (c, 8),
        None => (n, bits),
    };
    let space = if n == space.components() {
        space.clone()
    } else {
        match n {
            1 => Space::Gray,
            3 => Space::Rgb,
            _ => Space::Cmyk(None),
        }
    };
    let raw = unpack(&data, w, h, n, bits);
    let top = ((1u32 << bits) - 1) as f64;
    // How sample values map to colour values: `Decode`, or the space's defaults.
    let decode: Vec<(f64, f64)> = match numbers(doc, d, b"Decode") {
        Some(v) if v.len() >= 2 * n => v.chunks(2).take(n).map(|c| (c[0], c[1])).collect(),
        _ => space.default_decode(bits),
    };
    // Colour-key masking: pixels whose samples are all within the ranges aren't painted.
    let key: Option<Vec<(u16, u16)>> = d.get_deref(b"Mask", doc).ok().and_then(|m| {
        let a = m.as_array().ok()?;
        let v: Vec<f64> = a.iter().filter_map(num).collect();
        (v.len() >= 2 * n).then(|| {
            v.chunks(2)
                .take(n)
                .map(|c| (c[0] as u16, c[1] as u16))
                .collect()
        })
    });
    let mut memo: HashMap<u64, Rgb> = HashMap::new();
    let a = (alpha_scale * 255.0).round() as u8;
    let mut values = vec![0.0; n];
    for i in 0..w * h {
        let px = &raw[i * n..i * n + n];
        let packed = px.iter().fold(0u64, |k, v| (k << 16) | u64::from(*v));
        let color = match memo.get(&packed) {
            Some(c) => *c,
            None => {
                for (k, v) in px.iter().enumerate() {
                    let (lo, hi) = decode[k];
                    values[k] = lo + f64::from(*v) / top * (hi - lo);
                }
                let c = space.color(&values).unwrap_or(Rgb([0, 0, 0]));
                if memo.len() < 1 << 20 {
                    memo.insert(packed, c);
                }
                c
            }
        };
        let keyed = key
            .as_ref()
            .is_some_and(|k| px.iter().zip(k).all(|(v, (lo, hi))| v >= lo && v <= hi));
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[
            color.0[0],
            color.0[1],
            color.0[2],
            if keyed { 0 } else { a },
        ]);
    }
    // A soft mask, or a stencil mask, gives alpha (at its own size, stretched over).
    let mask = |key: &[u8]| d.get_deref(key, doc).ok().and_then(|m| m.as_stream().ok());
    let alpha: Option<(usize, usize, Vec<u8>)> = if let Some(sm) = mask(b"SMask") {
        soft_mask(doc, sm)
    } else if let Some(m) = mask(b"Mask") {
        stencil(doc, m).map(|(mw, mh, on)| {
            (
                mw,
                mh,
                on.into_iter().map(|p| if p { 255 } else { 0 }).collect(),
            )
        })
    } else {
        None
    };
    if let Some((mw, mh, alpha)) = alpha {
        for y in 0..h {
            let my = y * mh / h;
            for x in 0..w {
                let mx = x * mw / w;
                let m = f64::from(alpha[my * mw + mx]) / 255.0;
                let at = (y * w + x) * 4 + 3;
                rgba[at] = (f64::from(rgba[at]) * m).round() as u8;
            }
        }
    }
    Ok(shrink(Pixels {
        width: w as u32,
        height: h as u32,
        rgba,
    }))
}

/// At most [`MAX_SIDE`] pixels a side: averaged down by a whole factor.
fn shrink(p: Pixels) -> Pixels {
    let side = p.width.max(p.height);
    if side <= MAX_SIDE {
        return p;
    }
    let f = side.div_ceil(MAX_SIDE) as usize;
    let (w, h) = (p.width as usize, p.height as usize);
    let (nw, nh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for sy in y * f..((y + 1) * f).min(h) {
                for sx in x * f..((x + 1) * f).min(w) {
                    let at = (sy * w + sx) * 4;
                    for (s, v) in sum.iter_mut().zip(&p.rgba[at..at + 4]) {
                        *s += u32::from(*v);
                    }
                    count += 1;
                }
            }
            let at = (y * nw + x) * 4;
            for (o, s) in out[at..at + 4].iter_mut().zip(sum) {
                *o = (s / count.max(1)) as u8;
            }
        }
    }
    Pixels {
        width: nw as u32,
        height: nh as u32,
        rgba: out,
    }
}

/// An inline image's stream with its abbreviated keys and names spelled out, as an image
/// XObject has them.
pub(crate) fn expand_inline(inline: &Stream) -> Stream {
    fn long(k: &[u8]) -> &[u8] {
        match k {
            b"W" => b"Width",
            b"H" => b"Height",
            b"BPC" => b"BitsPerComponent",
            b"CS" => b"ColorSpace",
            b"IM" => b"ImageMask",
            b"F" => b"Filter",
            b"D" => b"Decode",
            b"DP" => b"DecodeParms",
            b"I" => b"Interpolate",
            other => other,
        }
    }
    let name = |n: &[u8]| -> Vec<u8> {
        match n {
            b"G" => b"DeviceGray".to_vec(),
            b"RGB" => b"DeviceRGB".to_vec(),
            b"CMYK" => b"DeviceCMYK".to_vec(),
            b"AHx" => b"ASCIIHexDecode".to_vec(),
            b"A85" => b"ASCII85Decode".to_vec(),
            b"LZW" => b"LZWDecode".to_vec(),
            b"Fl" => b"FlateDecode".to_vec(),
            b"RL" => b"RunLengthDecode".to_vec(),
            b"CCF" => b"CCITTFaxDecode".to_vec(),
            b"DCT" => b"DCTDecode".to_vec(),
            other => other.to_vec(),
        }
    };
    let mut dict = Dictionary::new();
    for (k, v) in inline.dict.iter() {
        let key = long(k);
        let v = match (key, v) {
            (b"ColorSpace" | b"Filter", Object::Name(n)) => Object::Name(name(n)),
            (b"Filter", Object::Array(a)) => Object::Array(
                a.iter()
                    .map(|f| match f {
                        Object::Name(n) => Object::Name(name(n)),
                        o => o.clone(),
                    })
                    .collect(),
            ),
            _ => v.clone(),
        };
        dict.set(key.to_vec(), v);
    }
    let mut out = Stream::new(dict, inline.content.clone());
    out.allows_compression = false;
    out
}
