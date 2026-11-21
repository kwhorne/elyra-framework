# RFC 0049 — Web search on OpenAI: the Responses API for provider tools

**Status:** accepted (2026-11-21) · **Target:** 0.52.0 · builds on [AI SDK](../ai.md), [RFC 0048](0048-more-providers.md) · see [Decisions](#decisions)

## Summary

`.web_search(…)` runs on Anthropic only. With `Provider::OpenAI` the chat
fails with `Error::Unsupported`, because OpenAI offers web search only
through its Responses API (`POST /v1/responses`), and the SDK speaks Chat
Completions. A failover from Anthropic to OpenAI therefore can't keep a
search, and an app with only an OpenAI key can't search at all.

This RFC adds a second OpenAI driver over the Responses API, used when an
OpenAI chat asks for web search:

```rust
let answer = ai.chat()
    .provider(Provider::OpenAI)
    .instructions("Answer with up-to-date facts and cite sources.")
    .web_search(
        WebSearch::new()
            .allow(["rust-lang.org", "docs.rs"])
            .location(UserLocation { country: Some("NO".into()), ..Default::default() }),
    )
    .tool(lookup_customer)              // the app's own tools still run
    .prompt("What changed in the latest Rust release?")
    .await?;

// And the same chat can fail over between the two labs.
ai.chat()
    .provider(Provider::Anthropic)
    .failover([Provider::OpenAI])
    .web_search(WebSearch::new())
    .prompt("…")
    .await?;
```

- **The same `WebSearch`** for both labs: `allow` and `location` map onto
  OpenAI's tool; the options OpenAI has no equivalent for are handled per
  open question 2.
- **The app's tools, structured output and streaming** work in a web-search
  chat on OpenAI as they do on Chat Completions.
- **Web fetch and code execution stay Anthropic's**: OpenAI has no tool
  that fetches a given URL, and its code interpreter is another tool than
  Anthropic's programmatic tool calling. Asking OpenAI for them stays
  `Error::Unsupported`, now saying which tool.

## Motivation

- **The roadmap's AI SDK line** has "OpenAI provider tools via the
  Responses API" next, after RFC 0048.
- **Failover is half there**: a chat with web search fails over from
  Anthropic only to providers that can't search, so the fallback always
  fails with `Unsupported`.
- **An app with one lab's key** chooses the lab by what it has, not by
  which features the SDK implements for it.

## Current state (0.52, after RFC 0048)

- `ProviderTool { WebSearch, WebFetch, CodeExecution }`, with
  `anthropic_spec` (`ai/src/provider_tool.rs`).
- `openai::run` and `openai::stream` (Chat Completions) return
  `Error::Unsupported("provider tools … require the Anthropic provider")`
  when any provider tool is set. `openai::stream` takes no tools at all.
- Chat Completions on every OpenAI-shaped provider since RFC 0048.

## Design

### When the Responses API is used

An OpenAI chat with `.web_search(…)` goes to `openai_responses::run` /
`stream`. Every other OpenAI chat stays on Chat Completions (open
question 1). The other OpenAI-shaped providers (Gemini, Groq, …) keep
`Unsupported` for provider tools: none of them is documented to take the
Responses API's tools.

### The request

`POST {base}/v1/responses`:

- `instructions` from the system prompt; `input` the conversation as
  `{role, content}` items.
- `tools`: the app's tools as `{type: "function", name, description,
  parameters}` (flat, not nested under `function` as in Chat Completions),
  and `{type: "web_search"}`, with `filters.allowed_domains` from `allow`
  and `user_location` (`{type: "approximate", country, city, region,
  timezone}`) from `location`.
- Structured output: `text.format = {type: "json_schema", name, schema,
  strict: false}`. On Chat Completions the SDK forces a tool call; the
  Responses API has the format itself.
- `max_output_tokens`, `temperature`, `stream`.

### The tool loop

The answer's `output` is a list of items. `message` items carry the text
(`content[].type == "output_text"`); `function_call` items (`call_id`,
`name`, `arguments`) are the app's tools to run; `web_search_call` items
are the search, already done by OpenAI. While there are `function_call`s,
the SDK runs them and sends the next request with the previous output
items and a `{type: "function_call_output", call_id, output}` per call
appended to `input`, up to `max_steps`. It doesn't use
`previous_response_id`, so nothing is stored server-side across steps
(`store: false`).

### Streaming

SSE events: `response.output_text.delta` (`delta`) becomes
`StreamChunk::Delta`; `response.completed` carries `response.usage`
(`input_tokens`, `output_tokens`) for `StreamChunk::Done`;
`response.failed` and `error` events become an `Error::Api`. A streamed
chat with web search and without the app's tools, as streaming is today.

### Citations

The answer's `url_citation` annotations (`url`, `title`) aren't surfaced
today on either lab: `Response` holds the text. Open question 3.

### Testing

Offline against a local server (`ai/tests/openai_responses.rs`): the
request (URL, tool specs, filters, location, `store: false`, structured
format), a function-call round trip with a web search in the same answer,
a stream, an error event, failover from Anthropic to OpenAI with web
search, and `web_fetch` on OpenAI still `Unsupported`. One opt-in live
test (`ELYRA_AI_LIVE=1`, `OPENAI_API_KEY`).

## Implementation plan

1. `ai/src/openai_responses.rs`: request, tool loop, stream, reading
   `output`.
2. `ProviderTool::openai_spec`, the checks for options and tools OpenAI
   lacks; `chat.rs` dispatches an OpenAI chat with web search to the new
   driver.
3. Tests, docs (`ai.md`: provider tools for both labs), roadmap,
   CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **The Responses API for every OpenAI chat, or only with web search?**
   It's OpenAI's newer API, and moving everything over leaves one OpenAI
   driver to keep. It's also a change to every OpenAI chat that works
   today, for no change in what they can do.
   *Recommendation:* only with web search, for now. Chat Completions stays
   the path for OpenAI and for every OpenAI-shaped provider; a move can
   come with a reason (a feature only the Responses API has).
2. **`WebSearch` options OpenAI has no equivalent for** (`max`, the number
   of searches; `block`, excluded domains): an error, or ignored?
   *Recommendation:* `block` is an `Error::Unsupported` that names it (a
   blocklist silently dropped shows the results it was meant to keep
   out). `max` is ignored with a `tracing` warning once per client: it
   bounds cost, not what's shown, and failover from Anthropic should still
   work with it set.
3. **Citations**: add `Response::citations() -> &[Citation]` (`url`,
   `title`) for both labs now?
   *Recommendation:* not in this RFC. It's new API for both drivers and
   the stream, and wants its own design (where a citation points in the
   text). The text still says what it cites when the instructions ask for
   sources.
4. **Streaming with the app's tools** on the Responses API: possible
   there, and not possible on Chat Completions today.
   *Recommendation:* no; streaming keeps one rule on both labs (no app
   tools), and changing that is its own RFC.

## Not in this RFC

- **OpenAI's other built-in tools**: file search, code interpreter, image
  generation, computer use, remote MCP.
- **`previous_response_id`** and server-stored conversations.
- **Web search on Gemini, Groq and the others**, each with its own.
- **Citations** (open question 3).

## Alternatives considered

- **Web search on Chat Completions** through OpenAI's search models
  (`gpt-4o-search-preview` and its successors): no new driver, but the
  app has to pick a special model, it can't be combined with the app's
  tools, and it has no domain filter.
- **Keep `Unsupported`** and document Anthropic as the lab for search:
  what the SDK says today, and the failover gap stays.

## Decisions

1. **The Responses API only for an OpenAI chat with web search**; Chat
   Completions stays the path for every other OpenAI chat and every
   OpenAI-shaped provider.
2. **`block` on OpenAI is `Error::Unsupported`**, naming it; **`max` is
   ignored there**, with a `tracing` warning once per client.
3. **No citations in this RFC.**
4. **Streaming keeps one rule on both labs**: no app tools.
