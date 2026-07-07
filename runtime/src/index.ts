/**
 * @elyra/runtime — the frontend half of the Elyra bridge.
 *
 * Wire format (see docs/wire-format.md):
 *   - command request body: a *compact* MessagePack array of the arguments,
 *   - command response: MessagePack, structs encoded as named maps -> objects,
 *   - event stream: a long-poll of `/__events` returning a MessagePack array of
 *     `[channel, value]` pairs — batched per flush, binary, no base64.
 *
 * Everything is same-origin under `elyra://localhost`, so `fetch` needs no CORS.
 */
import { encode, decode } from "@msgpack/msgpack";

const ORIGIN = "elyra://localhost";
const CMD_BASE = `${ORIGIN}/__cmd/`;
const EVENTS_URL = `${ORIGIN}/__events`;

/** Thrown when a command returns an error status. */
export class CommandError extends Error {
  constructor(
    public readonly command: string,
    message: string,
  ) {
    super(`elyra command "${command}" failed: ${message}`);
    this.name = "CommandError";
  }
}

/**
 * Invoke a Rust `#[command]` by name.
 *
 * @example
 * const greeting = await invoke<string>("greet", "World");
 * const sum = await invoke<number>("add", 2, 3);
 */
export async function invoke<T = unknown>(
  command: string,
  ...args: unknown[]
): Promise<T> {
  const res = await fetch(CMD_BASE + command, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(args), // compact array of arguments
  });

  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new CommandError(command, await res.text());
  }

  const buf = new Uint8Array(await res.arrayBuffer());
  return decode(buf) as T;
}

// --- Event bus (Rust -> frontend), one multiplexed long-poll connection -----

type Handler = (value: unknown) => void;

const subscribers = new Map<string, Set<Handler>>();
const lastValue = new Map<string, unknown>();
let pumping = false;

function dispatch(name: string, value: unknown) {
  lastValue.set(name, value);
  const set = subscribers.get(name);
  if (set) for (const handler of set) handler(value);
}

async function pump() {
  pumping = true;
  let backoff = 0;
  while (subscribers.size > 0) {
    try {
      const res = await fetch(EVENTS_URL, { headers: { accept: "application/msgpack" } });
      if (!res.ok) throw new Error(`events ${res.status}`);
      const batch = decode(new Uint8Array(await res.arrayBuffer())) as [string, unknown][];
      for (const [name, value] of batch) dispatch(name, value);
      backoff = 0;
    } catch {
      // Window closing or transient failure — back off, then retry.
      backoff = Math.min(backoff ? backoff * 2 : 100, 2000);
      await new Promise((r) => setTimeout(r, backoff));
    }
  }
  pumping = false;
}

/**
 * A server-pushed event channel. The return value is a Svelte-readable store,
 * so `$channel("cursor")` works directly in a component; it is also usable
 * standalone via `.subscribe(handler)`.
 *
 * @example
 * // In a .svelte component:
 * const ticks = channel<number>("tick");
 * // ...then use `$ticks` in markup.
 *
 * @example
 * const unsubscribe = channel<number>("tick").subscribe((n) => console.log(n));
 */
export function channel<T = unknown>(name: string): {
  subscribe: (handler: (value: T | undefined) => void) => () => void;
} {
  return {
    subscribe(handler: (value: T | undefined) => void) {
      // Svelte store contract: emit the current value immediately.
      handler(lastValue.get(name) as T | undefined);

      let set = subscribers.get(name);
      if (!set) subscribers.set(name, (set = new Set()));
      set.add(handler as Handler);

      if (!pumping) void pump();

      return () => {
        const s = subscribers.get(name);
        if (!s) return;
        s.delete(handler as Handler);
        if (s.size === 0) subscribers.delete(name);
      };
    },
  };
}
