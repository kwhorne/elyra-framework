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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::middleware::Origin;

use super::confirm::{self, Answer, Confirmations};
use super::{catalog, Mcp, Tool};

/// The revision this server speaks per request.
pub const MODERN: &str = "2026-07-28";

/// The `initialize`-based revisions it also answers, newest first.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// How long a client may cache `tools/list` and `server/discover`: the tools
/// are fixed for the life of the process.
const TTL_MS: u64 = 60 * 60 * 1000;

/// How long a legacy client's user has to answer a confirmation.
const ELICIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";
const META_SUBSCRIPTION: &str = "io.modelcontextprotocol/subscriptionId";

// JSON-RPC and MCP error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
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
    confirmations: Confirmations,
}

/// One client's connection — the unit an era is decided for.
pub struct Connection {
    server: McpServer,
    /// `Some(version)` once the client opened with `initialize`.
    legacy: Mutex<Option<String>>,
    /// The name a legacy client gave in `initialize`.
    legacy_client: Mutex<Option<String>>,
    /// Whether a legacy client said in `initialize` that it can elicit.
    legacy_elicit: Mutex<bool>,
    /// Where requests *to* the client go — set while [`McpServer::serve`]
    /// runs the connection, for a legacy client's confirmations.
    outgoing: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Those requests, waiting for the client's answer, by id.
    pending: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>,
    next_out: AtomicU64,
    /// Who this connection is to the live registry (its subscription limit).
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    key: String,
    /// Live subscriptions, by what opened them: a `subscriptions/listen`'s
    /// id, or a legacy `resources/subscribe`'s URI.
    subscriptions: Mutex<HashMap<String, Vec<String>>>,
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
                confirmations: Confirmations::new(),
            }),
        })
    }

    /// A new client connection.
    pub fn connect(&self) -> Connection {
        Connection {
            server: self.clone(),
            legacy: Mutex::new(None),
            legacy_client: Mutex::new(None),
            legacy_elicit: Mutex::new(false),
            outgoing: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next_out: AtomicU64::new(1),
            key: format!("mcp-{}", crate::security::random_token()),
            subscriptions: Mutex::new(HashMap::new()),
        }
    }

    /// The tools it serves.
    pub fn tools(&self) -> &[Tool] {
        &self.inner.tools
    }

    /// The tool behind resource `uri`, if it's one.
    fn resource(&self, uri: &str) -> Option<&Tool> {
        self.inner
            .tools
            .iter()
            .find(|t| t.resource_uri().as_deref() == Some(uri))
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

        *connection.outgoing.lock() = Some(tx.clone());

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
        // The client is gone: no answer to a question of ours is coming, and
        // nobody to tell about a change.
        connection.close();
        connection.outgoing.lock().take();
        drop(tx);
        write.await.map_err(std::io::Error::other)?
    }
}

