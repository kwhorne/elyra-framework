/**
 * The runtime's release (RFC 0057): the one `package.json` has, and a
 * warning when a debug build's shell is another.
 */
import { describe, expect, it } from "vitest";
import pkg from "../package.json";

const { VERSION, versionMismatch } = await import("./index.js");

describe("the runtime's version", () => {
  it("is package.json's", () => {
    expect(VERSION).toBe(pkg.version);
  });

  it("says when the shell is another release, and how to fix it", () => {
    expect(versionMismatch(undefined)).toBeNull();
    expect(versionMismatch(VERSION)).toBeNull();
    expect(versionMismatch("9.9.9", "0.58.0")).toBe(
      "@elyra/runtime is 0.58.0, the app's elyra crate 9.9.9: the page and the shell may not agree. " +
        "Run `rata upgrade 9.9.9`, then `rata check`.",
    );
  });
});
