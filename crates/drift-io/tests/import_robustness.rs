//! The importers never panic, hang or blow the stack on bad input: truncated and random files,
//! huge, NaN and infinite numbers, deep nesting and loops, empty and zero-size pages. Each
//! input either fails with an error or comes in partly (what is left out is counted in the
//! import's `skipped`), quickly. The corpus is synthetic; mutations come from proptest.

use std::sync::mpsc;
use std::time::Duration;

use drift_core::Document;
use drift_io::{dxf, pdf_import, svg_import, vector};
use lopdf::{Dictionary, Object, Stream, dictionary};
use proptest::prelude::*;

/// How long one input may take, all importers included (debug builds are slow).
const TIME_BOX: Duration = Duration::from_secs(20);
/// The stack the app's main thread has on Windows (1 MB). Debug builds use many times the
/// stack per call that release builds do, so they get more.
const STACK: usize = if cfg!(debug_assertions) {
    16 << 20
} else {
    1 << 20
};

/// Run `f` on a thread with the app's stack, failing the test if it panics or overruns.
fn survives(what: &str, f: impl FnOnce() + Send + 'static) {
    if std::env::var_os("DRIFT_TRACE").is_some() {
        eprintln!("-- {what}");
    }
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            f();
            let _ = tx.send(());
        })
        .expect("thread starts");
    match rx.recv_timeout(TIME_BOX) {
        Ok(()) => {
            let _ = handle.join();
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            panic!("{what}: the importer panicked");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{what}: took longer than {TIME_BOX:?}"),
    }
}

/// Everything an imported vector file goes through in the app.
fn take_artwork(art: &vector::Artwork) {
    let _ = vector::outlines(art);
    let _ = vector::guess_scale(art, 1.0);
    for scale in [1.0, 50.0, 0.0, f64::NAN] {
        let import = vector::to_import(
            art,
            &vector::ImportOptions {
                scale,
                ..Default::default()
            },
        );
        add(&import);
    }
}

/// Add an import to a drawing, which must then save and open again (no NaN or infinite
/// geometry gets in).
fn add(import: &dxf::DxfImport) {
    let mut doc = Document::new();
    let _ = dxf::add_to_document(&mut doc, import);
    let text = drift_io::save(&doc, &drift_io::Metadata::default());
    if let Err(e) = drift_io::load(&text) {
        let bad: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("null"))
            .take(3)
            .collect();
        panic!(
            "the imported drawing doesn't open again: {e}
{}",
            bad.join(
                "
"
            )
        );
    }
}

fn import_pdf(bytes: Vec<u8>) {
    let _ = pdf_import::page_count(&bytes);
    for page in [1, 2, 0] {
        let _ = pdf_import::read_page(&bytes, page);
        if let Ok(art) = pdf_import::read_artwork(&bytes, page, "art") {
            take_artwork(&art);
        }
    }
}

fn import_svg(text: String) {
    if let Ok(art) = svg_import::read(&text, "art") {
        take_artwork(&art);
    }
}

fn import_dxf(text: String) {
    for unit in [1.0, 25.4, 0.0] {
        if let Ok(import) = dxf::read(&text, unit) {
            let _ = dxf::underlay_prims(&import);
            add(&import);
            add(&import.scaled(1e300));
        }
    }
    let _ = dxf::read_at(&text, 1.0, f64::NAN);
}

// ---------------------------------------------------------------------------------------------
// PDF

/// One page with `content`, its resources made by `resources`, as file bytes.
fn pdf(
    content: &[u8],
    media: [f64; 4],
    rotate: i64,
    resources: impl FnOnce(&mut lopdf::Document) -> Dictionary,
) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let resources = resources(&mut doc);
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => media.iter().map(|v| Object::Real(*v as f32)).collect::<Vec<_>>(),
        "Rotate" => rotate,
        "Contents" => content_id,
        "Resources" => resources,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("the test PDF writes");
    out
}

