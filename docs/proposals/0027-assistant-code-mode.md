# RFC 0027 — The assistant's code mode

**Status:** implemented (2026-11-06) · **Target:** 0.38.0 · builds on [RFC 0011](0011-in-app-assistant.md), [RFC 0013](0013-assistant-sees-the-window.md) and [RFC 0018](0018-assistant-streaming.md) · see [Decisions](#decisions)

## Summary

"Find last year's invoices without a project code, put each on its
customer's default project, and tell me the total." Today the in-app
assistant does that one tool call at a time: each call a round trip
through the model, each result in its context, 10 to 30 turns for a task
a short loop would do. With code mode, Claude writes that loop, it runs in
Anthropic's code execution sandbox, and each of the app's commands it
calls comes back to the app as a tool call, through the same checks as
today. Only what the code prints reaches the model:

```rust
App::new().assistant(
    Assistant::new()
        .allow_ability("invoices.*")
        .confirm("invoices.delete")
        .code_mode(),            // Claude may call the app's tools from code
);
```

```text
user  → Put last year's invoices without a project on their customer's default
        project, and give me the total.
panel ← Ran a script: 214 invoices read, 37 updated (invoices_update ×37).
        Total of the 37: 41 280 €.
```

It's [programmatic tool calling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling),
which the Claude API has: no interpreter in the app.

## Motivation

- **A task over many records is many turns.** Each tool result goes into
  the model's context, and the model reads it to decide the next call:
  slow, and the context fills with rows nobody needs again.
- **The model is better at writing the loop than at being it.** Filtering
  214 invoices down to the 37 to change is a list comprehension, not 214
  reads.
- **The app already has what code mode needs.** Every command is a tool
  with an ability, a window scope (RFC 0016) and, where it can't be undone,
  a confirmation (RFC 0011). Calls from code go through all of it.
- **The first review asked for it** with Rune as the sandbox. The API has
  one already; this RFC weighs the two.

## Current state (0.37)

- **The assistant** (`framework/src/assistant.rs`): a tool loop in the app.
  The tools are the commands the grant and the user's abilities offer
  (`offered`), a call that needs confirmation asks in the panel first, and
  each step is shown with its link (RFC 0013). Answers stream (RFC 0018).
- **Where the model comes from:** the backend (`Route::elyraAssistant()`,
  which adds the server's key, caps the model, `max_tokens` and requests
  per user, and **refuses Claude's own server tools**), the user's key in
  the keychain, or `ANTHROPIC_API_KEY` in development.
- **The AI SDK** (`ai/src`): provider tools are `WebSearch` and `WebFetch`
  (`ProviderTool`); there's no code execution tool, no `allowed_callers` on
  a tool, no `caller` on a tool call, and no `container` kept between
  requests. Those come with this RFC.
- **Programmatic tool calling** in the API: the `code_execution_20260120`
  (or later) tool, and `allowed_callers: ["code_execution_20260120"]` on a
  client tool. Claude's code calls the tool as an async Python function; the
  response pauses with `stop_reason: "tool_use"`, a `container`, and a
  `tool_use` block whose `caller` names the code execution run. The app
  answers with `tool_result` blocks only, and the code goes on. A call left
  unanswered for about 4 minutes raises a `TimeoutError` in the code. Not
  eligible for zero data retention: code execution containers keep data up
  to 30 days. `allowed_callers` "is not a hard API-level block on direct
  calls" and isn't a security boundary.

## Design

### Turning it on

`Assistant::code_mode()` adds the code execution tool to each request and
`allowed_callers` to the tools that may be called from code. Off by
default: an app chooses it, knowing where the data goes (below).

### Which tools code may call

A tool is callable from code when the user may call it now (the same
`offered` filter) **and it needs no confirmation.** A tool behind
`.confirm(..)` stays direct-only: Claude calls it as today, one call at a
time, and the user confirms it in the panel. So a script can read 214
invoices and change 37 of them, but deleting goes through the user, one
by one, as now.

`allowed_callers` is guidance to the model, not a guard, so the app doesn't
trust it: every call, from code or direct, goes through the dispatcher as
today: the grant, the user's abilities, the window's scope, the rate
limit, and the confirmation of a tool that needs one. A call from code to a
tool that needs confirmation is refused with an error the code sees
("ask the user: call it directly"), never run unconfirmed.

### The loop

- A response that pauses for the app (`caller` of type `code_execution_…`)
  is answered with one user message of `tool_result` blocks only, one per
  pending call, run concurrently as today's parallel calls are.
- The `container` id is kept for the conversation and sent back with the
  next request, so the code's state survives the pause.
- A call that takes long (a slow backend) answers with an error before the
  4-minute limit rather than leaving the code to time out.
- With streaming (RFC 0018), the panel shows the code as Claude writes it,
  then each call as a step.

### In the panel

A script is one step that opens: the code, then each call it made, each a
step with its link (RFC 0013), and what it printed. The answer follows as
today. A failed call is shown where it happened.

### The Laravel side

`Route::elyraAssistant()` keeps refusing server tools, except code
execution when `ELYRA_ASSISTANT_CODE_EXECUTION=true`: the server's owner
decides that the company's data may go through Anthropic's containers. The
desktop learns it from the backend's answer (a `400` naming the tool) and
says so in the panel rather than failing silently.

### Security and data

- **Where the data goes:** the results of the calls the code makes go to
  Anthropic's code execution container, not only to the model. They're
  kept up to 30 days and the feature isn't eligible for zero data
  retention. That's the reason it's opt-in, per app and (through Laravel)
  per server, and the docs say it plainly.
- **Nothing new runs in the app:** no interpreter, no script from the
  network. The code runs in Anthropic's sandbox and reaches the app only
  as tool calls, each checked as any other.
- **Confirmation stays the user's:** nothing behind `.confirm(..)` runs
  from code.

### Testing

- The fake model the assistant's tests use answers with paused code
  execution responses: tool calls with a `caller`, then the code's output.
- Each call from code goes through the grant, the abilities and the scope;
  one to a confirmed tool is refused to the code; the `container` is sent
  back; the answer holds only `tool_result` blocks.
- Laravel: code execution refused by default, passed with the setting.
- The panel: a script step with its calls and links.

## Implementation plan

1. The AI SDK: a `CodeExecution` provider tool, `allowed_callers` on a
   tool, a tool call's `caller`, and the `container` carried between
   requests (Anthropic only; the OpenAI provider refuses it).
2. `Assistant::code_mode()`, the tool list with `allowed_callers`, the
   paused loop and the `container`, with the fake model.
3. The panel's script step.
4. `elyracode/elyra-backend`: the setting, and its test in the Laravel CI
   job against the stand-in for Anthropic.
5. Docs: `assistant.md` (code mode, where the data goes), `backend.md`;
   CHANGELOG.

## As built

- **Corrections to the design's current state:** the AI SDK ran a turn's
  tool calls one after another, not in parallel ("as today's parallel
  calls are" was wrong), and so does a code run's; Laravel refuses a
  server tool with a `422`, not a `400`; and `elyracode/elyra-backend` has
  no PHP tests: it's tested from Rust against a real Laravel app in CI
  (`tests/laravel_assistant.rs`), where the code-execution case is.
- **Where it lives:** the tool loop is the AI SDK's, so most of it is
  there: `Chat::code_execution()`, `Tool::callable_from_code()` (sent as
  `allowed_callers: ["direct", "code_execution_20260120"]`), the
  `container` read from each response (and from a stream's `message_start`
  and `message_delta`) and sent back, and `Chat::on_code` with
  `CodeEvent::Started`/`Finished`. The assistant turns it on and maps the
  events to `AssistantEvent::Script`.
- **The refusal is in the SDK:** a `tool_use` whose `caller` is code, for
  a tool that isn't `callable_from_code`, gets an error result ("call it
  directly, so the user can confirm it") and never runs, whatever the
  model sent; the assistant's tools behind `confirm` aren't.
- **Pauses aren't steps:** a code run that awaits 37 calls one by one
  pauses 37 times. Those continuations don't run the model, so they don't
  count against `max_steps` (8 by default, which would stop it); a prompt
  makes at most 500 calls from code, then fails as unsupported.
- **The container is kept for the turn,** not across turns: the
  conversation keeps text between turns, so each turn's code starts in a
  fresh container.
- **Both callers on a tool:** the API suggests one of `direct` or code per
  tool; a tool the app offers stays callable directly too, so a single
  lookup isn't forced through a script.
- **`AssistantEvent` became `#[non_exhaustive]`** with its new `Script`
  variant: the variant broke an exhaustive `match` anyway, and later ones
  won't.
- **The OpenAI provider refuses code execution** as it refuses the other
  provider tools.
- **Checked:** the SDK against a stand-in Messages API (calls from code,
  the container, `allowed_callers`, the refusal, the cap of 500, a
  streamed pause, OpenAI's refusal); the assistant end to end against one
  (the run and its calls between its start and end, the commands run
  through the pipeline, a confirmed tool refused to code and the user not
  asked, no code execution without `code_mode`); the panel's state; the
  Laravel setting in CI. **Not run:** against the real API, which costs
  money and wasn't asked for.

## Alternatives considered

- **Rune in the app**, as the first review proposed: Claude writes Rune,
  the app runs it in an embedded VM with instruction and memory budgets,
  the commands bound as functions.
  - For: the data stays between the app and the model; no container
    retention; it would work with any model provider.
  - Against: a new dependency and a VM in every app, whose budgets and
    bindings we'd have to trust and keep up; the model writes Python far
    better than Rune; and we'd rebuild what the API already does, pauses,
    concurrency and timeouts included. If an app needs it for data reasons,
    it can come later behind the same `code_mode` surface.
- **Customer rules and macros as Rune scripts from the server** (the first
  review's second idea): code the app downloads and runs. That needs the
  trust anchor RFC 0024 has for updates (signed by a known key) before it's
  safe, and is a separate RFC if an app asks for it.
- **Everything callable from code, confirmations included:** the code would
  pause on a confirmation while the user decides, against a 4-minute limit,
  and a loop could ask 37 times. Direct calls make the user's choice one at
  a time, as now.

## Open questions

1. **Programmatic tool calling, or Rune in the app?** (recommended: the
   API's; Rune only if an app needs the data kept out of containers)
2. **Opt-in per app with `code_mode()`** (recommended), or on by default?
3. **Tools behind `.confirm(..)` direct-only** (recommended), or callable
   from code with the confirmation in the panel?
4. **Laravel: code execution off unless `ELYRA_ASSISTANT_CODE_EXECUTION=true`**
   (recommended), or on whenever the desktop asks?
5. **The panel shows the code and each call** (recommended), or only the
   answer and a count of calls?

## Decisions

Settled on 2026-11-06, all as recommended:

1. **Programmatic tool calling**, the API's; Rune only if an app needs the
   data kept out of containers.
2. **Opt-in per app with `code_mode()`.**
3. **Tools behind `.confirm(..)` are direct-only.**
4. **Laravel: code execution off unless `ELYRA_ASSISTANT_CODE_EXECUTION=true`.**
5. **The panel shows the code and each call.**
