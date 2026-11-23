//! The solver bar's sketch corpus (#187), built from its scripts.
//!
//! Each sketch in `scripts/solver-bar/corpus/` is a Drift script (`<name>.driftscript`) and
//! the drawing it makes (`<name>.drift`). drift-edit's drag sweep (`drag_props::corpus`) reads
//! the drawings; the script interpreter lives here, so this is where they are built, and where
//! a drawing that no longer matches its script is caught:
//!
//! ```text
//! cargo test -p drift-app --lib corpus_build                        # checks
//! DRIFT_CORPUS_WRITE=1 cargo test -p drift-app --lib corpus_build   # writes the drawings
//! ```
//!
//! A script's `#>` lines (comments to the script itself) are what the script language has no
//! statement for, run after it in order:
//!
//! ```text
//! #> el = add_geometry {"items":[{"type":"ellipse","center":[0,0],"major":[30,0],"minor_radius":20}]}
//! #> add_relations {"relations":[{"type":"coradial","a":"$c1","b":"$c2"}]}
//! #> constraint smooth $p $h1 $h2
//! ```
//!
//! The first two are tool calls as the AI link takes them. `$name` is a name of the script (or
//! one bound by an earlier `name = add_geometry …`: the first shape made, and `name.<key>` for
//! each id in its result, and `name.c1`, `name.c2` for a curve's handles). `constraint` adds a relation no tool makes: `smooth p a b`,
//! `corner p a b`, `equal_distance a b c d`, `midpoint_of p a b`, `fix_radius c`, `fix_x p`,
//! `fix_y p`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use drift_core::{Constraint, EntityId, EntityKind};
use serde_json::{Value, json};

use crate::agent::Session;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/solver-bar/corpus")
}

fn entity(names: &BTreeMap<String, String>, token: &str) -> Result<EntityId, String> {
    let name = token.strip_prefix('$').unwrap_or(token);
    let id = names
        .get(name)
        .map(String::as_str)
        .or_else(|| name.starts_with('e').then_some(name))
        .ok_or_else(|| format!("no name `{name}`"))?;
    id[1..]
        .parse()
        .map(EntityId)
        .map_err(|_| format!("`{name}` is {id}, not an entity"))
}

