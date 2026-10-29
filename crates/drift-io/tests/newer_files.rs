//! A file from a newer Drift: what this build doesn't know is passed over when reading and
//! written back when saving (`carry.rs`), and a file this build can't read correctly (its
//! `reader` is newer) is refused. See `docs/file-format.md`, section 4.
#![cfg(not(feature = "demo"))]

use std::path::PathBuf;

use drift_core::{DVec2, EntityId};
use drift_io::{
    FORMAT_VERSION, LoadError, READER_VERSION, load, load_with_notes, save, save_checked,
};

fn current_fixture() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/v{FORMAT_VERSION}.drift"));
    std::fs::read_to_string(path)
        .expect("the current fixture exists")
        .replace("\r\n", "\n")
}

/// Replace `from` (which must be in `text` once) with `to`.
fn edit(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1)
}

/// The current fixture as a newer Drift (at `version`) might write it: an unknown table of
/// records and an unknown table of settings, and unknown fields on many kinds of record, at
/// the start, in the middle and at the end, some of them nested.
fn newer_file(version: u32) -> String {
    let mut t = current_fixture();
    t = edit(
        &t,
        &format!("  \"version\": {FORMAT_VERSION},\n"),
        &format!("  \"version\": {version},\n"),
    );
    t = edit(
        &t,
        &format!("  \"reader\": {READER_VERSION},\n"),
        &format!(
            "  \"reader\": {READER_VERSION},\n  \"wall_types\": {{\"brick\":{{\"width\":215.0}}}},\n"
        ),
    );
    t = edit(
        &t,
        r#"{"title":"Fixture","units":"mm"}"#,
        r#"{"title":"Fixture","units":"mm","sheet_set":"Planning"}"#,
    );
    t = edit(
        &t,
        r#""rules": {"areas_by_layer":true,"#,
        r#""rules": {"areas_by_layer":true,"wall_join":"miter","#,
    );
    t = edit(
        &t,
        r##""fill":"#d0c0a0","weight":0.5,"linetype":"continuous"}"##,
        r##""fill":"#d0c0a0","weight":0.5,"linetype":"continuous","pen":7}"##,
    );
    t = edit(
        &t,
        r#"{"type":"line","id":"e5","a":"e1""#,
        r#"{"type":"line","id":"e5","thickness":200.0,"a":"e1""#,
    );
    t = edit(
        &t,
        r#"{"type":"point","id":"e9","at":[3600.0,2400.0]}"#,
        r#"{"type":"point","id":"e9","at":[3600.0,2400.0],"tag":{"k":[1,2],"s":"é \"q\""}}"#,
    );
    t = edit(
        &t,
        r#"{"type":"circle","id":"e10","center":"e9","radius":150.0}"#,
        r#"{"type":"circle","id":"e10","center":"e9","radius":150.0,"cut":true}"#,
    );
    t = edit(
        &t,
        r#"{"type":"fix","id":"c1","point":"e1","at":[0.0,0.0]}"#,
        r#"{"priority":1,"type":"fix","id":"c1","point":"e1","at":[0.0,0.0]}"#,
    );
    t = edit(
        &t,
        r#""opacity":0.5,"path":"p1"}"#,
        r#""opacity":0.5,"path":"p1","blend":"multiply"}"#,
    );
    t = edit(
        &t,
        r#""Oak floor","at":[2500.0,3400.0],"leaders":[{"to":{"at":[2100.0,1500.0]}}"#,
        r#""Oak floor","at":[2500.0,3400.0],"leaders":[{"to":{"at":[2100.0,1500.0],"snap":"mid"}}"#,
    );
    t = edit(
        &t,
        r#""title_block":"modern","fields":{}}"#,
        r#""title_block":"modern","fields":{"reviewer":"CL"},"stamp":true}"#,
    );
    t = edit(
        &t,
        r#""items":[{"type":"line","a":[0.0,0.0],"b":[450.0,0.0]}"#,
        r#""items":[{"type":"line","a":[0.0,0.0],"b":[450.0,0.0],"hidden":true}"#,
    );
    t = edit(
        &t,
        r#"{"id":"t11","text":"LIVING","#,
        r#"{"id":"t11","rotation":90.0,"text":"LIVING","#,
    );
    // A whole-sketch option is a Drift file of its own.
    t = edit(
        &t,
        r#""sketch":{"annotations""#,
        r#""sketch":{"aisles":[{"id":"a1"}],"annotations""#,
    );
    t = edit(
        &t,
        "  \"constraints\": [\n",
        "  \"stairs\": [\n    {\"id\":\"s1\",\"from\":\"e1\",\"to\":\"e2\"},\n    \
         {\"id\":\"s2\",\"from\":\"e2\",\"to\":\"e3\"}\n  ],\n  \"constraints\": [\n",
    );
    t
}

