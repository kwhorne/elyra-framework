# RFC 0035 — Point at it, and hand it to the agent

**Status:** accepted (2026-11-19) · **Target:** 0.50.0 · builds on [RFC 0014](0014-elyra-inspector.md), [RFC 0026](0026-the-running-app-for-coding-agents.md), [RFC 0032](0032-microphone.md) and [RFC 0034](0034-the-state-matrix.md) · see [Decisions](#decisions)

## Summary

The developer sees what's wrong in the running app, and then has to
describe it to a coding agent in words: which screen, which table, the
button to the right of the search field. This lets them point at it
instead. In a debug build, hold ⌘⌥ (Ctrl+Alt) and drag a rectangle over
part of a window, type or say what to change, and press Enter:

> Replace this table with a SplitPane: the chosen customer in a Card on
> the right.

The shell makes a **UI task** of it: the note, a picture of what was
pointed at, the Svelte files and lines that drew it, the live queries the
window shows, and the window, route and frame. It's copied to the
clipboard for any agent, and `rata mcp dev` hands it to one that asks.

## Motivation

- **Words are a poor pointer.** "The table on the customers screen" is
  three guesses for an agent: which file, which component, which of the
  two tables. The developer is looking at the thing; the app knows which
  element it is and, in a Vite dev build, which line of which `.svelte`
  file made it.
- **The agent can already look, but not where the developer means.**
  `window_snapshot` and `window_screenshot` (RFC 0026) give it the whole
  window. Nothing says which part the developer is talking about.
- **The matrix shows what to fix** (RFC 0034): a button that wraps in the
  narrow frame, a colour that fails in light. Pointing at it in that frame
  says both what and where.

## Current state (0.49)

- **The agent script** (RFC 0026, debug only) walks the page into an
  accessibility tree with refs, and acts on them.
- **A window's picture** comes from `takeSnapshot` (macOS),
  `CapturePreview` (Windows) and WebKitGTK (Linux), at most 1280 CSS
  pixels wide.
- **`Showing`**: the live queries a window shows, with their arguments.
- **Svelte 5 in dev** puts `__svelte_meta` on every element it creates:
  `{ loc: { file, line, column }, parent }`, the file relative to the
  Vite project and the component stack above it. A build without Vite (the
  assets embedded) has none.
- **The microphone** (RFC 0032) records in the shell and transcribes
  through the app's server with `Backend::transcription()`.
- **The clipboard** is the `system` feature's.

## Design

### Pointing

- **⌘⌥-drag** (Ctrl+Alt-drag) draws a rectangle over the page; **⌘⌥-click**
  picks the element under the pointer. While ⌘⌥ is down, the pointer is a
  crosshair and the page gets no clicks: pointing never presses a button.
  **Esc** lets go.
- **The inspector's Point button** does the same without the keys, for a
  trackpad or a screen reader user.
- **In any window**, a frame of the matrix too. The task says which.

### The note

A small box opens at the selection, the page dimmed around it:

- **A text field.** Enter hands it off; Shift+Enter is a new line; Esc
  throws it away.
- **A microphone button**, when the app can transcribe
  (`Backend::transcription()`, RFC 0032): hold it, or the Space key while
  the field is empty, and say it. The text lands in the field to read
  before it's sent.
- **"Copied for your agent"** once handed off, with the task's id.

### What a task carries

```markdown
## UI task 7 — CRM, window `main`, route /customers?search=acme

Replace this table with a SplitPane: the chosen customer in a Card on the right.

Pointed at (picture: .elyra/tasks/7-selection.png, the whole window: .elyra/tasks/7-window.png):

- table "Customers" — src/views/Customers.svelte:41 (in App.svelte:88 › Customers.svelte)
  - row "Acme AS  Oslo  12 orders" …
- button "New customer" — src/views/Customers.svelte:12

The window shows the live queries customers_index { search: "acme" }.
Frame: none (the app as it is).
```

- **The elements** are those inside the rectangle, as the accessibility
  tree gives them (the same text as `window_snapshot`), cut to the
  outermost ones and at most 40 lines.
- **Their source** is `__svelte_meta`'s file and line, with the component
  stack above. Without Vite it says "no source locations: not a Vite dev
  build".
- **The pictures** are the window's picture and the selection cut from
  it, saved as PNG files the agent can open.
- **The live queries** are the window's `Showing`, with their arguments.
- **The frame** (RFC 0034), if it was one: its name, look and limits.

### Where it goes

- **The clipboard**, always: the task as Markdown, to paste into any
  agent's prompt.
- **`.elyra/tasks/`** in the workspace, when the app runs under `rata dev`
  (which says where the workspace is): `<id>.md` and the two pictures.
  Outside `rata dev`, the app's own folder. `rata new` and `rata
  ai:install` put `.elyra/` in `.gitignore`.
- **`rata mcp dev`**, two tools:

| Tool | What it does |
| --- | --- |
| `ui_tasks` | the tasks not yet done, newest first: the Markdown, and the selection's picture |
| `ui_task_done` | `{ id, summary }`: marks a task done; the app shows the summary in the window it came from, as a toast |

The project's `ratatosk/ai/guidelines.md` tells agents to check
`ui_tasks` when the developer says "the task" or "what I pointed at".

### Security

- **Debug builds only**, as the agent script and the inspector are. A
  release has no pointing, no routes, no tasks.
- **Nothing leaves the machine.** The pictures and the notes are the
  developer's own screen, written to the clipboard and to files on their
  machine, and given to the agent they connected through `rata mcp dev`
  (RFC 0026's endpoint, with its token).
- **The page gets no event** while ⌘⌥ is down over it, so pointing can't
  press a button or start a drag in the app.

### Testing

- **The agent script:** a rectangle gives the elements inside it, cut to
  the outermost; `__svelte_meta` gives file, line and the stack; no clicks
  reach the page while pointing.
- **Rust:** a task's Markdown; the selection cut from a picture; the
  files under `.elyra/tasks/`; `ui_tasks` and `ui_task_done` over the dev
  endpoint; nothing in a release (`inspector_release.rs`).
- **By hand** in the example app on macOS and Windows: point, type, hand
  off, paste into an agent.

## Implementation plan

1. Pointing and the note in the agent script and the runtime (debug).
2. `/__inspector/task`: the shell makes the task: the picture, the cut,
   the files, the clipboard.
3. Dictation in the note, when the app can transcribe.
4. `ui_tasks` and `ui_task_done` in `rata mcp dev`; the toast.
5. `.elyra/` in `.gitignore` from `rata new` and `ai:install`; the
   guidelines; docs (`docs/inspector.md`, `docs/ai-agents.md`).

## Not in this RFC

- **The agent starting on its own.** MCP has no way for a server to start
  an agent's turn; the developer pastes the task or says "do the task".
- **Several drafts of a screen side by side**, one picked: the review's
  third idea, left out of RFC 0034 too.
- **Pointing in a release build**, as a user's bug report. That's
  [RFC 0033](0033-diagnostics.md)'s ground, and a picture there would be
  the user's data.

## Alternatives considered

- **An MCP notification** when a task is made. Clients don't act on a
  server's notifications by starting a turn, so the developer would still
  have to say something; the clipboard works with every agent.
- **Only the clipboard.** Simple, but a long task with two pictures is a
  lot to paste, and an agent asked "what did I point at?" can't find it.
- **Vite's own inspector plugin** (open the file in the editor on click).
  It finds the source, but nothing for the agent: no note, no picture, no
  live queries, no frame.

## Decisions

1. **⌘⌥-drag and ⌘⌥-click, and the inspector's Point button** for a
   trackpad or a screen reader user.
2. **The clipboard, always.** It's what works with every agent, whether
   or not one has asked `rata mcp dev` for tasks.
3. **The files go in `.elyra/tasks/`** in the workspace under `rata dev`,
   and in the app's own folder otherwise.
4. **Dictation only when the app transcribes** through its own server
   (`Backend::transcription()`). The framework adds no transcription of
   its own for development.