/// `text` with each `$name` replaced by its id.
fn substituted(names: &BTreeMap<String, String>, text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || "._[]".contains(c)))
            .unwrap_or(tail.len());
        let name = &tail[..end];
        out.push_str(names.get(name).ok_or_else(|| format!("no name `{name}`"))?);
        rest = &tail[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A relation no tool makes, from `kind name…`.
fn raw_constraint(
    s: &Session,
    names: &BTreeMap<String, String>,
    words: &[&str],
) -> Result<Constraint, String> {
    let doc = &s.editor.doc;
    let e = |k: usize| {
        words
            .get(k)
            .ok_or_else(|| format!("`{}` needs more names", words[0]))
            .and_then(|w| entity(names, w))
    };
    let at = |p: EntityId| doc.point(p).map_err(|e| e.to_string());
    Ok(match words[0] {
        "smooth" => Constraint::Smooth {
            point: e(1)?,
            a: e(2)?,
            b: e(3)?,
        },
        "corner" => Constraint::Corner {
            point: e(1)?,
            a: e(2)?,
            b: e(3)?,
        },
        "equal_distance" => Constraint::EqualDistance {
            a: e(1)?,
            b: e(2)?,
            c: e(3)?,
            d: e(4)?,
        },
        "midpoint_of" => Constraint::MidpointOf {
            point: e(1)?,
            a: e(2)?,
            b: e(3)?,
        },
        "fix_radius" => {
            let curve = e(1)?;
            let radius = drift_solver::radius_of(doc, curve)
                .ok_or_else(|| format!("{curve} has no radius"))?;
            Constraint::FixRadius { curve, radius }
        }
        "fix_x" => {
            let point = e(1)?;
            Constraint::FixX {
                point,
                x: at(point)?.x,
            }
        }
        "fix_y" => {
            let point = e(1)?;
            Constraint::FixY {
                point,
                y: at(point)?.y,
            }
        }
        other => return Err(format!("unknown constraint `{other}`")),
    })
}

/// Build one sketch from its script: the drawing, as saved.
fn build(script: &str) -> Result<(String, Session), String> {
    let mut s = Session::new();
    let out = s.call("run_script", &json!({ "script": script }));
    if out.is_error {
        return Err(out.json["error"]
            .as_str()
            .map_or_else(|| out.json.to_string(), str::to_owned));
    }
    let mut names: BTreeMap<String, String> = out.json["names"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
        .collect();
    for (n, line) in script.lines().enumerate() {
        let Some(directive) = line.trim().strip_prefix("#>") else {
            continue;
        };
        let directive = directive.trim();
        let fail = |e: String| format!("line {}: {e}\n  {line}", n + 1);
        if let Some(rest) = directive.strip_prefix("constraint ") {
            let words: Vec<&str> = rest.split_whitespace().collect();
            let c = raw_constraint(&s, &names, &words).map_err(fail)?;
            s.editor
                .add_constraints("Relation", &[c])
                .map_err(|e| fail(e.to_string()))?;
            continue;
        }
        let (bind, call) = match directive.split_once('=') {
            Some((name, call)) if !name.contains('{') && name.split_whitespace().count() == 1 => {
                (Some(name.trim()), call.trim())
            }
            _ => (None, directive),
        };
        let (tool, args) = call
            .split_once(char::is_whitespace)
            .ok_or_else(|| fail("a tool and its arguments".into()))?;
        let args = substituted(&names, args).map_err(fail)?;
        let args: Value = serde_json::from_str(&args).map_err(|e| fail(e.to_string()))?;
        let out = s.call(tool, &args);
        if out.is_error {
            return Err(fail(out.json.to_string()));
        }
        for key in ["relations_skipped", "skipped"] {
            if let Some(skipped) = out.json.get(key) {
                return Err(fail(format!("skipped: {skipped}")));
            }
        }
        if let Some(bind) = bind {
            let made = out.json["created"]
                .get(0)
                .and_then(Value::as_object)
                .ok_or_else(|| fail(format!("nothing made to bind: {}", out.json)))?;
            for (k, v) in made {
                if let Some(id) = v.as_str().filter(|v| v.starts_with('e')) {
                    let name = if k == "id" {
                        bind.to_owned()
                    } else {
                        format!("{bind}.{k}")
                    };
                    names.insert(name, id.to_owned());
                }
            }
            // (A curve's handles, which the tool's result doesn't list.)
            if let Ok(id) = entity(&names, bind)
                && let Some(EntityKind::Bezier { c1, c2, .. }) =
                    s.editor.doc.get(id).map(|e| e.kind)
            {
                names.insert(format!("{bind}.c1"), c1.to_string());
                names.insert(format!("{bind}.c2"), c2.to_string());
            }
        }
    }
    s.editor.sync();
    let unmet = drift_solver::unmet(&s.editor.doc, 1e-8);
    if !unmet.is_empty() {
        return Err(format!("relations unmet as built: {unmet:?}"));
    }
    Ok((saved(&s.editor.doc), s))
}

/// A drawing as the corpus stores it: its geometry and relations, with no file's own details.
fn saved(doc: &drift_core::Document) -> String {
    drift_io::save(doc, &drift_io::Metadata::default())
}

/// Whether two saved drawings are the same drawing: the same in everything, their numbers to
/// a millionth (a script's solve ends a few ulps apart from one platform's `sin` and `cos`
/// to another's, and the drawings are built on one and checked on all).
fn same_drawing(a: &str, b: &str) -> bool {
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0),
                _ => x == y,
            },
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
            }
            _ => a == b,
        }
    }
    match (
        serde_json::from_str::<Value>(a),
        serde_json::from_str::<Value>(b),
    ) {
        (Ok(a), Ok(b)) => same(&a, &b),
        _ => false,
    }
}

/// What a sketch covers: its relation kinds, and which kinds of curve it draws.
fn covers(doc: &drift_core::Document, kinds: &mut BTreeMap<String, usize>) {
    for (_, c) in doc.constraints() {
        let name = format!("{c:?}");
        let name = name.split([' ', '{']).next().unwrap_or_default().to_owned();
        *kinds.entry(name).or_default() += 1;
    }
    for (_, e) in doc.iter() {
        let name = match e.kind {
            EntityKind::Circle { .. } => "circle",
            EntityKind::Ellipse { .. } => "ellipse",
            EntityKind::EllipseArc { .. } => "ellipse arc",
            EntityKind::Arc { .. } => "arc",
            EntityKind::Bezier { .. } => "bezier",
            EntityKind::Line { .. } => "line",
            _ => continue,
        };
        *kinds.entry(format!("({name})")).or_default() += 1;
    }
}

/// Every relation and dimension kind the corpus must hold at least once (the audit's known
/// gaps among them), and the curves.
const MUST_COVER: [&str; 33] = [
    "Coincident",
    "Horizontal",
    "Vertical",
    "HorizontalPoints",
    "VerticalPoints",
    "Parallel",
    "Perpendicular",
    "Equal",
    "Tangent",
    "Concentric",
    "Coradial",
    "Collinear",
    "PointOnCurve",
    "Symmetric",
    "Smooth",
    "Midpoint",
    "MidpointOf",
    "EqualDistance",
    "Corner",
    "Fix",
    "FixX",
    "FixY",
    "FixRadius",
    "Distance",
    "Radius",
    "Diameter",
    "HorizontalDistance",
    "VerticalDistance",
    "PointLineDistance",
    "Angle",
    "(circle)",
    "(ellipse)",
    "(arc)",
];

