//! `drift-mcp`: a Model Context Protocol server over stdio, so AI assistants (Claude Desktop,
//! Claude Code and other MCP clients) can create and edit Drift drawings.
//!
//! The tools are Drift's own (`drift_app::agent`); this program only carries them over MCP.
//! When a Drift window has its AI link on, calls go to the drawing open there (see
//! [`router`]); otherwise they work on a drawing of this server's own, on a thread of its
//! own (the editor isn't `Send`). Calls queue and run in order.

mod router;

use std::path::PathBuf;
use std::sync::Arc;

use drift_app::agent::{self, Session, ToolOutput};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

type Job = (String, Value, oneshot::Sender<ToolOutput>);

#[derive(Clone)]
struct Drift {
    jobs: mpsc::UnboundedSender<Job>,
    tools: Arc<Vec<Tool>>,
}

impl Drift {
    /// Start the drawing's thread, with the drawing at `open` if given.
    fn new(root: Option<PathBuf>, open: Option<String>, link: bool) -> Result<Self, String> {
        let (jobs, mut queue) = mpsc::unbounded_channel::<Job>();
        let (started, ready) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("drawing".into())
            .spawn(move || {
                let mut session = Session::new();
                if let Some(root) = &root {
                    session = session.with_root(root.clone());
                }
                // A drawing named on the command line is worked on here; otherwise the
                // drawing open in Drift, if its link is on.
                let connect = open.is_none();
                if let Some(path) = open {
                    let out = session.call("open_drawing", &serde_json::json!({ "path": path }));
                    if out.is_error {
                        let _ = started.send(Err(out.json["error"].to_string()));
                        return;
                    }
                }
                let dir = link.then(agent::link::link_dir).flatten();
                let mut router = router::Router::new(session, root, dir, connect);
                let _ = started.send(Ok(()));
                while let Some((name, args, reply)) = queue.blocking_recv() {
                    let _ = reply.send(router.call(&name, &args));
                }
            })
            .map_err(|e| e.to_string())?;
        ready.recv().map_err(|e| e.to_string())??;
        let link_tool = Tool::new(
            router::LINK_TOOL,
            router::link_tool_description(),
            Arc::new(match router::link_tool_schema() {
                Value::Object(o) => o,
                _ => Default::default(),
            }),
        )
        .with_annotations(ToolAnnotations::new().read_only(false).open_world(false));
        let tools = agent::tools()
            .into_iter()
            .map(|t| {
                let schema = match t.schema {
                    Value::Object(o) => o,
                    _ => Default::default(),
                };
                Tool::new(t.name, t.description, Arc::new(schema)).with_annotations(
                    ToolAnnotations::new()
                        .read_only(t.read_only)
                        .destructive(t.name == "delete" || t.name == "new_drawing")
                        .open_world(false),
                )
            })
            .chain(std::iter::once(link_tool))
            .collect();
        Ok(Self {
            jobs,
            tools: Arc::new(tools),
        })
    }
}

impl ServerHandler for Drift {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("drift", env!("CARGO_PKG_VERSION"))
                    .with_title("Drift")
                    .with_description("Parametric 2D CAD drawings: sketch, constrain, dimension, render and analyse"),
            )
            .with_instructions(format!("{}\n\n{}", agent::INSTRUCTIONS, LIVE_INSTRUCTIONS))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.to_vec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.tools.iter().any(|t| t.name == request.name) {
            return Err(ErrorData::invalid_params(
                format!("unknown tool `{}`", request.name),
                None,
            ));
        }
        let args = request.arguments.map_or(Value::Null, Value::Object);
        let (reply, answer) = oneshot::channel();
        self.jobs
            .send((request.name.to_string(), args, reply))
            .map_err(|_| ErrorData::internal_error("the drawing thread stopped", None))?;
        let out = answer
            .await
            .map_err(|_| ErrorData::internal_error("the drawing thread stopped", None))?;
        let text = serde_json::to_string_pretty(&out.json).unwrap_or_default();
        let mut content = vec![ContentBlock::text(text)];
        if let Some(png) = &out.image {
            content.push(ContentBlock::image(
                agent::link::base64_encode(png),
                "image/png",
            ));
        }
        let result = if out.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        Ok(result.into())
    }
}

/// How the live link changes things, added to the tools' instructions.
const LIVE_INSTRUCTIONS: &str = "\
When the user has Drift open with its AI link on, the tools work on the drawing in that window \
and the user watches each change as it is made (drawing_info names the window; drift_link \
says which drawing you're on). Treat it as the user's drawing: don't start or open another one \
unless asked, and save only when asked. Your changes are undo steps in Drift like the user's \
own, and what you just made is picked so the user sees it.";

const USAGE: &str = "\
drift-mcp: a Model Context Protocol server (stdio) for Drift drawings.

Usage: drift-mcp [--root <folder>] [--open <file.drift>] [--no-link]

  --root <folder>   only read and write files inside this folder
  --open <file>     start with this drawing open (rather than the one open in Drift)
  --no-link         never work on the drawing open in Drift, only on files
  --version         print the version

Configure it as an MCP server in Claude Desktop or Claude Code: see docs/ai.md.";

fn main() {
    let mut root: Option<PathBuf> = None;
    let mut open: Option<String> = None;
    let mut link = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--open" => open = args.next(),
            "--no-link" => link = false,
            "--version" | "-V" => {
                println!("drift-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("drift-mcp: unknown argument `{other}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let drift = match Drift::new(root, open, link) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("drift-mcp: {e}");
            std::process::exit(1);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("start the async runtime");
    let result = runtime.block_on(async move {
        let service = drift.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    });
    if let Err(e) = result {
        eprintln!("drift-mcp: {e}");
        std::process::exit(1);
    }
}
