/**
 * The built-ins (dialogs, toasts, the palette, the assistant panel) read
 * Elyra's design tokens (RFC 0012): every colour is an `--elyra-*` token
 * first, with the names apps used before and a default as fallbacks, so an
 * app on the old names keeps its look.
 */
/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";
import source from "./index.ts?raw";

describe("the built-ins' styles", () => {
  it("read an --elyra-* token first, with a fallback", () => {
    const tokens = [...source.matchAll(/var\(--elyra-[a-z0-9-]+(.)/g)];
    expect(tokens.length).toBeGreaterThan(50);
    for (const m of tokens) expect(m[1], m[0]).toBe(",");
  });

  it("use the old names only as fallbacks", () => {
    for (const m of source.matchAll(/(.)var\(--(?!elyra-)[a-z0-9-]+/g)) {
      expect(m[1], `${m[0]} at ${m.index}`).toBe(",");
    }
  });
});
