# Wire format

> The low-level IPC contract. For the app-facing API see
> [frontend runtime](frontend-runtime.md), [commands](commands.md), and
> [events](events.md). Index: [docs/README.md](README.md).

The bridge is **MessagePack over the `elyra://localhost` custom protocol**,
called from the frontend with `fetch`. Same origin for both the app and IPC, so
there is no CORS, no preflight, and no JSON anywhere in the hot path.

## Endpoints

| Path | Purpose |
|---|---|
| `elyra://localhost/` and `/index.html` | the frontend (embedded assets, or the fallback page) |
| `elyra://localhost/__cmd/<name>` | command invocation (`POST`) |
| `elyra://localhost/__events` | event stream long-poll (`GET`) |

## Request — arguments (compact)

The request body is a **compact** MessagePack array of the call arguments:

```
invoke("add", 2, 3)  ->  encode([2, 3])  ->  msgpack array [2, 3]
```

Rust decodes it into the command's argument tuple, e.g. `(i64, i64)`. Compact
(positional) encoding is used here because the tuple shape is fixed by the
function signature — field names would be dead weight.

Zero-argument commands ignore the body entirely, which avoids the
`()` → `nil` vs `[]` → empty-array mismatch between serde and the JS encoder.

## Response — results (named)

Results are encoded with `rmp_serde::to_vec_named`:

```
SystemInfo { os, arch, commands }  ->  msgpack map { "os": ..., "arch": ..., "commands": [...] }
```

Named encoding means structs decode to plain JS objects and **survive field
reordering** between Rust and TypeScript versions — the property that makes the
contract robust as the app evolves. Scalars (`String`, `i64`, …) encode
identically either way.

## Events (M1) — Rust → frontend push

The frontend keeps **one** request open against `/__events`. The shell holds it
until the next event batch is ready, then responds; the frontend immediately
reconnects. No `evaluate_script`, no base64 — a continuous binary stream over
the same custom protocol. `@elyra/runtime`'s `channel()` multiplexes all named
channels over this single connection.

The response body is a MessagePack **array of `[channel, value]` pairs**:

```
[ ["tick", 1], ["tick", 2], ["cursor", { "x": 10, "y": 20 }] ]
```

Each `value` is `to_vec_named`-encoded and appended to the batch **verbatim**
(the batch is framed with low-level `rmp`), so there is no double-encode and no
`bin` wrapper — one decode on the JS side.

### Batching

Emits accumulate in a queue and flush together, so N state changes cost **one**
IPC round, not N. With the default zero window the natural response→reconnect
gap coalesces bursts; `App::batch_window(..)` adds an explicit coalescing delay
for frame-level batching of sustained streams. After ~20s idle the poll returns
an empty batch as a keep-alive.

## Status

The `x-elyra-status` response header is `ok` or `error`. On `error` the body is
a UTF-8 message (HTTP 500). The frontend runtime turns that into a
`CommandError`.

## Decision summary

- **Arguments:** compact `to_vec` (positional array) — shape fixed by signature.
- **Results:** `to_vec_named` (maps) — resilient to field reordering.
- JS side uses `@msgpack/msgpack`, which matches `rmp-serde` byte-for-byte.

## Async, never blocking

The custom-protocol handler is wry's **asynchronous** variant: every request is
spawned onto the tokio runtime and responded to from there, so the UI thread
never blocks on a command or a long-poll. (M0's `block_on` is gone.)
