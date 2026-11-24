# RFC 0053 — Exit hooks: work done however the app ends

**Status:** accepted (2026-11-24) · **Target:** 0.54.0 · builds on [windows & lifecycle](../windows.md#lifecycle), [sidecar](../sidecar.md), [updater](../updater.md) · see [Decisions](#decisions)

## Summary

An app that starts something outside itself has to end it when it quits:
an `ssh -N` tunnel, a database server it launched, a lock file, a
half-written temp file. Elyra gives it no place to do that. The event loop
(tao) ends the process without running destructors, and the updater's
relaunch calls `process::exit(0)` directly. So whatever the app started
lives on after it, as ElyraSQL's tunnels do on macOS and Windows.

This RFC adds exit hooks. They run once, whichever way the process ends:
the last window closed, Quit, a relaunch into an update, the end of an
`--mcp` run, or a signal from the OS or a terminal.

```rust
App::new()
    .on_exit(|ctx| ctx.get::<Tunnels>().close_all())
    .run()?;
```

```rust
// A command-line tool built on the same crates, with no window (RFC 0054).
fn main() -> anyhow::Result<()> {
    let _exit = elyra::exit::guard();          // hooks on drop, and on Ctrl-C/SIGTERM
    elyra::exit::on_exit(tunnel::kill_all);
    run_cli()
}
```

- **One list, run once**: `App::on_exit` and the free
  `elyra::exit::on_exit` add to the same process-wide list. The first way
  out runs it, and every later way finds it empty.
- **Every way out the framework controls**: the event loop's end, the
  updater's relaunch, `--mcp`'s end, and `SIGINT`/`SIGTERM`/`SIGHUP` (Unix)
  and Ctrl-C, Ctrl-Break and closing the console (Windows), each followed by
  the exit it would have made.
- **The framework's own children too**: sidecars the app spawned with
  `Sidecar` are ended by a hook of the framework's, so `sidecar` stops
  leaving processes behind.

## Motivation

- **ElyraSQL**, moving from its fork onto the framework, keeps its SSH
  tunnels in `ssh -N` children. Its fork has a global `on_exit(fn())` run
  on `LoopDestroyed`, and its updater calls the hooks before it relaunches.
  Without that here, moving to the framework would leave tunnels running
  after every quit.
- **`Sidecar` has the same bug today**: its children are tokio `Child`ren
  without `kill_on_drop`, and nothing drops them anyway, so a sidecar
  outlives the app that started it.
- **`Drop` is the usual answer and doesn't work here.** tao's `run` never
  returns, and `process::exit` skips destructors, so an app can't rely on
  one.

## Current state (0.53)

- `shell/webview.rs`: `ControlFlow::Exit` when the last window closes
  (`lifetime::ends`) and on Quit. tao then sends `Event::LoopDestroyed` and
  exits the process. Nothing handles `LoopDestroyed`.
- The updater: `updater.rs` (`apply_and_relaunch`), `updater/runtime.rs`
  and `shell/update.rs` end with `std::process::exit(0)` after arranging
  the relaunch.
- `App::run_mcp` (`--mcp`) returns when stdin closes. Its caller's
  destructors run, but there's no hook for state that lives elsewhere.
- No signal handling: Ctrl-C in a terminal, or `kill`, ends the process at
  once.
- `Sidecar` spawns with `tokio::process::Command`, without
  `kill_on_drop(true)`, and kills a child only on `sidecar.kill(id)`.

## Design

### The list

```rust
pub mod exit {
    /// Run `hook` once when the process ends, whichever way. Hooks run in
    /// the reverse order they were added (the last thing set up is the
    /// first torn down), on the thread that's ending the process.
    pub fn on_exit(hook: impl FnOnce() + Send + 'static);

    /// Run the hooks now, once: what every way out calls. A second call
    /// finds the list empty.
    pub fn run();

    /// For a `main` without `App::run`: runs the hooks when dropped, and
    /// installs the signal handlers below. Hold it for the life of `main`.
    pub fn guard() -> ExitGuard;
}

impl App {
    /// `on_exit` with the app's `Ctx`, added when the app starts.
    pub fn on_exit(self, hook: impl FnOnce(&Ctx) + Send + 'static) -> Self;
}
```

A hook that panics is caught. The error is logged and the rest still run.

### Where it's called

| How the process ends | Where `exit::run()` is called |
|---|---|
| Last window closed, Quit, the tray's Quit, macOS ⌘Q | `Event::LoopDestroyed` in the event loop |
| A relaunch into an update | before each `process::exit(0)` in the updater |
| `--mcp` ends (stdin closed) | when `run_mcp` returns |
| `SIGINT`, `SIGTERM`, `SIGHUP` (Unix) | a signal thread: run the hooks, then exit with 128 + the signal |
| Ctrl-C, Ctrl-Break, console closed (Windows) | `SetConsoleCtrlHandler`: run the hooks, then exit |
| A CLI's `main` returns or unwinds | `ExitGuard`'s `Drop` |

The signal handlers are installed by `App::run` and by `exit::guard()`,
and not by a library that only links Elyra. An app that installs its own
handlers can turn them off (`App::exit_on_signals(false)`, open
question 3).

### What it can't catch

`SIGKILL`, a force-quit from Activity Monitor or Task Manager, a crash, and
a power loss end the process with no chance to run anything. On Linux a
child can still be ended with the parent through `PR_SET_PDEATHSIG`. The
docs say this, and point a process that must never be left behind at its
own watchdog: the child exits when its stdin closes, as `ssh -N` can be set
up to.

Windows log-off and shutdown reach a GUI app (with no console) as
`WM_QUERYENDSESSION`/`WM_ENDSESSION` to its windows. tao doesn't pass them
on today, so they're open question 2.

### Sidecars

`Sidecar` spawns with `kill_on_drop(true)` and registers one framework
hook that kills the children still running. So a sidecar ends with the
app, as the docs always implied.

### Testing

- **Unit**: order (reverse), once only (`run` twice, a hook added during
  `run`), a panicking hook not stopping the others, `App::on_exit` getting
  the app's `Ctx`.
- **Integration (Unix)**: a test binary that registers a hook writing a
  file, then is sent `SIGTERM`. The file is there, and the exit code is
  143. The same with `ExitGuard` on a normal return.
- **Windows**: a console child sent `CTRL_BREAK_EVENT`
  (`GenerateConsoleCtrlEvent`), checked the same way.
- **The running app in CI**: the example app adds a hook that writes a
  marker file. The macOS, Linux and Windows walk-throughs quit it as they
  do now and check the file.
- **Sidecar**: a spawned `sleep 60` is gone after the hooks run.

## Implementation plan

1. `framework/src/exit.rs`: the list, `run`, `guard`, the Unix signal
   thread (`signal-hook`, or tokio's `signal` feature), and the Windows
   console handler.
2. The calls: `LoopDestroyed`, the updater's three exits, `run_mcp`.
   `App::on_exit`, and `exit_on_signals` if decided.
3. `Sidecar`: `kill_on_drop` and the framework's hook.
4. Tests, the example's marker and CI checks, docs (`windows.md#lifecycle`,
   `sidecar.md`, a short `exit.md`), CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **Hook signature: `FnOnce` with or without `Ctx`?**
   *Recommendation:* both, as above. `elyra::exit::on_exit` takes a plain
   `FnOnce()` so a CLI and a library can use it without an `App`.
   `App::on_exit` hands over the app's `Ctx`, which is where an app keeps
   what it has to close.
2. **Windows log-off and shutdown for a GUI app**: handle
   `WM_ENDSESSION` through a hidden message window of our own, or wait for
   tao?
   *Recommendation:* our own message-only window, created on the event
   loop's thread, that runs the hooks on `WM_ENDSESSION` (wParam true).
   Without it, a tunnel survives every log-off, which is one of the cases
   ElyraSQL names. If it turns out to be more than a small, contained
   module, it moves to its own change and the docs say so.
3. **Signal handlers by default?**
   *Recommendation:* yes in `App::run` and `exit::guard()`, with
   `App::exit_on_signals(false)` for an app that installs its own. Ctrl-C
   in `rata dev` then ends the app the way Quit does, which is what a
   developer expects.
4. **A time limit for the hooks?** A hook that hangs keeps the process
   alive.
   *Recommendation:* no limit in the framework. Hooks run in order on the
   exiting thread, and the docs ask them to be quick and to signal
   children rather than wait for them. A limit would mean killing a thread
   half-way through closing something, which is worse than a slow quit.
5. **Async hooks?**
   *Recommendation:* no. At exit the runtime may already be shutting down.
   A hook that needs async work does `rt.block_on` on a handle it kept, or
   sends a signal and returns.

## Not in this RFC

- **Asking before quitting** ("unsaved changes"): that's `beforeclose`
  on the page, and a window's close request.
- **Restoring state at the next launch.**
- **The `gui` feature** a CLI needs to build without tao and wry: RFC 0054.

## Alternatives considered

- **`Drop` on a value in the container**: never dropped, because the
  process exits without unwinding.
- **Only `LoopDestroyed`**, as the fork does: it misses the updater's
  relaunch (the fork calls the hooks there by hand), signals, and a CLI.
- **`PR_SET_PDEATHSIG` everywhere**: Linux only, and only for children.

## Decisions

1. **Both signatures**: `elyra::exit::on_exit(FnOnce())` for a CLI or a
   library, `App::on_exit(FnOnce(&Ctx))` for an app.
2. **Windows log-off and shutdown** through a message-only window of our
   own that runs the hooks on `WM_ENDSESSION`; if it isn't a small,
   contained module, it moves to its own change and the docs say so.
3. **Signal handlers by default** in `App::run` and `exit::guard()`, with
   `App::exit_on_signals(false)` to leave them to the app.
4. **No time limit**: hooks run in order on the exiting thread, and the
   docs ask them to be quick.
5. **No async hooks.**
