use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    client::Ai,
    error::{Error, Result},
    message::Role,
    request::TextRequest,
    response::{Response, Usage},
    stream::{OnText, StreamChunk},
    tool::Tool,
};

/// The API version this driver speaks. From 2026-10-21 the Messages API
/// rejects `temperature`, `top_p` and `top_k` on every model.
const VERSION: &str = "2026-10-21";

/// Build the top-level `system` text and the user/assistant message list.
fn initial_messages(req: &TextRequest) -> (String, Vec<Value>) {
    let mut system = req.system.clone().unwrap_or_default();
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&m.content);
            }
            Role::User => messages.push(json!({"role": "user", "content": m.content})),
            Role::Assistant => messages.push(json!({"role": "assistant", "content": m.content})),
        }
    }
    (system, messages)
}

/// The request body both [`run`] and [`stream`] start from. A temperature is
/// never sent: Claude samples at its defaults (see [`warn_temperature`]).
fn base_body(req: &TextRequest, system: &str, messages: &[Value]) -> Value {
    warn_temperature(req);
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "messages": messages,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    body
}

/// Say once per process that a temperature set for Anthropic isn't sent.
fn warn_temperature(req: &TextRequest) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if req.temperature.is_some() {
        ONCE.call_once(|| {
            eprintln!(
                "elyra-ai: temperature is ignored for Anthropic; \
                 Claude models sample at their defaults"
            );
        });
    }
}

/// `schema` as structured outputs accept it: every object closed with
/// `additionalProperties: false`, and the constraints the API refuses
/// (number and length bounds, unknown formats) moved into the description,
/// where the model still reads them.
fn output_schema(schema: &Value) -> Value {
    const FORMATS: [&str; 10] = [
        "date-time",
        "time",
        "date",
        "duration",
        "email",
        "hostname",
        "uri",
        "ipv4",
        "ipv6",
        "uuid",
    ];
    const BOUNDS: [&str; 9] = [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "minLength",
        "maxLength",
        "maxItems",
        "uniqueItems",
    ];
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            let mut moved = serde_json::Map::new();
            for (k, v) in map {
                match k.as_str() {
                    "$schema" => {}
                    k if BOUNDS.contains(&k) => {
                        moved.insert(k.to_string(), v.clone());
                    }
                    "format" if !v.as_str().is_some_and(|f| FORMATS.contains(&f)) => {
                        moved.insert(k.to_string(), v.clone());
                    }
                    "minItems" if v.as_u64().is_some_and(|n| n > 1) => {
                        moved.insert(k.to_string(), v.clone());
                    }
                    // Maps of subschemas, keyed by name, not keywords.
                    "properties" | "$defs" | "definitions" => {
                        let inner = v
                            .as_object()
                            .map(|m| {
                                m.iter()
                                    .map(|(name, s)| (name.clone(), output_schema(s)))
                                    .collect()
                            })
                            .unwrap_or_default();
                        out.insert(k.to_string(), Value::Object(inner));
                    }
                    _ => {
                        out.insert(k.to_string(), output_schema(v));
                    }
                }
            }
            let is_object = out.contains_key("properties")
                || out.get("type").is_some_and(|t| {
                    t == "object"
                        || t.as_array()
                            .is_some_and(|a| a.iter().any(|t| t == "object"))
                });
            if is_object {
                out.insert("additionalProperties".into(), Value::Bool(false));
            }
            if !moved.is_empty() {
                let note = Value::Object(moved).to_string();
                let description = match out.get("description").and_then(Value::as_str) {
                    Some(d) => format!("{d} {note}"),
                    None => note,
                };
                out.insert("description".into(), json!(description));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(output_schema).collect()),
        other => other.clone(),
    }
}

/// The refusal in a response that stopped with `stop_reason: "refusal"`.
fn refusal(val: &Value) -> Option<Error> {
    (val["stop_reason"] == "refusal").then(|| {
        let details = &val["stop_details"];
        let reason = details["explanation"]
            .as_str()
            .or_else(|| details["category"].as_str())
            .unwrap_or("declined by the model's safeguards");
        Error::Refused(reason.to_string())
    })
}

