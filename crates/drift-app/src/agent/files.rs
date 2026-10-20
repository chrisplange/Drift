//! Files: new, open and save drawings; import artwork; export sheets and model space.

use std::path::{Path, PathBuf};

use drift_core::{DVec2, Document, LengthFormat, Units};
use serde_json::{Value, json};

use super::args;
use super::{Res, Session};

/// A path from a tool argument, kept inside the session's root folder when it has one.
fn resolve(s: &Session, raw: &str, must_exist: bool) -> Result<PathBuf, String> {
    let given = Path::new(raw);
    let path = match (&s.root, given.is_absolute()) {
        (Some(root), false) => root.join(given),
        _ => given.to_path_buf(),
    };
    let Some(root) = &s.root else {
        return Ok(path);
    };
    // The file, or for a new one its folder, must be inside the root.
    let real = if must_exist || path.exists() {
        path.canonicalize().map_err(|e| format!("{raw}: {e}"))?
    } else {
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path
            .file_name()
            .ok_or(format!("{raw} is not a file name"))?;
        dir.canonicalize()
            .map_err(|e| format!("{}: {e}", shown(dir)))?
            .join(name)
    };
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(format!(
            "{raw} is outside the folder this server may use ({})",
            shown(root)
        ))
    }
}

/// A path as people write it (without the `\\?\` that Windows puts on canonical paths).
pub(super) fn shown(p: &Path) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map_or(s.clone(), str::to_owned)
}

fn extension(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// In Drift, a drawing with unsaved changes is never replaced: that's the user's call.
fn may_replace(s: &Session) -> Result<(), String> {
    match &s.live {
        Some(live) if live.dirty => Err(
            "the drawing open in Drift has unsaved changes: ask the user to save it (or to \
             start the new drawing themselves) first"
                .into(),
        ),
        _ => Ok(()),
    }
}

/// Tell Drift its drawing was replaced (when working on it live).
fn replaced(s: &mut Session) {
    let (path, meta) = (s.path.clone(), s.meta.clone());
    if let Some(live) = &mut s.live {
        live.change = Some(super::LiveChange::Replaced { path, meta });
    }
}

pub(super) fn new_drawing(s: &mut Session, a: &Value) -> Res {
    may_replace(s)?;
    let mut doc = Document::new();
    if let Some(u) = args::opt_str(a, "units")? {
        let length = match u {
            "mm" => LengthFormat::Millimetre,
            "cm" => LengthFormat::Centimetre,
            "m" => LengthFormat::Metre,
            "in" => LengthFormat::Inch,
            "ft" => LengthFormat::Foot,
            "ft-in" => LengthFormat::Architectural,
            "in-frac" => LengthFormat::FractionalInch,
            other => {
                return Err(format!(
                    "unknown units `{other}` (mm, cm, m, in, ft, ft-in, in-frac)"
                ));
            }
        };
        let base = if length.is_metric() {
            Units::metric()
        } else {
            Units::architectural()
        };
        doc.set_units(Units { length, ..base });
    }
    if let Some(scale) = args::opt_f64(a, "scale")? {
        if scale <= 0.0 {
            return Err("`scale` is the n of 1:n, and must be positive".into());
        }
        let mut style = doc.draft_style();
        style.scale = scale;
        doc.set_draft_style(style);
    }
    s.replace(doc);
    s.path = None;
    s.meta = drift_io::Metadata::default();
    replaced(s);
    Ok(json!({ "created": true, "sketch": s.status() }))
}

pub(super) fn open(s: &mut Session, a: &Value) -> Res {
    may_replace(s)?;
    let path = resolve(s, args::str_(a, "path")?, true)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", shown(&path)))?;
    let (doc, meta, notes) = drift_io::load_with_notes(&text).map_err(|e| e.to_string())?;
    s.replace(doc);
    s.meta = meta;
    s.path = Some(path.clone());
    replaced(s);
    let mut v = super::inspect::info(s)?;
    if !notes.is_empty() {
        v["notes"] = json!(notes);
    }
    v["opened"] = json!(shown(&path));
    Ok(v)
}

pub(super) fn save(s: &mut Session, a: &Value) -> Res {
    let path = match args::opt_str(a, "path")? {
        Some(p) => {
            let mut path = resolve(s, p, false)?;
            if extension(&path) != "drift" {
                path.set_extension("drift");
            }
            path
        }
        // Only ever overwrite a drawing file, never the PDF or DXF it was opened from.
        None => s
            .path
            .clone()
            .filter(|p| extension(p) == "drift")
            .ok_or("the drawing has no .drift file yet: give a `path`")?,
    };
    s.meta.generator = Some(format!("Drift {}", drift_platform::VERSION));
    let text = drift_io::save(&s.editor.doc, &s.meta);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", shown(&path)))?;
    s.path = Some(path.clone());
    let meta = s.meta.clone();
    if let Some(live) = &mut s.live {
        live.change = Some(super::LiveChange::Saved {
            path: path.clone(),
            meta,
        });
    }
    Ok(json!({ "saved": shown(&path) }))
}

/// A scale argument: a number (model mm per paper mm) or "1:50".
fn scale_arg(a: &Value) -> Result<f64, String> {
    match a.get("scale") {
        None | Some(Value::Null) => Ok(1.0),
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|x| *x > 0.0)
            .ok_or("`scale` must be positive".into()),
        Some(Value::String(t)) => drift_io::vector::written_scale(t)
            .or_else(|| t.trim().parse().ok())
            .filter(|x: &f64| *x > 0.0)
            .ok_or(format!("can't read the scale `{t}` (try \"1:50\")")),
        Some(other) => Err(format!(
            "`scale` must be a number or like \"1:50\", got {other}"
        )),
    }
}