#[test]
fn a_newer_file_opens_and_saves_unchanged_byte_for_byte() {
    for version in [FORMAT_VERSION + 1, FORMAT_VERSION] {
        let text = newer_file(version);
        let (doc, meta, notes) =
            load_with_notes(&text).unwrap_or_else(|e| panic!("v{version} doesn't open: {e}"));
        assert!(notes.is_empty());
        assert!(meta.carried.any(), "v{version}: the unknown data is kept");
        // It reads as the drawing does without the additions.
        let (plain, _) = load(&current_fixture()).unwrap();
        assert_eq!(doc, plain, "v{version}");
        let saved = save_checked(&doc, &meta);
        assert!(!saved.loses_newer_data);
        assert_eq!(saved.text, text, "v{version}: saved unchanged");
    }
}

#[test]
fn a_changed_record_keeps_what_this_build_doesnt_know() {
    let text = newer_file(FORMAT_VERSION + 1);
    let (mut doc, meta) = load(&text).unwrap();
    doc.set_point(EntityId(9), DVec2::new(3700.0, 2400.0))
        .unwrap();
    let saved = save_checked(&doc, &meta);
    assert!(!saved.loses_newer_data);
    // Only the moved point's line changed, and it kept its unknown field after `at`.
    let changed: Vec<(&str, &str)> = text
        .lines()
        .zip(saved.text.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        [(
            r#"    {"type":"point","id":"e9","at":[3600.0,2400.0],"tag":{"k":[1,2],"s":"é \"q\""}},"#,
            r#"    {"type":"point","id":"e9","at":[3700.0,2400.0],"tag":{"k":[1,2],"s":"é \"q\""}},"#
        )]
    );
    assert_eq!(text.lines().count(), saved.text.lines().count());

    // Saved again (in this build or the newer one), it is the same file.
    let (doc, meta) = load(&saved.text).unwrap();
    assert!(meta.carried.any());
    assert_eq!(save(&doc, &meta), saved.text);
}

