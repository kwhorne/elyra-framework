/**
 * The design tokens (RFC 0012): both themes define the same colours, every
 * old name is an alias of a token, and every token @elyra/runtime's built-ins
 * read exists here.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
const runtime = readFileSync(new URL("../../runtime/src/index.ts", import.meta.url), "utf8");

/** The declarations in the block that starts at `selector {`, by name. */
function block(selector, nth = 0) {
  const starts = [...css.matchAll(new RegExp(`${selector.replace(/[[\]"]/g, "\\$&")}\\s*\\{`, "g"))];
  const start = starts[nth].index + starts[nth][0].length;
  const body = css.slice(start, css.indexOf("}", start));
  return Object.fromEntries(
    [...body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
  );
}

const dark = block(":root", 0);
const light = block(':root[data-theme="light"]');
const aliases = block(":root", 1);
/** What doesn't change with the theme. */
const SHAPE = ["--elyra-radius", "--elyra-radius-lg", "--elyra-space", "--elyra-font", "--elyra-font-mono", "--elyra-font-size"];

describe("tokens", () => {
  it("are all --elyra-* and themed alike", () => {
    expect(Object.keys(dark).every((t) => t.startsWith("--elyra-"))).toBe(true);
    const themed = Object.keys(dark).filter((t) => !SHAPE.includes(t)).sort();
    expect(Object.keys(light).sort()).toEqual(themed);
  });

  it("keep the old names as aliases of a token", () => {
    expect(Object.keys(aliases).sort()).toEqual(
      ["--accent", "--accent-2", "--amber", "--bg", "--bg-2", "--bg-3", "--border", "--font-mono",
        "--font-ui", "--green", "--muted", "--panel", "--red", "--surface", "--text", "--text-dim"].sort(),
    );
    for (const [name, value] of Object.entries(aliases)) {
      const target = value.match(/^var\((--elyra-[a-z0-9-]+)\)$/)?.[1];
      expect(target, `${name}: ${value}`).toBeDefined();
      expect(dark[target], `${name} -> ${target}`).toBeDefined();
    }
  });

  it("cover what the runtime's built-ins read", () => {
    const used = new Set([...runtime.matchAll(/var\((--elyra-[a-z0-9-]+)/g)].map((m) => m[1]));
    expect(used.size).toBeGreaterThan(0);
    for (const token of used) expect(dark[token], token).toBeDefined();
  });
});
