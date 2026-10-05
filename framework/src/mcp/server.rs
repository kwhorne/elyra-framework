//! The MCP server: JSON-RPC 2.0, one message per line (the stdio framing),
//! answering for both protocol eras.
//!
//! - **Modern** (2026-07-28): every request carries its version, client
//!   capabilities and identity in `_meta`; `server/discover` advertises what
//!   the server supports; results carry `resultType`.
//! - **Legacy** (2025-11-25 and earlier): an `initialize` request opens the
//!   connection; later requests carry no `_meta`.
//!
//! The era is a property of the connection: one that opened with
//! `initialize` is served the legacy way from then on.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::middleware::Origin;

use super::{catalog, Mcp, Tool};

/// The revision this server speaks per request.
pub const MODERN: &str = "2026-07-28";

/// The `initialize`-based revisions it also answers, newest first.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// How long a client may cache `tools/list` and `server/discover`: the tools
/// are fixed for the life of the process.
const TTL_MS: u64 = 60 * 60 * 1000;

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

// JSON-RPC and MCP error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_VERSION: i64 = -32022;

/// An app's MCP server: its tools, and the context to run them in.
#[derive(Clone)]
pub struct McpServer {
    inner: Arc<Inner>,
}

struct Inner {
    ctx: Ctx,
    registry: Arc<CommandRegistry>,
    mcp: Mcp,
    tools: Vec<Tool>,
    name: String,
    version: String,
}

/// One client's connection — the unit an era is decided for.
pub struct Connection {
    server: McpServer,
    /// `Some(version)` once the client opened with `initialize`.
    legacy: Mutex<Option<String>>,
    /// The name a legacy client gave in `initialize`.
    legacy_client: Mutex<Option<String>>,
}

impl McpServer {
    /// A server for `mcp`'s tools out of `registry`, run in `ctx`.
    pub fn new(
        ctx: Ctx,
        registry: Arc<CommandRegistry>,
        mcp: Mcp,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, String> {
        let tools = catalog(&registry, &mcp)?;
        Ok(Self {
            inner: Arc::new(Inner {
                ctx,
                registry,
                mcp,
                tools,
                name: name.into(),
                version: version.into(),
            }),
        })
    }

    /// A new client connection.
    pub fn connect(&self) -> Connection {
        Connection {
            server: self.clone(),
            legacy: Mutex::new(None),
            legacy_client: Mutex::new(None),
        }
    }

    /// The tools it serves.
    pub fn tools(&self) -> &[Tool] {
        &self.inner.tools
    }

    fn server_info(&self) -> Value {
        json!({ "name": self.inner.name, "version": self.inner.version })
    }

    /// Serve one connection over a line-framed stream — stdin/stdout, or a
    /// socket — until it closes. Requests run concurrently; each response is
    /// one line, written whole.
    pub async fn serve<R, W>(&self, reader: R, mut writer: W) -> std::io::Result<()>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let connection = Arc::new(self.connect());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let write = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                writer.write_all(line.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            Ok::<_, std::io::Error>(())
        });

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let connection = connection.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Some(reply) = connection.handle_line(&line).await {
                    let _ = tx.send(reply);
                }
            });
        }
        drop(tx);
        write.await.map_err(std::io::Error::other)?
    }
}

