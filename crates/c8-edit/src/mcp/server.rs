//! The MCP transports (rmcp): every request is sent to the thread that owns the documents and
//! answered from there. Headless mode serves stdio; live mode serves Streamable HTTP on
//! 127.0.0.1 with a bearer token, inside the editor.
use super::{Part, Workspace};
use base64::Engine;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation, ListPromptsResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, Role, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub enum Call {
    Tool(String, Value),
    Resource(String),
}

pub struct Request {
    pub call: Call,
    pub reply: tokio::sync::oneshot::Sender<Result<Vec<Part>, String>>,
}

/// Answer a request on the thread that owns the documents.
pub fn handle(ws: &mut dyn Workspace, req: Request) {
    let out = match &req.call {
        Call::Tool(name, args) => super::call(ws, name, args),
        Call::Resource(uri) => super::read_resource(ws, uri).map(|t| vec![Part::Text(t)]),
    };
    let _ = req.reply.send(out);
}

/// What the agent did, for the editor's activity log.
#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    pub fn push(&self, s: String) {
        let mut v = self.0.lock().unwrap_or_else(|e| e.into_inner());
        v.push(s);
        let n = v.len();
        if n > 500 {
            v.drain(..n - 500);
        }
    }
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[derive(Clone)]
pub struct Handler {
    tx: mpsc::Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// the user paused agent edits (read-only tools still work)
    pub paused: Arc<AtomicBool>,
    pub log: Log,
}

impl Handler {
    pub fn new(tx: mpsc::Sender<Request>, wake: Arc<dyn Fn() + Send + Sync>) -> Handler {
        Handler { tx, wake, paused: Arc::new(AtomicBool::new(false)), log: Log::default() }
    }

    async fn send(&self, call: Call) -> Result<Vec<Part>, String> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.tx.send(Request { call, reply }).map_err(|_| "the editor is closing".to_string())?;
        (self.wake)();
        rx.await.map_err(|_| "the editor dropped the request".to_string())?
    }
}

fn summary(args: &Value) -> String {
    let s = args.to_string();
    if s.len() > 160 { format!("{}…", &s[..s.floor_char_boundary(160)]) } else { s }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build()).with_server_info(Implementation::new("c8h18-edit", env!("CARGO_PKG_VERSION"))).with_instructions(super::INSTRUCTIONS)
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(super::tools()))
    }

    async fn call_tool(&self, req: CallToolRequestParams, _: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let name = req.name.to_string();
        let args = Value::Object(req.arguments.unwrap_or_default());
        if self.paused.load(Ordering::Relaxed) && !super::read_only(&name) {
            self.log.push(format!("refused {name} (paused)"));
            return Ok(CallToolResult::error(vec![ContentBlock::text("The user has paused agent edits in the editor; only reading tools work now. Ask them to resume.")]).into());
        }
        let out = self.send(Call::Tool(name.clone(), args.clone())).await;
        self.log.push(match &out {
            Ok(_) => format!("{name} {}", summary(&args)),
            Err(e) => format!("{name} {} FAILED: {e}", summary(&args)),
        });
        Ok(match out {
            Ok(parts) => CallToolResult::success(
                parts
                    .into_iter()
                    .map(|p| match p {
                        Part::Text(t) => ContentBlock::text(t),
                        Part::Png(png) => ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(png), "image/png"),
                    })
                    .collect(),
            ),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        }
        .into())
    }

    async fn list_resources(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(super::RESOURCES.iter().map(|(uri, name, desc)| Resource::new(*uri, *name).with_description(*desc).with_mime_type(if uri.starts_with("docs") { "text/markdown" } else { "application/json" })).collect()))
    }

    async fn read_resource(&self, req: ReadResourceRequestParams, _: RequestContext<RoleServer>) -> Result<ReadResourceResponse, ErrorData> {
        match self.send(Call::Resource(req.uri.clone())).await {
            Ok(parts) => {
                let text = parts.into_iter().find_map(|p| if let Part::Text(t) = p { Some(t) } else { None }).unwrap_or_default();
                Ok(ReadResourceResult::new(vec![ResourceContents::text(text, req.uri)]).into())
            }
            Err(e) => Err(ErrorData::resource_not_found(e, None)),
        }
    }

    async fn list_prompts(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(
            super::PROMPTS
                .iter()
                .map(|(name, desc)| {
                    let args = (*name == "design-track").then(|| vec![PromptArgument::new("idea").with_description("what the track should be like").with_required(false)]);
                    Prompt::new(*name, Some(*desc), args)
                })
                .collect(),
        ))
    }

    async fn get_prompt(&self, req: GetPromptRequestParams, _: RequestContext<RoleServer>) -> Result<GetPromptResponse, ErrorData> {
        let text = super::prompt(&req.name, &req.arguments.unwrap_or_default()).map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]).into())
    }
}

