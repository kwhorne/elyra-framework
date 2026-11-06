# The app's assistant

An assistant inside the app, for the people using it
([RFC 0011](proposals/0011-in-app-assistant.md)). Its tools are the app's own
commands, it can do no more than the signed-in user, and it asks in the app
before anything that can't be undone.

```rust
App::new()
    .commands(resources::commands())
    .backend(Backend::new(url).abilities().assistant())    // Claude, through Laravel
    .assistant(
        Assistant::new()
            .allow_abilities(["customers.*", "invoices.view"])
            .confirm("customers.delete")
            .instructions("You help staff at Acme manage customers and invoices."),
    )
```

```svelte
<script>
  import { openAssistant } from "@elyra/runtime";
</script>

<button onclick={() => openAssistant()}>Ask</button>
```

"Delete Ada's duplicate customer" → the assistant lists the customers, asks
the user before `customers_destroy` runs, and the open list updates through
its live query.

## Which commands are tools

`Assistant::allow_abilities([..])` offers the commands whose
`#[command(can = "…")]` the grant covers: an exact ability, or a namespace
ending in `*`. A command without `can` is never a tool. This grant is the
assistant's own, separate from the frontend's (`App::allow_ability`) and from
an MCP agent's (`App::mcp`).

Each tool is the command as [MCP](mcp.md) describes it: its name, its doc
comment as the description, and JSON Schemas built from its argument types.
**The doc comment is what the model reads**, so say what the arguments do.
`rata make:resource` writes doc comments like this:

```rust
/// A page of customers, searched and sorted. `search` matches `name`, `email`;
/// `sort` is one of `id`, `name`, `email`; `direction` is `asc` or `desc`;
/// `per_page` is at most 100.
#[command(live, can = "customers.view")]
pub async fn customers_index(ctx: Ctx, query: CustomerQuery) -> Result<Page<Customer>> { … }
```

## What may run

A call runs only when all of these hold:

