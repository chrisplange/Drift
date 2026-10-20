//! File records for the fonts a drawing carries (format 19): each family's font files as
//! base64, and the family its annotations are set in.

#[cfg(not(feature = "demo"))]
use std::collections::BTreeSet;

use drift_core::{EmbeddedFont, FontFile, FontSet};
use serde::{Deserialize, Serialize};

use crate::LoadError;

#[cfg(not(feature = "demo"))]
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[cfg(not(feature = "demo"))]
pub(crate) fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(crate) fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(v);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Some(out)
}

fn is_zero(i: &u32) -> bool {
    *i == 0
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    /// The font file, base64.
    data: String,
    /// The face's index in it, for a font collection.
    #[serde(default, skip_serializing_if = "is_zero")]
    index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilyRecord {
    family: String,
    regular: FileRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bold: Option<FileRecord>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FontsRecord {
    /// The family annotations and sheets are set in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawing: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    embedded: Vec<FamilyRecord>,
}

#[cfg(not(feature = "demo"))]
/// The record to save: the drawing's font, and only the families `used` (one that nothing
/// is set in any more isn't worth its megabytes).
pub(crate) fn fonts_record(fonts: &FontSet, used: &BTreeSet<String>) -> Option<FontsRecord> {
    let file = |f: &FontFile| FileRecord {
        data: base64(&f.data),
        index: f.index,
    };
    let embedded: Vec<FamilyRecord> = fonts
        .embedded
        .iter()
        .filter(|(family, _)| used.contains(*family))
        .map(|(family, font)| FamilyRecord {
            family: family.clone(),
            regular: file(&font.regular),
            bold: font.bold.as_ref().map(file),
        })
        .collect();
    (fonts.drawing.is_some() || !embedded.is_empty()).then(|| FontsRecord {
        drawing: fonts.drawing.clone(),
        embedded,
    })
}

pub(crate) fn parse_fonts(r: FontsRecord) -> Result<FontSet, LoadError> {
    let mut set = FontSet {
        drawing: r.drawing.filter(|d| !d.trim().is_empty()),
        ..Default::default()
    };
    for family in r.embedded {
        let bad = || LoadError::MissingField {
            id: format!("font {}", family.family),
            field: "data",
        };
        let file = |f: &FileRecord| -> Result<FontFile, LoadError> {
            let data = unbase64(&f.data)
                .filter(|d| !d.is_empty())
                .ok_or_else(bad)?;
            Ok(FontFile::new(data, f.index))
        };
        let font = EmbeddedFont {
            regular: file(&family.regular)?,
            bold: family.bold.as_ref().map(file).transpose()?,
        };
        set.embedded.insert(family.family.clone(), font);
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        for len in [0, 1, 2, 3, 4, 255, 256, 999, 1000] {
            assert_eq!(unbase64(&base64(&bytes[..len])).unwrap(), &bytes[..len]);
        }
        assert!(unbase64("not base64!").is_none());
    }
}