/// Run a request against the Anthropic Messages API, driving the tool loop.
pub(crate) async fn run(ai: &Ai, req: TextRequest, tools: &[Box<dyn Tool>]) -> Result<Response> {
    let url = format!("{}/v1/messages", ai.base_url(req.provider));

    let (system, mut messages) = initial_messages(&req);

    let mut tool_specs: Vec<Value> = tools
        .iter()
        .map(|t| json!({"name": t.name(), "description": t.description(), "input_schema": t.parameters()}))
        .collect();
    // Native provider tools (web search / fetch) run server-side.
    let mut beta: Option<&'static str> = None;
    for pt in &req.provider_tools {
        tool_specs.push(pt.anthropic_spec(&req.model));
        if let Some(b) = pt.anthropic_beta(&req.model) {
            beta = Some(b);
        }
    }

    let mut usage = Usage::default();
    let mut steps = 0u32;
    loop {
        let mut body = base_body(&req, &system, &messages);
        if !tool_specs.is_empty() {
            body["tools"] = json!(tool_specs);
        }
        // Structured output: the final text is JSON matching the schema.
        // Forcing a tool call instead is refused by the newest models.
        if let Some(f) = &req.force {
            body["output_config"] =
                json!({"format": {"type": "json_schema", "schema": output_schema(&f.schema)}});
        }
        // `on_text`: each step streamed, its text told as it comes.
        if req.on_text.is_some() {
            body["stream"] = json!(true);
        }

        // Asked each step: a bearer token may be renewed mid-run.
        let auth = ai.auth(req.provider)?;
        // Asked for, so a gateway (Laravel) answers its own errors in JSON
        // too rather than redirecting to a login page.
        let mut builder = auth
            .apply(ai.http.post(&url))
            .header("accept", "application/json")
            .header("anthropic-version", VERSION);
        if let Some(b) = beta {
            builder = builder.header("anthropic-beta", b);
        }
        let resp = ai.send(builder.json(&body)).await?;
        let status = resp.status();
        let streamed = req.on_text.is_some()
            && status.is_success()
            && resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|t| t.starts_with("text/event-stream"));
        let val: Value = match (&req.on_text, streamed) {
            (Some(on_text), true) => read_stream(resp.bytes_stream(), steps, on_text).await?,
            _ => resp
                .json()
                .await
                .map_err(|e| Error::Decode(e.to_string()))?,
        };
        // Asked to stream and answered whole (a gateway that doesn't): the
        // step's text, told at once.
        if let (Some(on_text), false, true) = (&req.on_text, streamed, status.is_success()) {
            let text: String = val["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect();
            if !text.is_empty() {
                on_text(steps, &text);
            }
        }
        if !status.is_success() {
            let message = val["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            return Err(Error::Api {
                status: status.as_u16(),
                message,
            });
        }
        if let Some(u) = val.get("usage") {
            usage.input_tokens += u["input_tokens"].as_u64().unwrap_or(0) as u32;
            usage.output_tokens += u["output_tokens"].as_u64().unwrap_or(0) as u32;
        }

        if let Some(refused) = refusal(&val) {
            return Err(refused);
        }

        let content = val["content"].as_array().cloned().unwrap_or_default();

        // The server's own tool loop (web search / fetch) paused: send the
        // turn back as it is and the server resumes it. No "continue" message.
        if val["stop_reason"] == "pause_turn" {
            if steps >= req.max_steps {
                return Err(Error::MaxSteps(req.max_steps));
            }
            messages.push(json!({"role": "assistant", "content": content}));
            steps += 1;
            continue;
        }

        let tool_uses: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
        if tool_uses.is_empty() {
            let text = content
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() {
                return Err(Error::Empty);
            }
            return Ok(Response { text, usage, steps });
        }

        if steps >= req.max_steps {
            return Err(Error::MaxSteps(req.max_steps));
        }
        messages.push(json!({"role": "assistant", "content": content}));
        let mut results: Vec<Value> = Vec::new();
        for tu in tool_uses {
            let name = tu["name"].as_str().unwrap_or_default().to_string();
            let id = tu["id"].as_str().unwrap_or_default().to_string();
            let args = tu["input"].clone();
            let output = match tools.iter().find(|t| t.name() == name) {
                Some(t) => t.call(args).await.unwrap_or_else(|e| format!("error: {e}")),
                None => format!("error: unknown tool `{name}`"),
            };
            results.push(json!({"type": "tool_result", "tool_use_id": id, "content": output}));
        }
        messages.push(json!({"role": "user", "content": results}));
        steps += 1;
    }
}

