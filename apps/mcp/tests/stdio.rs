//! End-to-end: run the `drift-mcp` server and talk MCP to it over stdio, as Claude Desktop
//! or Claude Code would.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    /// Start the server (only allowed to use files in `root`) and initialise the session.
    fn start(root: &PathBuf) -> Self {
        // Drift windows are looked for only in the test's own (empty) link folder, never
        // in the user's.
        let links = root.join("links");
        std::fs::create_dir_all(&links).expect("link folder");
        Self::start_linked(root, &links)
    }

    /// Start the server, looking for Drift windows' link files in `links`.
    fn start_linked(root: &PathBuf, links: &PathBuf) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_drift-mcp"))
            .arg("--root")
            .arg(root)
            .env("DRIFT_LINK_DIR", links)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start drift-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut server = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = server.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "drift-e2e", "version": "1.0" }
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "drift", "{init}");
        assert!(
            init["instructions"]
                .as_str()
                .is_some_and(|i| i.contains("add_relations")),
            "the server explains how to use the tools"
        );
        server.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        server
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").expect("write");
        self.stdin.flush().expect("flush");
    }

    /// Send a request and wait for its response (skipping any notifications).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).expect("read");
            assert!(n > 0, "the server closed its output");
            let msg: Value = serde_json::from_str(&line).expect("JSON-RPC");
            if msg["id"] == id {
                assert!(msg.get("error").is_none(), "{method} failed: {msg}");
                return msg["result"].clone();
            }
        }
    }

    /// Call a tool; returns its JSON result and any image (base64 PNG).
    fn call(&mut self, tool: &str, args: Value) -> (Value, Option<String>) {
        let r = self.request("tools/call", json!({ "name": tool, "arguments": args }));
        let content = r["content"].as_array().expect("content");
        let text = content
            .iter()
            .find(|c| c["type"] == "text")
            .and_then(|c| c["text"].as_str())
            .expect("a text result");
        let json: Value = serde_json::from_str(text).expect("the text is JSON");
        assert_ne!(r["isError"], true, "{tool} failed: {json}");
        let image = content.iter().find(|c| c["type"] == "image").map(|c| {
            assert_eq!(c["mimeType"], "image/png");
            c["data"].as_str().expect("data").to_owned()
        });
        (json, image)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drift-mcp-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.canonicalize().expect("canonical")
}

/// Decode standard base64 (for the test's own check of the image).
fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, c)| n | (u32::from(val(*c)) << (18 - 6 * i)));
        for k in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    out
}

#[test]
fn lists_its_tools() {
    let dir = temp_dir("tools");
    let mut server = Server::start(&dir);
    let tools = server.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for want in [
        "add_geometry",
        "add_relations",
        "add_dimensions",
        "render",
        "analyze",
        "import_file",
        "batch",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    let render = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "render")
        .unwrap();
    assert_eq!(render["annotations"]["readOnlyHint"], true);
    assert_eq!(render["inputSchema"]["type"], "object");
}

