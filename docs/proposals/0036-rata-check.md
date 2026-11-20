# RFC 0036 — `rata check`: the app, checked as a whole

**Status:** implemented (2026-11-19) · **Target:** 0.49.0 · builds on [RFC 0010](0010-laravel-abilities.md), [RFC 0011](0011-in-app-assistant.md), [RFC 0016](0016-window-capabilities.md) and [RFC 0026](0026-the-running-app-for-coding-agents.md) · see [Decisions](#decisions)

## Summary

The compiler checks each command, and the startup checks catch a missing
feature. Neither looks at the app as a whole: a command that deletes and
needs no ability, an ability a command asks for that nothing grants, an
assistant tool that writes without asking, typed bindings that no longer
match the commands. A coding agent writing much of the code makes these
easier to miss. `rata check` looks for them:

```text
$ rata check
error    customers.delete is declared by customers_destroy but granted nowhere:
         the command is refused in every window. Grant it with
         App::allow_abilities, or from Laravel with Backend::abilities().
warning  notes_purge writes and needs no ability: any page script can call it.
         Give it #[command(can = "notes.delete")].
warning  The assistant can call invoices_send without asking the user.
         Put it behind Assistant::confirm("invoices.send").
error    app/src/bindings.ts is out of date: run `rata codegen`.
info     reports_rebuild isn't called by the frontend, and the assistant and
         MCP can't reach it.

2 errors, 2 warnings, 1 note. `rata check --json` for tools.
```

It exits non-zero on an error, for CI. `rata mcp dev` gets `app_check`, so
an agent can run it after a change.

## Motivation

- **The mistakes that matter are between the pieces.** Each command
  compiles, each grant is valid, and together they leave a write open to
  any script, or a screen that's always refused. Today they're found when
  a user can't delete, or when something is deleted that shouldn't have
  been.
- **The pieces already say what they are.** The command registry knows
  each command's ability and whether it's a live query (a read). The app
  knows its grants, its assistant's and MCP's tools and which ask first,
  and its startup problems. `rata assistant inspect` already counts the
  tools that write without asking. Nothing puts them together.
- **An agent needs a verdict, not a tour.** After a change, "is the app
  still sound?" should be one call with a list of what's wrong and how to
  fix each.

## Current state (0.49)

- **`App::problems`** (startup): a feature a setting needs, a tray item
  without its setup. They stop the app with a message.
- **`rata mcp inspect` / `rata assistant inspect`** run the app in an
  inspect mode (`ELYRA_MCP_INSPECT`, `ELYRA_ASSISTANT_INSPECT`): it writes
  its catalog as JSON and exits.
- **Writes and reads:** the framework counts a command as a read when it's
  `live`, and as a write otherwise. MCP's `readOnlyHint` comes from this.
- **`rata codegen`** writes the typed bindings; nothing says when they're
  stale.

## Design

### How it looks

`rata check` runs the app once in a new inspect mode, `ELYRA_CHECK`,
which writes everything the checks need as JSON and exits:

- each command's name, ability, whether it's live, and its middleware;
- the abilities the app grants, and whether Laravel grants them
  (`Backend::abilities()`);
- the assistant's tools and MCP's, each with whether it asks first;
- the windows the app opens at start, with their limits;
- the startup problems.

Then it reads the frontend's sources (`app/src/**/*.{svelte,ts,js}`) for
the commands they call, and runs codegen to a temporary file to compare
with the bindings.

### The checks

| Check | Level | What it finds |
|---|---|---|
| `ability-not-granted` | error | A command's ability that the app doesn't grant, with no `Backend::abilities()` to grant it: refused everywhere. |
| `startup-problem` | error | What `App::problems` would stop the app for. |
| `bindings-stale` | error | `bindings.ts` isn't what `rata codegen` would write. |
| `write-without-ability` | warning | A command that isn't live and has no `can`: any page script with `Capability::Commands` can call it. |
| `assistant-writes-unasked` | warning | An assistant tool that writes without `Assistant::confirm`. |
| `mcp-writes-unasked` | warning | An MCP tool that writes without confirmation, for agents outside the app. |
| `granted-unused` | warning | An ability the app grants that no command declares and isn't a pattern (`customers.*`): a typo, or a command removed. |
| `uncalled` | info | A command the frontend doesn't call and the assistant and MCP can't reach. Rust code calling it can't be seen, so it's a note, not a warning. |

Each finding says what's wrong, why it matters and how to fix it, in one
line or two, with the command or ability named.

### Saying no to one

Some findings are deliberate: a `greet` command any page may call. An
app says so in `elyra.toml`, with the reason:

```toml
[check]
allow = [
  { check = "write-without-ability", command = "greet", why = "harmless, no data" },
]
```

A finding allowed is listed under "allowed" with its reason, and doesn't
count. An `allow` that matches nothing is itself a warning, so they don't
outlive what they were for.

### For tools and agents

- **`rata check --json`**: the findings as a list of `{ check, level,
  subject, message, fix }`, for CI annotations or an editor.
- **Exit code**: 1 with an error, 0 otherwise. `--strict` counts warnings
  too.
- **`rata mcp dev`'s `app_check`**: the same as text. The guidelines tell
  agents to run it after adding or changing commands, abilities, the
  assistant or MCP.
- **In CI**: `docs/cli.md` shows the step to add after the tests.

### Security

It reads the app's code and runs the app in an inspect mode that writes
a file and exits, as `rata mcp inspect` does: no window, no network, no
database writes. The JSON holds names and settings, never data.

### Testing

- The inspect mode's JSON from a test app with each kind of command,
  grant, assistant and MCP setting.
- Each check against a fixture that has the finding and one that hasn't,
  and `allow` with and without a match.
- The example app passes, with its `allow`s.
- `rata check` in CI on the example app.

## Implementation plan

1. `ELYRA_CHECK` in the framework: the app's shape as JSON.
2. The checks in rata, the text and the JSON, the exit code.
3. The frontend scan (`uncalled`) and the bindings comparison
   (`bindings-stale`).
4. `[check] allow` in `elyra.toml`.
5. `app_check` in `rata mcp dev`; the guidelines; `docs/cli.md`; CI on the
   example app.

## As built

- **`ELYRA_CHECK` runs before the startup problems**, which stop the app
  in any other mode: here they're findings. It writes, per command, its
  ability, whether it's live, and whether the frontend, the assistant and
  MCP reach it (with `confirm`); the grants and whether a command uses
  each; the problems; and the bindings, generated by the same code as
  `ELYRA_CODEGEN_OUT`.
- **`ability-not-granted` counts the assistant's and MCP's grants too.** A
  command only an agent reaches is refused nowhere it's meant for, so it
  isn't a finding. Laravel's abilities aren't grants: they narrow per user
  what the app grants.
- **"May write", not "writes".** The framework only knows a live query is
  a read, so `write-without-ability` says a command "isn't a live query,
  so it may write".
- **`granted-unused` also reads the pages:** an ability a page names in a
  string (`can="reports.view"`) is used, even with no command asking for
  it.
- **`uncalled`** reads the frontend's identifiers, so `api.notes_purge`,
  `live.notes_index` and a bare `invoke("notes_purge")` all count.
- **`allow-unused`** is a warning of its own, and an `allow` without
  `command` or `ability` allows a check for the whole app.
- **In `rata mcp dev`**, `app_check` runs the app with its output on
  stderr, so the MCP protocol on stdout stays whole.
- **The example app** had 20 warnings and 6 notes the first time: a demo,
  open on purpose. Four `allow`s with their reasons leave it with none, and
  CI runs `rata check` on it in the MCP inspector job.

## Not in this RFC

- **The App Blueprint**: a picture of the app, its screens with their
  pictures, the commands, live queries and abilities they use, and lines
  between them. It would read the same JSON and the screenshots of
  RFC 0026. A later RFC, once the checks have shown which links matter.
- **Routes**: links to a route nothing handles, and screens nothing links
  to. The router is the app's own Svelte, so finding its routes reliably
  needs more than a scan of the sources.
- **Laravel's side**: whether the server's policies agree with the
  abilities the desktop asks for. That's the Laravel package's to check.

## Alternatives considered

- **More startup problems.** They stop the app, which is right for a
  missing feature and wrong for "this command has no ability": a
  developer would turn the check off to get on with their work.
- **Clippy lints or a proc-macro error** for a command without `can`.
  The macro sees one command; whether its ability is granted, or the
  assistant asks first, is only known when the app is assembled.
- **The checks in `rata mcp dev` only.** An agent would see them, but CI
  and a developer without an agent wouldn't.

## Decisions

1. **`write-without-ability` is a warning**, not an error. Many apps start
   with commands open to their own pages; `--strict` makes it count.
2. **`uncalled` is shown, as a note.** It's how a removed screen's
   commands are found, and it never fails the check.
3. **`allow` lives in `elyra.toml`**, with its reason, not on the command:
   the reasons are in one place, and the check stays out of the code.
4. **`rata check`**, next to `rata codegen` and `rata build`.