// ---- headless: stdio ----------------------------------------------------------------------------

/// Serve MCP on stdin/stdout until the client disconnects.
pub fn run_stdio(mut ws: super::headless::Headless) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Request>();
    // the documents stay on this thread; the transport runs on a tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().map_err(|e| e.to_string())?;
    let handler = Handler::new(tx, Arc::new(|| {}));
    let server = rt.spawn(async move {
        use rmcp::ServiceExt;
        let service = handler.serve(rmcp::transport::stdio()).await.map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    });
    // the channel closes when the service (and with it the handler) is dropped
    while let Ok(req) = rx.recv() {
        handle(&mut ws, req);
    }
    rt.block_on(server).map_err(|e| e.to_string())?
}

// ---- live: Streamable HTTP inside the editor ----------------------------------------------------

pub struct Live {
    pub rx: mpsc::Receiver<Request>,
    pub port: u16,
    token: Arc<Mutex<String>>,
    pub handler: Handler,
    cancel: tokio_util::sync::CancellationToken,
    _thread: std::thread::JoinHandle<()>,
}

impl Live {
    pub fn token(&self) -> String {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// Replace the token; clients with the old one are refused from now on.
    pub fn set_token(&self, t: String) {
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }
    /// The command that adds this server to Claude Code.
    pub fn claude_command(&self) -> String {
        format!("claude mcp add --transport http c8h18-edit {} --header \"Authorization: Bearer {}\"", self.url(), self.token())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// A random token (hex, 128 bits) from the OS-seeded hasher keys.
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// Start the live endpoint on 127.0.0.1:`port` (0 = any free port).
pub fn start_live(port: u16, token: String, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Live, String> {
    use rmcp::transport::streamable_http_server::{session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService};
    let (tx, rx) = mpsc::channel::<Request>();
    let handler = Handler::new(tx, wake);
    let cancel = tokio_util::sync::CancellationToken::new();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).or_else(|_| std::net::TcpListener::bind(("127.0.0.1", 0))).map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let token = Arc::new(Mutex::new(token));
    let (h, c, tok) = (handler.clone(), cancel.clone(), token.clone());
    let thread = std::thread::Builder::new()
        .name("mcp-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => return log::warn!("MCP: {e}"),
            };
            rt.block_on(async move {
                let mut config = StreamableHttpServerConfig::default();
                config.cancellation_token = c.child_token();
                // browsers must not reach it: refuse every request that carries an Origin
                let config = config.enforce_origin_validation();
                let service = StreamableHttpService::new(move || Ok(h.clone()), Arc::new(LocalSessionManager::default()), config);
                let router = axum::Router::new().nest_service("/mcp", service).layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let expected = format!("Bearer {}", tok.lock().unwrap_or_else(|e| e.into_inner()));
                    let given = req.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
                    // compare in constant time
                    let ok = given.len() == expected.len() && given.bytes().zip(expected.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
                    async move {
                        if ok {
                            next.run(req).await
                        } else {
                            axum::response::IntoResponse::into_response((axum::http::StatusCode::UNAUTHORIZED, "missing or wrong bearer token (see the editor's MCP panel)"))
                        }
                    }
                }));
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
                let _ = axum::serve(listener, router).with_graceful_shutdown(async move { c.cancelled().await }).await;
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(Live { rx, port, token, handler, cancel, _thread: thread })
}
