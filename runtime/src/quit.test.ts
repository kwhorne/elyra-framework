/**
 * `quit()` (RFC 0020): the app ends, as the tray's Quit item ends it; how an
 * app that stays in the tray is quit from a button.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { errorResponse, okResponse, stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { quit } = await import("./index.js");

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("quit", () => {
  it("asks the shell to end the app", async () => {
    const calls = stubFetch(() => okResponse(true));
    expect(await quit()).toBe(true);
    expect(calls.map((c) => [c.url, c.init?.method])).toEqual([
      ["elyra://localhost/__window/quit", "POST"],
    ]);
  });

  it("fails where the window may not control windows", async () => {
    stubFetch(() => errorResponse("capability `window` not granted", "denied", 403));
    await expect(quit()).rejects.toThrow(/quit.*not granted/);
  });
});