#[test]
fn draws_constrains_renders_and_analyses_a_room() {
    let dir = temp_dir("room");
    let mut server = Server::start(&dir);
    // Draw: a rough room, as a model would sketch it.
    let (made, _) = server.call(
        "add_geometry",
        json!({ "items": [{ "type": "polyline", "closed": true,
            "points": [[0, 0], [4180, 40], [4200, 3320], [-30, 3300]] }] }),
    );
    let lines: Vec<String> = made["created"][0]["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l.as_str().unwrap().to_owned())
        .collect();
    let corner = made["created"][0]["points"][0].as_str().unwrap().to_owned();
    assert_eq!(
        made["relations_added"], 4,
        "near-level and near-plumb walls: {made}"
    );
    // Constrain: 4200 × 3300, the corner at the origin.
    let (dims, _) = server.call(
        "add_dimensions",
        json!({ "dimensions": [
            { "type": "length", "line": lines[0], "value": "4.2 m" },
            { "type": "length", "line": lines[1], "value": 3300 }
        ]}),
    );
    assert_eq!(dims["added"].as_array().map(Vec::len), Some(2), "{dims}");
    let (fixed, _) = server.call(
        "add_relations",
        json!({ "relations": [{ "type": "fix", "point": corner, "at": [0, 0] }] }),
    );
    assert_eq!(fixed["sketch"]["fully_defined"], true, "{fixed}");
    // Render.
    let (pic, image) = server.call("render", json!({ "width": 600 }));
    let png = unbase64(&image.expect("an image"));
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
    assert!(pic["mm_per_pixel"].as_f64().unwrap() > 0.0);
    // Analyse.
    let (a, _) = server.call("analyze", json!({}));
    assert_eq!(a["dof"], 0, "{a}");
    let regions = a["regions"].as_array().expect("regions");
    assert_eq!(regions.len(), 1);
    assert!(
        (regions[0]["area_m2"].as_f64().unwrap() - 13.86).abs() < 1e-6,
        "{a}"
    );
    // The whole session undoes step by step, and the drawing saves.
    let (undone, _) = server.call("undo", json!({}));
    assert_eq!(undone["undone"][0], "Assistant: add relations");
    server.call("redo", json!({}));
    let (saved, _) = server.call("save_drawing", json!({ "path": "room.drift" }));
    assert!(saved["saved"].as_str().unwrap().ends_with("room.drift"));
    assert!(dir.join("room.drift").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_an_svg_at_a_scale() {
    let dir = temp_dir("svg");
    // A 100 × 60 mm plan with a 20 mm circle, drawn at 1:50.
    std::fs::write(
        dir.join("plan.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="60mm" viewBox="0 0 100 60">
  <rect x="0" y="0" width="100" height="60" fill="none" stroke="black" stroke-width="0.5"/>
  <circle cx="50" cy="30" r="10" fill="none" stroke="black" stroke-width="0.5"/>
</svg>"#,
    )
    .expect("write svg");
    let mut server = Server::start(&dir);
    let (imp, _) = server.call(
        "import_file",
        json!({ "path": "plan.svg", "scale": "1:50" }),
    );
    assert!(imp["curves"].as_u64().unwrap() >= 5, "{imp}");
    let b: Vec<f64> = imp["bounds"]
        .as_array()
        .expect("bounds")
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert!(
        (b[2] - b[0] - 5000.0).abs() < 1.0,
        "100 mm at 1:50 is 5 m: {imp}"
    );
    assert!((b[3] - b[1] - 3000.0).abs() < 1.0, "{imp}");
    let (circles, _) = server.call("list_entities", json!({ "kind": "circle" }));
    let r = circles["entities"][0]["radius"].as_f64().expect("a circle");
    assert!((r - 500.0).abs() < 0.5, "{circles}");
    // Paths outside the root are refused.
    let r = server.request(
        "tools/call",
        json!({ "name": "import_file", "arguments": { "path": "../../elsewhere.svg" } }),
    );
    assert_eq!(r["isError"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for a Drift window with its AI link on: it answers calls from its own
/// drawing until `stop` is set.
fn fake_drift(
    links: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<usize> {
    use drift_app::agent::link::LinkServer;
    use std::sync::atomic::Ordering;
    let (ready, started) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let server = LinkServer::start(&links, "plan.drift", || {}).expect("link");
        let mut drawing = drift_app::agent::Session::new();
        let mut calls = 0;
        ready.send(()).unwrap();
        while !stop.load(Ordering::Relaxed) {
            match server.poll() {
                Some(job) => {
                    calls += 1;
                    let out = drawing.call(&job.tool, &job.args);
                    job.reply(out);
                }
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        calls
    });
    started.recv().expect("the window started");
    handle
}

#[test]
fn works_on_the_drawing_open_in_drift() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = temp_dir("live");
    let links = dir.join("links");
    std::fs::create_dir_all(&links).unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let window = fake_drift(links.clone(), stop.clone());
    let mut server = Server::start_linked(&dir, &links);
    let (info, _) = server.call("drawing_info", json!({}));
    assert_eq!(info["drift_window"], "plan.drift", "{info}");
    let (status, _) = server.call("drift_link", json!({ "action": "status" }));
    assert_eq!(status["drift_window"], "plan.drift");
    let (made, _) = server.call(
        "add_geometry",
        json!({ "items": [{ "type": "circle", "center": [0, 0], "radius": 300 }] }),
    );
    assert_eq!(made["created"][0]["type"], "circle");
    // The window closes: calls are refused (not quietly moved to another drawing) until
    // the assistant says what to do.
    stop.store(true, Ordering::Relaxed);
    let calls = window.join().unwrap();
    assert_eq!(calls, 2, "both calls went to the window");
    let r = server.request(
        "tools/call",
        json!({ "name": "list_entities", "arguments": {} }),
    );
    assert_eq!(r["isError"], true);
    assert!(
        r["content"][0]["text"].as_str().unwrap().contains("lost"),
        "{r}"
    );
    server.call("drift_link", json!({ "action": "disconnect" }));
    let (circles, _) = server.call("list_entities", json!({ "kind": "circle" }));
    assert_eq!(
        circles["count"], 0,
        "now on the server's own, empty drawing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
