//! Where tool calls go: to the drawing open in Drift when its AI link is on (the live
//! link), otherwise to this server's own drawing.

use std::path::PathBuf;

use drift_app::agent::link::LinkClient;
use drift_app::agent::{Session, ToolOutput};
use serde_json::{Value, json};

/// The extra tool for choosing between the live link and this server's own drawing.
pub const LINK_TOOL: &str = "drift_link";

pub fn link_tool_description() -> &'static str {
    "The live link to Drift. When the user has turned on Preferences → General → \"Let AI \
     assistants edit the open drawing\" in Drift, every tool works on the drawing open in that \
     window and the user sees each change as it happens; otherwise tools work on a drawing of \
     this server's own (open and save files to share it). action: status (which drawing the \
     tools are working on), connect (to the Drift window used last), or disconnect (work on \
     files here instead)."
}

pub fn link_tool_schema() -> Value {
    json!({ "type": "object", "properties": {
        "action": { "type": "string", "enum": ["status", "connect", "disconnect"] }
    }})
}

enum Link {
    /// Working on this server's own drawing.
    Off,
    /// Working on the drawing open in a Drift window.
    Live(LinkClient),
    /// The window went away: calls are refused until the assistant chooses what to do, so
    /// its edits never land on a different drawing without its knowing.
    Lost(String),
}

pub struct Router {
    session: Session,
    root: Option<PathBuf>,
    /// Where Drift windows leave their link files (`None`: the link is never used).
    dir: Option<PathBuf>,
    link: Link,
}

fn error(text: String) -> ToolOutput {
    ToolOutput {
        json: json!({ "error": text }),
        image: None,
        is_error: true,
    }
}

impl Router {
    /// Route calls for `session`; with `connect`, to a Drift window straight away if one
    /// has its link on.
    pub fn new(
        session: Session,
        root: Option<PathBuf>,
        dir: Option<PathBuf>,
        connect: bool,
    ) -> Self {
        let root = root.map(|r| r.canonicalize().unwrap_or(r));
        let mut router = Self {
            session,
            root,
            dir,
            link: Link::Off,
        };
        if connect {
            router.try_connect();
        }
        router
    }

    /// Connect to the Drift window used last, if one has its link on.
    fn try_connect(&mut self) -> Option<String> {
        let dir = self.dir.as_ref()?;
        match LinkClient::connect(dir) {
            Ok(c) => {
                self.link = Link::Live(c);
                None
            }
            Err(e) => Some(e),
        }
    }

    fn status(&self) -> Value {
        match &self.link {
            Link::Live(c) => json!({
                "working_on": "the drawing open in Drift",
                "drift_window": c.info.title,
            }),
            Link::Off => json!({
                "working_on": "this server's own drawing (open and save .drift files to share it)",
                "live_link": if self.dir.is_some() { "no Drift window has its AI link on" } else { "off (--no-link)" },
            }),
            Link::Lost(why) => {
                json!({ "working_on": "nothing: the link to Drift was lost", "why": why })
            }
        }
    }

    fn link_tool(&mut self, args: &Value) -> ToolOutput {
        let ok = |json| ToolOutput {
            json,
            image: None,
            is_error: false,
        };
        match args["action"].as_str().unwrap_or("status") {
            "status" => ok(self.status()),
            "connect" => {
                if self.dir.is_none() {
                    return error("this server was started with --no-link".into());
                }
                match self.try_connect() {
                    None => ok(self.status()),
                    Some(e) => error(format!(
                        "{e}. Ask the user to turn on Preferences → General → \"Let AI assistants \
                         edit the open drawing\" in Drift."
                    )),
                }
            }
            "disconnect" => {
                self.link = Link::Off;
                ok(self.status())
            }
            other => error(format!(
                "unknown action `{other}` (status, connect or disconnect)"
            )),
        }
    }

    pub fn call(&mut self, name: &str, args: &Value) -> ToolOutput {
        if name == LINK_TOOL {
            return self.link_tool(args);
        }
        // A Drift window that turns its link on before any work is done here is picked up.
        if matches!(self.link, Link::Off) && self.session.is_untouched() {
            self.try_connect();
        }
        match &mut self.link {
            Link::Off => self.session.call(name, args),
            Link::Lost(why) => error(format!(
                "the link to Drift was lost ({why}). Call drift_link with action \"connect\" to \
                 reconnect, or \"disconnect\" to work on files here instead."
            )),
            Link::Live(client) => match client.call(name, args, self.root.as_deref()) {
                Ok(mut out) => {
                    if name == "drawing_info"
                        && let Some(o) = out.json.as_object_mut()
                    {
                        o.insert("drift_window".into(), json!(client.info.title));
                    }
                    out
                }
                Err(e) => {
                    let why = format!("{e}: Drift was closed, or the link turned off");
                    self.link = Link::Lost(why.clone());
                    error(format!(
                        "the link to Drift was lost ({why}); this call may not have been made. \
                         Call drift_link to reconnect or to work on files here."
                    ))
                }
            },
        }
    }
}
