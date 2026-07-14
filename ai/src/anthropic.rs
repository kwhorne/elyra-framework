use serde_json::{json, Value};

use crate::{
    client::Ai,
    error::{Error, Result},
    message::Role,
    request::TextRequest,
    response::{Response, Usage},
    tool::Tool,
};

/// Run a request against the Anthropic Messages API, driving the tool loop.
pub(crate) async fn run(ai: &Ai, req: TextRequest, tools: &[Box<dyn Tool>]) -> Result<Response> {
    let key = ai.key(req.provider)?.to_string();
    let url = format!("{}/v1/messages", ai.base_url(req.provider));

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

    let mut tool_specs: Vec<Value> = tools
        .iter()
        .map(|t| json!({"name": t.name(), "description": t.description(), "input_schema": t.parameters()}))
        .collect();
    if let Some(f) = &req.force {
        tool_specs.push(json!({
            "name": f.name,
            "description": "Return the final structured result.",
            "input_schema": f.schema,
        }));
    }

    let mut usage = Usage::default();
    let mut steps = 0u32;
    loop {
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if !tool_specs.is_empty() {
            body["tools"] = json!(tool_specs);
        }
        if let Some(f) = &req.force {
            body["tool_choice"] = json!({"type": "tool", "name": f.name});
        }

        let resp = ai
            .http
            .post(&url)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01")
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
            usage.input_tokens += u["input_tokens"].as_u64().unwrap_or(0) as u32;
            usage.output_tokens += u["output_tokens"].as_u64().unwrap_or(0) as u32;
        }

        let content = val["content"].as_array().cloned().unwrap_or_default();

        if let Some(f) = &req.force {
            for block in &content {
                if block["type"] == "tool_use" && block["name"] == f.name.as_str() {
                    return Ok(Response {
                        text: block["input"].to_string(),
                        usage,
                        steps,
                    });
                }
            }
            return Err(Error::Empty);
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
