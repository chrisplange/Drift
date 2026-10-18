//! The live link: assistants editing the drawing open in Drift.
//!
//! Drift (with the link turned on) listens on a random localhost port and writes the port
//! and a random token to a file in the user's own data folder. `drift-mcp` reads the file,
//! connects, presents the token, and then sends tool calls, one JSON object per line; Drift
//! runs each between frames on its own editor and answers with the result. Only programs
//! that can read the user's files can connect.
//!
//! Requests: `{"id": 1, "tool": "add_geometry", "args": {...}, "root": "C:/Drawings"}`.
//! Answers: `{"id": 1, "json": {...}, "image": "<base64 PNG>" | null, "is_error": false}`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::ToolOutput;

/// What the link file and the handshake say the protocol is.
const PROTOCOL: &str = "drift-link-1";
/// How long a call may wait for Drift (busy with an edit the user is making, say).
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Where running Drift windows leave their link files: `DRIFT_LINK_DIR` if set, else a
/// folder in the user's own application data.
pub fn link_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DRIFT_LINK_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    }?;
    Some(base.join("Drift").join("link"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// 128 random bits as hex, from the standard library's randomly keyed hasher.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(now_ms() ^ i);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Write `text` to `path` readable only by the user (where the system has such modes).
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&tmp)?.write_all(text.as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

/// Standard base64 (images travel as text).
pub fn base64_encode(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ABC[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let mut n = 0;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        for k in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Some(out)
}

fn output_json(id: u64, out: &ToolOutput) -> Value {
    json!({
        "id": id,
        "json": out.json,
        "image": out.image.as_deref().map(base64_encode),
        "is_error": out.is_error,
    })
}

// ----- Drift's side ------------------------------------------------------------------------

/// A tool call from an assistant, waiting for Drift to run it.
pub struct Job {
    pub tool: String,
    pub args: Value,
    /// The folder the assistant's server keeps files in, if it was given one.
    pub root: Option<PathBuf>,
    reply: mpsc::Sender<ToolOutput>,
}

impl Job {
    pub fn reply(self, out: ToolOutput) {
        let _ = self.reply.send(out);
    }
}

/// Drift's end of the link, while it is turned on. Dropping it closes the link.
pub struct LinkServer {
    file: PathBuf,
    port: u16,
    token: String,
    jobs: mpsc::Receiver<Job>,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    streams: Arc<Mutex<Vec<TcpStream>>>,
    title: String,
}

impl LinkServer {
    /// Start listening and leave a link file in `dir`. `wake` is called when a call
    /// arrives, so a waiting app can run it.
    pub fn start(
        dir: &Path,
        title: &str,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let (send, jobs) = mpsc::channel::<Job>();
        let stop = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(AtomicUsize::new(0));
        let streams: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        {
            let (stop, clients, streams, token) = (
                stop.clone(),
                clients.clone(),
                streams.clone(),
                token.clone(),
            );
            std::thread::Builder::new()
                .name("drift-link".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let (send, wake, clients, streams, token) = (
                                    send.clone(),
                                    wake.clone(),
                                    clients.clone(),
                                    streams.clone(),
                                    token.clone(),
                                );
                                let _ = std::thread::Builder::new()
                                    .name("drift-link-client".into())
                                    .spawn(move || {
                                        serve(stream, &token, &send, &*wake, &clients, &streams)
                                    });
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(50));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(200)),
                        }
                    }
                })?;
        }
        let server = Self {
            file: dir.join(format!("drift-{}.json", std::process::id())),
            port,
            token,
            jobs,
            stop,
            clients,
            streams,
            title: title.to_owned(),
        };
        server.write_file()?;
        Ok(server)
    }

    fn write_file(&self) -> std::io::Result<()> {
        let info = json!({
            "protocol": PROTOCOL,
            "port": self.port,
            "token": self.token,
            "pid": std::process::id(),
            "title": self.title,
            "active": now_ms(),
        });
        write_private(&self.file, &info.to_string())
    }

    /// Note that this window is the one in use now (assistants pick the most recent), and
    /// what its drawing is called.
    pub fn touch(&mut self, title: &str) {
        self.title = title.to_owned();
        let _ = self.write_file();
    }

    /// The next call waiting to run, if any.
    pub fn poll(&self) -> Option<Job> {
        self.jobs.try_recv().ok()
    }

    /// How many assistants are connected.
    pub fn connected(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }

    /// Drop every assistant's connection (they can connect again).
    pub fn disconnect_all(&self) {
        if let Ok(mut streams) = self.streams.lock() {
            for s in streams.drain(..) {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.disconnect_all();
        let _ = std::fs::remove_file(&self.file);
    }
}

/// One assistant's connection: the handshake, then calls until it goes.
fn serve(
    stream: TcpStream,
    token: &str,
    jobs: &mpsc::Sender<Job>,
    wake: &(dyn Fn() + Send + Sync),
    clients: &AtomicUsize,
    streams: &Mutex<Vec<TcpStream>>,
) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // The handshake must come quickly and carry the token.
    let _ = reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)));
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let hello: Value = serde_json::from_str(&line).unwrap_or_default();
    let given = hello["token"].as_str().unwrap_or_default();
    // Compared in full whatever the first difference, so timing doesn't tell.
    let same = given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |d, (a, b)| d | (a ^ b))
            == 0;
    if hello["protocol"] != PROTOCOL || !same {
        let _ = writeln!(writer, "{}", json!({ "ok": false, "error": "refused" }));
        return;
    }
    if writeln!(
        writer,
        "{}",
        json!({ "ok": true, "pid": std::process::id() })
    )
    .is_err()
    {
        return;
    }
    let _ = reader.get_ref().set_read_timeout(None);
    if let Ok(clone) = writer.try_clone()
        && let Ok(mut s) = streams.lock()
    {
        s.push(clone);
    }
    clients.fetch_add(1, Ordering::Relaxed);
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({ "error": format!("bad request: {e}") })
                );
                continue;
            }
        };
        let id = req["id"].as_u64().unwrap_or(0);
        let (reply, answer) = mpsc::channel();
        let job = Job {
            tool: req["tool"].as_str().unwrap_or_default().to_owned(),
            args: req.get("args").cloned().unwrap_or(json!({})),
            root: req["root"].as_str().map(PathBuf::from),
            reply,
        };
        if jobs.send(job).is_err() {
            break;
        }
        wake();
        let out = answer.recv().unwrap_or_else(|_| ToolOutput {
            json: json!({ "error": "Drift closed its link" }),
            image: None,
            is_error: true,
        });
        if writeln!(writer, "{}", output_json(id, &out)).is_err() {
            break;
        }
    }
    clients.fetch_sub(1, Ordering::Relaxed);
}

