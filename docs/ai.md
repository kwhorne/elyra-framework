# AI SDK

An ergonomic, Laravel-inspired AI SDK for Elyra apps — **agents**, **tools**,
**structured output**, **images**, and **embeddings** over Anthropic and OpenAI.
It lives in the [`elyra-ai`](../ai) crate and is re-exported as `elyra::ai`
behind the `ai` feature. Everything runs in the Rust backend, so API keys never
reach the frontend.

```toml
elyra = { version = "0.5", features = ["ai"] }
```

## Configuration

Keys are read from the environment:

```
ANTHROPIC_API_KEY=…
OPENAI_API_KEY=…
# optional proxy/gateway overrides
ANTHROPIC_BASE_URL=…
OPENAI_BASE_URL=…
```

The default provider is **Anthropic** with the `claude-sonnet-5-5` text model;
images default to OpenAI `gpt-image-1`, embeddings to `text-embedding-3-small`.
Build a client with `Ai::from_env()` or `Ai::builder()`.

A key compiled into a desktop app is a key anyone can extract. Behind a
gateway that holds the key (your own server, with the user's session),
authenticate with a **bearer token** instead. `token()` is asked for each
request, so a renewed token is picked up, and with none (the user signed out)
the call fails with `Error::NoToken` before anything is sent:

```rust
let ai = Ai::builder()
    .base_url(Provider::Anthropic, "https://crm.example.com/api/elyra/assistant")
    .bearer(Provider::Anthropic, move || session.token())   // Option<String>
    .build();
```

The request is then `Authorization: Bearer <token>` instead of `x-api-key`.

### Binding into the app

Add the provider so commands can resolve the client:

```rust
use elyra::App;
use elyra::ai::{Ai, AiProvider};

App::new().provider(AiProvider).run()?;
```

```rust
#[command]
async fn ask(ctx: Ctx, prompt: String) -> Result<String, String> {
    ctx.get::<Ai>()
        .chat()
        .instructions("You are a concise assistant.")
        .prompt(prompt)
        .await
        .map(|r| r.text().to_string())
        .map_err(|e| e.to_string())
}
```

## Anonymous agents (one-off chats)

The Rust analogue of Laravel's `agent(...)` helper:

```rust
let reply = ai.chat()
    .instructions("You are a concise Rust expert.")
    .prompt("What is ownership?")
    .await?;
println!("{reply}");                 // Display = the text
println!("{:?}", reply.usage());     // token usage
```

`.temperature(t)` applies to OpenAI only. Anthropic's Messages API (version
`2026-10-21`, which the SDK speaks) refuses `temperature` on every model, so
the SDK doesn't send it there and Claude samples at its defaults; it says so
once on stderr.

When Claude declines a request for safety reasons, the call fails with
`Error::Refused`, carrying the model's explanation.

Override the provider/model per call:

```rust
use elyra::ai::Provider;

ai.chat()
    .provider(Provider::OpenAI)
    .model("gpt-4o-mini")
    .prompt("Summarize this…")
    .await?;
```

## Named agents

Implement [`Agent`] for reusable agents (like a Laravel agent class). Only
`instructions` is required; override the rest for context, tools, or model
config.

```rust
use elyra::ai::{Agent, Message, Provider, Tool};

struct SalesCoach { history: Vec<Message> }

impl Agent for SalesCoach {
    fn instructions(&self) -> String {
        "You are a sales coach. Give concise, actionable feedback.".into()
    }
    fn messages(&self) -> Vec<Message> { self.history.clone() }
    fn provider(&self) -> Option<Provider> { Some(Provider::Anthropic) }
    fn max_steps(&self) -> u32 { 6 }
}

let resp = ai.prompt(&SalesCoach { history: vec![] }, "Analyze this transcript…").await?;
```

## Tools

Implement [`Tool`] — a `name`, `description`, a JSON-Schema for the parameters,
and `call`. The SDK runs the tool loop automatically (up to `max_steps`).