impl Connection {
    /// Handle one line: a JSON-RPC message in, maybe one out.
    pub async fn handle_line(&self, line: &str) -> Option<String> {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(message).await?,
            Err(e) => error(Value::Null, PARSE_ERROR, &format!("not JSON: {e}"), None),
        };
        Some(reply.to_string())
    }

    /// Handle one JSON-RPC message. Notifications get no reply.
    pub async fn handle(&self, message: Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "a JSON-RPC message is an object (batches aren't part of MCP)",
                None,
            ));
        };
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = object.get("id").cloned() else {
            // `notifications/initialized`, `notifications/cancelled`: nothing
            // to answer.
            return None;
        };
        Some(self.request(id, method, &params).await)
    }

    async fn request(&self, id: Value, method: &str, params: &Value) -> Value {
        let server = &self.server;
        let meta = params.get("_meta");
        let requested = meta
            .and_then(|m| m.get(META_VERSION))
            .and_then(Value::as_str);
        let legacy = self.legacy.lock().clone();

        match method {
            // Opens a legacy connection.
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked
                    .filter(|v| LEGACY.contains(v))
                    .unwrap_or(LEGACY[0])
                    .to_owned();
                *self.legacy.lock() = Some(version.clone());
                *self.legacy_client.lock() = params
                    .pointer("/clientInfo/name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                return result(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": capabilities(),
                        "serverInfo": server.server_info(),
                    }),
                    None,
                );
            }
            // The probe a modern client may send first — answered whatever it
            // carries, so the era can always be found out.
            "server/discover" => {
                let mut versions = vec![MODERN];
                versions.extend(LEGACY);
                return result(
                    id,
                    json!({
                        "supportedVersions": versions,
                        "capabilities": capabilities(),
                        "ttlMs": TTL_MS,
                        "cacheScope": "private",
                    }),
                    Some(server),
                );
            }
            "ping" => return result(id, json!({}), legacy.is_none().then_some(server)),
            _ => {}
        }

        // Everything else: a legacy connection, or the modern version in `_meta`.
        let modern = match (requested, &legacy) {
            (Some(v), _) if v == MODERN => true,
            (Some(v), _) => {
                return unsupported(id, v);
            }
            (None, Some(_)) => false,
            (None, None) => {
                return error(
                    id,
                    INVALID_PARAMS,
                    &format!(
                        "requests carry `{META_VERSION}` in `_meta` (or the connection \
                         opens with `initialize`)"
                    ),
                    Some(json!({ "supported": supported() })),
                );
            }
        };
        let info = modern.then_some(server);

        match method {
            "tools/list" => {
                let tools: Vec<Value> = server
                    .inner
                    .tools
                    .iter()
                    .map(|t| tool_json(t, modern))
                    .collect();
                let mut body = json!({ "tools": tools });
                if modern {
                    body["ttlMs"] = json!(TTL_MS);
                    body["cacheScope"] = json!("private");
                }
                result(id, body, info)
            }
            "tools/call" => {
                let client = meta
                    .and_then(|m| m.get(META_CLIENT))
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| self.legacy_client.lock().clone())
                    .unwrap_or_else(|| "agent".into());
                match self.call(params, &client, modern).await {
                    Ok(body) => result(id, body, info),
                    Err((code, message)) => error(id, code, &message, None),
                }
            }
            other => error(
                id,
                METHOD_NOT_FOUND,
                &format!("unknown method `{other}`"),
                None,
            ),
        }
    }

    /// `tools/call`: an `Ok` is a tool result (which may be a tool error the
    /// model can act on), an `Err` a protocol error.
    async fn call(
        &self,
        params: &Value,
        client: &str,
        modern: bool,
    ) -> Result<Value, (i64, String)> {
        let server = &self.server.inner;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "`name` is required".to_string()))?;
        let tool = server
            .tools
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| (INVALID_PARAMS, format!("unknown tool `{name}`")))?;

        if server.mcp.needs_confirmation(&tool.ability) {
            return Ok(tool_error(&format!(
                "`{name}` needs the user's confirmation, which this server can't ask for yet"
            )));
        }

        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Ok(tool_error("`arguments` must be an object")),
        };
        if let Some(unknown) = arguments.keys().find(|k| !tool.args.contains(k)) {
            return Ok(tool_error(&format!(
                "`{name}` has no argument `{unknown}`; it takes: {}",
                tool.args.join(", ")
            )));
        }
        // Commands take their arguments positionally; one left out is `null`
        // (an `Option`'s `None` — anything else fails to decode, and says so).
        let positional: Vec<Value> = tool
            .args
            .iter()
            .map(|a| arguments.get(a).cloned().unwrap_or(Value::Null))
            .collect();
        let body = rmp_serde::to_vec(&positional).map_err(|e| (INVALID_PARAMS, e.to_string()))?;

        let started = Instant::now();
        let registry = server.registry.clone();
        let ctx = server.ctx.clone();
        let command = name.to_owned();
        let origin = Origin::Agent {
            client: client.to_owned(),
        };
        // On its own task, as the shell runs commands: a panic is an error.
        let outcome =
            tokio::spawn(async move { registry.dispatch_from(ctx, &command, &body, origin).await })
                .await;

        let ok = matches!(outcome, Ok(Ok(_)));
        crate::info!(
            target: "elyra::mcp",
            "{client} called {name}: {} in {:?}",
            if ok { "ok" } else { "failed" },
            started.elapsed()
        );

        match outcome {
            Ok(Ok(bytes)) => {
                let value: Value = rmp_serde::from_slice(&bytes).unwrap_or(Value::Null);
                let text = value.to_string();
                let mut body = json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": false,
                });
                // Before 2026-07-28, structured content had to be an object.
                if modern || value.is_object() {
                    body["structuredContent"] = value;
                }
                Ok(body)
            }
            Ok(Err(e)) => {
                let message = e.to_string();
                Ok(match validation_bag(&message) {
                    Some(bag) => {
                        let lines: Vec<String> = bag
                            .iter()
                            .flat_map(|(field, messages)| {
                                messages.iter().map(move |m| format!("- {field}: {m}"))
                            })
                            .collect();
                        let mut body = tool_error(&format!("Invalid input:\n{}", lines.join("\n")));
                        body["structuredContent"] = json!({ "errors": bag });
                        body
                    }
                    None => tool_error(&message),
                })
            }
            Err(e) => Ok(tool_error(&format!("`{name}` panicked: {e}"))),
        }
    }
}

fn capabilities() -> Value {
    json!({ "tools": { "listChanged": false } })
}

fn supported() -> Vec<&'static str> {
    let mut all = vec![MODERN];
    all.extend(LEGACY);
    all
}

fn tool_json(tool: &Tool, modern: bool) -> Value {
    let mut value = serde_json::to_value(tool).unwrap_or(Value::Null);
    if !modern {
        // Before 2026-07-28 an output schema had to describe an object.
        let object = tool.output_schema.get("type").and_then(Value::as_str) == Some("object");
        if !object {
            if let Some(map) = value.as_object_mut() {
                map.remove("outputSchema");
            }
        }
    }
    value
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn validation_bag(message: &str) -> Option<std::collections::BTreeMap<String, Vec<String>>> {
    crate::validation::is_validation_bag(message)
        .then(|| serde_json::from_str(message).ok())
        .flatten()
}

fn result(id: Value, mut body: Value, server: Option<&McpServer>) -> Value {
    if let Some(map) = body.as_object_mut() {
        map.insert("resultType".into(), json!("complete"));
        if let Some(server) = server {
            let meta = map.entry("_meta").or_insert_with(|| json!({}));
            meta[META_SERVER] = server.server_info();
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": body })
}

fn error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn unsupported(id: Value, requested: &str) -> Value {
    error(
        id,
        UNSUPPORTED_VERSION,
        "Unsupported protocol version",
        Some(json!({ "supported": supported(), "requested": requested })),
    )
}