#[test]
fn a_changed_layer_and_metadata_keep_their_unknown_fields() {
    let text = newer_file(FORMAT_VERSION + 1);
    let (mut doc, mut meta) = load(&text).unwrap();
    let (id, mut layer) = doc
        .layers()
        .find(|(_, l)| l.name == "Walls")
        .map(|(id, l)| (id, l.clone()))
        .unwrap();
    layer.name = "Masonry".into();
    doc.set_layer(id, layer);
    meta.generator = Some("Drift test".into());
    let saved = save_checked(&doc, &meta);
    assert!(!saved.loses_newer_data);
    assert!(saved.text.contains(r#""name":"Masonry","#));
    assert!(saved.text.contains(r#""linetype":"continuous","pen":7}"#));
    assert!(
        saved.text.contains(
            r#"{"title":"Fixture","units":"mm","sheet_set":"Planning","generator":"Drift test"}"#
        ) || saved.text.contains(
            r#"{"title":"Fixture","units":"mm","generator":"Drift test","sheet_set":"Planning"}"#
        ),
        "{}",
        saved.text.lines().nth(4).unwrap_or_default()
    );
    // What this build knows reads back as edited.
    let (again, _) = load(&saved.text).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn deleting_a_record_with_unknown_fields_warns_and_keeps_the_rest() {
    let text = newer_file(FORMAT_VERSION + 1);
    let (mut doc, meta) = load(&text).unwrap();
    // The text has no unknown fields: deleting it loses nothing.
    let text_id = doc.texts().next().unwrap().0;
    let (mut plain_delete, _) = load(&newer_file(FORMAT_VERSION + 1)).unwrap();
    let other = plain_delete.instances().next().unwrap().0;
    plain_delete.remove_instance(other);
    assert!(!save_checked(&plain_delete, &meta).loses_newer_data);
    // The text had `rotation`, which goes with it.
    doc.remove_text(text_id);
    let saved = save_checked(&doc, &meta);
    assert!(
        saved.loses_newer_data,
        "a record with unknown fields was deleted"
    );
    assert!(!saved.text.contains("rotation"));
    // Once written, the next save compares with that file: no warning again.
    let mut meta = meta;
    meta.carried = saved.carried.clone();
    assert!(meta.carried.any());
    let again = save_checked(&doc, &meta);
    assert!(!again.loses_newer_data);
    assert_eq!(again.text, saved.text);
    // Everything else from the newer build is still there.
    for kept in [
        r#""wall_types": {"brick":{"width":215.0}},"#,
        r#"{"id":"s1","from":"e1","to":"e2"}"#,
        r#""pen":7"#,
        r#""thickness":200.0"#,
        r#""cut":true"#,
        r#""priority":1"#,
        r#""blend":"multiply""#,
        r#""snap":"mid""#,
        r#""stamp":true"#,
        r#""hidden":true"#,
        r#""wall_join":"miter""#,
        r#""aisles":[{"id":"a1"}]"#,
    ] {
        assert!(saved.text.contains(kept), "{kept}");
    }
    load(&saved.text).unwrap();
}

#[test]
fn a_file_whose_reader_is_newer_is_refused() {
    let newer = FORMAT_VERSION + 1;
    let text = current_fixture();
    let version = format!("  \"version\": {FORMAT_VERSION},\n");
    let reader = format!("  \"reader\": {READER_VERSION},\n");
    // A newer version that this build still reads.
    let ok = edit(&text, &version, &format!("  \"version\": {},\n", newer + 5));
    load(&ok).unwrap();
    // A newer reader.
    let refused = edit(&ok, &reader, &format!("  \"reader\": {newer},\n"));
    let err = load(&refused).unwrap_err();
    assert!(matches!(err, LoadError::UnsupportedVersion(v) if v == newer + 5));
    assert!(err.to_string().contains("please update Drift"), "{err}");
    // A newer version without a reader (as every file before 42) is refused as before.
    let old_style = edit(
        &edit(&text, &reader, ""),
        &version,
        &format!("  \"version\": {newer},\n"),
    );
    assert!(matches!(
        load(&old_style),
        Err(LoadError::UnsupportedVersion(v)) if v == newer
    ));
    // A reader that isn't a version.
    for bad in ["0", "\"42\"", "-1", "1.5"] {
        let bad_reader = edit(&text, &reader, &format!("  \"reader\": {bad},\n"));
        assert!(
            matches!(load(&bad_reader), Err(LoadError::NotADriftFile)),
            "reader {bad}"
        );
    }
}

#[test]
fn an_older_file_carries_nothing() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v41.drift");
    let text = std::fs::read_to_string(path).unwrap();
    let (_, meta) = load(&text).unwrap();
    assert!(!meta.carried.any());
}

/// The current fixture's newest entity, and the fixture with `to` put in after `reader` and
/// `field` added to its text.
fn naming_the_newest_entity(table: &str, field: &str) -> (EntityId, String) {
    let text = current_fixture();
    let (plain, _) = load(&text).unwrap();
    let newest = plain.iter().map(|(id, _)| id).max().unwrap();
    let reader = format!("  \"reader\": {READER_VERSION},\n");
    let mut t = text.clone();
    if !table.is_empty() {
        let table = table.replace("NEWEST", &newest.to_string());
        t = edit(&t, &reader, &format!("{reader}{table}"));
    }
    if !field.is_empty() {
        let field = field.replace("NEWEST", &newest.to_string());
        t = edit(
            &t,
            r#"{"id":"t11","text":"LIVING","#,
            &format!(r#"{{"id":"t11",{field},"text":"LIVING","#),
        );
    }
    (newest, t)
}

/// Unknown data that names something deleted since is kept, but the save warns (the newer
/// build drops the reference), and the id it names is never given to anything made here, in
/// this session or a later one, so the reference never points at something else.
#[test]
fn unknown_data_naming_a_deleted_record_warns_and_its_id_is_never_used_again() {
    for (table, field) in [
        (
            "  \"marks\": [\n    {\"id\":\"m1\",\"on\":\"NEWEST\"}\n  ],\n",
            "",
        ),
        ("", r#""near":{"NEWEST":true}"#),
    ] {
        let (newest, text) = naming_the_newest_entity(table, field);
        let names = |text: &str| text.contains(&format!("\"{newest}\""));
        let (mut doc, meta) = load(&text).unwrap();
        assert!(meta.carried.any());
        // Saved unchanged, or after an edit that keeps it: no warning.
        assert!(!save_checked(&doc, &meta).loses_newer_data);
        let (p, _) = doc.iter().find(|(_, e)| e.kind.is_point()).unwrap();
        let at = doc.point(p).unwrap();
        doc.set_point(p, at + DVec2::X).unwrap();
        assert!(!save_checked(&doc, &meta).loses_newer_data);
        // Deleted: kept as it was, with the warning.
        doc.delete([newest]);
        assert!(doc.get(newest).is_none());
        let saved = save_checked(&doc, &meta);
        assert!(
            saved.loses_newer_data,
            "{table}{field}: deleting what it names warns"
        );
        assert!(names(&saved.text), "{table}{field}: what names it is kept");
        // Made in this session after the delete: a new id.
        let made = doc.add_point(DVec2::ZERO);
        assert!(made > newest, "{made}");
        // Opened again (here or by the newer build), something new doesn't take its id.
        let (mut again, again_meta) = load(&saved.text).unwrap();
        assert!(again.get(newest).is_none());
        let made = again.add_point(DVec2::ZERO);
        assert_ne!(
            made, newest,
            "{table}{field}: the named id was handed out again"
        );
        let resaved = save_checked(&again, &again_meta);
        assert!(
            !resaved.loses_newer_data,
            "already left out: no warning again"
        );
        assert!(names(&resaved.text));
        let (last, _) = load(&resaved.text).unwrap();
        assert!(last.get(newest).is_none(), "nothing has the id it names");
    }
}

/// An unknown table a newer build laid out another way is written back one record per line,
/// as this build lays out every file, once anything around it changed.
#[test]
fn an_unknown_table_written_another_way_is_laid_out_as_files_are() {
    let reader = format!("  \"reader\": {READER_VERSION},\n");
    let text = edit(
        &current_fixture(),
        &reader,
        &format!(
            "{reader}  \"marks\":[\n\t{{ \"id\": \"m1\" }} ,\n\n        {{\"id\":\"m2\",\"x\":[1,\n 2]}}\n   ]  ,\n  \"note\" :  \"x\",\n"
        ),
    );
    let (mut doc, meta) = load(&text).unwrap();
    assert!(meta.carried.any());
    assert_eq!(save(&doc, &meta), text, "unchanged: as it was");
    let (p, _) = doc.iter().find(|(_, e)| e.kind.is_point()).unwrap();
    let at = doc.point(p).unwrap();
    doc.set_point(p, at + DVec2::X).unwrap();
    let saved = save(&doc, &meta);
    assert!(
        saved.contains(&format!(
            "{reader}  \"marks\": [\n    {{ \"id\": \"m1\" }},\n    {{\"id\":\"m2\",\"x\":[1,\n 2]}}\n  ],\n  \"note\": \"x\",\n"
        )),
        "{}",
        saved.lines().take(8).collect::<Vec<_>>().join("\n")
    );
    load(&saved).unwrap();
}
