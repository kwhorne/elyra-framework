/**
 * Outside the tray's popover (RFC 0019), Esc is the page's: the runtime
 * doesn't listen for it. In a file of its own, as `popover.test.ts`.
 */
import { describe, expect, it } from "vitest";

const types: string[] = [];
const scope = globalThis as Record<string, unknown>;
scope.__ELYRA__ = { token: "test-token" };
scope.addEventListener = (type: string) => types.push(type);

await import("./index.js");

describe("Esc in another window", () => {
  it("isn't listened for", () => {
    expect(types).not.toContain("keydown");
  });
});
