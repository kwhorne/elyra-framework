# RFC 0018 — The assistant's answer, as it's written

**Status:** implemented (2026-11-02) · **Target:** 0.31.0 · builds on [RFC 0011](0011-in-app-assistant.md) and [RFC 0013](0013-assistant-sees-the-window.md) · see [Decisions](#decisions)

## Summary

Today the assistant's panel shows its steps as they run ("customers_index ✓")
and then, after a pause, the whole answer at once. A long answer is several
seconds of nothing. This RFC streams the answer into the panel as Claude
writes it, through the tool steps, over the user's own key and through
Laravel's `Route::elyraAssistant()` alike:

```
You:        Which invoices are overdue, and who should I call first?
            ✓ invoices_index
Assistant:  Three invoices are overdue. The largest is Ada Lovelace AS,
            12 400 kr, 31 days late; she's also the one wh▍
```

Nothing changes in an app: the panel and the `assistant` store do it.

## Motivation

- **Waiting on nothing.** A turn with a tool step or two and a paragraph of
  answer takes five to fifteen seconds; most of that is the answer being
  written. Seeing it come is what makes the wait fine.
- **Stopping early.** With the answer coming in, the user sees it's going
  the wrong way and presses Stop. Today Stop only helps before the answer
  arrives, all at once.
- **The pieces are there.** `elyra-ai` streams plain text from Anthropic
  (`Chat::stream`) and the assistant has a Stop that aborts the turn. What's
  missing is streaming *with tools*, which is what the assistant uses, and a
  way through Laravel, which refuses `stream: true` today.

## Current state (0.30)

- **`elyra-ai`:** `Chat::prompt` runs the tool loop: one Messages request
  per step, each answered whole. `Chat::stream` streams over SSE, but
  without tools.
- **The assistant** (`assistant.rs`) runs a turn with `chat.prompt(..)`.
  Each command it runs sends `AssistantEvent::Tool` to the window (started,
  finished, with its link). At the end, it sends `Answer { text }` (the
  last step's text) or `Error`. Stop aborts the task and sends `Stopped`.
  The conversation keeps the user's message and the answer.
- **Laravel** (`AssistantController`) refuses `stream: true` with a `422`
  ("Streaming isn't proxied"), adds the server's key, caps the model and
  `max_tokens`, rate-limits per user, and records `AssistantUsed` with the
  usage from the JSON answer.
- **The runtime** (`assistant` store) holds `items`: user, answer, tool,
  error, stopped. The panel renders each; the log is `aria-live="polite"`.

## Design

### In `elyra-ai`: streaming with tools

`Chat::on_text(|step, delta| ..)` makes `prompt` stream. Each step's
request has `stream: true`; text deltas reach the callback as they come,
with the step's number. Tool use blocks are gathered from their
`input_json_delta`s. The step ends at `message_stop` with the same content,
stop reason and usage the whole answer would have had. The loop is then the
same as now: run the tools, send the results, next step. `prompt` still
returns the final `Response`.

- The callback is a plain `Fn(u32, &str)`: `elyra-ai` doesn't know about
  windows.
- Anthropic only (see open question 5). With OpenAI, `on_text` is called
  once per step with the step's whole text.
- `AiFake` streams its scripted text in small pieces when `on_text` is
  set, so tests see deltas, tools and all.

### In the assistant: `AssistantEvent::Text`

A turn sets `on_text` and sends the deltas to the window as
`AssistantEvent::Text { step, text }`:

- **Coalesced:** the deltas a step writes within 50 ms go as one event, so
  a fast stream is ~20 events a second, not hundreds.
- **Each step's text** is its own: the text before a tool step ("Let me
  look at the invoices.") is one, the answer after it another (open
  question 1).
- **`Answer { text }`** still ends the turn, with the final step's text, so
  a window that missed a delta (it was reloading) still gets the whole
  answer, and an older runtime sees what it sees today.
- **Stop mid-answer:** the text so far stays in the panel, marked
  stopped, and goes into the conversation (open question 3).

### Through Laravel

`AssistantController` proxies a stream when the desktop asks for one, with
the same checks as now:

- **The response:** `text/event-stream`, the upstream's SSE passed through
  as it comes (`Http::withOptions(['stream' => true])`, read and flushed in
  chunks), with `X-Accel-Buffering: no` so nginx doesn't hold it.
- **Usage:** read off the stream as it passes (`message_start` and
  `message_delta`), so `AssistantUsed` is recorded as now.
- **Config:** `elyra.assistant.stream` (default `true`); `false` keeps
  refusing it.
- **An older package, or `stream` off:** the desktop sees the `422` and
  asks again without streaming, and doesn't ask that server to stream again
  in this run. The answer arrives whole, as today.

### In the runtime

- **`items`:** an answer item while it's written is `{ kind: "answer",
  text, step, streaming: true }`. Each `Text` event appends to its step's
  item, making it if it's the first. A tool step after it ends it
  (`streaming: false`).
- **`Answer`** replaces the last answer item's text with the final one and
  ends it. If none was streamed (an older shell, OpenAI), it adds one, as
  now.
- **The panel** grows the bubble as text comes, with a caret while it's
  written. The bubble is `aria-busy` until it ends, so a screen reader reads
  it once, whole, rather than in pieces.
- **Stop** keeps the bubble, marked "stopped".

### What it doesn't change

- **Confirmation before a command** (RFC 0011): a step that wants to run a
  confirmed command still asks first. Its preamble text has streamed by then.
- **The window note** (RFC 0013), the tools offered, rate limits, the
  conversation's token limit.
- **The MCP server:** an agent's tools aren't the assistant's.

### Testing

- `elyra-ai`: the SSE parser on recorded streams (text, a tool use with its
  input in pieces, two tools, a refusal, `pause_turn`, an error mid-stream);
  `on_text` with the fake.
- The assistant: deltas reach the window coalesced, step by step, and
  `Answer` ends it; Stop mid-stream keeps the partial text in the
  conversation; a turn that falls back to no streaming.
- The runtime: items built from `Text` and `Answer` events, a missed delta
  healed by `Answer`, Stop; the panel's bubble and `aria-busy` in the smoke
  harness.
- The Laravel CI job: a streamed turn with a tool step through
  `Route::elyraAssistant()` against the stand-in for Anthropic, usage
  recorded; `stream` off, and the desktop's fallback.

## Security

- **Nothing new is reached:** the same requests, streamed. The server's
  checks (model, `max_tokens`, server tools, rate limit) run before the
  stream starts, on the same body.
- **The key stays on the server:** the proxy streams the upstream's body,
  not its headers.
- **A stream that never ends:** the Laravel side keeps its 120 s timeout,
  and the desktop's Stop aborts the request.

## As built

- **`elyra-ai`:** `Chat::on_text(|step, piece| ..)`. With Anthropic each
  step is sent with `stream: true`, and `read_stream` reads its SSE back
  into the message a whole answer would have been: text, tool uses (their
  input from its `input_json_delta` pieces), thinking, citations,
  `stop_reason`, `stop_details`, usage. The tool loop after it is
  unchanged. An `error` event is `Error::Api` (529 when overloaded); a
  stream that ends before `message_stop` is `Error::Decode`. A gateway
  that answers JSON to a stream request is told each step's text at once.
  OpenAI is told the answer once. `AiFake` tells its script a word at a
  time, with `pace(..)` and `tool_call_saying(..)`.
- **The assistant:** a `Stream` per turn sits between the turn and its
  window. It gathers the pieces for 50 ms and sends any other event only
  after the text waiting, so a tool step never shows before what was said
  before it. Stop flushes it, and keeps the latest step's text in the
  conversation, ending "(stopped by the user)". A `422` whose message
  says "Streaming" makes the turn ask again whole, and `whole` is set for
  the run.
- **The runtime** drops a piece that comes when no turn is running (one
  in flight when Stop was pressed).
- **Laravel:** the proxy streams only when the upstream's answer is
  `text/event-stream`. An error or a JSON answer is passed on whole, read
  once, since a streamed body reads once. The CI case checks that the
  first words reach the desktop before the rest is written; a variant of
  the controller that read the whole stream first failed it.
- **Askr** (1.9.2), serving the Laravel app, holds a streamed response
  until it's done: the answer arrives right, tool steps and all, but at
  once. The backend guide says to set `ELYRA_ASSISTANT_STREAM=false` there
  until it passes streams on. The CI job serves Laravel with `php artisan
  serve`, as before.

## Alternatives considered

- **Stream only the last step,** and show tool steps as now: the same
  first-token wait for a turn with tools, which most are.
- **Stream through the event bus token by token:** hundreds of events a
  second, each a message to the page; coalescing costs ~50 ms of latency
  and saves almost all of it.
- **WebSockets from Laravel** (Reverb) instead of SSE through the proxy:
  another moving part; SSE is what the Messages API speaks already.

## Implementation plan

1. `elyra-ai`: `on_text`, the streaming tool loop for Anthropic, the
   fake's pieces; tests on recorded streams.
2. The assistant: `AssistantEvent::Text`, coalescing, Stop keeping the
   partial text, the fallback when a server refuses to stream.
3. The runtime's items and panel; the smoke harness.
4. Laravel: the streaming proxy and its config; the CI case.
5. Docs: assistant, backend, the Laravel package's README and skill.

## Open questions

1. **Text before a tool step:** show it as its own bubble ("Let me look at
   the invoices."), or hide it and stream only the answer? Recommended:
   **show it.** It's what the model said, it fills the wait before a tool
   step, and hiding it means buffering each step until it's known whether
   a tool follows.
2. **How often deltas go to the window:** every 50 ms (recommended), or
   every delta?
3. **Stopped mid-answer:** keep the partial text in the conversation (so
   "go on" works) or only in the panel? Recommended: **in the
   conversation**, ending "(stopped by the user)", so the model knows what
   it said and that it was cut off.
4. **Laravel streams by default** (`elyra.assistant.stream` `true`), or
   only when turned on? Recommended: **by default.** The checks are the
   same; a server whose proxy buffers (an nginx without
   `X-Accel-Buffering`) still works, just not as smoothly.
5. **OpenAI:** stream with tools there too, or only Anthropic for now?
   Recommended: **Anthropic only.** The assistant reaches Claude through
   Laravel or the user's key; OpenAI only through an app's own `Ai`, and
   it keeps working, whole.
6. **Askr:** a Laravel app served by Askr must pass a streamed response
   through as it's written. Recommended: **check it in the CI job against
   Askr too**; if it buffers, document `stream: false` for it until it
   doesn't.

## Decisions

Settled on 2026-11-02, all as recommended:

1. **The text before a tool step shows,** as its own bubble.
2. **Deltas go to the window every 50 ms,** coalesced.
3. **A stopped answer stays in the conversation,** ending "(stopped by
   the user)".
4. **Laravel streams by default:** `elyra.assistant.stream` is `true`.
5. **Anthropic only:** OpenAI answers whole, as now.
6. **Askr is checked in the CI job;** if it buffers, its docs say
   `stream: false` until it doesn't.
