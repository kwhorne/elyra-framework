// @vitest-environment node
/**
 * The design tokens (RFC 0012): both themes define the same colours, every
 * old name is an alias of a token, and every token @elyra/runtime's built-ins
 * read exists here.
 */
import { readdirSync, readFileSync } from "node:fs";
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
/** How a change moves: the same in both themes. */
const MOTION = ["--elyra-ease", "--elyra-ease-spring", "--elyra-duration-fast", "--elyra-duration", "--elyra-duration-slow"];
const DURATIONS = MOTION.filter((t) => t.includes("duration"));

/** The declarations of the first block `selector {` outside a media query. */
function plain(selector) {
  const start = css.indexOf(`${selector} {`) + selector.length + 2;
  const body = css.slice(start, css.indexOf("}", start));
  return Object.fromEntries([...body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}

/** The declarations of the block `selector {` inside `@media (query) {`. */
function inMedia(query, selector) {
  const media = css.indexOf(`@media (${query}) {`);
  const start = css.indexOf(`${selector} {`, media) + selector.length + 2;
  const body = css.slice(start, css.indexOf("}", start));
  return Object.fromEntries([...body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}

describe("tokens", () => {
  it("are all --elyra-* and themed alike", () => {
    expect(Object.keys(dark).every((t) => t.startsWith("--elyra-"))).toBe(true);
    const themed = Object.keys(dark).filter((t) => !SHAPE.includes(t) && !MOTION.includes(t)).sort();
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

  it("say how a change moves, and stand still for a user who asked for less motion", () => {
    for (const token of MOTION) expect(dark[token], token).toBeDefined();
    const reduced = inMedia("prefers-reduced-motion: reduce", ":root");
    expect(Object.keys(reduced).sort()).toEqual([...DURATIONS].sort());
    for (const token of DURATIONS) expect(reduced[token], token).toBe("0ms");
  });

  it("give more contrast, asked for in the OS or on <html>, alike", () => {
    const NAMES = ["--elyra-border", "--elyra-focus", "--elyra-muted"];
    const darkMore = inMedia("prefers-contrast: more", ':root:not([data-theme="light"])');
    const lightMore = inMedia("prefers-contrast: more", ':root[data-theme="light"]');
    expect(Object.keys(darkMore).sort()).toEqual(NAMES);
    expect(Object.keys(lightMore).sort()).toEqual(NAMES);
    // data-contrast="more" says the same, so the two can't drift.
    expect(plain(':root[data-contrast="more"]:not([data-theme="light"])')).toEqual(darkMore);
    expect(plain(':root[data-contrast="more"][data-theme="light"]')).toEqual(lightMore);
    // And it is more: none of them is the ordinary value.
    for (const name of NAMES) {
      expect(darkMore[name]).not.toBe(dark[name]);
      expect(lightMore[name]).not.toBe(light[name]);
    }
  });

  it("time every transition and animation, so less motion reaches them all", () => {
    const dir = new URL("./", import.meta.url);
    const own = readdirSync(dir).filter((f) => f.endsWith(".svelte"));
    // A spinner and a shimmer show that something is happening: they keep
    // their own pace, and slow down or stop under less motion.
    const loading = /animation: elyra-(spin|shimmer) /;
    let timed = 0;
    for (const file of own) {
      const source = readFileSync(new URL(file, dir), "utf8");
      for (const [line] of source.matchAll(/(?:transition|animation):[^;]*;/g)) {
        if (loading.test(line)) {
          expect(source, `${file} slows its ${line} for less motion`).toMatch(/prefers-reduced-motion: reduce/);
          continue;
        }
        expect(line, `${file}: ${line}`).not.toMatch(/\b\d*\.?\d+m?s\b/);
        timed += 1;
      }
    }
    expect(timed).toBeGreaterThan(10);
  });

  it("cover what the runtime's built-ins read", () => {
    const used = new Set([...runtime.matchAll(/var\((--elyra-[a-z0-9-]+)/g)].map((m) => m[1]));
    expect(used.size).toBeGreaterThan(0);
    for (const token of used) expect(dark[token], token).toBeDefined();
  });
});
