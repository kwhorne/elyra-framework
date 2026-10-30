//! A scripted model for tests: no network, no key, and the real tool loop.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    error::{Error, Result},
    message::Role,
    request::TextRequest,
    response::{Response, Usage},
    stream::StreamChunk,
    tool::Tool,
};

/// A stand-in for the model, answering from a script, for tests.
///
/// Each prompt step takes the next reply: text ends the run, a tool call runs
/// the agent's real [`Tool`] and goes on with its result, as a provider
/// would. What was sent and what the tools answered are kept for asserting.
///
/// ```
/// use elyra_ai::{Ai, AiFake};
/// use serde_json::json;
///
/// let fake = AiFake::new()
///     .tool_call("lookup", json!({ "name": "Ada" }))
///     .text("Ada has two overdue invoices.");
/// let ai = Ai::fake(fake.clone());
/// // … prompt an agent with a `lookup` tool, then:
/// // assert_eq!(fake.calls()[0].output, "…");
/// # let _ = ai;
/// ```
///
/// A run that needs a reply the script doesn't have panics, naming the
/// step: a test that prompts more than it meant to fails loudly.
#[derive(Clone, Default)]
pub struct AiFake {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    replies: VecDeque<(Reply, Usage)>,
    requests: Vec<FakeRequest>,
    calls: Vec<FakeCall>,
}

enum Reply {
    Text(String),
    ToolCalls(Vec<(String, Value)>),
    Refusal(String),
    Error(u16, String),
}

/// One step's request, as the fake received it.
#[derive(Clone, Debug)]
pub struct FakeRequest {
    pub model: String,
    /// The instructions, with any system messages appended.
    pub system: String,
    /// The conversation so far, in the Messages API's shape: `user` and
    /// `assistant` turns, with `tool_use` and `tool_result` blocks.
    pub messages: Vec<Value>,
    /// The names of the tools offered.
    pub tools: Vec<String>,
    /// Whether structured output was asked for.
    pub structured: bool,
    pub max_tokens: u32,
}

/// A tool the script called, and what it answered.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeCall {
    pub name: String,
    pub input: Value,
    /// The tool's output, as the model would read it; `error: …` when it
    /// failed or doesn't exist.
    pub output: String,
}

impl AiFake {
    /// An empty script.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reply with `text`, ending the run. For structured output, the JSON.
    pub fn text(self, text: impl Into<String>) -> Self {
        self.push(Reply::Text(text.into()))
    }

    /// Reply with a call to the tool `name`.
    pub fn tool_call(self, name: impl Into<String>, input: Value) -> Self {
        self.push(Reply::ToolCalls(vec![(name.into(), input)]))
    }

    /// Reply with several tool calls in one turn, run in order.
    pub fn tool_calls<I, S>(self, calls: I) -> Self
    where
        I: IntoIterator<Item = (S, Value)>,
        S: Into<String>,
    {
        let calls = calls.into_iter().map(|(n, i)| (n.into(), i)).collect();
        self.push(Reply::ToolCalls(calls))
    }

    /// Decline, as the model's safeguards do: [`Error::Refused`].
    pub fn refusal(self, explanation: impl Into<String>) -> Self {
        self.push(Reply::Refusal(explanation.into()))
    }

    /// Fail with an API error: [`Error::Api`].
    pub fn error(self, status: u16, message: impl Into<String>) -> Self {
        self.push(Reply::Error(status, message.into()))
    }

    /// The token usage the last reply reports (default: none).
    pub fn usage(self, input_tokens: u32, output_tokens: u32) -> Self {
        if let Some((_, usage)) = self.inner.lock().replies.back_mut() {
            *usage = Usage {
                input_tokens,
                output_tokens,
            };
        }
        self
    }

    /// Every step's request so far, in order.
    pub fn requests(&self) -> Vec<FakeRequest> {
        self.inner.lock().requests.clone()
    }