impl Connection {
    /// Where the messages the server sends on its own arrive — subscription
    /// notifications, and its requests to a legacy client. [`McpServer::serve`]
    /// wires this to the stream; call it to drive a connection by hand.
    pub fn outbox(&self) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *self.outgoing.lock() = Some(tx);
        rx
    }

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
        // The client's answer to a request of ours (a legacy confirmation).
        if !object.contains_key("method")
            && (object.contains_key("result") || object.contains_key("error"))
        {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                if let Some(waiting) = self.pending.lock().remove(id) {
                    let _ = waiting.send(message.clone());
                }
            }
            return None;
        }
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = object.get("id").cloned() else {
            // A notification: nothing to answer. `notifications/cancelled`
            // ends a subscription.
            if method == "notifications/cancelled" {
                if let Some(request) = params.get("requestId") {
                    self.unwatch(&request.to_string());
                }
            }
            return None;
        };
        if method == "subscriptions/listen" {
            return self.listen(id, &params).await;
        }
        Some(self.request(id, method, &params).await)
    }

    /// Open a `subscriptions/listen` stream: acknowledge what it'll honor,
    /// then notify on it until it's cancelled. Its answer comes only if the
    /// server ends it, so there's none now.
    async fn listen(&self, id: Value, params: &Value) -> Option<Value> {
        match params
            .pointer("/_meta/io.modelcontextprotocol~1protocolVersion")
            .and_then(Value::as_str)
        {
            Some(MODERN) => {}
            Some(other) => return Some(unsupported(id, other)),
            None => {
                return Some(error(
                    id,
                    INVALID_PARAMS,
                    &format!("`subscriptions/listen` carries `{META_VERSION}` in `_meta`"),
                    Some(json!({ "supported": [MODERN] })),
                ))
            }
        }
        let Some(outgoing) = self.outgoing.lock().clone() else {
            return Some(error(
                id,
                INTERNAL_ERROR,
                "subscriptions need a stream to arrive on (`McpServer::serve`)",
                None,
            ));
        };
        let client = self.client_name(params.get("_meta"));
        let mut notifications = json!({});
        if let Some(uris) = params
            .pointer("/notifications/resourceSubscriptions")
            .and_then(Value::as_array)
        {
            let mut honored = Vec::new();
            let mut watching = Vec::new();
            for uri in uris.iter().filter_map(Value::as_str) {
                if let Some(watch) = self.watch(uri, &client).await {
                    honored.push(uri.to_owned());
                    watching.push((uri.to_owned(), watch));
                }
            }
            // Acknowledged first: nothing on the subscription comes before it.
            notifications["resourceSubscriptions"] = json!(honored);
            self.acknowledge(&outgoing, &id, &notifications);
            let ids = watching
                .into_iter()
                .map(|(uri, (live_id, changed))| {
                    forward(changed, outgoing.clone(), updated(&uri, Some(&id)));
                    live_id
                })
                .collect();
            self.subscriptions.lock().insert(id.to_string(), ids);
        } else {
            self.acknowledge(&outgoing, &id, &notifications);
        }
        None
    }

    fn acknowledge(
        &self,
        outgoing: &tokio::sync::mpsc::UnboundedSender<String>,
        id: &Value,
        notifications: &Value,
    ) {
        let ack = json!({
            "jsonrpc": "2.0",
            "method": "notifications/subscriptions/acknowledged",
            "params": { "_meta": { META_SUBSCRIPTION: id }, "notifications": notifications },
        });
        let _ = outgoing.send(ack.to_string());
    }

    /// Follow resource `uri` in the live registry: its subscription's id, and
    /// where its changes arrive. `None` for an unknown URI, or without live
    /// queries (the `database` feature).
    #[cfg(feature = "database")]
    async fn watch(
        &self,
        uri: &str,
        client: &str,
    ) -> Option<(String, tokio::sync::mpsc::UnboundedReceiver<()>)> {
        let tool = self.server.resource(uri)?;
        let live = self
            .server
            .inner
            .ctx
            .try_get::<crate::live::LiveRegistry>()?;
        let body = rmp_serde::to_vec(tool.defaults.as_ref()?).ok()?;
        let (changed, changes) = tokio::sync::mpsc::unbounded_channel();
        let origin = Origin::Agent {
            client: client.to_owned(),
        };
        match live
            .subscribe_agent(&self.key, &tool.name, &body, origin, changed)
            .await
        {
            Ok(id) => Some((id, changes)),
            Err(e) => {
                crate::debug!(target: "elyra::mcp", "can't watch {uri}: {e}");
                None
            }
        }
    }

    #[cfg(not(feature = "database"))]
    async fn watch(
        &self,
        _uri: &str,
        _client: &str,
    ) -> Option<(String, tokio::sync::mpsc::UnboundedReceiver<()>)> {
        None
    }

    /// End the subscriptions `key` opened.
    fn unwatch(&self, key: &str) {
        let Some(ids) = self.subscriptions.lock().remove(key) else {
            return;
        };
        #[cfg(feature = "database")]
        if let Some(live) = self.server.inner.ctx.try_get::<crate::live::LiveRegistry>() {
            for id in &ids {
                live.unsubscribe(&self.key, id);
            }
        }
        let _ = ids;
    }

    /// The connection is over: end everything it subscribed to.
    fn close(&self) {
        let keys: Vec<String> = self.subscriptions.lock().keys().cloned().collect();
        for key in keys {
            self.unwatch(&key);
        }
        self.pending.lock().clear();
    }

    /// The client's name: from `_meta`, or what a legacy one said in
    /// `initialize`.
    fn client_name(&self, meta: Option<&Value>) -> String {
        meta.and_then(|m| m.get(META_CLIENT))
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.legacy_client.lock().clone())
            .unwrap_or_else(|| "agent".into())
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
                // Elicitation came in 2025-06-18.
                *self.legacy_elicit.lock() = version != "2025-03-26"
                    && params
                        .pointer("/capabilities/elicitation")
                        .is_some_and(Value::is_object);
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
                let client = self.client_name(meta);
                match self.call(params, &client, modern).await {
                    Ok(body) => result(id, body, info),
                    Err((code, message)) => error(id, code, &message, None),
                }
            }
            "resources/list" => {
                let resources: Vec<Value> = server
                    .inner
                    .tools
                    .iter()
                    .filter_map(|t| {
                        let mut resource = json!({
                            "uri": t.resource_uri()?,
                            "name": t.name,
                            "mimeType": "application/json",
                        });
                        if !t.description.is_empty() {
                            resource["description"] = json!(t.description);
                        }
                        Some(resource)
                    })
                    .collect();
                let mut body = json!({ "resources": resources });
                if modern {
                    body["ttlMs"] = json!(TTL_MS);
                    body["cacheScope"] = json!("private");
                }
                result(id, body, info)
            }
            "resources/templates/list" => {
                let mut body = json!({ "resourceTemplates": [] });
                if modern {
                    body["ttlMs"] = json!(TTL_MS);
                    body["cacheScope"] = json!("private");
                }
                result(id, body, info)
            }
            "resources/read" => {
                let client = self.client_name(meta);
                match self.read(params, &client).await {
                    Ok(mut body) => {
                        if modern {
                            // Live data: never fresh for long.
                            body["ttlMs"] = json!(0);
                            body["cacheScope"] = json!("private");
                        }
                        result(id, body, info)
                    }
                    Err((code, message, data)) => error(id, code, &message, data),
                }
            }
            // The legacy era's subscriptions: one URI at a time.
            "resources/subscribe" if !modern => {
                let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                    return error(id, INVALID_PARAMS, "`uri` is required", None);
                };
                let key = format!("uri:{uri}");
                if self.subscriptions.lock().contains_key(&key) {
                    return result(id, json!({}), None);
                }
                let outgoing = self.outgoing.lock().clone();
                let client = self.client_name(meta);
                match (outgoing, self.watch(uri, &client).await) {
                    (Some(outgoing), Some((live_id, changed))) => {
                        forward(changed, outgoing, updated(uri, None));
                        self.subscriptions.lock().insert(key, vec![live_id]);
                        result(id, json!({}), None)
                    }
                    _ => not_found(id, uri),
                }
            }
            "resources/unsubscribe" if !modern => {
                if let Some(uri) = params.get("uri").and_then(Value::as_str) {
                    self.unwatch(&format!("uri:{uri}"));
                }
                result(id, json!({}), None)
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

        // `Mcp::confirm`: the user says yes first, in the client.
        if server.mcp.needs_confirmation(&tool.ability) {
            let question = confirm::message(client, name, &tool.description, &arguments);
            let confirmed = if modern {
                let args = Value::Array(positional);
                match server.confirmations.answer(
                    params.get("requestState"),
                    params.get("inputResponses"),
                    name,
                    &args,
                    client,
                ) {
                    Answer::Accepted => true,
                    Answer::Declined => false,
                    Answer::Ask => {
                        let capabilities =
                            params.pointer("/_meta/io.modelcontextprotocol~1clientCapabilities");
                        if !confirm::can_elicit(capabilities) {
                            return Ok(cant_ask(name));
                        }
                        crate::info!(target: "elyra::mcp", "{client} asks to run {name}: confirming");
                        return Ok(json!({
                            "resultType": "input_required",
                            "inputRequests": {
                                confirm::KEY: {
                                    "method": "elicitation/create",
                                    "params": confirm::elicitation(&question, true),
                                },
                            },
                            "requestState": server.confirmations.issue(name, &args, client),
                        }));
                    }
                }
            } else {
                match self.elicit(&question).await {
                    Some(answer) => answer,
                    None => return Ok(cant_ask(name)),
                }
            };
            if !confirmed {
                crate::info!(target: "elyra::mcp", "{client} called {name}: not confirmed");
                return Ok(tool_error(&format!(
                    "The user didn't confirm `{name}`, so it did not run."
                )));
            }
        }

        let outcome = self.run(name, body, client, "called").await;

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

impl Connection {
    /// Run `name` for the agent — on its own task, as the shell runs
    /// commands, so a panic is an error — and log it (not the arguments,
    /// which may be personal data).
    async fn run(
        &self,
        name: &str,
        body: Vec<u8>,
        client: &str,
        verb: &str,
    ) -> Result<crate::Result<Vec<u8>>, tokio::task::JoinError> {
        let server = &self.server.inner;
        let started = Instant::now();
        let registry = server.registry.clone();
        let ctx = server.ctx.clone();
        let command = name.to_owned();
        let origin = Origin::Agent {
            client: client.to_owned(),
        };
        let outcome =
            tokio::spawn(async move { registry.dispatch_from(ctx, &command, &body, origin).await })
                .await;
        let ok = matches!(outcome, Ok(Ok(_)));
        crate::info!(
            target: "elyra::mcp",
            "{client} {verb} {name}: {} in {:?}",
            if ok { "ok" } else { "failed" },
            started.elapsed()
        );
        outcome
    }

    /// `resources/read`: run the live command behind the URI.
    async fn read(
        &self,
        params: &Value,
        client: &str,
    ) -> Result<Value, (i64, String, Option<Value>)> {
        let uri = params.get("uri").and_then(Value::as_str).ok_or((
            INVALID_PARAMS,
            "`uri` is required".to_string(),
            None,
        ))?;
        let tool = self.server.resource(uri).ok_or_else(|| {
            (
                INVALID_PARAMS,
                "Resource not found".to_string(),
                Some(json!({ "uri": uri })),
            )
        })?;
        let body = rmp_serde::to_vec(tool.defaults.as_deref().unwrap_or_default())
            .map_err(|e| (INTERNAL_ERROR, e.to_string(), None))?;
        match self.run(&tool.name, body, client, "read").await {
            Ok(Ok(bytes)) => {
                let value: Value = rmp_serde::from_slice(&bytes).unwrap_or(Value::Null);
                Ok(json!({ "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": value.to_string(),
                }] }))
            }
            Ok(Err(e)) => Err((INTERNAL_ERROR, e.to_string(), Some(json!({ "uri": uri })))),
            Err(e) => Err((
                INTERNAL_ERROR,
                format!("`{}` panicked: {e}", tool.name),
                Some(json!({ "uri": uri })),
            )),
        }
    }

    /// Ask a legacy client's user to confirm, with a request of our own.
    /// `None` when it can't ask; otherwise whether the user accepted (no
    /// answer in time is a no).
    async fn elicit(&self, message: &str) -> Option<bool> {
        let version = self.legacy.lock().clone()?;
        if !*self.legacy_elicit.lock() {
            return None;
        }
        let outgoing = self.outgoing.lock().clone()?;
        let id = format!("elyra-{}", self.next_out.fetch_add(1, Ordering::Relaxed));
        let (answer, answered) = tokio::sync::oneshot::channel();
        self.pending.lock().insert(id.clone(), answer);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "elicitation/create",
            // `mode` came in 2025-11-25.
            "params": confirm::elicitation(message, version == LEGACY[0]),
        });
        if outgoing.send(request.to_string()).is_err() {
            self.pending.lock().remove(&id);
            return None;
        }
        let reply = tokio::time::timeout(ELICIT_TIMEOUT, answered).await;
        self.pending.lock().remove(&id);
        Some(matches!(
            reply,
            Ok(Ok(reply)) if reply.pointer("/result/action").and_then(Value::as_str) == Some("accept")
        ))
    }
}

