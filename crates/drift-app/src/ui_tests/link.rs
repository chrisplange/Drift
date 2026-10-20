use super::*;
use crate::agent::link::LinkClient;
use serde_json::json;

fn link_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("drift-ui-link-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run frames until the assistant's thread is done with its calls.
fn serve_until_done<T>(
    h: &mut Harness<'static, DriftApp>,
    assistant: std::thread::JoinHandle<T>,
) -> T {
    for _ in 0..2000 {
        if assistant.is_finished() {
            break;
        }
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assistant.join().expect("the assistant's calls")
}

#[test]
fn an_assistant_draws_in_the_open_window_through_the_link() {
    let Some(mut h) = harness() else { return };
    let dir = link_dir("draw");
    {
        let app = h.state_mut();
        app.link.dir = Some(dir.clone());
        app.ai_link = true;
    }
    h.run();
    assert_eq!(
        crate::agent::link::find(&dir).len(),
        1,
        "the window offers its link"
    );
    let d = dir.clone();
    let assistant = std::thread::spawn(move || {
        let mut c = LinkClient::connect(&d).expect("connect");
        let made = c
            .call(
                "add_geometry",
                &json!({ "items": [{ "type": "rectangle", "corner": [0, 0], "width": 4000, "height": 3000 }] }),
                None,
            )
            .expect("call");
        // An unsaved drawing is never replaced from outside.
        let new = c.call("new_drawing", &json!({}), None).expect("call");
        let picture = c
            .call("render", &json!({ "width": 300 }), None)
            .expect("call");
        (made, new, picture, c)
    });
    let (made, new, picture, mut client) = serve_until_done(&mut h, assistant);
    assert!(!made.is_error, "{}", made.json);
    assert!(new.is_error, "{}", new.json);
    assert!(
        new.json["error"].as_str().unwrap().contains("unsaved"),
        "{}",
        new.json
    );
    assert!(picture.image.is_some_and(|png| png.starts_with(b"\x89PNG")));
    {
        let app = h.state_mut();
        let lines = app
            .editor
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Line { .. }))
            .count();
        assert_eq!(lines, 4, "the rectangle is in the window's drawing");
        assert_eq!(app.editor.selection.len(), 4, "and picked, so it shows");
        assert_eq!(
            app.editor.history.undo_label(),
            Some("Assistant: add geometry")
        );
        assert_eq!(app.link.server.as_ref().map(|s| s.connected()), Some(1));
        // The user undoes it like any edit of their own.
        assert!(app.editor.undo());
        assert!(app.editor.doc.is_empty());
    }
    // Turning the link off closes it: the assistant's next call fails, and the file goes.
    h.state_mut().ai_link = false;
    h.run();
    assert!(crate::agent::link::find(&dir).is_empty());
    assert!(client.call("analyze", &json!({}), None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_assistant_saves_and_opens_through_the_window() {
    let Some(mut h) = harness() else { return };
    let dir = link_dir("files");
    {
        let app = h.state_mut();
        app.link.dir = Some(dir.clone());
        app.ai_link = true;
    }
    h.run();
    let d = dir.clone();
    let assistant = std::thread::spawn(move || {
        let mut c = LinkClient::connect(&d).expect("connect");
        let root = Some(d.as_path());
        let a = c.call(
            "add_geometry",
            &json!({ "items": [{ "type": "circle", "center": [0, 0], "radius": 500 }] }),
            root,
        );
        let saved = c.call("save_drawing", &json!({ "path": "disc.drift" }), root);
        // Saved, so the window may start a new drawing, and open the saved one again.
        let new = c.call("new_drawing", &json!({}), root);
        let opened = c.call("open_drawing", &json!({ "path": "disc.drift" }), root);
        let outside = c.call(
            "save_drawing",
            &json!({ "path": "../elsewhere.drift" }),
            root,
        );
        [a, saved, new, opened, outside].map(|r| r.expect("call"))
    });
    let [a, saved, new, opened, outside] = serve_until_done(&mut h, assistant);
    for r in [&a, &saved, &new, &opened] {
        assert!(!r.is_error, "{}", r.json);
    }
    assert!(
        outside.is_error,
        "the server's root folder holds in the window too"
    );
    let app = h.state_mut();
    assert_eq!(app.file.name, "disc.drift");
    assert!(!app.is_dirty(), "the opened drawing is clean");
    assert_eq!(
        app.editor
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Circle { .. }))
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_assistant_runs_a_script_in_the_open_window() {
    let Some(mut h) = harness() else { return };
    let dir = link_dir("script");
    {
        let app = h.state_mut();
        app.link.dir = Some(dir.clone());
        app.ai_link = true;
    }
    h.run();
    let d = dir.clone();
    let script = r#"let W = 3.6 m
let T = 200
grid cols A..C spacing W + T, 2400 + T length 7.5 m at -100,-1.4 m fixed
room bed W x 1.25*W at 0,0 walls T fixed
room bath 2400 x 3 m right of bed walls T
door d1 900 in bed.south centred
window w1 1200 in bed.north offset 600 from bed.nw
door d2 800 in bath.north offset 300 from bath.ne
label bed "Bedroom" height 250
label bath "Bath" height 250
hatch bed.walls ANSI31
hatch bath.walls ANSI31"#;
    let assistant = std::thread::spawn(move || {
        let mut c = LinkClient::connect(&d).expect("connect");
        c.call("run_script", &json!({ "script": script }), None)
            .expect("call")
    });
    let out = serve_until_done(&mut h, assistant);
    assert!(!out.is_error, "{}", out.json);
    assert_eq!(out.json["sketch"]["fully_defined"], true, "{}", out.json);
    {
        let app = h.state_mut();
        assert_eq!(
            app.editor.history.undo_label(),
            Some("Assistant: script"),
            "the whole script is one step in the window's history"
        );
        assert_eq!(app.editor.history.undo_depth(), 1);
        let over = app.editor.overlapping();
        let desc: Vec<String> = over
            .iter()
            .map(|id| {
                format!(
                    "{id} {:?} {:?}",
                    app.editor.doc.get(*id).map(|e| e.style),
                    app.editor.doc.shape(*id)
                )
            })
            .collect();
        assert!(over.is_empty(), "no curve lies on another: {desc:?}");
        app.editor.selection.clear();
        app.fit_target = Some(app.fit_bounds());
    }
    h.run();
    save_png("drift_live_script.png", &mut h);
    // Change the width from outside: the door stays centred, the bath moves along.
    let d = dir.clone();
    let assistant = std::thread::spawn(move || {
        let mut c = LinkClient::connect(&d).expect("connect");
        c.call("run_script", &json!({ "script": "let W = 4.4 m" }), None)
            .expect("call")
    });
    let out = serve_until_done(&mut h, assistant);
    assert_eq!(out.json["sketch"]["fully_defined"], true, "{}", out.json);
    {
        let app = h.state_mut();
        app.editor.selection.clear();
        app.fit_target = Some(app.fit_bounds());
    }
    h.run();
    save_png("drift_live_script_wider.png", &mut h);
    // One undo takes the change back.
    assert!(h.state_mut().editor.undo());
    assert_eq!(
        h.state().editor.history.undo_label(),
        Some("Assistant: script")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn preferences_show_the_server_and_copy_its_config() {
    let Some(mut h) = harness() else { return };
    let dir = link_dir("prefs");
    let server = dir.join(format!("drift-mcp{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&server, b"").unwrap();
    h.state_mut().link.mcp_server = Some(server.clone());
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    let shown = server.to_string_lossy().into_owned();
    assert!(
        h.query_by_label(&shown).is_some(),
        "the server's path is shown"
    );
    save_png("drift_ui_preferences_ai.png", &mut h);
    h.get_by_label("Copy Claude Desktop config").click();
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        });
    let config: serde_json::Value =
        serde_json::from_str(&copied.expect("the config was copied")).unwrap();
    let command = config["mcpServers"]["drift"]["command"].as_str().unwrap();
    assert_eq!(std::path::Path::new(command), server);
    // Not installed: it says so instead.
    std::fs::remove_file(&server).unwrap();
    h.run();
    assert!(h.query_by_label("Copy Claude Desktop config").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
