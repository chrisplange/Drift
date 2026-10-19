//! Reading DWG (R12 to R2018, AC1009–AC1032) with the acadrust crate.
//!
//! acadrust decodes the binary drawing into its document model and writes that back out as
//! ASCII DXF, which [`crate::dxf_import`] then reads like any other DXF file. Going through
//! DXF keeps one mapping from AutoCAD's entities to Drift's: DWG and DXF drawings come in
//! with the same layers, blocks, hatches, text, dimensions and layouts, and what Drift
//! can't hold is reported the same way.

use std::io::Cursor;

use acadrust::{DwgReadOptions, DwgReader, DxfWriter};

use crate::dxf_import::{DxfImport, decode, read as read_dxf};

/// Whether `bytes` look like a DWG file (they start with its version, `AC10..`).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes[4..6].iter().all(u8::is_ascii_digit)
}

/// The AutoCAD release a DWG file was saved by, from its version code.
fn release(bytes: &[u8]) -> Option<&'static str> {
    Some(match bytes.get(..6)? {
        b"AC1009" => "R12",
        b"AC1012" => "R13",
        b"AC1014" => "R14",
        b"AC1015" => "2000",
        b"AC1018" => "2004",
        b"AC1021" => "2007",
        b"AC1024" => "2010",
        b"AC1027" => "2013",
        b"AC1032" => "2018",
        _ => return None,
    })
}

/// The drawing in a DWG file as ASCII DXF text. A file acadrust can't read whole is read
/// again leniently, skipping what is damaged; the second value says so.
pub fn to_dxf(bytes: &[u8]) -> Result<(String, bool), String> {
    if !is_dwg(bytes) {
        return Err("not a DWG file".into());
    }
    let Some(release) = release(bytes) else {
        let code = String::from_utf8_lossy(&bytes[..6]);
        return Err(format!(
            "this DWG version ({code}) isn't one Drift can read"
        ));
    };
    let strict = DwgReader::from_stream(Cursor::new(bytes)).read();
    let (doc, partial) = match strict {
        Ok(doc) => (doc, false),
        Err(first) => {
            let mut reader =
                DwgReader::from_stream_with_options(Cursor::new(bytes), DwgReadOptions::failsafe());
            match reader.read() {
                // Something must have survived for the lenient read to count.
                Ok(doc) if doc.entities().next().is_some() => (doc, true),
                _ => {
                    return Err(format!(
                        "could not read this AutoCAD {release} drawing: {first}"
                    ));
                }
            }
        }
    };
    let dxf = DxfWriter::new(&doc)
        .write_to_vec()
        .map_err(|e| format!("could not convert this AutoCAD {release} drawing: {e}"))?;
    Ok((decode(&dxf), partial))
}

/// Read a DWG file. Files that don't say their units are read in `default_mm_per_unit`
/// (the document's display unit).
pub fn read(bytes: &[u8], default_mm_per_unit: f64) -> Result<DxfImport, String> {
    let (text, partial) = to_dxf(bytes)?;
    let mut import = read_dxf(&text, default_mm_per_unit)?;
    if partial {
        import.notes.insert(
            0,
            "Parts of this DWG file are damaged or in a form Drift can't read yet; the rest \
             came in."
                .into(),
        );
    }
    Ok(import)
}

#[cfg(test)]
mod tests {
    use acadrust::entities::{Circle, EntityType, Line, Text};
    use acadrust::types::{DxfVersion, Vector3};
    use acadrust::{CadDocument, DwgWriter};
    use drift_core::{DVec2, Shape};

    use super::*;

    /// A drawing saved as DWG by acadrust, in `version`.
    fn dwg(version: DxfVersion) -> Vec<u8> {
        let mut doc = CadDocument::with_version(version);
        doc.add_entity(EntityType::Line(Line::from_coords(
            0.0, 0.0, 0.0, 100.0, 50.0, 0.0,
        )))
        .unwrap();
        doc.add_entity(EntityType::Circle(Circle::from_coords(
            20.0, 30.0, 0.0, 5.0,
        )))
        .unwrap();
        let text = Text::with_value("Hello", Vector3::new(10.0, 10.0, 0.0)).with_height(2.5);
        doc.add_entity(EntityType::Text(text)).unwrap();
        DwgWriter::write_to_vec(&doc).unwrap()
    }

    #[test]
    fn dwg_files_come_in_like_dxf_ones() {
        for version in [
            DxfVersion::AC1015,
            DxfVersion::AC1018,
            DxfVersion::AC1021,
            DxfVersion::AC1024,
            DxfVersion::AC1027,
            DxfVersion::AC1032,
        ] {
            let bytes = dwg(version);
            assert!(is_dwg(&bytes), "{version:?}");
            let import = read(&bytes, 1.0).unwrap_or_else(|e| panic!("{version:?}: {e}"));
            // The file's units are its own: compare in them.
            let Some(Shape::Line { a, b }) = import
                .entities
                .iter()
                .map(|e| &e.shape)
                .find(|s| matches!(s, Shape::Line { .. }))
            else {
                panic!("{version:?}: no line in {:?}", import.entities);
            };
            let k = b.x / 100.0;
            assert!(k > 0.0, "{version:?}");
            assert!(a.length() < 1e-9 && (*b - DVec2::new(100.0, 50.0) * k).length() < 1e-6);
            assert!(
                import.entities.iter().any(|e| matches!(e.shape,
                    Shape::Circle { center, radius }
                        if (center - DVec2::new(20.0, 30.0) * k).length() < 1e-6
                            && (radius - 5.0 * k).abs() < 1e-6)),
                "{version:?}: no circle"
            );
            assert!(
                import.texts.iter().any(|t| t.text == "Hello"),
                "{version:?}: no text in {:?}",
                import.texts
            );
            assert!(
                import.notes.iter().all(|n| !n.contains("damaged")),
                "{version:?}"
            );
        }
    }

    #[test]
    fn other_files_say_what_is_wrong() {
        assert!(
            read(b"0\nSECTION\n", 1.0)
                .unwrap_err()
                .contains("not a DWG")
        );
        assert!(
            read(b"AC1099 and more", 1.0)
                .unwrap_err()
                .contains("AC1099")
        );
        // A DWG header with nothing after it.
        assert!(read(b"AC1032\0\0\0\0\0\0", 1.0).is_err());
    }
}