/// A step's SSE, read into the message a whole answer would have been:
/// `content` (text, tool uses with their input, server tool blocks),
/// `stop_reason`, `stop_details` and `usage`. Each text delta is told to
/// `on_text` as it comes, with `step`.
pub(crate) async fn read_stream<S, B, E>(mut bytes: S, step: u32, on_text: &OnText) -> Result<Value>
where
    S: futures_util::Stream<Item = std::result::Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Into<Error>,
{
    let mut message = json!({ "content": [], "usage": { "input_tokens": 0, "output_tokens": 0 } });
    let mut blocks: Vec<Value> = Vec::new();
    // Each tool use's input, as its JSON comes in pieces, by block.
    let mut inputs: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut stopped = false;
    while let Some(chunk) = bytes.next().await {
        buf.extend_from_slice(chunk.map_err(Into::into)?.as_ref());
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            let index = event["index"].as_u64().unwrap_or(0) as usize;
            match event["type"].as_str() {
                Some("message_start") => {
                    message["usage"]["input_tokens"] = json!(event["message"]["usage"]
                        ["input_tokens"]
                        .as_u64()
                        .unwrap_or(0));
                }
                Some("content_block_start") => {
                    if blocks.len() <= index {
                        blocks.resize(index + 1, Value::Null);
                    }
                    blocks[index] = event["content_block"].clone();
                }
                Some("content_block_delta") => {
                    let Some(block) = blocks.get_mut(index) else {
                        continue;
                    };
                    let delta = &event["delta"];
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            let piece = delta["text"].as_str().unwrap_or_default();
                            let text =
                                format!("{}{piece}", block["text"].as_str().unwrap_or_default());
                            block["text"] = json!(text);
                            if !piece.is_empty() {
                                on_text(step, piece);
                            }
                        }
                        Some("input_json_delta") => {
                            inputs
                                .entry(index)
                                .or_default()
                                .push_str(delta["partial_json"].as_str().unwrap_or_default());
                        }
                        Some("thinking_delta") => {
                            let text = format!(
                                "{}{}",
                                block["thinking"].as_str().unwrap_or_default(),
                                delta["thinking"].as_str().unwrap_or_default()
                            );
                            block["thinking"] = json!(text);
                        }
                        Some("signature_delta") => block["signature"] = delta["signature"].clone(),
                        Some("citations_delta") => {
                            if !block["citations"].is_array() {
                                block["citations"] = json!([]);
                            }
                            if let Some(list) = block["citations"].as_array_mut() {
                                list.push(delta["citation"].clone());
                            }
                        }
                        _ => {}
                    }
                }
                Some("content_block_stop") => {
                    if let (Some(block), Some(input)) =
                        (blocks.get_mut(index), inputs.remove(&index))
                    {
                        block["input"] = if input.trim().is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str(&input)
                                .map_err(|e| Error::Decode(format!("a tool's input: {e}")))?
                        };
                    }
                }
                Some("message_delta") => {
                    for key in ["stop_reason", "stop_details"] {
                        if !event["delta"][key].is_null() {
                            message[key] = event["delta"][key].clone();
                        }
                    }
                    if let Some(out) = event["usage"]["output_tokens"].as_u64() {
                        message["usage"]["output_tokens"] = json!(out);
                    }
                }
                Some("message_stop") => stopped = true,
                Some("error") => {
                    return Err(Error::Api {
                        status: if event["error"]["type"] == "overloaded_error" {
                            529
                        } else {
                            500
                        },
                        message: event["error"]["message"]
                            .as_str()
                            .unwrap_or("the stream failed")
                            .to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    if !stopped {
        return Err(Error::Decode(
            "the stream ended before its message did".into(),
        ));
    }
    blocks.retain(|b| !b.is_null());
    message["content"] = Value::Array(blocks);
    Ok(message)
}

/// Stream a plain-text response (no tools) via SSE.
pub(crate) async fn stream(ai: &Ai, req: TextRequest, tx: UnboundedSender<Result<StreamChunk>>) {
    let auth = match ai.auth(req.provider) {
        Ok(auth) => auth,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    let url = format!("{}/v1/messages", ai.base_url(req.provider));
    let (system, messages) = initial_messages(&req);

    let mut body = base_body(&req, &system, &messages);
    body["stream"] = json!(true);

    let resp = match auth
        .apply(ai.http.post(&url))
        .header("anthropic-version", VERSION)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Err(e.into()));
            return;
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let message = resp.text().await.unwrap_or_default();
        let _ = tx.send(Err(Error::Api {
            status: status.as_u16(),
            message,
        }));
        return;
    }

    let mut usage = Usage::default();
    let mut byte_stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = byte_stream.next().await {
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => {
                let _ = tx.send(Err(e.into()));
                return;
            }
        };
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            let Ok(val) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            match val["type"].as_str() {
                Some("content_block_delta") => {
                    if let Some(t) = val["delta"]["text"].as_str() {
                        if tx.send(Ok(StreamChunk::Delta(t.to_string()))).is_err() {
                            return;
                        }
                    }
                }
                Some("message_start") => {
                    usage.input_tokens += val["message"]["usage"]["input_tokens"]
                        .as_u64()
                        .unwrap_or(0) as u32;
                }
                Some("message_delta") => {
                    usage.output_tokens +=
                        val["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
                    if let Some(refused) = refusal(&val["delta"]) {
                        let _ = tx.send(Err(refused));
                        return;
                    }
                }
                Some("message_stop") => {
                    let _ = tx.send(Ok(StreamChunk::Done(usage)));
                    return;
                }
                _ => {}
            }
        }
    }
    let _ = tx.send(Ok(StreamChunk::Done(usage)));
}

