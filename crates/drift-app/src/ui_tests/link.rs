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
