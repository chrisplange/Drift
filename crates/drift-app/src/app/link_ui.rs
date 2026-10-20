//! The live link: AI assistants (through `drift-mcp`) editing the drawing open here.
//!
//! With the preference on, Drift listens on localhost (see [`crate::agent::link`]). Calls
//! run between frames on this window's editor, each one undo step, and what a call made is
//! picked so it shows.

use std::path::PathBuf;

use crate::agent::{self, LiveChange, LiveDoc, link::LinkServer};
use crate::*;

/// The link while it is on, and what it last told assistants about this window.
#[derive(Default)]
pub(crate) struct Link {
    pub(crate) server: Option<LinkServer>,
    /// Where the link file goes (tests use their own folder).
    pub(crate) dir: Option<PathBuf>,
    /// Why the link couldn't start (it isn't tried again until the preference is turned
    /// off and on).
    pub(crate) failed: Option<String>,
    /// Where `drift-mcp` is, if not next to this program (tests use their own).
    pub(crate) mcp_server: Option<PathBuf>,
    /// When this window last told assistants it is the one in use, and under which name.
    touched: (f64, String),
}

impl DriftApp {
    /// Each frame: start or stop the link to match the preference, keep its file current,
    /// and run the calls waiting for this window.
    pub(crate) fn link_tick(&mut self, ctx: &egui::Context, now: f64) {
        if !self.ai_link {
            self.link.server = None;
            self.link.failed = None;
            return;
        }
        if self.link.server.is_none() && self.link.failed.is_none() {
            let dir = self.link.dir.clone().or_else(agent::link::link_dir);
            let waker = ctx.clone();
            let started = dir
                .ok_or_else(|| "no folder for the link file".to_owned())
                .and_then(|dir| {
                    LinkServer::start(&dir, &self.file.name, move || waker.request_repaint())
                        .map_err(|e| e.to_string())
                });
            match started {
                Ok(server) => self.link.server = Some(server),
                Err(e) => {
                    self.set_status(format!("Couldn't start the AI link: {e}"), true, now);
                    self.link.failed = Some(e);
                    return;
                }
            }
        }
        // Assistants use the window used last: say so while this one has the focus (at most
        // every few seconds), and whenever the drawing's name changes.
        let focused = ctx.input(|i| i.focused);
        let (at, name) = &self.link.touched;
        if (focused && now - at > 3.0) || *name != self.file.name {
            self.link.touched = (now, self.file.name.clone());
            if let Some(server) = &mut self.link.server {
                server.touch(&self.file.name);
            }
        }
        // Calls wait while the user is in the middle of an edit.
        while !self.tool.is_busy() && !self.editor.doc.in_transaction() {
            let Some(job) = self.link.server.as_ref().and_then(LinkServer::poll) else {
                break;
            };
            self.run_link_call(ctx, job, now);
        }
    }

    fn run_link_call(&mut self, ctx: &egui::Context, job: agent::link::Job, now: f64) {
        let before: std::collections::BTreeSet<_> =
            self.editor.doc.iter().map(|(id, _)| id).collect();
        let dirty = self.is_dirty();
        let doc = LiveDoc {
            editor: &mut self.editor,
            path: self.file.path.clone(),
            meta: self.file.meta.clone(),
            dirty,
            ctx: ctx.clone(),
        };
        let (out, change) = agent::call_live(doc, &job.tool, &job.args, job.root.clone());
        match change {
            Some(LiveChange::Replaced { path, meta }) => {
                let doc = std::mem::take(&mut self.editor.doc);
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map_or(UNTITLED.into(), |n| n.to_string_lossy().into_owned());
                if let Some(p) = &path {
                    self.remember(p.clone());
                }
                self.replace_document(doc, name, path, meta, true);
            }
            Some(LiveChange::Saved { path, meta }) => {
                self.file.name = path
                    .file_name()
                    .map_or(self.file.name.clone(), |n| n.to_string_lossy().into_owned());
                self.file.meta = meta;
                self.file.clean_revision = Some(self.editor.doc.revision());
                self.remember(path.clone());
                self.file.path = Some(path);
            }
            None => {
                // What the call made is picked, so it shows on the canvas.
                let doc = &self.editor.doc;
                let made: std::collections::BTreeSet<_> = doc
                    .iter()
                    .filter(|(id, e)| !before.contains(id) && !e.kind.is_point())
                    .map(|(id, _)| id)
                    .collect();
                if !made.is_empty() {
                    self.editor.selection = made;
                }
            }
        }
        self.editor.sync();
        if out.is_error
            && let Some(e) = out.json["error"].as_str()
        {
            self.set_status(format!("Assistant: {e}"), false, now);
        }
        job.reply(out);
        ctx.request_repaint();
    }

