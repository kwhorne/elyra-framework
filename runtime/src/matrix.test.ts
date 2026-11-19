/**
 * A frame of the matrix (RFC 0034): its page keeps the look it was given,
 * follows the route of the window it came from, and opens no matrix of its
 * own. In a file of its own: being a frame is the window's, for the
 * module's life (`__ELYRA_FRAME__`, set by the shell before any script).
 */
import { describe, expect, it, vi } from "vitest";
import { encode } from "@msgpack/msgpack";
import { nthCall, stubPendingFetch, tick } from "./test-support.js";

const scope = globalThis as Record<string, unknown>;
scope.__ELYRA__ = { token: "test-token" };
scope.__ELYRA_FRAME__ = {
  name: "Can't delete",
  theme: "light",
  contrast: true,
  abilities: { deny: ["*.delete"] },
};
const attributes = new Map<string, string>([["data-theme", "dark"]]);
const keydown: Array<(e: KeyboardEvent) => void> = [];
scope.document = {
  documentElement: {
    getAttribute: (name: string) => attributes.get(name) ?? null,
    setAttribute: (name: string, value: string) => attributes.set(name, value),
  },
  addEventListener: (type: string, listener: (e: KeyboardEvent) => void) => {
    if (type === "keydown") keydown.push(listener);
  },
  readyState: "complete",
};
scope.location = { hash: "#/customers" };
scope.addEventListener = () => {};

const pending = stubPendingFetch();
const { matrixFrame } = await import("./index.js");

describe("a frame of the matrix", () => {
  it("has its look on <html>, whatever was there, and knows its name", () => {
    expect(attributes.get("data-theme")).toBe("light");
    expect(attributes.get("data-contrast")).toBe("more");
    expect(matrixFrame()).toEqual({
      name: "Can't delete",
      theme: "light",
      contrast: true,
      abilities: { deny: ["*.delete"] },
    });
  });

  it("goes where the window it came from goes", async () => {
    const poll = await nthCall(pending, 1);
    expect(poll.url).toMatch(/\/__events/);
    poll.resolve(new Response(encode([["elyra:matrix-route", { route: "/customers/7?tab=notes" }]])));
    await vi.waitFor(() => expect((scope.location as { hash: string }).hash).toBe("#/customers/7?tab=notes"));
  });

  it("opens no matrix of its own", async () => {
    const before = pending.length;
    const prevented: boolean[] = [];
    for (const listener of keydown) {
      listener({
        key: "E",
        code: "KeyE",
        metaKey: true,
        altKey: true,
        shiftKey: true,
        ctrlKey: false,
        preventDefault: () => prevented.push(true),
      } as unknown as KeyboardEvent);
    }
    await tick();
    expect(prevented).toEqual([true]);
    expect(pending.slice(before).filter((c) => c.url.includes("/__inspector/"))).toEqual([]);
  });
});