pub(super) fn import(s: &mut Session, a: &Value) -> Res {
    let path = resolve(s, args::str_(a, "path")?, true)?;
    let name = path
        .file_name()
        .map_or("drawing".into(), |n| n.to_string_lossy().into_owned());
    let stem = path
        .file_stem()
        .map_or("drawing".into(), |n| n.to_string_lossy().into_owned());
    let ext = extension(&path);
    let scale = scale_arg(a)?;
    let page = args::opt_f64(a, "page")?.unwrap_or(1.0).max(1.0) as u32;
    let underlay = args::opt_str(a, "as")?.unwrap_or("geometry") == "underlay";
    let offset = args::opt_vec2(a, "offset")?.unwrap_or(DVec2::ZERO);
    let layer = args::opt_layer(&s.editor.doc, a, "layer")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", shown(&path)))?;
    let text = || String::from_utf8_lossy(&bytes).into_owned();
    let unit = s.editor.doc.units().length.mm_per_unit();
    if underlay {
        let ed = &mut s.editor;
        let report = match ext.as_str() {
            "svg" => {
                let art = drift_io::svg_import::read(&text(), &stem)?;
                ed.import_svg_underlay(&name, &art, scale)?
            }
            "pdf" | "ai" => ed.import_pdf_underlay(&name, &bytes, page, scale)?,
            "dxf" => ed.import_dxf_underlay(&name, &text(), unit * scale)?,
            "dwg" => ed.import_dwg_underlay(&name, &bytes, unit * scale)?,
            other => {
                return Err(format!(
                    "can't import .{other} files (svg, pdf, ai, dxf or dwg)"
                ));
            }
        };
        if offset != DVec2::ZERO {
            ed.update_underlay(report.id, "Move underlay", |u| u.at += offset);
        }
        return Ok(json!({
            "underlay": report.id.to_string(), "curves": report.curves,
            "skipped": report.skipped,
        }));
    }
    let mut pictures = None;
    let import = match ext.as_str() {
        "svg" | "pdf" | "ai" => {
            let art = if ext == "svg" {
                drift_io::svg_import::read(&text(), &stem)?
            } else {
                drift_io::pdf_import::read_artwork(&bytes, page, &stem)?
            };
            let options = drift_io::vector::ImportOptions {
                scale,
                construction_edges: true,
            };
            pictures = crate::underlays::picture_underlay(&name, &art.images, scale, offset);
            drift_io::vector::to_import(&art, &options)
        }
        // A DXF says its units (bare ones are read in the drawing's); `scale` multiplies.
        "dxf" => drift_io::dxf::read(&text(), unit).map_err(|e| e.to_string())?,
        "dwg" => crate::underlays::read_dwg(&bytes, unit)?,
        other => {
            return Err(format!(
                "can't import .{other} files (svg, pdf, ai, dxf or dwg)"
            ));
        }
    };
    let ed = &mut s.editor;
    let before: std::collections::BTreeSet<_> = ed.doc.iter().map(|(id, _)| id).collect();
    ed.doc.begin();
    let report = drift_io::dxf::add_to_document(&mut ed.doc, &import);
    let pictures = pictures.map(|u| ed.doc.add_underlay(u));
    let made: Vec<_> = ed
        .doc
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    let points: Vec<_> = made
        .iter()
        .copied()
        .filter(|id| ed.doc.get(*id).is_some_and(|e| e.kind.is_point()))
        .collect();
    if (ext == "dxf" || ext == "dwg") && scale != 1.0 {
        for p in &points {
            if let Ok(at) = ed.doc.point(*p) {
                let _ = ed.doc.set_point(*p, at * scale);
            }
        }
    }
    if offset != DVec2::ZERO {
        ed.doc.translate(points.iter().copied(), offset);
    }
    if let Some(l) = layer {
        for id in &made {
            ed.doc.set_entity_layer(*id, l).map_err(|e| e.to_string())?;
        }
    }
    if !ed.commit("Import") {
        return Err(ed.notice.take().unwrap_or("the import failed".into()));
    }
    let curves = made
        .iter()
        .filter(|id| ed.doc.get(**id).is_some_and(|e| !e.kind.is_point()))
        .count();
    let b = ed.bounds_of(&made);
    let far = std::mem::take(&mut ed.far_off);
    let mut notes = report.notes.clone();
    if !far.is_empty() {
        notes.push(format!(
            "more than 100,000 km from the origin (likely stray objects): {}",
            crate::checker::far_off_count(&ed.doc, &far)
        ));
    }
    Ok(json!({
        "curves": curves,
        "bounds": b.map(|b| json!([args::num(b.min.x), args::num(b.min.y), args::num(b.max.x), args::num(b.max.y)])),
        "skipped": report.skipped.iter().map(|(what, n)| format!("{n} {what}")).collect::<Vec<_>>(),
        "notes": notes,
        "pictures": pictures.map(|id| id.to_string()),
    }))
}