    /// The status bar's pill while the link is on: whether an assistant is connected, and a
    /// way to disconnect it.
    pub(crate) fn link_pill(&mut self, ui: &mut egui::Ui) {
        let Some(server) = &self.link.server else {
            return;
        };
        let connected = server.connected();
        let dark = ui.visuals().dark_mode;
        let (text, color) = if connected > 0 {
            ("Assistant connected", theme::accent(dark))
        } else {
            ("AI link on", ui.visuals().weak_text_color())
        };
        let pill = theme::pill(ui, text, color);
        let click = ui
            .interact(pill.rect, ui.id().with("ai_link"), egui::Sense::click())
            .on_hover_text(if connected > 0 {
                "An AI assistant is editing this drawing through drift-mcp. Its changes are \
                 undo steps like yours. Click to disconnect it."
            } else {
                "AI assistants (through drift-mcp) can connect to edit this drawing. Turn it \
                 off in Preferences → General."
            });
        if click.clicked() && connected > 0 {
            server.disconnect_all();
        }
    }

    /// Preferences → General → AI assistants.
    pub(crate) fn link_prefs(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "AI assistants");
        ui.checkbox(&mut self.ai_link, "Let AI assistants edit the open drawing")
            .on_hover_text(
                "Claude and other assistants connected through drift-mcp work on the drawing \
                 in this window, and you see each change as it is made. Only programs \
                 running as you on this computer can connect.",
            );
        if let Some(e) = &self.link.failed {
            ui.label(
                egui::RichText::new(format!("The link couldn't start: {e}"))
                    .small()
                    .color(ui.visuals().error_fg_color),
            );
        }
        let Some(server) = self.link.mcp_server.clone().or_else(mcp_server_path) else {
            return;
        };
        let shown = server.to_string_lossy().into_owned();
        if !server.is_file() {
            ui.label(
                egui::RichText::new(format!(
                    "drift-mcp isn't installed next to Drift ({shown})."
                ))
                .small()
                .weak(),
            );
            return;
        }
        theme::row(ui, "Server", |ui| {
            ui.add(egui::Label::new(egui::RichText::new(&shown).small()).truncate())
                .on_hover_text(format!(
                    "{shown}

drift-mcp, the MCP server assistants start to work with Drift"
                ));
        });
        if ui
            .button("Copy Claude Desktop config")
            .on_hover_text(
                "The mcpServers entry for claude_desktop_config.json (Claude Desktop → \
                 Settings → Developer → Edit Config). It lets the server use files in your \
                 Documents folder only.",
            )
            .clicked()
        {
            let root = std::env::home_dir()
                .map(|home| home.join("Documents"))
                .filter(|docs| docs.is_dir());
            ui.ctx().copy_text(claude_config(&server, root.as_deref()));
            let now = ui.input(|i| i.time);
            self.set_status(
                "Copied. Paste it into claude_desktop_config.json (merge it into mcpServers if \
                 there are other servers there), then restart Claude Desktop."
                    .to_owned(),
                false,
                now,
            );
        }
    }
}

/// Where the installed `drift-mcp` is: next to this program (in `Drift.app/Contents/MacOS`
/// on macOS).
fn mcp_server_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(
        exe.parent()?
            .join(format!("drift-mcp{}", std::env::consts::EXE_SUFFIX)),
    )
}

/// Claude Desktop's config with Drift's server in it, limited to `root` when there is one.
/// Windows paths are written with forward slashes, which read better in JSON and work the same.
fn claude_config(server: &std::path::Path, root: Option<&std::path::Path>) -> String {
    let path = |p: &std::path::Path| {
        let p = p.to_string_lossy();
        if cfg!(windows) {
            p.replace('\\', "/")
        } else {
            p.into_owned()
        }
    };
    let mut drift = serde_json::json!({ "command": path(server) });
    if let Some(root) = root {
        drift["args"] = serde_json::json!(["--root", path(root)]);
    }
    let config = serde_json::json!({ "mcpServers": { "drift": drift } });
    serde_json::to_string_pretty(&config).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_config_names_the_server_and_its_root() {
        let server = std::path::Path::new("/Applications/Drift.app/Contents/MacOS/drift-mcp");
        let root = std::path::Path::new("/Users/you/Documents");
        let config: serde_json::Value =
            serde_json::from_str(&claude_config(server, Some(root))).unwrap();
        let drift = &config["mcpServers"]["drift"];
        assert_eq!(
            drift["command"],
            "/Applications/Drift.app/Contents/MacOS/drift-mcp"
        );
        assert_eq!(
            drift["args"],
            serde_json::json!(["--root", "/Users/you/Documents"])
        );
        let bare: serde_json::Value = serde_json::from_str(&claude_config(server, None)).unwrap();
        assert!(bare["mcpServers"]["drift"].get("args").is_none());
    }
}
