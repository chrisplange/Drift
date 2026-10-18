//! `drift-mcp`: a Model Context Protocol server over stdio, so AI assistants (Claude Desktop,
//! Claude Code and other MCP clients) can create and edit Drift drawings.
//!
//! The tools are Drift's own (`drift_app::agent`); this program only carries them over MCP.
//! The drawing lives on a thread of its own (the editor isn't `Send`), and tool calls queue
//! for it in order.

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
    fn new(root: Option<PathBuf>, open: Option<String>) -> Result<Self, String> {
        let (jobs, mut queue) = mpsc::unbounded_channel::<Job>();
        let (started, ready) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("drawing".into())
            .spawn(move || {
                let mut session = Session::new();
                if let Some(root) = root {
                    session = session.with_root(root);
                }
                if let Some(path) = open {
                    let out = session.call("open_drawing", &serde_json::json!({ "path": path }));
                    if out.is_error {
                        let _ = started.send(Err(out.json["error"].to_string()));
                        return;
                    }
                }
                let _ = started.send(Ok(()));
                while let Some((name, args, reply)) = queue.blocking_recv() {
                    let _ = reply.send(session.call(&name, &args));
                }
            })
            .map_err(|e| e.to_string())?;
        ready.recv().map_err(|e| e.to_string())??;
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
            .collect();
        Ok(Self {
            jobs,
            tools: Arc::new(tools),
        })
    }
}

/// Standard base64, for images in results.
fn base64(bytes: &[u8]) -> String {
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

impl ServerHandler for Drift {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("drift", env!("CARGO_PKG_VERSION"))
                    .with_title("Drift")
                    .with_description("Parametric 2D CAD drawings: sketch, constrain, dimension, render and analyse"),
            )
            .with_instructions(agent::INSTRUCTIONS)
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
            content.push(ContentBlock::image(base64(png), "image/png"));
        }
        let result = if out.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        Ok(result.into())
    }
}

const USAGE: &str = "\
drift-mcp: a Model Context Protocol server (stdio) for Drift drawings.

Usage: drift-mcp [--root <folder>] [--open <file.drift>]

  --root <folder>   only read and write files inside this folder
  --open <file>     start with this drawing open
  --version         print the version

Configure it as an MCP server in Claude Desktop or Claude Code: see docs/ai.md.";

fn main() {
    let mut root: Option<PathBuf> = None;
    let mut open: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--open" => open = args.next(),
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
    let drift = match Drift::new(root, open) {
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
