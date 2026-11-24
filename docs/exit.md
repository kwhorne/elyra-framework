# Work done when the app ends

An app that starts something outside itself has to end it when it quits:
an `ssh -N` tunnel, a server it launched, a lock file, a half-written temp
file. A `Drop` can't be relied on for that: the event loop ends the
process without running destructors, and so does the updater's relaunch.
An **exit hook** can ([RFC 0053](proposals/0053-exit-hooks.md)).

```rust
App::new()
    .on_exit(|ctx| ctx.get::<Tunnels>().close_all())
    .run()?;
```

Hooks run **once**, last added first, on the thread that's ending the
process, whichever way it ends:

| How the app ends | |
|---|---|
| The last window closed, Quit, the tray's Quit, ⌘Q | when the event loop ends |
| A relaunch into an update | before the updater exits |
| An `--mcp` run ends (its client closed stdin) | when the run returns |
| `SIGINT`, `SIGTERM`, `SIGHUP` (macOS, Linux) | then the process exits with 128 + the signal |
| Ctrl-C, Ctrl-Break, the console closed (Windows) | then the process exits as Windows would |
| Log-off, shutdown, a restart for updates (Windows) | on `WM_ENDSESSION`, before Windows ends the process |

A hook that panics is logged, and the rest still run. Keep hooks quick:
signal a child to end rather than wait for it. A hook that hangs keeps the
process alive.

**Nothing runs** on `SIGKILL`, a force-quit from Activity Monitor or Task
Manager, a crash, or a power loss. A child that must never be left behind
should watch for its parent itself, for instance by exiting when its stdin
closes.

## Sidecars end with the app

Processes started with [`Sidecar`](sidecar.md) are ended by a hook of the
framework's own: whatever is still running when the app ends is killed.

## Without `App::run`

A `main` that doesn't call `App::run` (a command-line tool built on the same
crates) holds an `ExitGuard`: it runs the hooks when `main` returns or
unwinds, and installs the same signal handlers.

```rust
fn main() -> anyhow::Result<()> {
    let _exit = elyra::exit::guard();
    elyra::exit::on_exit(tunnel::kill_all);
    run_cli()
}
```

`elyra::exit::on_exit` adds to the same list as `App::on_exit`, without a
`Ctx`, and `elyra::exit::run()` runs it at once (a later call finds it
empty).

## An app with its own signal handling

`App::exit_on_signals(false)` leaves `SIGINT`, `SIGTERM` and the Windows
console's events to the app. It then calls `elyra::exit::run()` itself
before it ends the process. The event loop, the updater and `--mcp` still
run the hooks.

## Related

- [Windows & lifecycle](windows.md#lifecycle) · [Sidecar processes](sidecar.md) · [Updater](updater.md)
