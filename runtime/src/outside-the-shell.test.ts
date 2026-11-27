/**
 * A page outside the Elyra shell (a component reference in a browser) has no
 * `/__events`: subscribing to a channel asks for nothing, rather than a
 * request that fails every two seconds. In the shell, it pumps as ever.
 */
import { describe, expect, it } from "vitest";
import { stubFetch } from "./test-support.js";

delete (globalThis as Record<string, unknown>).__ELYRA__;
const { channel, word } = await import("./index.js");

describe("outside the shell", () => {
  it("doesn't wait on events that can't come", async () => {
    const calls = stubFetch(() => new Promise<Response>(() => {}) as unknown as Response);
    const stop = channel("elyra:about").subscribe(() => {});
    await new Promise((r) => setTimeout(r, 20));
    expect(calls.filter((c) => c.url.endsWith("/__events"))).toHaveLength(0);
    stop();
  });

  it("asks for no catalog for the framework's words", async () => {
    const calls = stubFetch(() => new Promise<Response>(() => {}) as unknown as Response);
    expect(word("elyra.modal.close", "Close")).toBe("Close");
    await new Promise((r) => setTimeout(r, 20));
    expect(calls.filter((c) => c.url.endsWith("/__i18n"))).toHaveLength(0);
  });

  it("does once the shell has said it's there", async () => {
    (globalThis as Record<string, unknown>).__ELYRA__ = { token: "t" };
    const calls = stubFetch(() => new Promise<Response>(() => {}) as unknown as Response);
    const stop = channel("elyra:fresh").subscribe(() => {});
    await new Promise((r) => setTimeout(r, 20));
    expect(calls.filter((c) => c.url.endsWith("/__events")).length).toBeGreaterThan(0);
    stop();
  });
});