pub(super) fn export(s: &mut Session, a: &Value) -> Res {
    let path = resolve(s, args::str_(a, "path")?, false)?;
    let format = match args::opt_str(a, "format")? {
        Some(f) => f.to_ascii_lowercase(),
        None => extension(&path),
    };
    let title = s.title();
    let painter = s.painter();
    let bytes = match format.as_str() {
        "pdf" => {
            let pages =
                crate::output::sheet_pages(&mut s.editor, &mut s.patterns, &painter, &title);
            drift_io::pdf::write(&pages, &title)
        }
        "svg" => {
            let pages =
                crate::output::sheet_pages(&mut s.editor, &mut s.patterns, &painter, &title);
            let index = args::opt_f64(a, "sheet")?.unwrap_or(1.0).max(1.0) as usize - 1;
            let page = pages
                .get(index)
                .ok_or(format!("there are {} sheet(s)", pages.len()))?;
            drift_io::svg::write(page).into_bytes()
        }
        "dxf" => {
            s.patterns.update(s.editor.doc.custom_patterns());
            let d = crate::output::model_drawing(&mut s.editor, &s.patterns, &painter);
            drift_io::dxf::write(&d, drift_io::dxf::DxfUnits::of(&s.editor.doc.units()))
                .into_bytes()
        }
        "png" => {
            let mut image = None;
            super::render::render(s, a, &mut image)?;
            image.ok_or("nothing to draw")?
        }
        other => return Err(format!("can't export .{other} (pdf, svg, dxf or png)")),
    };
    std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", shown(&path)))?;
    Ok(json!({ "exported": shown(&path), "bytes": bytes.len() }))
}
