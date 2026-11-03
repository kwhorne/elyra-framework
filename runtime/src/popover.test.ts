/**
 * The tray's popover (RFC 0019): Esc hides it, unless the page handled it.
 * In a file of its own: whether this is a popover is the window's, for the
 * module's life (`__ELYRA_POPOVER__`, set by the shell before any script).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { okResponse, stubFetch, tick } from "./test-support.js";

type Listener = (event: KeyboardEvent) => void;
const listeners: Array<[string, Listener]> = [];
const scope = globalThis as Record<string, unknown>;
scope.__ELYRA__ = { token: "test-token" };
scope.__ELYRA_POPOVER__ = true;
scope.addEventListener = (type: string, listener: Listener) => listeners.push([type, listener]);

await import("./index.js");

function press(key: string, defaultPrevented = false): void {
  for (const [type, listener] of listeners) {
    if (type === "keydown") listener({ key, defaultPrevented } as KeyboardEvent);
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("Esc in the tray's popover", () => {
  it("hides it", async () => {
    const calls = stubFetch(() => okResponse(true));
    press("Escape");
    await tick();
    expect(calls.map((c) => c.url)).toEqual(["elyra://localhost/__window/hide"]);
  });

  it("leaves it when the page handled it, or for another key", async () => {
    const calls = stubFetch(() => okResponse(true));
    press("Escape", true);
    press("a");
    await tick();
    expect(calls).toEqual([]);
  });
});