```rust
use elyra::ai::{async_trait, json, Result, Tool, Value};

struct RandomNumber;

#[async_trait]
impl Tool for RandomNumber {
    fn name(&self) -> String { "random_number".into() }
    fn description(&self) -> String { "Generate a random integer in [min, max].".into() }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "min": { "type": "integer" },
                "max": { "type": "integer" }
            },
            "required": ["min", "max"]
        })
    }
    async fn call(&self, args: Value) -> Result<String> {
        let min = args["min"].as_i64().unwrap_or(0);
        let max = args["max"].as_i64().unwrap_or(100);
        Ok(((min + max) / 2).to_string())
    }
}

let reply = ai.chat()
    .instructions("Use tools when useful.")
    .tool(RandomNumber)
    .prompt("Pick a number between 1 and 10.")
    .await?;
```

## Sub-agents

Delegate to a specialized agent by adding it as a **sub-agent** — an [`Agent`]
used as a tool. The delegate runs in isolation (it does **not** see the parent's
history), exactly like Laravel's sub-agents. Override `name`/`description` so the
parent knows when to call it.

```rust
use elyra::ai::{Agent, Provider};

struct RefundsAgent;
impl Agent for RefundsAgent {
    fn instructions(&self) -> String {
        "You are a refunds specialist. Give concise eligibility guidance.".into()
    }
    fn name(&self) -> String { "refunds_specialist".into() }
    fn description(&self) -> String { "Answer refund eligibility questions.".into() }
    fn provider(&self) -> Option<Provider> { Some(Provider::Anthropic) }
}

let reply = ai.chat()
    .instructions("You help customers. Delegate refund questions to the specialist.")
    .sub_agent(RefundsAgent)
    .prompt("Can I return an item I bought 40 days ago?")
    .await?;
```

Sub-agents can have their own tools and sub-agents, composing into a hierarchy.
For full control over the tool name/description at the call site, build the
wrapper directly:

```rust
use elyra::ai::AgentTool;

let tool = AgentTool::new(&ai, RefundsAgent)
    .with_name("refunds")
    .with_description("Refund policy expert.");
```

## Provider tools (web search / fetch)