const A4: [f64; 4] = [0.0, 0.0, 595.0, 842.0];

fn simple_pdf(content: &str) -> Vec<u8> {
    pdf(content.as_bytes(), A4, 0, |_| dictionary! {})
}

/// Drawings that exercise the PDF reader, each valid or nearly so.
fn pdf_corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out = vec![
        (
            "paths",
            simple_pdf(
                "0.5 w 1 0 0 RG 10 10 m 100 10 l 100 100 l h S 0 0 1 rg 20 20 50 50 re f \
                 10 10 m 20 30 40 30 50 10 c 60 0 70 0 v 80 10 90 10 y S [3 2] 0 d 0 0 m 9 9 l S",
            ),
        ),
        (
            "huge numbers",
            simple_pdf(
                "1e38 0 0 1e38 0 0 cm 1e38 1e38 m -1e38 1e38 l S 3e38 w 0 0 m 1 1 l S \
                 99999999999999999999 99999999999999999999 m 0 0 l S",
            ),
        ),
        (
            "degenerate transforms and dashes",
            simple_pdf(
                "0 0 0 0 0 0 cm 0 0 m 10 10 l S Q Q Q q q q [0 0] 0 d 0 0 m 100 0 l S \
                 [-1 -2] 0 d 0 0 m 100 0 l S [1e-30] 0 d 0 0 m 1e30 0 l S [] 0 d \
                 0 0 1e-30 1e-30 re f 0 0 m h S h f* W n",
            ),
        ),
        (
            "missing operands",
            simple_pdf("m l c re S 1 cm Do Tf Tj TJ ' \" BI ID EI cs sc gs sh"),
        ),
        ("deep save stack", simple_pdf(&"q ".repeat(50_000))),
        (
            "deep operand arrays",
            simple_pdf(&format!(
                "{}0 0 m{} 1 1 l S",
                "[".repeat(5_000),
                "]".repeat(5_000)
            )),
        ),
        (
            "deep operand dictionaries",
            simple_pdf(&format!(
                "/P {}{} BDC 0 0 m 1 1 l S EMC",
                "<</A ".repeat(5_000),
                ">>".repeat(5_000)
            )),
        ),
        (
            "zero-size page",
            pdf(
                b"0 0 m 10 10 l S",
                [0.0, 0.0, 0.0, 0.0],
                0,
                |_| dictionary! {},
            ),
        ),
        (
            "upside-down rotated page",
            pdf(
                b"0 0 m 10 10 l S",
                [500.0, 500.0, 0.0, 0.0],
                -630,
                |_| dictionary! {},
            ),
        ),
    ];
    // A form that draws itself, and a chain of forms deeper than any reader nests.
    out.push((
        "self-drawing form",
        pdf(b"/F Do", A4, 0, |doc| {
            let id = doc.new_object_id();
            let form = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                    "Matrix" => vec![0.5.into(), 0.into(), 0.into(), 0.5.into(), 1.into(), 1.into()],
                    "Resources" => dictionary! { "XObject" => dictionary! { "F" => id } },
                },
                b"0 0 m 10 10 l S /F Do".to_vec(),
            );
            doc.objects.insert(id, Object::Stream(form));
            dictionary! { "XObject" => dictionary! { "F" => id } }
        }),
    ));
    out.push((
        "deep forms",
        pdf(b"/F Do", A4, 0, |doc| {
            let mut inner: Option<lopdf::ObjectId> = None;
            for _ in 0..200 {
                let resources = match inner {
                    Some(id) => dictionary! { "XObject" => dictionary! { "F" => id } },
                    None => dictionary! {},
                };
                let form = Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Form",
                        "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                        "Resources" => resources,
                    },
                    b"q 0 0 m 10 10 l S /F Do Q".to_vec(),
                );
                inner = Some(doc.add_object(form));
            }
            dictionary! { "XObject" => dictionary! { "F" => inner.unwrap() } }
        }),
    ));
    // Images: huge, empty, short of data, with odd bits and colour spaces.
    for (name, dict, data) in [
        (
            "huge image",
            dictionary! { "Width" => 1_000_000_000, "Height" => 1_000_000_000,
            "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB" },
            vec![0u8; 16],
        ),
        (
            "empty image",
            dictionary! { "Width" => 0, "Height" => 0, "BitsPerComponent" => 8,
            "ColorSpace" => "DeviceGray" },
            Vec::new(),
        ),
        (
            "negative image",
            dictionary! { "Width" => -5, "Height" => 3, "BitsPerComponent" => 0,
            "ColorSpace" => "DeviceCMYK" },
            vec![1, 2, 3],
        ),
        (
            "short image",
            dictionary! { "Width" => 64, "Height" => 64, "BitsPerComponent" => 16,
            "ColorSpace" => "DeviceRGB", "Decode" => vec![0.into()] },
            vec![9u8; 10],
        ),
        (
            "indexed image",
            dictionary! { "Width" => 4, "Height" => 4, "BitsPerComponent" => 8,
            "ColorSpace" => vec!["Indexed".into(), "DeviceRGB".into(), 255.into(),
                Object::string_literal(vec![1u8, 2])] },
            vec![200u8; 16],
        ),
        (
            "mask image",
            dictionary! { "Width" => 3, "Height" => 3, "ImageMask" => true,
            "Decode" => vec![1.into(), 0.into()] },
            vec![0xff],
        ),
        (
            "bad jpeg",
            dictionary! { "Width" => 8, "Height" => 8, "BitsPerComponent" => 8,
            "ColorSpace" => "DeviceRGB", "Filter" => "DCTDecode" },
            vec![0xff, 0xd8, 0xff, 0x00, 0x01],
        ),
        (
            "bad flate",
            dictionary! { "Width" => 8, "Height" => 8, "BitsPerComponent" => 8,
            "ColorSpace" => "DeviceGray", "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 15, "Columns" => 0 } },
            vec![0x78, 0x9c, 1, 2, 3],
        ),
    ] {
        out.push((
            name,
            pdf(b"q 100 0 0 100 10 10 cm /I Do Q", A4, 0, move |doc| {
                let mut dict = dict;
                dict.set("Type", "XObject");
                dict.set("Subtype", "Image");
                let id = doc.add_object(Stream::new(dict, data));
                dictionary! { "XObject" => dictionary! { "I" => id } }
            }),
        ));
    }
    // Colour spaces: a broken ICC profile, a short lookup, Lab, a separation.
    out.push((
        "colour spaces",
        pdf(
            b"/C0 cs 0.1 0.2 0.3 0.4 sc 0 0 10 10 re f /C1 cs 300 sc 0 0 10 10 re f \
              /C2 cs 50 -500 1e9 sc 0 0 10 10 re f /C3 CS 0.5 SC 0 0 m 5 5 l S /C4 cs sc f",
            A4,
            0,
            |doc| {
                let icc = doc.add_object(Stream::new(
                    dictionary! { "N" => 4 },
                    b"acspAAAA garbage profile with no tags".to_vec(),
                ));
                dictionary! { "ColorSpace" => dictionary! {
                    "C0" => vec!["ICCBased".into(), icc.into()],
                    "C1" => vec!["Indexed".into(), "DeviceCMYK".into(), 1000.into(),
                        Object::string_literal(vec![1u8])],
                    "C2" => vec!["Lab".into(), dictionary! { "WhitePoint" => vec![0.into()] }.into()],
                    "C3" => vec!["Separation".into(), "Spot".into(), "DeviceCMYK".into(),
                        dictionary! { "FunctionType" => 2, "N" => -1, "C1" => vec![] }.into()],
                    "C4" => vec![],
                } }
            },
        ),
    ));
    // Text in fonts with broken widths and character maps.
    out.push((
        "text",
        pdf(
            b"BT /F1 12 Tf 10 10 Td (Hello) Tj [(a) -1e30 (b)] TJ 0 Tr 1e30 Tz (x) ' ET \
              BT /F2 0 Tf <00010002> Tj ET BT /F3 -5 Tf 1 2 3 4 5 6 Tm (y) Tj ET",
            A4,
            0,
            |doc| {
                let cmap = doc.add_object(Stream::new(
                    dictionary! {},
                    b"begincmap 1 beginbfrange <0000> <FFFF> <0041> endbfrange \
                      2 beginbfchar <01> <> <02> <D800> endbfchar"
                        .to_vec(),
                ));
                dictionary! { "Font" => dictionary! {
                    "F1" => dictionary! { "Type" => "Font", "Subtype" => "Type1",
                        "FirstChar" => 4_000_000_000_i64, "Widths" => vec![1.into()],
                        "ToUnicode" => cmap },
                    "F2" => dictionary! { "Type" => "Font", "Subtype" => "Type0",
                        "Encoding" => "Identity-H",
                        "DescendantFonts" => vec![dictionary! { "W" => vec![0.into(),
                            vec![].into(), 5.into()] }.into()] },
                    "F3" => dictionary! { "Type" => "Font", "Subtype" => "Type3" },
                } }
            },
        ),
    ));
    out
}