    /// Every tool call so far, with its output.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.inner.lock().calls.clone()
    }

    /// Whether every scripted reply was used.
    pub fn is_spent(&self) -> bool {
        self.inner.lock().replies.is_empty()
    }

    fn push(self, reply: Reply) -> Self {
        self.inner
            .lock()
            .replies
            .push_back((reply, Usage::default()));
        self
    }

    /// Record `request` and take the reply to it.
    fn next(&self, request: FakeRequest) -> (Reply, Usage) {
        let mut inner = self.inner.lock();
        let step = inner.requests.len() + 1;
        inner.requests.push(request);
        inner.replies.pop_front().unwrap_or_else(|| {
            panic!("AiFake: step {step} asked for a reply, and the script has no more")
        })
    }
}

/// The conversation `req` starts with, as [`FakeRequest`] shows it.
fn initial(req: &TextRequest) -> (String, Vec<Value>) {
    let mut system = req.system.clone().unwrap_or_default();
    let mut messages = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&m.content);
            }
            Role::User | Role::Assistant => {
                messages.push(json!({ "role": m.role.as_str(), "content": m.content }));
            }
        }
    }
    (system, messages)
}

fn add(total: &mut Usage, usage: Usage) {
    total.input_tokens += usage.input_tokens;
    total.output_tokens += usage.output_tokens;
}

/// [`Chat::prompt`](crate::Chat::prompt) against the script.
pub(crate) async fn run(
    fake: &AiFake,
    req: TextRequest,
    tools: &[Box<dyn Tool>],
) -> Result<Response> {
    let (system, mut messages) = initial(&req);
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    let mut usage = Usage::default();
    let mut steps = 0u32;
    loop {
        let (reply, used) = fake.next(FakeRequest {
            model: req.model.clone(),
            system: system.clone(),
            messages: messages.clone(),
            tools: names.clone(),
            structured: req.force.is_some(),
            max_tokens: req.max_tokens,
        });
        add(&mut usage, used);
        let calls = match reply {
            Reply::Text(text) => return Ok(Response { text, usage, steps }),
            Reply::Refusal(why) => return Err(Error::Refused(why)),
            Reply::Error(status, message) => return Err(Error::Api { status, message }),
            Reply::ToolCalls(calls) => calls,
        };
        if steps >= req.max_steps {
            return Err(Error::MaxSteps(req.max_steps));
        }
        let mut uses = Vec::new();
        let mut results = Vec::new();
        for (n, (name, input)) in calls.into_iter().enumerate() {
            let id = format!("toolu_fake_{steps}_{n}");
            let output = match tools.iter().find(|t| t.name() == name) {
                Some(t) => t
                    .call(input.clone())
                    .await
                    .unwrap_or_else(|e| format!("error: {e}")),
                None => format!("error: unknown tool `{name}`"),
            };
            uses.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }));
            results.push(json!({ "type": "tool_result", "tool_use_id": id, "content": output }));
            fake.inner.lock().calls.push(FakeCall {
                name,
                input,
                output,
            });
        }
        messages.push(json!({ "role": "assistant", "content": uses }));
        messages.push(json!({ "role": "user", "content": results }));
        steps += 1;
    }
}

/// [`Chat::stream`](crate::Chat::stream) against the script: the text a
/// word at a time. Tools aren't used in streaming mode.
pub(crate) async fn stream(
    fake: &AiFake,
    req: TextRequest,
    tx: UnboundedSender<Result<StreamChunk>>,
) {
    let (system, messages) = initial(&req);
    let (reply, usage) = fake.next(FakeRequest {
        model: req.model.clone(),
        system,
        messages,
        tools: Vec::new(),
        structured: false,
        max_tokens: req.max_tokens,
    });
    match reply {
        Reply::Text(text) => {
            for word in text.split_inclusive(' ') {
                let _ = tx.send(Ok(StreamChunk::Delta(word.to_owned())));
            }
            let _ = tx.send(Ok(StreamChunk::Done(usage)));
        }
        Reply::Refusal(why) => {
            let _ = tx.send(Err(Error::Refused(why)));
        }
        Reply::Error(status, message) => {
            let _ = tx.send(Err(Error::Api { status, message }));
        }
        Reply::ToolCalls(_) => {
            let _ = tx.send(Err(Error::Unsupported(
                "AiFake: a tool call in a stream (tools aren't used in streaming mode)".into(),
            )));
        }
    }
}
