//! OpenAI's Responses API (`POST /v1/responses`), for a chat with web search
//! (RFC 0049). Every other OpenAI chat stays on Chat Completions
//! ([`crate::openai`]).

use serde_json::{json, Value};

use crate::{
    client::Ai,
    error::{Error, Result},
    openai::error_message,
    provider_tool::ProviderTool,
    request::TextRequest,
    response::{Response, Usage},
    tool::Tool,
};

/// The provider tools as Responses API tools: web search, with what OpenAI
/// can honour. Web fetch and code execution are Anthropic's.
fn provider_tool_specs(tools: &[ProviderTool]) -> Result<Vec<Value>> {
    tools
        .iter()
        .map(|tool| match tool {
            ProviderTool::WebSearch(search) => search.openai_spec(),
            ProviderTool::WebFetch(_) => Err(Error::Unsupported(
                "web fetch requires the Anthropic provider (OpenAI has no tool that fetches a \
                 given URL)"
                    .into(),
            )),
            ProviderTool::CodeExecution => Err(Error::Unsupported(
                "code execution requires the Anthropic provider".into(),
            )),
        })
        .collect()
}

/// Run a request with web search against the Responses API, driving the tool
/// loop: the app's `function_call`s run here, the search runs at OpenAI.
pub(crate) async fn run(ai: &Ai, req: TextRequest, tools: &[Box<dyn Tool>]) -> Result<Response> {
    let mut tool_specs = provider_tool_specs(&req.provider_tools)?;
    tool_specs.extend(tools.iter().map(|t| {
        json!({
            "type": "function",
            "name": t.name(),
            "description": t.description(),
            "parameters": t.parameters(),
        })
    }));
    let url = format!(
        "{}/v1/responses",
        ai.base_url(req.provider).trim_end_matches('/')
    );

    let mut input: Vec<Value> = req
        .messages
        .iter()
        .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
        .collect();

    let mut usage = Usage::default();
    let mut steps = 0u32;
    loop {
        let mut body = json!({
            "model": req.model,
            "input": input,
            "tools": tool_specs,
            "max_output_tokens": req.max_tokens,
            // Nothing kept at OpenAI between steps: each request carries the
            // conversation, reasoning included.
            "store": false,
            "include": ["reasoning.encrypted_content"],
        });
        if let Some(system) = req.system.as_deref().filter(|s| !s.is_empty()) {
            body["instructions"] = json!(system);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(f) = &req.force {
            body["text"] = json!({"format": {
                "type": "json_schema", "name": f.name, "schema": f.schema, "strict": false,
            }});
        }

        let auth = ai.auth(req.provider)?;
        let resp = ai.send(auth.apply(ai.http.post(&url)).json(&body)).await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: error_message(&text),
            });
        }
        let val: Value = serde_json::from_str(&text).map_err(|e| Error::Decode(e.to_string()))?;
        if val["status"] == "failed" {
            return Err(Error::Api {
                status: status.as_u16(),
                message: val["error"]["message"]
                    .as_str()
                    .unwrap_or("the response failed")
                    .to_string(),
            });
        }
        if let Some(u) = val.get("usage") {
            usage.input_tokens += u["input_tokens"].as_u64().unwrap_or(0) as u32;
            usage.output_tokens += u["output_tokens"].as_u64().unwrap_or(0) as u32;
        }

        let output = val["output"].as_array().cloned().unwrap_or_default();
        let calls: Vec<&Value> = output
            .iter()
            .filter(|item| item["type"] == "function_call")
            .collect();

        if calls.is_empty() {
            let mut answer = String::new();
            for item in output.iter().filter(|i| i["type"] == "message") {
                for part in item["content"].as_array().into_iter().flatten() {
                    match part["type"].as_str() {
                        Some("output_text") => answer.push_str(part["text"].as_str().unwrap_or("")),
                        Some("refusal") => {
                            return Err(Error::Refused(
                                part["refusal"].as_str().unwrap_or("refused").to_string(),
                            ))
                        }
                        _ => {}
                    }
                }
            }
            if answer.is_empty() {
                return Err(Error::Empty);
            }
            return Ok(Response {
                text: answer,
                usage,
                steps,
            });
        }

        if steps >= req.max_steps {
            return Err(Error::MaxSteps(req.max_steps));
        }
        // The answer so far goes back as it came (its searches, reasoning and
        // calls), then each call's output.
        let mut outputs = Vec::with_capacity(calls.len());
        for call in &calls {
            let name = call["name"].as_str().unwrap_or_default();
            let args: Value = serde_json::from_str(call["arguments"].as_str().unwrap_or("{}"))
                .unwrap_or_else(|_| json!({}));
            let result = match tools.iter().find(|t| t.name() == name) {
                Some(t) => t.call(args).await.unwrap_or_else(|e| format!("error: {e}")),
                None => format!("error: unknown tool `{name}`"),
            };
            outputs.push(json!({
                "type": "function_call_output",
                "call_id": call["call_id"],
                "output": result,
            }));
        }
        input.extend(output.iter().cloned());
        input.extend(outputs);
        steps += 1;
    }
}