#[test]
fn pdfs_that_are_odd_or_broken_come_in_or_fail_cleanly() {
    for (name, bytes) in pdf_corpus() {
        survives(name, move || import_pdf(bytes));
    }
    for (name, bytes) in [
        ("empty", Vec::new()),
        ("header only", b"%PDF-1.7\n".to_vec()),
        ("not a pdf", b"hello, world".to_vec()),
        (
            "random",
            (0..4096u32)
                .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
                .collect(),
        ),
    ] {
        survives(name, move || import_pdf(bytes));
    }
    // Every cut of a small valid file.
    let whole = pdf_corpus().remove(0).1;
    for end in (0..whole.len()).step_by(7) {
        let cut = whole[..end].to_vec();
        survives(&format!("cut at {end}"), move || import_pdf(cut));
    }
}

// ---------------------------------------------------------------------------------------------
// SVG

fn svg(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">{body}</svg>"#
    )
}

fn svg_corpus() -> Vec<(&'static str, String)> {
    let deep = |n: usize| {
        format!(
            "{}<line x1='0' y1='0' x2='5' y2='5' stroke='black'/>{}",
            "<g transform='translate(1 1)'>".repeat(n),
            "</g>".repeat(n)
        )
    };
    vec![
        (
            "shapes",
            svg(r##"<rect x="1" y="2" width="30" height="20" rx="3" fill="red" stroke="blue"/>
                <circle cx="50" cy="25" r="10"/><ellipse cx="70" cy="25" rx="10" ry="5"/>
                <polyline points="0,0 10,10 20,0" stroke="#000"/><polygon points="1 1 5 5 9 1"/>
                <path d="M0 0 L10 10 H20 V30 C1 2 3 4 5 6 S7 8 9 10 Q1 2 3 4 T5 6 A5 5 0 1 0 20 20 Z"
                    stroke-dasharray="3 1"/>
                <text x="5" y="5" font-size="4">Hi</text>"##),
        ),
        (
            "numbers",
            svg(r##"<path d="M1e400 1e400 L-1e400 0 L NaN inf l 1e308 1e308 1e308 1e308"/>
                <rect width="1e308" height="1e308" transform="scale(1e308)"/>
                <circle r="-5"/><circle r="NaN"/><ellipse rx="0" ry="1e-320"/>
                <path d="M0 0 A0 0 0 0 0 10 10 A1e-300 1e300 720 1 1 0 0"/>
                <line x1="0" y1="0" x2="1" y2="1" stroke-width="inf" stroke-dasharray="0 0"/>
                <line x1="0" y1="0" x2="1e300" y2="0" stroke-dasharray="1e-300"/>
                <g transform="matrix(0 0 0 0 0 0) rotate(NaN) skewX(90)"><rect width="1" height="1"/></g>"##),
        ),
        (
            "broken path data",
            svg(r##"<path d="M"/><path d="Z Z Z"/><path d="L 1"/><path d="M 1 2 3"/>
                <path d="M0,0c"/><path d="x y z"/><path d="M0 0 A 1 1 0 2 3 4 5"/>
                <polyline points="1"/><polygon points=""/><rect width="-1"/>"##),
        ),
        ("deep groups", svg(&deep(400))),
        (
            "use loops",
            svg(r##"<defs><g id="a"><use href="#b"/></g><g id="b"><use href="#a"/></g>
                <g id="c"><use href="#c"/><rect width="1" height="1"/></g></defs>
                <use href="#a"/><use href="#c"/><use href="#missing"/><use xlink:href="#a"/>"##),
        ),
        (
            "styles",
            svg(r##"<style>.a{stroke:url(#x);fill:rgb(300,-1,NaN)} /* unclosed </style>
                <rect class="a" width="5" height="5" style="stroke-width:;fill:#12;opacity:-3"/>"##),
        ),
        (
            "zero-size",
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0" viewBox="0 0 0 0"><rect width="1" height="1"/></svg>"#.into(),
        ),
        ("empty svg", r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.into()),
        (
            "entities",
            r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "<g><g><g><g>&b;</g></g></g></g>">
                <!ENTITY b "<rect width='1' height='1'/><rect width='2' height='2'/>">
                <!ENTITY c "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;"><!ENTITY d "&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;">
                <!ENTITY e "&f;&f;&f;&f;&f;&f;&f;&f;&f;&f;"><!ENTITY f "<line x2='1' stroke='red'/>">
                <!ENTITY loop "&loop;">]>
                <svg xmlns="http://www.w3.org/2000/svg">&a;&a;&c;</svg>"#.into(),
        ),
        (
            "entity loop",
            r#"<!DOCTYPE svg [<!ENTITY x "&y;"><!ENTITY y "&x;">]><svg xmlns="http://www.w3.org/2000/svg">&x;</svg>"#.into(),
        ),
    ]
}

#[test]
fn svgs_that_are_odd_or_broken_come_in_or_fail_cleanly() {
    for (name, text) in svg_corpus() {
        survives(name, move || import_svg(text));
    }
    for (name, text) in [
        ("empty", String::new()),
        ("not xml", "hello <<<".into()),
        ("not svg", "<html><body/></html>".into()),
        ("billion laughs", {
            let mut dtd = String::from("<!ENTITY l0 'lol'>");
            for i in 1..10 {
                dtd.push_str(&format!(
                    "<!ENTITY l{i} '{}'>",
                    format!("&l{};", i - 1).repeat(10)
                ));
            }
            format!(
                "<!DOCTYPE svg [{dtd}]><svg xmlns='http://www.w3.org/2000/svg'><text>&l9;</text></svg>"
            )
        }),
        ("nesting made by entities", {
            let mut dtd = String::from("<!ENTITY g0 '<line x2=\"1\" stroke=\"red\"/>'>");
            for i in 1..3000 {
                dtd.push_str(&format!("<!ENTITY g{i} '<g>&g{};</g>'>", i - 1));
            }
            format!("<!DOCTYPE svg [{dtd}]><svg xmlns='http://www.w3.org/2000/svg'>&g2999;</svg>")
        }),
        (
            "very deep xml",
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg'>{}{}</svg>",
                "<g>".repeat(5000),
                "</g>".repeat(5000)
            ),
        ),
    ] {
        survives(name, move || import_svg(text));
    }
    let whole = svg_corpus().remove(0).1;
    for end in (0..whole.len()).step_by(5) {
        let cut: String = whole.chars().take(end).collect();
        survives(&format!("cut at {end}"), move || import_svg(cut));
    }
}

// ---------------------------------------------------------------------------------------------
// DXF

fn dxf_doc(entities: &str, blocks: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
         0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn dxf_corpus() -> Vec<(&'static str, String)> {
    vec![
        (
            "entities",
            dxf_doc(
                "0\nLINE\n8\nA\n10\n0\n20\n0\n11\n100\n21\n0\n\
                 0\nCIRCLE\n8\nA\n10\n50\n20\n50\n40\n10\n\
                 0\nARC\n8\nA\n10\n0\n20\n0\n40\n5\n50\n0\n51\n90\n\
                 0\nLWPOLYLINE\n8\nB\n90\n3\n70\n1\n10\n0\n20\n0\n42\n0.5\n10\n10\n20\n0\n10\n10\n20\n10\n\
                 0\nELLIPSE\n10\n0\n20\n0\n11\n10\n21\n0\n40\n0.5\n41\n0\n42\n6.28\n\
                 0\nSPLINE\n71\n3\n72\n8\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n73\n4\n\
                 10\n0\n20\n0\n10\n1\n20\n1\n10\n2\n20\n0\n10\n3\n20\n1\n\
                 0\nTEXT\n10\n1\n20\n1\n40\n2.5\n1\nHello\n\
                 0\nMTEXT\n10\n1\n20\n1\n40\n2.5\n1\n\\PHi{\\fArial;x}\n\
                 0\nHATCH\n2\nSOLID\n70\n1\n91\n1\n92\n2\n72\n1\n73\n1\n93\n2\n10\n0\n20\n0\n10\n5\n20\n5\n\
                 0\nINSERT\n2\nCHAIR\n10\n5\n20\n5\n41\n2\n42\n2\n50\n30\n",
                "0\nBLOCK\n2\nCHAIR\n10\n0\n20\n0\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n",
            ),
        ),
        (
            "numbers",
            dxf_doc(
                "0\nLINE\n10\nnan\n20\ninf\n11\n1e308\n21\n-1e308\n\
                 0\nCIRCLE\n10\n0\n20\n0\n40\n-inf\n\
                 0\nCIRCLE\n10\n1e400\n20\n0\n40\n1e300\n\
                 0\nARC\n10\n0\n20\n0\n40\n1\n50\nnan\n51\n1e300\n\
                 0\nELLIPSE\n10\n0\n20\n0\n11\n0\n21\n0\n40\n0\n41\nnan\n42\ninf\n\
                 0\nLWPOLYLINE\n90\n-1\n10\n0\n20\n0\n42\ninf\n10\n0\n20\n0\n\
                 0\nSPLINE\n71\n99999\n72\n-1\n73\n4000000000\n40\nnan\n10\n0\n20\n0\n\
                 0\nINSERT\n2\nB\n41\n1e308\n42\n0\n50\nnan\n70\n100000\n71\n100000\n44\n1\n45\n1\n\
                 0\nTEXT\n40\nnan\n1\nx\n",
                "0\nBLOCK\n2\nB\n0\nLINE\n11\n1\n0\nENDBLK\n",
            ),
        ),
        (
            "blocks inserting each other",
            dxf_doc(
                "0\nINSERT\n2\nA\n10\n0\n20\n0\n",
                "0\nBLOCK\n2\nA\n0\nINSERT\n2\nB\n0\nLINE\n11\n1\n0\nENDBLK\n\
                 0\nBLOCK\n2\nB\n0\nINSERT\n2\nA\n0\nINSERT\n2\nB\n0\nENDBLK\n",
            ),
        ),
        (
            "deep blocks",
            {
                let mut blocks = String::new();
                for i in 0..300 {
                    blocks.push_str(&format!(
                        "0\nBLOCK\n2\nB{i}\n0\nINSERT\n2\nB{}\n41\n1.01\n0\nLINE\n11\n1\n0\nENDBLK\n",
                        i + 1
                    ));
                }
                dxf_doc("0\nINSERT\n2\nB0\n", &blocks)
            },
        ),
        (
            "broken groups",
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\nabc\n999999999999\nx\n-5\n\n0\nCIRCLE\n0\nENDSEC\n"
                .into(),
        ),
        ("no EOF", "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n".into()),
        ("odd lines", "0\nSECTION\n2\n".into()),
    ]
}

#[test]
fn dxfs_that_are_odd_or_broken_come_in_or_fail_cleanly() {
    for (name, text) in dxf_corpus() {
        survives(name, move || import_dxf(text));
    }
    for (name, text) in [
        ("empty", String::new()),
        ("one line", "0".into()),
        ("binary dxf", "AutoCAD Binary DXF\r\n\x1a\0garbage".into()),
        (
            "random",
            (0..4096u32)
                .map(|i| char::from(b' ' + ((i.wrapping_mul(2654435761) >> 13) % 90) as u8))
                .collect(),
        ),
    ] {
        survives(name, move || import_dxf(text));
    }
    let _ = dxf::decode(&[0xff, 0xfe, 0x00, 0xd8, 0x00]);
    let whole = dxf_corpus().remove(0).1;
    for end in (0..whole.len()).step_by(11) {
        let cut: String = whole.chars().take(end).collect();
        survives(&format!("cut at {end}"), move || import_dxf(cut));
    }
}

// ---------------------------------------------------------------------------------------------
// DWG (read through acadrust into the DXF importer)

#[cfg(feature = "dwg")]
#[test]
fn dwgs_that_are_broken_fail_cleanly() {
    use drift_io::dwg_import;
    let mut header = b"AC1032".to_vec();
    header.resize(0x100, 0);
    for (name, bytes) in [
        ("empty", Vec::new()),
        ("signature only", b"AC1032".to_vec()),
        ("header of zeros", header.clone()),
        ("old version", b"AC1009\0\0\0\0\0\0".to_vec()),
        ("random", {
            let mut r = header.clone();
            r.extend((0..8192u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8));
            r
        }),
    ] {
        survives(name, move || {
            let _ = dwg_import::is_dwg(&bytes);
            let _ = dwg_import::read(&bytes, 1.0);
        });
    }
}

/// A small valid DWG, saved by acadrust (2018 and 2000 formats).
#[cfg(feature = "dwg")]
fn dwg(version: acadrust::types::DxfVersion) -> Vec<u8> {
    use acadrust::entities::{Arc, Circle, EntityType, Line, Text};
    use acadrust::types::Vector3;
    let mut doc = acadrust::CadDocument::with_version(version);
    let entities = [
        EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 100.0, 50.0, 0.0)),
        EntityType::Circle(Circle::from_coords(20.0, 30.0, 0.0, 5.0)),
        EntityType::Arc(Arc {
            center: Vector3::new(50.0, 50.0, 0.0),
            radius: 10.0,
            start_angle: 0.0,
            end_angle: 1.5,
            ..Default::default()
        }),
        EntityType::Text(Text::with_value("Hello", Vector3::new(10.0, 10.0, 0.0)).with_height(2.5)),
    ];
    for e in entities {
        doc.add_entity(e).expect("the test drawing takes it");
    }
    acadrust::DwgWriter::write_to_vec(&doc).expect("the test DWG writes")
}

// ---------------------------------------------------------------------------------------------
// Mutations of the valid inputs above

fn mutate(mut bytes: Vec<u8>, edits: &[(usize, u8, u8)]) -> Vec<u8> {
    for &(at, kind, value) in edits {
        if bytes.is_empty() {
            bytes.push(value);
            continue;
        }
        let i = at % bytes.len();
        match kind % 6 {
            0 => bytes[i] = value,
            1 => bytes[i] ^= 1 << (value % 8),
            2 => {
                bytes.remove(i);
            }
            3 => bytes.insert(i, value),
            4 => bytes.truncate(i),
            // Repeat a stretch, to nest or multiply what is there.
            _ => {
                let end = (i + usize::from(value) + 1).min(bytes.len());
                let stretch = bytes[i..end].to_vec();
                for _ in 0..(value % 4 + 1) {
                    bytes.splice(i..i, stretch.iter().copied());
                }
            }
        }
    }
    bytes
}

/// Characters that mean something to the text formats, mutated in.
const TOKENS: &[&str] = &[
    "-", "e308", "nan", "inf", "0", "\n", "<", ">", "\"", "#", "M", "A", " ", "1e-320", "9999999",
];

fn mutate_text(text: &str, edits: &[(usize, u8, u8)]) -> String {
    let mut s = text.to_owned();
    for &(at, kind, value) in edits {
        let mut i = at % (s.len() + 1);
        while !s.is_char_boundary(i) {
            i -= 1;
        }
        match kind % 3 {
            0 => s.insert_str(i, TOKENS[usize::from(value) % TOKENS.len()]),
            1 => {
                let mut end = (i + usize::from(value % 8) + 1).min(s.len());
                while !s.is_char_boundary(end) {
                    end += 1;
                }
                s.replace_range(i..end, "");
            }
            _ => {
                let mut end = (i + usize::from(value) + 1).min(s.len());
                while !s.is_char_boundary(end) {
                    end += 1;
                }
                let stretch = s[i..end].to_owned();
                s.insert_str(i, &stretch.repeat(usize::from(value % 3) + 1));
            }
        }
    }
    s
}

fn edits() -> impl Strategy<Value = Vec<(usize, u8, u8)>> {
    prop::collection::vec((any::<usize>(), any::<u8>(), any::<u8>()), 1..8)
}

proptest! {
    // 48 cases each by default; `PROPTEST_CASES=5000` for a longer run.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn mutated_pdf_content_never_panics(which in 0usize..64, edits in edits(), whole_file in any::<bool>()) {
        let corpus = pdf_corpus();
        let (name, bytes) = &corpus[which % corpus.len()];
        let bytes = if whole_file {
            mutate(bytes.clone(), &edits)
        } else {
            // Mutate the page's drawing and keep the file readable.
            let content = mutate(b"0.5 w 1 0 0 RG 10 10 m 100 10 l 20 30 40 30 50 10 c h S \
                0 0 1 rg 20 20 50 50 re f q 2 0 0 2 5 5 cm [3 2] 0 d 0 0 m 9 9 l S Q".to_vec(), &edits);
            pdf(&content, A4, 0, |_| dictionary! {})
        };
        survives(name, move || import_pdf(bytes));
    }

    #[test]
    fn mutated_svg_never_panics(which in 0usize..64, edits in edits()) {
        let corpus = svg_corpus();
        let (name, text) = &corpus[which % corpus.len()];
        let text = mutate_text(text, &edits);
        survives(name, move || import_svg(text));
    }

    #[cfg(feature = "dwg")]
    #[test]
    fn mutated_dwg_never_panics(old in any::<bool>(), edits in edits()) {
        use acadrust::types::DxfVersion;
        let bytes = mutate(dwg(if old { DxfVersion::AC1015 } else { DxfVersion::AC1032 }), &edits);
        survives("dwg", move || {
            if let Ok(import) = drift_io::dwg_import::read(&bytes, 1.0) {
                add(&import);
            }
        });
    }

    #[test]
    fn mutated_dxf_never_panics(which in 0usize..64, edits in edits()) {
        let corpus = dxf_corpus();
        let (name, text) = &corpus[which % corpus.len()];
        let text = mutate_text(text, &edits);
        survives(name, move || import_dxf(text));
    }
}
