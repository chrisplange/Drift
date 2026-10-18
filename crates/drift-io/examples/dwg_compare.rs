//! Import DWG files, and the DXF files saved from the same drawings, and print what came in
//! side by side: `cargo run -p drift-io --features dwg --release --example dwg_compare --
//! a.dwg [a.dxf] [b.dwg [b.dxf] ...]` (a DXF goes with the DWG before it).

use std::collections::BTreeMap;
use std::time::Instant;

use drift_io::dxf::DxfImport;

fn summary(import: &DxfImport) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut add = |k: String, n: usize| *out.entry(k).or_insert(0) += n;
    for e in &import.entities {
        let kind = format!("{:?}", e.shape);
        add(
            format!("curve {}", kind.split([' ', '(']).next().unwrap_or("")),
            1,
        );
    }
    add("texts".into(), import.texts.len());
    add("hatches".into(), import.hatches.len());
    add("layers".into(), import.layers.len());
    add("blocks".into(), import.blocks.len());
    add(
        "block contents".into(),
        import
            .blocks
            .iter()
            .map(|b| b.entities.len() + b.texts.len())
            .sum(),
    );
    add("inserts".into(), import.inserts.len());
    add("layouts".into(), import.layouts.len());
    add(
        "layout views".into(),
        import.layouts.iter().map(|l| l.views.len()).sum(),
    );
    add("dimensions".into(), import.dimensions);
    for (k, n) in &import.skipped {
        add(format!("skipped {k}"), *n);
    }
    out
}

/// Each curve and text, with its numbers rounded to 0.01, for finding what differs.
fn signatures(import: &DxfImport) -> BTreeMap<String, isize> {
    let round = |s: String| {
        let mut out = String::new();
        let mut num = String::new();
        for c in s.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' && !num.is_empty() {
                num.push(c);
            } else {
                if !num.is_empty() {
                    match num.parse::<f64>() {
                        Ok(v) => {
                            out.push_str(&format!("{:.2}", if v.abs() < 0.005 { 0.0 } else { v }))
                        }
                        Err(_) => out.push_str(&num),
                    }
                    num.clear();
                }
                out.push(c);
            }
        }
        out
    };
    let mut out = BTreeMap::new();
    for e in &import.entities {
        *out.entry(round(format!("{} {:?}", e.layer, e.shape)))
            .or_insert(0) += 1;
    }
    for t in &import.texts {
        *out.entry(round(format!(
            "{} {:?} {:?} {:.2}",
            t.layer, t.text, t.at, t.height
        )))
        .or_insert(0) += 1;
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let dwg = &args[i];
        i += 1;
        let dxf = args
            .get(i)
            .filter(|p| p.to_ascii_lowercase().ends_with(".dxf"));
        if dxf.is_some() {
            i += 1;
        }
        let bytes = std::fs::read(dwg).expect("read dwg");
        let started = Instant::now();
        let from_dwg = drift_io::dwg_import::read(&bytes, 1.0);
        let dwg_time = started.elapsed();
        println!(
            "== {dwg} ({}, {:.0} ms)",
            String::from_utf8_lossy(&bytes[..6]),
            dwg_time.as_secs_f64() * 1e3
        );
        let from_dwg = match from_dwg {
            Ok(import) => import,
            Err(e) => {
                println!("   FAILED: {e}");
                continue;
            }
        };
        let a = summary(&from_dwg);
        let dxf_import = dxf.map(|p| {
            let text = drift_io::dxf::decode(&std::fs::read(p).expect("read dxf"));
            drift_io::dxf::read(&text, 1.0).expect("read dxf")
        });
        let b = dxf_import.as_ref().map(summary);
        let mut keys: Vec<&String> = a.keys().collect();
        if let Some(b) = &b {
            keys.extend(b.keys());
        }
        keys.sort();
        keys.dedup();
        println!(
            "   {:<32} {:>8} {:>8}",
            "",
            "dwg",
            if b.is_some() { "dxf" } else { "" }
        );
        for k in keys {
            let x = a.get(k).copied().unwrap_or(0);
            let y = b.as_ref().map(|b| b.get(k).copied().unwrap_or(0));
            let mark = if y.is_some_and(|y| y != x) {
                "  <-"
            } else {
                ""
            };
            println!(
                "   {k:<32} {x:>8} {:>8}{mark}",
                y.map(|y| y.to_string()).unwrap_or_default()
            );
        }
        if let Some(other) = &dxf_import {
            let mut diff = signatures(&from_dwg);
            for (k, n) in signatures(other) {
                *diff.entry(k).or_insert(0) -= n;
            }
            let diff: Vec<_> = diff.into_iter().filter(|(_, n)| *n != 0).collect();
            println!("   geometry/text differing from the DXF: {}", diff.len());
            for (k, n) in diff.iter().take(40) {
                let k: String = k.chars().take(150).collect();
                println!("     {} {k}", if *n > 0 { "dwg only" } else { "dxf only" });
            }
        }
        for note in &from_dwg.notes {
            println!("   note: {note}");
        }
        // Also add it to a document, as the app does.
        let mut doc = drift_core::Document::default();
        doc.begin();
        let report = drift_io::dxf::add_to_document(&mut doc, &from_dwg);
        println!(
            "   added: {} curves, {} texts, {} fills, {} copies, {} blocks, {} sheets",
            report.curves, report.texts, report.fills, report.copies, report.blocks, report.sheets
        );
    }
}
