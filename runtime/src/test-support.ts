/**
 * Shared plumbing for the runtime's tests. `fetch` is the only seam between this
 * package and the Rust shell, so every test stubs it and asserts on the wire.
 *
 * Not part of the published surface: `tsconfig.build.json` compiles `src/index.ts`
 * only, and `package.json#files` ships `dist/` alone.
 */
import { expect, vi } from "vitest";
import { encode } from "@msgpack/msgpack";

export interface RecordedCall {
  url: string;
  init?: RequestInit;
}

/** A resolvable request, for tests that need to control poll timing. */
export interface PendingCall extends RecordedCall {
  resolve: (res: Response) => void;
  reject: (err: unknown) => void;
}

export type Handler = (url: string, init?: RequestInit) => Response | Promise<Response>;

/** Build a successful msgpack response shaped like the shell's. */
export function okResponse(value: unknown): Response {
  return new Response(encode(value), {
    status: 200,
    headers: { "content-type": "application/msgpack", "x-elyra-status": "ok" },
  });
}

/**
 * Build a failed response. The shell signals failure with `x-elyra-status` and
 * names the variety in `x-elyra-error-kind`; the body is the raw detail.
 */
export function errorResponse(message: string, kind = "command", status = 500): Response {
  return new Response(message, {
    status,
    headers: { "x-elyra-status": "error", "x-elyra-error-kind": kind },
  });
}

/** Stub `fetch` with a handler, recording every call. */
export function stubFetch(handler: Handler): RecordedCall[] {
  const calls: RecordedCall[] = [];
  vi.stubGlobal("fetch", (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return Promise.resolve(handler(url, init));
  });
  return calls;
}

/**
 * Stub `fetch` so every request stays pending until the test resolves it. Needed
 * to observe the event pump's loop: a poll that resolves on its own makes the
 * pump reconnect immediately, and the interesting transitions happen between
 * one poll finishing and the next starting.
 */
export function stubPendingFetch(): PendingCall[] {
  const pending: PendingCall[] = [];
  vi.stubGlobal(
    "fetch",
    (url: string, init?: RequestInit) =>
      new Promise<Response>((resolve, reject) => {
        pending.push({ url, init, resolve, reject });
      }),
  );
  return pending;
}

/** Wait until at least `n` requests have been made, then return the nth (1-based). */
export async function nthCall(pending: PendingCall[], n: number): Promise<PendingCall> {
  await vi.waitFor(() => expect(pending.length).toBeGreaterThanOrEqual(n));
  return pending[n - 1]!;
}

/** Await a call that must reject, returning the error narrowed to `T`. */
export async function rejection<T>(
  call: Promise<unknown>,
  ctor: new (...args: never[]) => T,
): Promise<T> {
  try {
    await call;
  } catch (e) {
    if (e instanceof ctor) return e;
    throw e;
  }
  throw new Error("expected the call to reject, but it resolved");
}

/** Read a recorded request's headers. */
export function headersOf(call: RecordedCall | undefined): Headers {
  return new Headers(call?.init?.headers);
}

/** Let queued microtasks (and any already-due timers) run. */
export function tick(ms = 0): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Bring the event pump to a stop between tests.
 *
 * The runtime keeps the subscriber map and the `pumping` flag in module scope,
 * and vitest shares that scope across a file. A test that walks away while a
 * poll is still in flight leaves the pump parked on a promise that will never
 * settle, so `pumping` stays true and *every later test* fails to start a pump.
 *
 * Dropping the subscribers and answering the outstanding polls lets the loop
 * re-check its condition and exit cleanly.
 */
export async function drainPump(
  pending: PendingCall[],
  unsubscribes: Array<() => void>,
): Promise<void> {
  for (const off of unsubscribes) off();
  for (const call of pending) {
    call.resolve(new Response(encode([]), { status: 200 }));
  }
  // Long enough to outlast the pump's first backoff step (100ms).
  await tick(150);
}
