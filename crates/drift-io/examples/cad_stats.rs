//! Audit probe: one JSON line per DXF/DWG file with what Drift's reader made of it.
//! `cargo run --release -p drift-io --features dwg --example cad_stats -- files...`

use std::collections::BTreeMap;
use std::time::Instant;

use drift_core::Shape;
use serde_json::json;

fn main() {
    for file in std::env::args().skip(1) {
        let f2 = file.clone();
        let r = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || one(&f2))
            .unwrap()
            .join();
        if r.is_err() {
            println!("{}", json!({"file": file, "panic": true}));
        }
    }
}

fn one(file: &str) {
    let bytes = std::fs::read(file).unwrap();
    let t = Instant::now();
    let dwg = file.to_ascii_lowercase().ends_with(".dwg");
    let read = if dwg {
        drift_io::dwg_import::read(&bytes, 1.0)
    } else {
        drift_io::dxf::read(&drift_io::dxf::decode(&bytes), 1.0)
    };
    let ms = t.elapsed().as_millis();
    let imp = match read {
        Ok(i) => i,
        Err(e) => {
            println!("{}", json!({"file": file, "error": e, "ms": ms}));
            return;
        }
    };
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let kind = |s: &Shape| {
        match s {
            Shape::Point(_) => "point",
            Shape::Line { .. } => "line",
            Shape::Circle { .. } => "circle",
            Shape::Arc { .. } => "arc",
            Shape::Ellipse { .. } => "ellipse",
            Shape::Bezier { .. } => "bezier",
        }
        .to_string()
    };
    let mut b = drift_core::Bounds::EMPTY;
    for e in &imp.entities {
        *kinds.entry(kind(&e.shape)).or_default() += 1;
        let bb = e.shape.bounds();
        if !bb.is_far_off() {
            b.include(bb.min);
            b.include(bb.max);
        }
    }
    let solid = imp.hatches.iter().filter(|h| h.solid).count();
    let holes = imp.hatches.iter().filter(|h| h.loops.len() > 1).count();
    let block_items: usize = imp.blocks.iter().map(|b| b.entities.len()).sum();
    let block_texts: usize = imp.blocks.iter().map(|b| b.texts.len()).sum();
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();
    for h in imp.hatches.iter().filter(|h| !h.solid) {
        *patterns.entry(h.pattern.clone()).or_default() += 1;
    }
    let text_sample: Vec<&str> = imp.texts.iter().take(6).map(|t| t.text.as_str()).collect();
    println!(
        "{}",
        json!({
            "file": file, "ms": ms, "kinds": kinds, "texts": imp.texts.len(),
            "text_sample": text_sample,
            "hatches": imp.hatches.len(), "solid": solid, "holed": holes, "patterns": patterns,
            "layers": imp.layers.len(), "blocks": imp.blocks.len(), "block_items": block_items,
            "block_texts": block_texts, "inserts": imp.inserts.len(),
            "layouts": imp.layouts.iter().map(|l| json!({"name": l.name, "views": l.views.len(), "dropped": l.dropped, "paper": l.paper.map(|p| [p.x, p.y])})).collect::<Vec<_>>(),
            "dims": imp.dimensions, "skipped": imp.skipped, "notes": imp.notes,
            "bounds": if b.is_empty() { json!(null) } else { json!([b.min.x, b.min.y, b.max.x, b.max.y]) },
        })
    );
}