// ----- The assistant's side ----------------------------------------------------------------

/// A running Drift window that has its link turned on, from its link file.
#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub file: PathBuf,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub title: String,
    /// When the window was last in use (ms since 1970).
    pub active: u64,
}

/// The Drift windows offering a link, most recently used first.
pub fn find(dir: &Path) -> Vec<LinkInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<LinkInfo> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|file| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).ok()?).ok()?;
            (v["protocol"] == PROTOCOL).then_some(())?;
            Some(LinkInfo {
                port: u16::try_from(v["port"].as_u64()?).ok()?,
                token: v["token"].as_str()?.to_owned(),
                pid: u32::try_from(v["pid"].as_u64()?).ok()?,
                title: v["title"].as_str().unwrap_or_default().to_owned(),
                active: v["active"].as_u64().unwrap_or(0),
                file,
            })
        })
        .collect();
    found.sort_by_key(|i| std::cmp::Reverse(i.active));
    found
}

/// The assistant's end of a link to one Drift window.
pub struct LinkClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    pub info: LinkInfo,
    next_id: u64,
}

impl LinkClient {
    /// Connect to the most recently used Drift window with its link on. Link files left by
    /// windows that are gone are cleared away.
    pub fn connect(dir: &Path) -> Result<Self, String> {
        let windows = find(dir);
        if windows.is_empty() {
            return Err("no Drift window has its AI link turned on".into());
        }
        let mut last = String::new();
        for info in windows {
            match Self::connect_to(info.clone()) {
                Ok(c) => return Ok(c),
                Err((gone, e)) => {
                    if gone {
                        let _ = std::fs::remove_file(&info.file);
                    }
                    last = e;
                }
            }
        }
        Err(format!("couldn't reach Drift: {last}"))
    }