#[cfg(test)]
mod tests {

    /// `sse` (a step's events) read as the network might hand it over: in
    /// pieces of `size` bytes, lines split anywhere.
    fn read(sse: &str, size: usize) -> (Result<Value>, Vec<(u32, String)>) {
        let told = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen = told.clone();
        let on_text: OnText = std::sync::Arc::new(move |step, piece: &str| {
            seen.lock().push((step, piece.to_owned()));
        });
        let pieces: Vec<std::result::Result<Vec<u8>, Error>> = sse
            .as_bytes()
            .chunks(size)
            .map(|c| Ok(c.to_vec()))
            .collect();
        let out =
            futures_executor_block(read_stream(futures_util::stream::iter(pieces), 2, &on_text));
        let told = told.lock().clone();
        (out, told)
    }

    fn futures_executor_block<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn events(list: &[Value]) -> String {
        list.iter()
            .map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e))
            .collect()
    }

    #[test]
    fn a_streamed_answer_is_told_as_it_comes_and_read_whole() {
        let sse = events(&[
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 12, "output_tokens": 1 } } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
            json!({ "type": "ping" }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Three invoices" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": " are overdue: æøå." } }),
            json!({ "type": "content_block_stop", "index": 0 }),
            json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 9 } }),
            json!({ "type": "message_stop" }),
        ]);
        for size in [1, 7, 4096] {
            let (val, told) = read(&sse, size);
            let val = val.unwrap();
            assert_eq!(
                val["content"],
                json!([{ "type": "text", "text": "Three invoices are overdue: æøå." }])
            );
            assert_eq!(val["stop_reason"], "end_turn");
            assert_eq!(
                val["usage"],
                json!({ "input_tokens": 12, "output_tokens": 9 })
            );
            assert_eq!(
                told,
                [
                    (2, "Three invoices".to_owned()),
                    (2, " are overdue: æøå.".to_owned())
                ],
                "in pieces of {size}"
            );
        }
    }

    #[test]
    fn a_tool_use_is_read_from_its_pieces_after_the_text_before_it() {
        let sse = events(&[
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 30 } } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Let me look." } }),
            json!({ "type": "content_block_stop", "index": 0 }),
            json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "invoices_index", "input": {} } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"query\": {\"st" } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "atus\": \"overdue\"}}" } }),
            json!({ "type": "content_block_stop", "index": 1 }),
            json!({ "type": "content_block_start", "index": 2, "content_block": { "type": "tool_use", "id": "toolu_2", "name": "ping", "input": {} } }),
            json!({ "type": "content_block_stop", "index": 2 }),
            json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 40 } }),
            json!({ "type": "message_stop" }),
        ]);
        let (val, told) = read(&sse, 5);
        let val = val.unwrap();
        assert_eq!(
            val["content"],
            json!([
                { "type": "text", "text": "Let me look." },
                { "type": "tool_use", "id": "toolu_1", "name": "invoices_index", "input": { "query": { "status": "overdue" } } },
                { "type": "tool_use", "id": "toolu_2", "name": "ping", "input": {} },
            ])
        );
        assert_eq!(val["stop_reason"], "tool_use");
        assert_eq!(told, [(2, "Let me look.".to_owned())]);
    }

    #[test]
    fn a_refusal_an_error_and_a_cut_off_stream_are_said() {
        let refused = events(&[
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 3 } } }),
            json!({ "type": "message_delta", "delta": { "stop_reason": "refusal", "stop_details": { "explanation": "no" } }, "usage": { "output_tokens": 1 } }),
            json!({ "type": "message_stop" }),
        ]);
        let val = read(&refused, 64).0.unwrap();
        assert!(matches!(refusal(&val), Some(Error::Refused(why)) if why == "no"));

        let overloaded = events(&[
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 3 } } }),
            json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }),
        ]);
        assert!(matches!(
            read(&overloaded, 64).0,
            Err(Error::Api { status: 529, .. })
        ));
        let other = events(&[
            json!({ "type": "error", "error": { "type": "api_error", "message": "boom" } }),
        ]);
        assert!(
            matches!(read(&other, 64).0, Err(Error::Api { status: 500, ref message }) if message == "boom")
        );

        let cut = events(&[
            json!({ "type": "message_start", "message": { "usage": { "input_tokens": 3 } } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hal" } }),
        ]);
        assert!(matches!(read(&cut, 64).0, Err(Error::Decode(_))));
        let bad_input = events(&[
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "t", "name": "x", "input": {} } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": "{\"a\": " } }),
            json!({ "type": "content_block_stop", "index": 0 }),
        ]);
        assert!(matches!(read(&bad_input, 64).0, Err(Error::Decode(_))));
    }

    use super::*;

    #[allow(dead_code)]
    #[derive(schemars::JsonSchema)]
    struct Order {
        /// Who ordered.
        customer: Customer,
        quantity: u32,
        note: Option<String>,
        lines: Vec<String>,
        placed: Placed,
    }

    #[allow(dead_code)]
    #[derive(schemars::JsonSchema)]
    struct Customer {
        email: String,
    }

    #[allow(dead_code)]
    #[derive(schemars::JsonSchema)]
    enum Placed {
        Web,
        Phone,
    }

    #[test]
    fn a_schemars_schema_becomes_one_structured_outputs_accept() {
        let schema = output_schema(&serde_json::to_value(schemars::schema_for!(Order)).unwrap());
        assert!(schema.get("$schema").is_none());
        assert_eq!(schema["additionalProperties"], false);
        // The nested struct lives in $defs and is closed too.
        let customer = &schema["$defs"]["Customer"];
        assert_eq!(customer["additionalProperties"], false);
        assert_eq!(customer["properties"]["email"]["type"], "string");
        // schemars bounds a u32 with `minimum` and an unknown `format`; both
        // move to the description, where the model still reads them.
        let quantity = &schema["properties"]["quantity"];
        assert!(quantity.get("minimum").is_none());
        assert!(quantity.get("format").is_none());
        let description = quantity["description"].as_str().unwrap();
        assert!(description.contains("\"minimum\":0"), "{description}");
        assert!(description.contains("uint32"), "{description}");
        // Property names that look like keywords stay properties.
        assert!(schema["properties"]["note"].is_object());
        assert_eq!(
            schema["required"],
            json!(["customer", "quantity", "lines", "placed"])
        );
    }

    #[test]
    fn supported_formats_and_small_min_items_stay() {
        let schema = output_schema(&json!({
            "type": "object",
            "description": "A meeting.",
            "properties": {
                "at": {"type": "string", "format": "date-time"},
                "guests": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                "rooms": {"type": "array", "items": {"type": "string"}, "minItems": 2, "maxItems": 4},
                "code": {"type": "string", "description": "Room code.", "maxLength": 6},
                // A property named like a keyword is still a property.
                "format": {"type": "string"},
            },
            "$defs": {"uniqueItems": {"type": "object", "properties": {}}},
        }));
        assert_eq!(schema["properties"]["format"], json!({"type": "string"}));
        assert_eq!(schema["description"], "A meeting.");
        assert_eq!(schema["properties"]["at"]["format"], "date-time");
        assert_eq!(schema["properties"]["guests"]["minItems"], 1);
        let rooms = &schema["properties"]["rooms"];
        assert!(rooms.get("minItems").is_none() && rooms.get("maxItems").is_none());
        assert_eq!(rooms["description"], r#"{"maxItems":4,"minItems":2}"#);
        assert_eq!(
            schema["properties"]["code"]["description"],
            r#"Room code. {"maxLength":6}"#
        );
        // So is a definition.
        assert_eq!(schema["$defs"]["uniqueItems"]["type"], "object");
    }

    #[test]
    fn a_refusal_carries_its_explanation() {
        let refused = refusal(&json!({
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "cyber", "explanation": "Not this."},
        }));
        assert!(matches!(refused, Some(Error::Refused(r)) if r == "Not this."));
        let refused =
            refusal(&json!({"stop_reason": "refusal", "stop_details": {"category": "bio"}}));
        assert!(matches!(refused, Some(Error::Refused(r)) if r == "bio"));
        assert!(refusal(&json!({"stop_reason": "end_turn"})).is_none());
    }
}
