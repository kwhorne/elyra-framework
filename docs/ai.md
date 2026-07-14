# AI SDK

An ergonomic, Laravel-inspired AI SDK for Elyra apps — **agents**, **tools**,
**structured output**, **images**, and **embeddings** over Anthropic and OpenAI.
It lives in the [`elyra-ai`](../ai) crate and is re-exported as `elyra::ai`
behind the `ai` feature. Everything runs in the Rust backend, so API keys never
reach the frontend.

```toml
elyra = { version = "0.3", features = ["ai"] }
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

The default provider is **Anthropic** with the `claude-sonnet-5` text model;
images default to OpenAI `gpt-image-1`, embeddings to `text-embedding-3-small`.
Build a client with `Ai::from_env()` or `Ai::builder()`.

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
    .temperature(0.3)
    .prompt("What is ownership?")
    .await?;
println!("{reply}");                 // Display = the text
println!("{:?}", reply.usage());     // token usage
```

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

## Structured output

Return typed JSON with `prompt_as::<T>()`, where `T` derives `serde::Deserialize`
and `schemars::JsonSchema`. The SDK forces the model to emit matching JSON (via a
synthetic tool) and deserializes it — works across both providers.

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

Flat structs work best; deeply nested schemas depend on provider strictness.

## Images

```rust
let image = ai.image("A donut on a kitchen counter, warm light")
    .landscape()          // or .portrait() / .square() / .size("1024x1024")
    .quality("high")
    .generate()
    .await?;
image.save("donut.png")?;         // or image.bytes()
```

## Embeddings

```rust
let vectors = ai.embeddings(["Napa Valley has great wine.", "Elyra is a Rust framework."])
    .dimensions(1536)
    .generate()
    .await?;                       // Vec<Vec<f32>>
```

## Verification status

The SDK compiles, is clippy-clean, and has offline unit tests (provider
metadata, builder, message helpers). The **live** provider calls are exercised
only when `ELYRA_AI_LIVE=1` and the relevant key are set (`ai/tests/live.rs`) —
they make real, paid API calls and are skipped in CI and by default.

## Related

- [Commands](commands.md) · [Container & providers](container-and-providers.md)
- [Events](events.md) — stream tokens/progress to the frontend.
