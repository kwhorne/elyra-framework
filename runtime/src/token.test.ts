/**
 * The no-token case, in its own file because the runtime snapshots
 * `globalThis.__ELYRA__.token` at import time — one module instance can only
 * ever see one token.
 *
 * This is the shape a hand-written page or a stray iframe sees: the shell
 * answers 403, and the failure has to be legible rather than looking like a
 * broken command.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { headersOf, okResponse, rejection, stubFetch } from "./test-support.js";

// No `__ELYRA__` at all: the shell never injected its bootstrap script.
delete (globalThis as Record<string, unknown>).__ELYRA__;

const { invoke, ForbiddenError } = await import("./index.js");

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("without an injected token", () => {
  it("omits the token header instead of sending an empty one", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("ping");

    // An empty `x-elyra-token` would be a token the shell has to compare and
    // reject; leaving it off keeps the rejection unambiguous.
    expect(headersOf(calls[0]).has("x-elyra-token")).toBe(false);
  });

  it("still identifies the document so the shell can queue events for it", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("ping");

    expect(headersOf(calls[0]).get("x-elyra-client-id")).toMatch(/.+/);
  });

  it("reports the 403 as ForbiddenError with an actionable message", async () => {
    stubFetch(
      () =>
        new Response("missing or invalid x-elyra-token", {
          status: 403,
          headers: { "x-elyra-status": "error", "x-elyra-error-kind": "forbidden" },
        }),
    );

    const error = await rejection(invoke("greet", "World"), ForbiddenError);
    expect(error.message).toContain("Only pages loaded by the app can call native APIs");
  });
});
