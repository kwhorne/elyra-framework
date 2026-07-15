use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    client::Ai,
    error::{Error, Result},
    request::TextRequest,
    response::{Response, Usage},
    stream::StreamChunk,
    tool::Tool,
};

/// Build the OpenAI message list (system + user/assistant).
fn initial_messages(req: &TextRequest) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(s) = &req.system {
        if !s.is_empty() {
            messages.push(json!({"role": "system", "content": s}));
        }
    }
    for m in &req.messages {
        messages.push(json!({"role": m.role.as_str(), "content": m.content}));
    }
    messages
}

/// Run a request against the OpenAI Chat Completions API, driving the tool loop.
pub(crate) async fn run(ai: &Ai, req: TextRequest, tools: &[Box<dyn Tool>]) -> Result<Response> {
    if !req.provider_tools.is_empty() {
        return Err(Error::Unsupported(
            "provider tools (web_search / web_fetch) require the Anthropic provider".into(),
        ));
    }
    let key = ai.key(req.provider)?.to_string();
    let url = format!("{}/v1/chat/completions", ai.base_url(req.provider));

    let mut messages = initial_messages(&req);

    let mut tool_specs: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({"type": "function", "function": {
                "name": t.name(), "description": t.description(), "parameters": t.parameters(),
            }})
        })
        .collect();
    if let Some(f) = &req.force {
        tool_specs.push(json!({"type": "function", "function": {
            "name": f.name, "description": "Return the final structured result.", "parameters": f.schema,
        }}));
    }

    let mut usage = Usage::default();
    let mut steps = 0u32;
    loop {
        let mut body =
            json!({"model": req.model, "messages": messages, "max_tokens": req.max_tokens});
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if !tool_specs.is_empty() {
            body["tools"] = json!(tool_specs);
        }
        if let Some(f) = &req.force {
            body["tool_choice"] = json!({"type": "function", "function": {"name": f.name}});
        }

        let resp = ai
            .http
            .post(&url)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let val: Value = resp
            .json()
            .await
            .map_err(|e| Error::Decode(e.to_string()))?;
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
            usage.input_tokens += u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
            usage.output_tokens += u["completion_tokens"].as_u64().unwrap_or(0) as u32;
        }

        let msg = val["choices"][0]["message"].clone();
        let tool_calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();

        if let Some(f) = &req.force {
            for tc in &tool_calls {
                if tc["function"]["name"] == f.name.as_str() {
                    let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    return Ok(Response {
                        text: args.to_string(),
                        usage,
                        steps,
                    });
                }
            }
            return Err(Error::Empty);
        }

        if tool_calls.is_empty() {
            let text = msg["content"].as_str().unwrap_or_default().to_string();
            if text.is_empty() {
                return Err(Error::Empty);
            }
            return Ok(Response { text, usage, steps });
        }

        if steps >= req.max_steps {
            return Err(Error::MaxSteps(req.max_steps));
        }
        messages.push(msg);
        for tc in &tool_calls {
            let name = tc["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let id = tc["id"].as_str().unwrap_or_default().to_string();
            let args: Value =
                serde_json::from_str(tc["function"]["arguments"].as_str().unwrap_or("{}"))
                    .unwrap_or_else(|_| json!({}));
            let output = match tools.iter().find(|t| t.name() == name) {
                Some(t) => t.call(args).await.unwrap_or_else(|e| format!("error: {e}")),
                None => format!("error: unknown tool `{name}`"),
            };
            messages.push(json!({"role": "tool", "tool_call_id": id, "content": output}));
        }
        steps += 1;
    }
}

/// Stream a plain-text response (no tools) via SSE.
pub(crate) async fn stream(ai: &Ai, req: TextRequest, tx: UnboundedSender<Result<StreamChunk>>) {
    let key = match ai.key(req.provider) {
        Ok(k) => k.to_string(),
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    let url = format!("{}/v1/chat/completions", ai.base_url(req.provider));
    let messages = initial_messages(&req);

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }

    let resp = match ai
        .http
        .post(&url)
        .bearer_auth(&key)
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
            if data == "[DONE]" {
                let _ = tx.send(Ok(StreamChunk::Done(usage)));
                return;
            }
            let Ok(val) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(t) = val["choices"][0]["delta"]["content"].as_str() {
                if tx.send(Ok(StreamChunk::Delta(t.to_string()))).is_err() {
                    return;
                }
            }
            if let Some(u) = val.get("usage").filter(|u| !u.is_null()) {
                usage.input_tokens += u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                usage.output_tokens += u["completion_tokens"].as_u64().unwrap_or(0) as u32;
            }
        }
    }
    let _ = tx.send(Ok(StreamChunk::Done(usage)));
}