1. The assistant's grant covers the command's ability.
2. With [`Backend::abilities()`](backend.md#what-the-user-may-do), the
   signed-in user has it. A tool they lack isn't offered at all, and the
   check is made again at each call, so a sign-out or a role change during a
   turn stops the next call. Signed out, the assistant has no tools.
3. The tool's rate limit isn't spent: `Assistant::rate_limit(ability, max, per)`,
   60 calls a minute by default, counted across conversations.
4. For an ability `Assistant::confirm` names, the user accepted *this* call.

A refused or failed call is told to the model in words it can act on: the
validation errors per field, "The user may not do this: it takes the
`customers.delete` ability.", or "The user didn't confirm
`customers_destroy`, so it did not run." Commands run through the middleware
with `Origin::Assistant`, and each one dispatches `AgentCalled` with
`client: "assistant"`, without its arguments.

## Code mode

A task over many records ("put last year's invoices without a project on
their customer's default project, and give me the total") is one tool call
per record, each a turn through the model with its result in the context.
In code mode (RFC 0027) Claude writes the loop instead: it runs in
Anthropic's code execution sandbox, each command it calls comes back to the
app as a tool call, and only what the code prints reaches the model.

```rust
Assistant::new()
    .allow_abilities(["invoices.*"])
    .confirm("invoices.delete")
    .code_mode()
```

- **Every call from code is checked as any call is:** the grant, the
  user's abilities, the window's scope, the rate limit.
- **A tool behind `confirm` isn't callable from code.** Claude calls it
  directly and the user confirms each call, as before. A call to it from
  code is refused to the code ("call it directly, so the user can confirm
  it"), whatever the model sent.
- **The panel** shows a run as one step that opens: the code, each call it
  made (with its link), and what it printed.
- **Where the data goes:** the results of the calls the code makes go to
  Anthropic's code execution container, kept up to 30 days, and code
  execution isn't covered by zero data retention. That's why it's off
  until the app turns it on, and, through a [Laravel
  backend](backend.md#the-apps-assistant), until the server allows it
  (`ELYRA_ASSISTANT_CODE_EXECUTION=true`). Anthropic's models only.
- A pause while code waits for a call isn't one of `max_steps`; a turn's
  code makes at most 500 calls.

## Asking the user

A command whose ability `Assistant::confirm(..)` names waits for the user.
The window gets the question on `elyra:assistant`, and `@elyra/runtime`
shows the themed dialog with the command's description and arguments:
**Allow** or **Don't**. The answer goes back on `/__assistant/confirm` with
the question's id, which is random, good once, for that window only, and
expires after five minutes. The model has no tool for it, so it can't answer
for the user.

Put everything that can't be undone behind `confirm`. `rata assistant
inspect` shows what asks first and what doesn't:

```
$ rata assistant inspect
add_todo(title) — todos.create [runs without asking]
    Add a todo with this title.
delete_todo(id) — todos.delete [asks first]
    Delete the todo with this id. It can't be undone.
list_todos() — todos.view [read-only]
    Every todo, oldest first.

3 tool(s): 1 ask the user first, 1 write without asking.
```

`--json` prints the full definitions, schemas included.

## Where the model comes from

The assistant tries these in order (`/__assistant/source` says which one it
uses):

1. **The backend.** With `Backend::assistant()`, requests go to
   `POST /api/elyra/assistant/v1/messages` on your Laravel app, with the
   user's Sanctum token. `Route::elyraAssistant()` in `elyracode/elyra-backend`
   adds the server's `ANTHROPIC_API_KEY` and caps the model, `max_tokens` and
   requests per user. See [the Laravel side](backend.md#the-apps-assistant).
   Signed out, the assistant says "Sign in to use the assistant."
2. **The keychain.** The user's own Anthropic key, kept with
   `SecretsProvider` under `assistant.anthropic_key`. The panel asks for it
   when there's no model; `/__assistant/key` keeps it or forgets it, and never
   sends it back.
3. **The environment.** The container's `elyra::ai::Ai`: `AiProvider`, from
   `ANTHROPIC_API_KEY`. For development.

**Never compile a provider key into the app.** Anyone with the app can read
it. `rata bundle` warns when the release binary contains something that looks
like one.

## The conversation

Each window has its own conversation, kept in memory and gone when the window
closes. One turn runs at a time. The system prompt is today's date, the
signed-in user's name, a rule that tool results are data and not
instructions, and then your `instructions`.

- `Assistant::model(..)` chooses the model (default: the AI client's,
  `claude-sonnet-5-5`).
- `Assistant::max_steps(n)` caps the tool steps per turn (default 8).
- `Assistant::max_tokens(n)` caps what each request to the model may write,
  thinking included (default 16 000, `assistant::DEFAULT_MAX_TOKENS`). On a
  model that always thinks, such as `claude-opus-5-5`, the thinking counts
  against it, so a small cap cuts answers short. Behind
  `Backend::assistant()`, Laravel's route lowers it to the server's own cap.
- `Assistant::max_tokens_per_conversation(n)` ends a conversation that has
  used `n` tokens. The window can start a new one.
- Within a turn, each step's answer goes back to the model as it came,
  thinking blocks and their signatures included, with the tool results.
  Between turns only the text is kept.

**The answer as it's written** ([RFC 0018](proposals/0018-assistant-streaming.md)):
the panel shows what Claude writes as it writes it, through the turn's tool
steps. "Let me look at the invoices." goes in its own bubble, the tool step
follows, and then the answer, word by word.

- **To the window**, the pieces go as `AssistantEvent::Text { step, text }`,
  gathered for 50 ms, and `Answer { text }` ends the turn with the whole
  answer. A window that missed a piece still gets the answer right.
- **Stopping a turn** stops the model; a command already running finishes.
  What it had written stays in the panel, and in the conversation, ending
  "(stopped by the user)", so "go on" works.
- **Through Laravel** it streams too, with this release's
  `elyracode/elyra-backend`. An older package, or `ELYRA_ASSISTANT_STREAM=false`, answers that it
  doesn't. The assistant then asks for the answer whole, and doesn't ask that
  server to stream again in this run.
- **Over OpenAI** (an app's own `Ai`), the answer arrives whole, as before.

## What it sees of the window

The user asks from a screen, and says "her", "this one", "these invoices".
So each turn carries a short note of the window it was asked from
([RFC 0013](proposals/0013-assistant-sees-the-window.md)), before the
user's words:

```text
<window>
{"route":"/customers/42?tab=invoices",
 "showing":[{"command":"customers_show","arguments":{"id":42}},
            {"command":"invoices_index","arguments":{"customer_id":42}}]}
</window>

Set her credit limit to 50 000.
```

- **`route`** is the window's route, as the router has it. The runtime
  sends it with each question.
- **`showing`** is the window's live queries (`live.customers_show(42)`),
  newest first. Each one is listed by its command and arguments, with
  unset arguments left out. A query is listed only when its command is a
  tool the signed-in user is offered. Another window's queries, an MCP
  agent's, and the ones outside the grant aren't.
- **`selected`** is what the app says is selected (below), while the
  window stays on the route it was selected on.
- **Names, not data.** The rows the window shows are never in it. To read
  one, the model calls the command, which is held to the user's abilities
  like any call.
- **It's small.** At most 16 queries. A string argument is cut at 200
  characters and the route at 500. Past 2 KB, the oldest queries are
  dropped until it fits, then ids from the longest selection. A window
  with no route, no queries and no selection sends no note.
- **The conversation keeps it**, so a later "and her invoices?" still knows
  who. The system prompt says the block is data about the screen, not
  instructions.

Nothing in the app has to change for this. A screen that runs
`live.customers_show(id)` already says which customer it's on.

### What's selected

"Send these a reminder" needs to know which. A `<Table>` from `@elyra/ui`
says so by itself with `select="customers"`
([selecting rows](ui/table.md#selecting-rows)). Otherwise the app says
so, by kind and ids:

```svelte
<script>
  import { assistant } from "@elyra/runtime";

  let selected = $state([]);            // the app's own selection of customers
  $effect(() => {
    assistant.selected("customers", selected.map((c) => c.id));
  });
</script>
```

```text
<window>
{"route":"/customers?search=acme","selected":{"customers":[4,7,9]}}
</window>
```

- `[]` clears a kind. A window has 8 kinds at most.
- A selection holds on the route it was made on. A question asked from
  another route drops it, and so does starting a new conversation.
- Only ids go. The model reads a record by calling its command.
- The first 100 ids of a kind are kept. When there were more,
  `more_selected` says how many: `{"customers": 150}`.
- In an app without an assistant it does nothing.

`Assistant::window_context(false)` leaves the note out, for an app whose
paths, ids or search terms shouldn't reach the model. To keep just one
query out, leave its command out of the grant.

## Links to the records

A command that declares where its record is
(`#[command(link = "/customers/{id}")]`, see [Commands](commands.md#where-its-record-is))
gets a link in the panel when the assistant runs it: "customers update ✓"
opens `#/customers/42` in the window. The `tool` event carries it as `link`
once the step succeeds, and a custom panel reads it from the step in the
`assistant` store.

- Only a path in the app becomes a link, through the router. The model
  can't make the panel open another site.
- The arguments themselves still aren't sent to the frontend or logged.
- `make:resource` writes `link` on its commands. MCP clients don't get it.

## Suggestions

Before the first message, the panel suggests what to ask: the app's
[`Prompt`s](mcp.md#prompts), as buttons.

```rust
use elyra::Prompt;

let overdue = Prompt::new("overdue_invoices", "Overdue invoices",
        "Find the invoices that are overdue and draft a reminder for each.")
    .needs("invoices.view");
let summary = Prompt::new("customer_summary", "Summarize a customer",
        "Summarize {customer}: open invoices, last contact, notes.")
    .argument("customer", "The customer's name or number")
    .needs("customers.view");

App::new()
    .assistant(Assistant::new().allow_abilities(["customers.*", "invoices.*"]).prompts([&overdue, &summary]))
    .mcp(Mcp::new().allow_abilities(["customers.view"]).prompts([&overdue, &summary]))   // the same, over MCP
```

- **Only what the user may do.** A suggestion shows when the assistant's
  grant covers every ability it `needs`, the user has them (by Laravel,
  with `Backend::abilities()`), and the window allows them
  ([RFC 0016](windows.md#giving-a-window-less)). The inspector's mask
  hides one too.
- **Choosing one** asks it at once. One with arguments goes into the input
  instead, its first `{argument}` selected to type over.
- **Completed as it's typed.** An argument with a source
  (`.complete("customer", Completion::new("customers_index", "name"))`,
  see [completions](mcp.md#completions)) shows a list over the input as
  the user types it: <kbd>↓</kbd> <kbd>↑</kbd> move in it, <kbd>Enter</kbd>
  takes the one moved to, <kbd>Tab</kbd> that one or the first, and
  <kbd>Esc</kbd> closes it. Taking one selects the next `{argument}`. The
  source runs as the user, in the window's limits, so the list holds only
  what they could read. A source the assistant's grant doesn't offer, or
  that may write, stops the app at start.
- They go once the conversation starts, and come back with **New**.
- `assistant.prompts()` lists them for a panel of your own, an argument
  with a source marked `completes`; `assistant.complete(prompt, argument,
  value)` asks for its values.

## In the frontend

`openAssistant(prompt?)` opens the built-in panel, and a ⌘K query that
matches no command offers **Ask the assistant**. For your own panel, the
`assistant` store holds the conversation. See
[UI components](components.md#assistant-panel).

What a window can do is `/__assistant/{prompt,confirm,stop,reset,tools,prompts,complete,source,key}`,
behind `Capability::Assistant` (60 calls a minute). `deny_frontend(Capability::Assistant)`
turns it off.

## Security

- **The user's rights bound the assistant**, checked when the tools are
  offered and again at each call.
- **Prompt injection is expected, not prevented.** A customer's notes can
  ask the model to delete everything, and the model may try. The call is
  still held to the user's abilities, to `confirm` and to the rate limit.
- **Data leaves the machine.** Tool results go to the model provider, and,
  through the backend, past your server. Keep commands whose data mustn't
  leave out of the grant. The [window's note](#what-it-sees-of-the-window)
  sends the route and the live queries' arguments (ids, search terms) too.
  `window_context(false)` turns it off.
- **The window's note is data.** A search term or a link's query string can
  hold words aimed at the model. The note is JSON in a delimited block,
  and nothing in it widens what the model may call.
- **The model's words are text.** The panel never renders them as HTML.
- **Code mode sends the results of code's calls to Anthropic's sandbox**
  ([code mode](#code-mode)): opt-in, per app and per server.

See also [Security](security.md#the-apps-assistant).

## Testing

`TestApp::assistant()` talks to the assistant as a window does, with
[`Ai::fake`](ai.md#testing-with-a-fake) scripting the model:

```rust
let fake = AiFake::new()
    .tool_call("customers_destroy", json!({ "id": 7 }))
    .text("Deleted.");
let app = TestApp::new(
    App::new()
        .commands(commands![customers_destroy])
        .swap(Ai::fake(fake.clone()))
        .assistant(Assistant::new().allow_ability("customers.delete").confirm("customers.delete")),
);

let turn = app.assistant().prompt("Delete customer 7").await;     // declines by default
assert_eq!(turn.asked(), ["customers_destroy"]);
assert_eq!(fake.calls()[0].output, "The user didn't confirm `customers_destroy`, so it did not run.");

let turn = app.assistant().accepting().prompt("Yes, delete it").await;
assert_eq!(turn.ran(), [("customers_destroy", true)]);
```

`turn.written()` is what the model wrote as it went, `(step, text)` per
step: `AiFake::tool_call_saying("Let me look.", ..)` says something before a
tool step, and `.pace(Duration)` spreads the words out, to stop a turn
mid-answer.

`.window(name)` talks from another window, with its own conversation.
`.at("/customers/42")` asks from that route, `.live("customers_show", (42,))`
(with the `database` feature) has the window show that query, and
`.selected("customers", [4, 7])` selects on that route. All three are then
in the turn's note, which a test reads from `fake.requests()`.
`framework/tests/assistant_live.rs` runs one round trip against Claude itself
when `ELYRA_AI_LIVE=1` and `ANTHROPIC_API_KEY` are set.

## Related

- [AI agents (MCP)](mcp.md): the same commands for an agent outside the app.
- [AI SDK](ai.md) · [A Laravel backend](backend.md#the-apps-assistant) ·
  [UI components](components.md#assistant-panel)
