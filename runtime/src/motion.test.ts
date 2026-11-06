/**
 * The runtime's own pieces (the About dialog, dialogs, toasts, ⌘K, the
 * context menu, the update toast, the assistant panel and the inspector)
 * move as @elyra/ui does: timed by its motion tokens, and still for a user
 * who asked for less motion, with or without `@elyra/ui/tokens.css`.
 */
// vitest runs this in Node; the runtime itself has no Node types.
// @ts-expect-error -- no @types/node in the runtime
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source: string = readFileSync(new URL("./index.ts", import.meta.url), "utf8");

/** Each stylesheet the runtime injects: a template literal of CSS rules. */
const sheets = [...source.matchAll(/`\n(\.elyra-[\s\S]*?)\n`;/g)].map((m) => m[1]);

/** What a blinking caret does to say "still writing": its own pace, stopped
 * for less motion. */
const PACED = ["elyra-caret"];

describe("the runtime's motion", () => {
  it("is timed by the motion tokens, with a fallback for an app without them", () => {
    const timed = [...source.matchAll(/(?:animation|transition):([^;}]*)/g)].map((m) => m[1]);
    expect(timed.length).toBeGreaterThan(8);
    for (const value of timed) {
      if (PACED.some((name) => value.includes(name)) || value.trim() === "none") continue;
      expect(value, value).toMatch(/var\(--elyra-duration(-fast|-slow)?,\d+ms\)/);
      expect(value, value).not.toMatch(/(^|\s)\.?\d+m?s\b(?![^(]*\))/);
    }
  });

  it("stops in every sheet that moves, for a user who asked for less motion", () => {
    const moving = sheets.filter((sheet) => /animation:(?!none)/.test(sheet) || /transition:(?!none)/.test(sheet));
    expect(moving.length).toBeGreaterThanOrEqual(5);
    for (const sheet of moving) {
      const reduced = sheet.match(/@media \(prefers-reduced-motion:reduce\)\{(.*)\}$/m)?.[1];
      expect(reduced, sheet.slice(0, 80)).toBeDefined();
      // The selectors the reduced-motion rule stops, each one whole.
      const stopped = new Set(
        [...(reduced ?? "").matchAll(/([^{}]+)\{[^}]*\}/g)].flatMap((m) => m[1].split(",").map((s: string) => s.trim())),
      );
      // Every animated selector is in its sheet's reduced-motion rule.
      for (const [, selectors] of sheet.matchAll(/^([^@{}\n]+)\{[^}]*animation:(?!none)[^}]*\}/gm)) {
        for (const selector of selectors.split(",")) {
          expect(stopped.has(selector.trim()), `${selector.trim()} keeps moving`).toBe(true);
        }
      }
    }
  });
});
