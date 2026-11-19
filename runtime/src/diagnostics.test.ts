/**
 * Diagnostics (RFC 0033): the text and the save dialog behind
 * `App::diagnostics()`, and the page's errors reported to the shell, which
 * keeps them for the file and logs them, in every app.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decode } from "@msgpack/msgpack";
import { errorResponse, okResponse, stubFetch, tick } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { diagnostics, __onPageError, __onRejection, __resetPageErrors } = await import("./index.js");

function bodyOf(call: { init?: RequestInit } | undefined): unknown {
  return decode(call?.init?.body as Uint8Array);
}

beforeEach(() => __resetPageErrors());
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("diagnostics", () => {
  it("hands over the text, and the saved file's path", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/text") ? okResponse("Elyra diagnostics — Acme 1.0\n") : okResponse("/Users/kari/Downloads/acme.txt"),
    );
    expect(await diagnostics.text()).toBe("Elyra diagnostics — Acme 1.0\n");
    expect(await diagnostics.save()).toBe("/Users/kari/Downloads/acme.txt");
    expect(calls.map((c) => c.url)).toEqual([
      "elyra://localhost/__diagnostics/text",
      "elyra://localhost/__diagnostics/save",
    ]);
    expect(new Headers(calls[0]!.init?.headers).get("x-elyra-token")).toBe("test-token");
  });

  it("is null when the user cancels the dialog", async () => {
    stubFetch(() => okResponse(null));
    expect(await diagnostics.save()).toBeNull();
  });

  it("fails without App::diagnostics()", async () => {
    stubFetch(() => errorResponse("capability Diagnostics is not granted", "forbidden", 403));
    await expect(diagnostics.text()).rejects.toThrow(/Diagnostics is not granted/);
  });

  it("says diagnostics are off when the app has no setup", async () => {
    stubFetch(() => errorResponse("diagnostics are off: add App::diagnostics()", "off", 404));
    await expect(diagnostics.save()).rejects.toMatchObject({ kind: "off" });
  });
});

describe("the page's errors", () => {
  it("sends an uncaught error with its first frame", async () => {
    const calls = stubFetch(() => okResponse(true));
    const error = new TypeError("Cannot read properties of undefined (reading 'id')");
    error.stack = [
      "TypeError: Cannot read properties of undefined (reading 'id')",
      "    at CustomerCard (elyra://localhost/assets/CustomerCard.svelte?t=1:41:12)",
      "    at render (elyra://localhost/assets/index-B1x9.js:2:100)",
    ].join("\n");
    __onPageError({ error });
    await tick();
    expect(calls[0]!.url).toBe("elyra://localhost/__page-error");
    expect(bodyOf(calls[0])).toEqual({
      message: "TypeError: Cannot read properties of undefined (reading 'id')",
      at: "CustomerCard.svelte:41",
      kind: "error",
    });
  });

  it("reads WebKit's stacks, and the event's file without one", async () => {
    const calls = stubFetch(() => okResponse(true));
    const error = new Error("boom");
    error.stack = "load@elyra://localhost/assets/app-B1x9.js:7:30\nglobal code@elyra://localhost/:1:1";
    __onPageError({ error });
    __onPageError({ message: "Uncaught SyntaxError", filename: "elyra://localhost/assets/x.js?v=2", lineno: 3 });
    await tick();
    expect((bodyOf(calls[0]) as { at: string }).at).toBe("app-B1x9.js:7");
    expect(bodyOf(calls[1])).toEqual({ message: "Uncaught SyntaxError", at: "x.js:3", kind: "error" });
  });

  it("sends an unhandled rejection, whatever its reason", async () => {
    const calls = stubFetch(() => okResponse(true));
    __onRejection(new Error("an answer from /api/customers/7 was not JSON"));
    __onRejection({ status: 500 });
    await tick();
    expect(bodyOf(calls[0])).toMatchObject({
      message: "Error: an answer from /api/customers/7 was not JSON",
      kind: "rejection",
    });
    expect(bodyOf(calls[1])).toEqual({ message: '{"status":500}', at: null, kind: "rejection" });
  });

  it("leaves out the browser's noise, the same error again at once, and a flood", async () => {
    vi.useFakeTimers();
    const calls = stubFetch(() => okResponse(true));
    __onPageError({ message: "ResizeObserver loop completed with undelivered notifications." });
    __onPageError({ message: "Script error." });
    __onPageError({ message: "same" });
    __onPageError({ message: "same" });
    vi.advanceTimersByTime(3_000);
    __onPageError({ message: "same" });
    for (let i = 0; i < 40; i++) __onPageError({ message: `loop ${i}` });
    expect(calls.length).toBe(20);
    vi.advanceTimersByTime(61_000);
    __onPageError({ message: "a minute later" });
    expect(calls.length).toBe(21);
  });

  it("never throws, even when the shell can't be reached", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new Error("no shell")));
    expect(() => __onRejection("x")).not.toThrow();
    await tick();
  });
});
