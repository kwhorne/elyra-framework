/**
 * Unit tests for the runtime's wire handling. `fetch` is stubbed, so these cover
 * the framing, the headers, the error contract, and the event pump without a
 * running app.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";

// The module reads `globalThis.__ELYRA__` at import time.
(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const {
  invoke,
  invokeCancellable,
  channel,
  CommandError,
  ValidationError,
  ForbiddenError,
  validationErrors,
} = await import("./index.js");

type Handler = (url: string, init?: RequestInit) => Response | Promise<Response>;

/** Build a msgpack response like the shell's. */
function okResponse(value: unknown): Response {
  const body = encode(value);
  return new Response(body, {
    status: 200,
    headers: { "content-type": "application/msgpack", "x-elyra-status": "ok" },
  });
}

function errorResponse(message: string, kind = "command", status = 500): Response {
  return new Response(message, {
    status,
    headers: { "x-elyra-status": "error", "x-elyra-error-kind": kind },
  });
}

let calls: { url: string; init?: RequestInit }[] = [];

/** Await a call that must reject, and return the thrown error narrowed to `T`. */
async function rejection<T>(
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

function stubFetch(handler: Handler) {
  vi.stubGlobal("fetch", (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return Promise.resolve(handler(url, init));
  });
}

beforeEach(() => {
  calls = [];
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("invoke", () => {
  it("posts a compact msgpack array of arguments and decodes the result", async () => {
    stubFetch(() => okResponse(42));
    const result = await invoke<number>("add", 2, 40);

    expect(result).toBe(42);
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe("elyra://localhost/__cmd/add");
    expect(calls[0].init?.method).toBe("POST");
    expect(decode(calls[0].init?.body as Uint8Array)).toEqual([2, 40]);
  });

  it("attaches the IPC token and a client id to every request", async () => {
    stubFetch(() => okResponse(null));
    await invoke("ping");

    const headers = new Headers(calls[0].init?.headers);
    expect(headers.get("x-elyra-token")).toBe("test-token");
    expect(headers.get("x-elyra-client-id")).toMatch(/.+/);
  });

  it("throws a CommandError carrying the kind and detail", async () => {
    stubFetch(() => errorResponse("kaboom"));
    await expect(invoke("explode")).rejects.toBeInstanceOf(CommandError);

    stubFetch(() => errorResponse("kaboom"));
    const error = await rejection(invoke("explode"), CommandError);
    expect(error.command).toBe("explode");
    expect(error.kind).toBe("command");
    expect(error.detail).toBe("kaboom");
    expect(error.message).toContain("kaboom");
  });

  it("throws a ValidationError with a parsed field bag", async () => {
    const bag = { email: ["The email must be a valid email address."] };
    stubFetch(() => errorResponse(JSON.stringify(bag), "validation"));

    const error = await rejection(invoke("create_account", {}), ValidationError);
    expect(error.errors).toEqual(bag);
    expect(validationErrors(error)).toEqual(bag);
  });

  it("throws ForbiddenError when the token is rejected", async () => {
    stubFetch(() => errorResponse("missing or invalid x-elyra-token", "forbidden", 403));
    await expect(invoke("add", 1, 2)).rejects.toBeInstanceOf(ForbiddenError);
  });

  it("surfaces a panic as a CommandError of kind panic", async () => {
    stubFetch(() => errorResponse("command `x` panicked: boom", "panic"));
    const error = await rejection(invoke("x"), CommandError);
    expect(error.kind).toBe("panic");
  });
});

describe("validationErrors", () => {
  it("returns null for non-validation errors", () => {
    expect(validationErrors(new Error("nope"))).toBeNull();
    expect(validationErrors("not an error")).toBeNull();
    expect(validationErrors(new CommandError("x", "plain message"))).toBeNull();
  });
});

describe("invokeCancellable", () => {
  it("sends a request id and can cancel it", async () => {
    stubFetch((url) => (url.endsWith("/__cancel") ? okResponse(true) : okResponse(1)));

    const job = invokeCancellable<number>("slow");
    expect(await job.result).toBe(1);
    const headers = new Headers(calls[0].init?.headers);
    expect(headers.get("x-elyra-request-id")).toBe(job.id);

    await job.cancel();
    const cancelCall = calls.find((c) => c.url.endsWith("/__cancel"));
    expect(cancelCall).toBeDefined();
    expect(decode(cancelCall!.init?.body as Uint8Array)).toBe(job.id);
  });
});

describe("channel", () => {
  /**
   * Serve one batch, then leave the long-poll pending forever — mirroring the
   * shell, which only answers when events are ready. Resolving immediately would
   * make the pump reconnect in a tight loop.
   */
  function serveOneBatch(batch: [string, unknown][]) {
    let served = false;
    stubFetch(() => {
      if (served) return new Promise<Response>(() => {}) as unknown as Response;
      served = true;
      return new Response(encode(batch), { status: 200 });
    });
  }

  it("delivers batched events and replays the last value to new subscribers", async () => {
    serveOneBatch([
      ["tick", 1],
      ["tick", 2],
      ["other", "x"],
    ]);

    const seen: number[] = [];
    const unsubscribe = channel<number>("tick").subscribe((value) => {
      if (value !== undefined) seen.push(value);
    });

    // Only the subscribed channel is delivered, in batch order.
    await vi.waitFor(() => expect(seen).toEqual([1, 2]));

    // Svelte store contract: a later subscriber sees the cached value at once.
    const replay: (number | undefined)[] = [];
    const off2 = channel<number>("tick").subscribe((v) => replay.push(v));
    expect(replay[0]).toBe(2);

    off2();
    unsubscribe();
  });
});
