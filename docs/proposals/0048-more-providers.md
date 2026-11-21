# RFC 0048 — More providers: Gemini, Groq, Mistral and the rest that speak OpenAI

**Status:** accepted (2026-11-21) · **Target:** 0.52.0 · builds on [AI SDK](../ai.md) · see [Decisions](#decisions)

## Summary

The AI SDK has two providers, Anthropic and OpenAI. Most other labs and
hosts offer an endpoint shaped like OpenAI's Chat Completions: Google's
Gemini (its OpenAI-compatible endpoint), Groq, Mistral, DeepSeek, xAI,
OpenRouter, and a local Ollama. The OpenAI driver already speaks that
shape. What's missing is the provider itself: its base URL, where the
chat path sits under it, its key's environment variable, and whether it
needs a key at all.

This RFC adds them as `Provider` variants that run on the OpenAI driver,
plus `Provider::Compatible` for any other endpoint of that shape:

```rust
use elyra::ai::{Ai, Provider};

// GEMINI_API_KEY, GROQ_API_KEY, MISTRAL_API_KEY … from the environment.
let ai = Ai::from_env();

let reply = ai.chat()
    .provider(Provider::Gemini)
    .model("gemini-2.5-flash")
    .prompt("Summarize the meeting notes…")
    .await?;

// Tools, structured output, streaming and failover, as with OpenAI.
let reply = ai.chat()
    .provider(Provider::Groq)
    .model("llama-3.3-70b-versatile")
    .failover([Provider::OpenAI])
    .tool(lookup_customer)
    .prompt("Who is customer 42?")
    .await?;

// A local model, no key.
let local = ai.chat().provider(Provider::Ollama).model("llama3.2").prompt("Hi").await?;

// Any other endpoint of the same shape.
let ai = Ai::builder()
    .base_url(Provider::Compatible, "https://llm.internal.example.com/v1")
    .provider_key(Provider::Compatible, key)
    .build();
```

- **Chat only**: text, tools, structured output, streaming. Images,
  speech, transcription and embeddings stay on OpenAI.
- **Provider tools** (web search, web fetch, code execution) stay
  Anthropic's; asking another provider for them is the same
  `Error::Unsupported` OpenAI gives today.
- **Each provider has its own model**: `Ai::builder().text_model_for(Provider::Groq, "…")`,
  and failover uses each provider's own.

## Motivation

- **Choice of lab is the app's**: an organisation with a Google Cloud
  agreement wants Gemini, a privacy-minded one a local model, a
  cost-minded one Groq or DeepSeek. The roadmap's AI SDK line lists
  "additional providers (Gemini, Groq, …)".
- **The driver exists**: every one of these accepts the request the
  OpenAI driver sends and answers in the shape it reads. What differs is
  configuration.
- **Failover across labs is a reason to have two**, and only works with
  more than one key the app can get.

## Current state (0.51)

- `Provider { Anthropic, OpenAI }`, with `as_str`, `env_key`,
  `env_base_url`, `default_base_url` and `default_text_model`
  (`ai/src/provider.rs`).
- `openai::run` and `openai::stream` post to
  `{base_url}/v1/chat/completions` with a bearer key.
- `Ai::from_env` reads the keys and base URLs of both providers.
- `chat.rs` dispatches by provider, with a `match` in `execute` and in
  `stream`.
- **One text model for every provider**: `Ai::builder().text_model(m)`
  sets it, and `model_for(provider)` returns it whatever the provider. The
  docs say "each fallback uses its own default model", and that's only
  true while `text_model` is unset. With it set, the fallback provider is
  sent the primary's model name.

## Design

### The providers

| Variant | Key env | Base URL | Chat path |
|---|---|---|---|
| `Gemini` | `GEMINI_API_KEY` | `https://generativelanguage.googleapis.com/v1beta/openai` | `/chat/completions` |
| `Groq` | `GROQ_API_KEY` | `https://api.groq.com/openai/v1` | `/chat/completions` |
| `Mistral` | `MISTRAL_API_KEY` | `https://api.mistral.ai/v1` | `/chat/completions` |
| `DeepSeek` | `DEEPSEEK_API_KEY` | `https://api.deepseek.com/v1` | `/chat/completions` |
| `XAi` | `XAI_API_KEY` | `https://api.x.ai/v1` | `/chat/completions` |
| `OpenRouter` | `OPENROUTER_API_KEY` | `https://openrouter.ai/api/v1` | `/chat/completions` |
| `Ollama` | none | `http://localhost:11434/v1` | `/chat/completions` |
| `Compatible` | `ELYRA_AI_COMPATIBLE_KEY` (optional) | none: set it | `/chat/completions` |

Each has `{NAME}_BASE_URL` beside its key, as the two today do. A new
provider's base URL includes its version segment (Gemini's is `v1beta`,
Groq's sits under `/openai/v1`), so the chat path after it is the same for
all of them. OpenAI's own base URL is without `/v1` today, and the driver
appends it; whether that changes is open question 2.

- **A provider without a key** (`Ollama`; `Compatible` without one) sends
  no `Authorization` header. Others fail with `Error::MissingKey` before
  sending, as today.
- **`Compatible` without a base URL** fails with a new
  `Error::NotConfigured("compatible")` before sending.

### Models

- `Ai::builder().text_model_for(provider, model)` sets one provider's
  model. `text_model(model)` keeps its meaning (every provider), and the
  per-provider one wins.
- `model_for(provider)`: the chat's `.model(…)` for the primary; then the
  provider's own; then `text_model`; then the provider's default.
- Failover asks `model_for` for each fallback, so a fallback with its own
  model gets it. That fixes the case above.

### Reading the answer

The OpenAI driver reads `choices[0].message` and `usage.prompt_tokens`. The
providers agree on these; where one leaves `usage` out (some Ollama
versions do), tokens are counted as 0, as a missing field is today. An
error body that isn't OpenAI's `{"error": {"message"}}` (a list, or plain
text from a proxy) is reported with its raw text instead of today's
"unknown error", so a status is never without a message.

### What doesn't change

`Provider::Anthropic` and `Provider::OpenAI` behave as today. Apps that set
`OPENAI_BASE_URL` to a gateway keep working: see open question 2. The
Anthropic driver, provider tools and the assistant are untouched.

### Testing

Offline, against a local server (as `ai/tests/anthropic.rs` does for
Anthropic): for each provider, the URL and auth header it sends; a tool
round trip and a streamed answer through `Groq` (the driver is shared, so
one provider covers it); `Ollama` sending no auth; `Compatible` without a
base URL; failover from one provider to another with each one's model; an
error body that isn't OpenAI's shape. Live calls stay opt-in (`ELYRA_AI_LIVE=1`),
one per provider with a key set.

## Implementation plan

1. `ai/src/provider.rs`: the variants, their env names, base URLs, and
   `needs_key`; `#[non_exhaustive]` on `Provider`.
2. `ai/src/client.rs`: `from_env` over every provider, `text_model_for`,
   `model_for`, auth without a key, `Error::NotConfigured`.
3. `ai/src/openai.rs`: the URL from the base URL, error bodies of other
   shapes; `chat.rs` dispatches every OpenAI-shaped provider to it.
4. Tests and docs (`ai.md`: configuration, a providers table, failover),
   roadmap, CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **Default models for the new providers?** A built-in default goes
   stale as labs rename models, and a wrong one fails only at the call.
   *Recommendation:* none for the new providers. A chat to one of them
   without a model (from `.model`, `text_model_for` or `text_model`) fails
   with `Error::NotConfigured("model for groq")` before sending.
   `default_text_model` returns `Option<&'static str>`: `Some` for
   Anthropic and OpenAI, `None` for the rest. That's said under Upgrading.
2. **The version segment in base URLs**: today `OPENAI_BASE_URL` is
   without `/v1` (the driver appends it). Moving `/v1` into the base
   breaks a gateway URL set without it.
   *Recommendation:* keep OpenAI's and Anthropic's base URLs as they are
   (version appended), and give the new providers a base URL that includes
   their version, with the path after it fixed. Two rules, but no app's
   gateway breaks.
3. **`#[non_exhaustive]` on `Provider`**: an app matching on it today must
   then add a `_` arm.
   *Recommendation:* yes, now, while adding eight variants is a breaking
   change anyway; later providers then aren't one.
4. **The native Gemini API** (`generateContent`) instead of its
   OpenAI-compatible endpoint: more features (its own grounding and
   caching), at the cost of a third driver.
   *Recommendation:* the compatible endpoint now; a native driver if apps
   ask for what only it has.

## Not in this RFC

- **OpenAI's Responses API** and its provider tools (the other half of the
  roadmap's AI SDK line).
- **Images, speech, transcription and embeddings** on other providers.
- **Live provider tests in CI** (paid calls; the roadmap's third item).
- **The assistant** (`App::assistant`) on another provider: it's built on
  Anthropic's tool shapes and code execution.

## Alternatives considered

- **Only `Compatible`**, with the app setting base URL and key: no list to
  keep, but every app looks up the same URLs, and `from_env` can't find
  keys by a known name.
- **A driver per provider**: the same request and response eight times.
  The OpenAI shape is the common one; a provider gets its own driver when
  it needs one (as Anthropic has).

## Decisions

1. **No default model for the new providers.** A chat to one of them
   without a model fails with `Error::NotConfigured` before sending;
   `default_text_model` returns `Option<&'static str>` (`Some` for
   Anthropic and OpenAI), said under Upgrading.
2. **OpenAI's and Anthropic's base URLs stay as they are**, version
   appended by the driver; a new provider's base URL includes its version,
   with the chat path after it fixed.
3. **`Provider` is `#[non_exhaustive]`** from 0.52, said under Upgrading.
4. **Gemini through its OpenAI-compatible endpoint**; a native driver if
   apps ask for what only it has.