/// A tool that needs confirmation, called by a client that can't ask.
fn cant_ask(name: &str) -> Value {
    tool_error(&format!(
        "`{name}` needs the user's confirmation, and this client can't ask for it \
         (it doesn't support elicitation), so it did not run."
    ))
}

/// Tell the client, on every change, that `changes` resource updated — until
/// the subscription ends (its sender dropped) or the client goes.
fn forward(
    mut changes: tokio::sync::mpsc::UnboundedReceiver<()>,
    outgoing: tokio::sync::mpsc::UnboundedSender<String>,
    notification: String,
) {
    tokio::spawn(async move {
        while changes.recv().await.is_some() {
            if outgoing.send(notification.clone()).is_err() {
                return;
            }
        }
    });
}

/// `notifications/resources/updated` for `uri` — on a modern subscription,
/// with its id.
fn updated(uri: &str, subscription: Option<&Value>) -> String {
    let mut params = json!({ "uri": uri });
    if let Some(id) = subscription {
        params["_meta"] = json!({ META_SUBSCRIPTION: id });
    }
    json!({ "jsonrpc": "2.0", "method": "notifications/resources/updated", "params": params })
        .to_string()
}

fn not_found(id: Value, uri: &str) -> Value {
    error(
        id,
        INVALID_PARAMS,
        "Resource not found",
        Some(json!({ "uri": uri })),
    )
}

fn capabilities() -> Value {
    json!({
        "tools": { "listChanged": false },
        "resources": { "subscribe": cfg!(feature = "database"), "listChanged": false },
    })
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
        map.entry("resultType").or_insert_with(|| json!("complete"));
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
