/**
 * Where the page calls the shell: the origin the shell gives it
 * (`globalThis.__ELYRA__.origin`): `http://elyra.localhost` on Windows, where
 * WebView2 serves a custom protocol that way, else `elyra://localhost`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { okResponse, stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token", origin: "http://elyra.localhost" };

const { invoke, quit } = await import("./index.js");

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the shell's origin", () => {
  it("is where the page calls it: commands, window controls", async () => {
    const calls = stubFetch(() => okResponse(5));
    await invoke("add", 2, 3);
    await quit();
    expect(calls.map((c) => c.url)).toEqual([
      "http://elyra.localhost/__cmd/add",
      "http://elyra.localhost/__window/quit",
    ]);
  });
});