Provider tools are **executed by the AI provider**, not your app (unlike the
[`Tool`](#tools) trait above). They run server-side and the results are folded
into the answer.

> **Anthropic only.** Web search/fetch run natively on Anthropic within a turn.
> OpenAI exposes these through its Responses API, which this SDK doesn't use yet,
> so combining them with `Provider::OpenAI` returns an `Unsupported` error. These
> paths are **not live-tested** here. Opus 4.6 and later and Sonnet 4.6 and
> later get the 2026-02-09 tools, which filter results before they reach the
> context; Haiku and older models get the basic ones. When a long search pauses
> the turn (`pause_turn`), the SDK sends it back and the server resumes it;
> each resume counts against `max_steps`.

```rust
use elyra::ai::{Provider, WebSearch, WebFetch, UserLocation};

let answer = ai.chat()
    .provider(Provider::Anthropic)
    .instructions("Answer with up-to-date facts and cite sources.")
    .web_search(
        WebSearch::new()
            .max(5)
            .allow(["rust-lang.org", "docs.rs"])
            .location(UserLocation { country: Some("NO".into()), ..Default::default() }),
    )
    .prompt("What changed in the latest Rust release?")
    .await?;

// Web fetch (retrieves a specific URL):
ai.chat()
    .web_fetch(WebFetch::new().max(3).allow(["doc.rust-lang.org"]))
    .prompt("Summarize https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html")
    .await?;
```

## Structured output

Return typed JSON with `prompt_as::<T>()`, where `T` derives `serde::Deserialize`
and `schemars::JsonSchema`. The model answers JSON matching `T`'s schema, and
the SDK deserializes it. With Anthropic this is structured outputs
(`output_config.format`), so the agent's tools still run first; with OpenAI the
model is forced to call a synthetic tool.

```rust
use elyra::ai::JsonSchema;

#[derive(serde::Deserialize, JsonSchema)]
struct Sentiment {
    label: String,   // positive | negative | neutral
    score: i32,      // 1–10
}

let s: Sentiment = ai.chat()
    .instructions("Classify the sentiment.")
    .prompt_as("I love building with Elyra!")
    .await?;
```

Anthropic accepts a subset of JSON Schema: the SDK closes every object
(`additionalProperties: false`) and moves what it refuses (number and length
bounds such as the `minimum` schemars gives an unsigned integer, and formats
other than dates, times, emails, hostnames, URIs, IPs and UUIDs) into the
field's description, where the model still reads it. Recursive types aren't
supported.

## Streaming

Stream a plain-text answer token-by-token with `stream(input)` — ideal for
piping to the [event bus](events.md) so the UI paints as tokens arrive. Tools
and structured output are not used in streaming mode.

```rust
use elyra::ai::StreamChunk;

let mut chunks = ai.chat().instructions("Be brief.").stream("Explain lifetimes.");
while let Some(chunk) = chunks.next().await {
    match chunk? {
        StreamChunk::Delta(text) => print!("{text}"),
        StreamChunk::Done(usage) => eprintln!("\n{usage:?}"),
    }
}
// or: let full = ai.chat().stream("…").collect_text().await?;
```

### Streaming to the frontend

Emit each delta on a channel and subscribe in Svelte:

```rust
#[command]
async fn ask_stream(ctx: Ctx, prompt: String) -> Result<(), String> {
    use elyra::ai::StreamChunk;
    let bus = ctx.get::<EventBus>();
    let mut chunks = ctx.get::<Ai>().chat().stream(prompt);
    while let Some(chunk) = chunks.next().await {
        if let StreamChunk::Delta(text) = chunk.map_err(|e| e.to_string())? {
            let _ = bus.emit("elyra:ai", &text);
        }
    }
    Ok(())
}
```

```ts
import { channel, api } from "@elyra/runtime";

let answer = "";
channel<string>("elyra:ai").subscribe((delta) => { if (delta) answer += delta; });
await api.ask_stream("Explain lifetimes.");
```

## Images

```rust
let image = ai.image("A donut on a kitchen counter, warm light")
    .landscape()          // or .portrait() / .square() / .size("1024x1024")
    .quality("high")
    .generate()
    .await?;
image.save("donut.png")?;         // or image.bytes()
```

## Audio (text-to-speech & transcription)

OpenAI text-to-speech and speech-to-text.

```rust
// TTS
let audio = ai.speech("I love building with Elyra.")
    .female()                 // or .male() / .voice("onyx")
    .instructions("Warm and upbeat.")
    .format("mp3")
    .generate()
    .await?;
audio.save("hello.mp3")?;     // or audio.bytes()

// STT (transcription)
let bytes = std::fs::read("hello.mp3").map_err(|e| e.to_string())?;
let text = ai.transcribe(bytes, "hello.mp3")
    .language("en")
    .generate()
    .await?;
```

Defaults: TTS `gpt-4o-mini-tts`, transcription `whisper-1` (override with
`AiBuilder::tts_model` / `transcribe_model`).

## Embeddings

```rust
let vectors = ai.embeddings(["Napa Valley has great wine.", "Elyra is a Rust framework."])
    .dimensions(1536)
    .generate()
    .await?;                       // Vec<Vec<f32>>
```

## Retrieval (RAG)

A portable, in-memory [`VectorStore`] ranks embeddings by cosine similarity in
Rust. Elyra's database layer uses the sqlx `Any` driver, which has no native
vector type (no pgvector), so ranking happens in-process — a good fit for the
small-to-medium corpora typical of desktop apps.

```rust
use elyra::ai::VectorStore;

let mut store = VectorStore::new();
store.add_texts(&ai, vec![
    ("Napa Valley is famous for wine.", 1u32),      // payload = row id
    ("Rust has a strong ownership model.", 2u32),
    ("Elyra is a Rust + Svelte framework.", 3u32),
]).await?;

let hits = store.search_text(&ai, "best wineries", 3).await?;
for hit in &hits {
    println!("{:.3}  id={}", hit.score, hit.payload);
}
```

Feed the top hits into a prompt as context:

```rust
let context = hits.iter().map(|h| h.payload.to_string()).collect::<Vec<_>>().join("\n");
let answer = ai.chat()
    .instructions(format!("Answer using only this context:\n{context}"))
    .prompt("Where should I taste wine?")
    .await?;
```

### Persisting embeddings

Store each embedding as a JSON/text column and rebuild the store per query (or
keep it warm in the container):

```rust
// migration: add `embedding TEXT` to your documents table
let json = serde_json::to_string(&embedding)?;      // Vec<f32> -> text
// on load:
let embedding: Vec<f32> = serde_json::from_str(&row_json)?;
store.add(embedding, row_id);
```

Use [`cosine_similarity`] directly if you rank rows yourself.

## Reliability (retries, failover, caching)

**Retries** are on by default: transient failures (timeouts, connection errors)
and retryable statuses (429, 5xx, 529) are retried with exponential backoff
(capped at 8s). Configure on the client:

```rust
use std::time::Duration;

let ai = Ai::builder()
    .retries(3)                                // default 2; 0 disables
    .retry_backoff(Duration::from_millis(400)) // base; doubles each attempt
    .build();
```

**Failover** tries other providers (in order) if the primary fails after its
retries; each fallback uses its own default model:

```rust
use elyra::ai::Provider;

let reply = ai.chat()
    .provider(Provider::Anthropic)
    .failover([Provider::OpenAI])
    .prompt("Summarize the meeting notes…")
    .await?;
```

**Caching** memoizes plain prompts (no tools / provider tools) in-process,
keyed by provider + model + full conversation:

```rust
let ai = Ai::builder()
    .cache(true)                               // or .cache_ttl(Duration::from_secs(3600))
    .build();
// identical prompts now skip the network; ai.clear_cache() empties it.
```

Cached hits report zero token usage. Requests that use tools or provider tools
are never cached (they may have side effects).

## Testing with a fake

`Ai::fake(AiFake)` answers prompts and streams from a script: no network, no
key. Each step takes the next reply. Text ends the run, and a tool call runs
the agent's real tool and goes on with its result, as a provider would.

```rust
use elyra::ai::{Ai, AiFake};

let fake = AiFake::new()
    .tool_call("weather", json!({ "city": "Oslo" }))
    .text("It's sunny in Oslo.");
let reply = Ai::fake(fake.clone()).chat().tool(Weather).prompt("Oslo?").await?;

assert_eq!(fake.calls()[0].output, "Sunny in Oslo");   // what the tool answered
assert_eq!(fake.requests()[0].tools, ["weather"]);      // what each step was sent
assert!(fake.is_spent());
```

- `.text(..)` (the JSON, for structured output), `.tool_call(name, input)`,
  `.tool_calls([..])` for several in one turn, `.refusal(..)` and
  `.error(status, message)`. `.usage(input, output)` sets what the last reply
  reports, which counts against a `token_budget`.
- A stream gets the text a word at a time, then `Done` with the usage.
- A run that needs a reply the script doesn't have panics, naming the step.
- `Ai::builder().fake(..)` keeps the rest of a configuration (a budget, a
  cache). In an app, swap it in: `App::new().swap(Ai::fake(fake.clone()))`.
  Images, embeddings and audio aren't faked.

## Verification status

The SDK compiles, is clippy-clean, and has offline tests (provider metadata,
builder, message helpers, and what the Anthropic driver sends and reads,
against a local server: `ai/tests/anthropic.rs`; the fake: `ai/tests/fake.rs`). The **live** provider calls are exercised
only when `ELYRA_AI_LIVE=1` and the relevant key are set (`ai/tests/live.rs`) —
they make real, paid API calls and are skipped in CI and by default.

## Related

- [Commands](commands.md) · [Container & providers](container-and-providers.md)
- [Events](events.md) — stream tokens/progress to the frontend.