    /// Connect to one window; the error says whether it is gone (nothing listening).
    fn connect_to(info: LinkInfo) -> Result<Self, (bool, String)> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], info.port));
        // Nothing listening: refused, or on Windows a time-out (it retries a refused
        // localhost connection for a couple of seconds).
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| {
            let gone = matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
            );
            (gone, e.to_string())
        })?;
        let io = |e: std::io::Error| (false, e.to_string());
        stream.set_read_timeout(Some(CALL_TIMEOUT)).map_err(io)?;
        let mut writer = stream.try_clone().map_err(io)?;
        let mut reader = BufReader::new(stream);
        writeln!(
            writer,
            "{}",
            json!({ "protocol": PROTOCOL, "token": info.token })
        )
        .map_err(io)?;
        let mut line = String::new();
        reader.read_line(&mut line).map_err(io)?;
        let answer: Value = serde_json::from_str(&line).unwrap_or_default();
        if answer["ok"] != true {
            return Err((false, "Drift refused the connection".into()));
        }
        Ok(Self {
            reader,
            writer,
            info,
            next_id: 1,
        })
    }

    /// Run a tool in Drift. An error means the link is broken (Drift closed it, or quit).
    pub fn call(
        &mut self,
        tool: &str,
        args: &Value,
        root: Option<&Path>,
    ) -> Result<ToolOutput, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "id": id, "tool": tool, "args": args,
            "root": root.map(|r| r.to_string_lossy().into_owned()),
        });
        writeln!(self.writer, "{req}").map_err(|e| e.to_string())?;
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => return Err("Drift closed the link".into()),
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return Err(e.to_owned());
        }
        Ok(ToolOutput {
            json: v["json"].clone(),
            image: v["image"].as_str().and_then(base64_decode),
            is_error: v["is_error"] == true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("drift-link-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..10 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 5) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(base64_encode(b"Drift"), "RHJpZnQ=");
    }

    #[test]
    fn calls_reach_drift_and_answers_come_back() {
        let dir = temp_dir("calls");
        let server = LinkServer::start(&dir, "room.drift", || {}).unwrap();
        let answering = std::thread::spawn(move || {
            // Drift's frames: answer each call as it comes.
            let mut served = 0;
            while served < 2 {
                if let Some(job) = server.poll() {
                    let out = ToolOutput {
                        json: json!({ "tool": job.tool, "root": job.root }),
                        image: Some(vec![1, 2, 3]),
                        is_error: false,
                    };
                    job.reply(out);
                    served += 1;
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            server
        });
        let mut client = LinkClient::connect(&dir).unwrap();
        assert_eq!(client.info.title, "room.drift");
        let out = client
            .call("analyze", &json!({}), Some(Path::new("C:/x")))
            .unwrap();
        assert_eq!(out.json["tool"], "analyze");
        assert_eq!(out.json["root"], "C:/x");
        assert_eq!(out.image, Some(vec![1, 2, 3]));
        client.call("render", &json!({}), None).unwrap();
        let server = answering.join().unwrap();
        assert_eq!(server.connected(), 1);
        // Closing the link breaks the connection and removes the file.
        drop(server);
        assert!(client.call("analyze", &json!({}), None).is_err());
        assert!(find(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_token_is_refused_and_stale_files_are_cleared() {
        let dir = temp_dir("token");
        let server = LinkServer::start(&dir, "a", || {}).unwrap();
        let mut info = find(&dir).remove(0);
        info.token = "0".repeat(info.token.len());
        assert!(LinkClient::connect_to(info).is_err());
        drop(server);
        // A file left by a window that crashed: nothing listens on its port.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let stale = dir.join("drift-1.json");
        let text =
            json!({ "protocol": PROTOCOL, "port": port, "token": "t", "pid": 1, "active": 1 });
        std::fs::write(&stale, text.to_string()).unwrap();
        assert!(LinkClient::connect(&dir).is_err());
        assert!(!stale.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