/// Every script builds, each committed drawing is what its script makes (or, with
/// `DRIFT_CORPUS_WRITE`, is written), the corpus has at least 40 sketches, and together they
/// hold every relation and dimension kind, circles, arcs and ellipses.
#[test]
fn corpus_build() {
    let dir = corpus_dir();
    let write = std::env::var_os("DRIFT_CORPUS_WRITE").is_some();
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|f| Some(f.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "driftscript"))
        .collect();
    scripts.sort();
    let mut kinds = BTreeMap::new();
    let mut failures = Vec::new();
    let mut drawings = 0;
    for path in &scripts {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let script = std::fs::read_to_string(path).unwrap();
        let (built, s) = match build(&script) {
            Ok(built) => built,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        covers(&s.editor.doc, &mut kinds);
        let dof = s.editor.analysis().dof;
        if std::env::var_os("DRIFT_CORPUS_LIST").is_some() {
            println!(
                "{name}: {} entities, {} relations, dof {dof:?}",
                s.editor.doc.iter().count(),
                s.editor.doc.constraints().count()
            );
        }
        if dof == Some(0) {
            failures.push(format!("{name}: fully defined, nothing to drag"));
        }
        let file = path.with_extension("drift");
        if write {
            std::fs::write(&file, built.replace("\r\n", "\n")).unwrap();
            drawings += 1;
            continue;
        }
        let committed = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) => {
                failures.push(format!(
                    "{name}: no drawing ({e}): run with DRIFT_CORPUS_WRITE=1"
                ));
                continue;
            }
        };
        match drift_io::load(&committed) {
            // (Both as this build saves them: a format change alone is no difference.)
            Ok((doc, _)) if same_drawing(&saved(&doc), &built) => drawings += 1,
            Ok(_) => failures.push(format!(
                "{name}: the drawing isn't what its script makes: run with DRIFT_CORPUS_WRITE=1"
            )),
            Err(e) => failures.push(format!("{name}: its drawing doesn't load: {e:?}")),
        }
    }
    // (The drawings with no script: real drawings kept as fixtures.)
    let fixtures = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|f| Some(f.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "drift"))
        .filter(|p| !p.with_extension("driftscript").exists());
    for path in fixtures {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        match drift_io::load(&std::fs::read_to_string(&path).unwrap()) {
            Ok((doc, _)) => {
                covers(&doc, &mut kinds);
                drawings += 1;
            }
            Err(e) => failures.push(format!("{name}: doesn't load: {e:?}")),
        }
    }
    if std::env::var_os("DRIFT_CORPUS_LIST").is_some() {
        println!("{kinds:#?}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        drawings >= 40,
        "{drawings} sketches: the corpus is at least 40"
    );
    let missing: Vec<&str> = MUST_COVER
        .into_iter()
        .filter(|k| !kinds.contains_key(*k))
        .collect();
    assert!(missing.is_empty(), "no sketch has {missing:?}");
}

/// Bring a real drawing into the corpus as a fixture: `DRIFT_CORPUS_IMPORT=<file>` and
/// `DRIFT_CORPUS_AS=<name>` write its geometry and relations (none of the file's own
/// details: its project, logo, fonts, sheets, texts and fills are left out) as
/// `<name>.drift`.
#[test]
#[ignore = "a tool: needs DRIFT_CORPUS_IMPORT and DRIFT_CORPUS_AS"]
fn corpus_import() {
    let (Ok(from), Ok(name)) = (
        std::env::var("DRIFT_CORPUS_IMPORT"),
        std::env::var("DRIFT_CORPUS_AS"),
    ) else {
        return;
    };
    let (doc, _) = drift_io::load(&std::fs::read_to_string(from).unwrap()).unwrap();
    let mut bare = drift_core::Document::new();
    bare.begin();
    let mut ids = BTreeMap::new();
    // (Points first: curves name them.)
    for points in [true, false] {
        for (id, e) in doc.iter().filter(|(_, e)| e.kind.is_point() == points) {
            let at = |p: EntityId| ids[&p];
            let made = match e.kind {
                EntityKind::Point(p) => Some(bare.add_point(p)),
                EntityKind::Line { a, b } => bare.add_line(at(a), at(b)).ok(),
                EntityKind::Arc { center, start, end } => {
                    bare.add_arc_exact(at(center), at(start), at(end)).ok()
                }
                EntityKind::Circle { center, radius } => bare.add_circle(at(center), radius).ok(),
                EntityKind::Ellipse {
                    center,
                    major,
                    minor,
                } => bare.add_ellipse(at(center), at(major), at(minor)).ok(),
                _ => None,
            };
            if let Some(made) = made {
                ids.insert(id, made);
            }
        }
    }
    let mut kept = 0;
    for (_, c) in doc.constraints() {
        if c.entities().all(|e| ids.contains_key(&e)) {
            kept += usize::from(bare.add_constraint(c.map_entities(|e| ids[&e])).is_ok());
        }
    }
    bare.commit("Import");
    println!(
        "{} entities and {kept} of {} relations kept",
        ids.len(),
        doc.constraints().count()
    );
    let file = corpus_dir().join(format!("{name}.drift"));
    std::fs::write(file, saved(&bare).replace("\r\n", "\n")).unwrap();
}
