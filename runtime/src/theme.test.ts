/**
 * The Theme Studio's pieces that need no page (RFC 0037): WCAG contrast,
 * the checks of a look, and the draft as the windows' stylesheet and as
 * blocks for app.css.
 */
import { describe, expect, it } from "vitest";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
const { themeAppCss, themeChecks, themeContrast, themeDraftCss } = await import("./index.js");

const DARK = {
  "--elyra-bg": "#16161e",
  "--elyra-surface": "#1a1b26",
  "--elyra-raised": "#1e1f2b",
  "--elyra-inset": "#1f2030",
  "--elyra-text": "#c0caf5",
  "--elyra-muted": "#787c99",
  "--elyra-border": "#2a2b3d",
  "--elyra-focus": "#7aa2f7",
  "--elyra-accent": "#7aa2f7",
  "--elyra-on-accent": "#16161e",
  "--elyra-danger": "#f7768e",
  "--elyra-success": "#9ece6a",
  "--elyra-warning": "#e0af68",
};

describe("theme studio", () => {
  it("measures contrast as WCAG does, and says when it can't", () => {
    expect(themeContrast("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(themeContrast("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
    expect(themeContrast("#787c99", "#1f2030")).toBeCloseTo(3.93, 2);
    expect(themeContrast("var(--x)", "#fff")).toBeNull();
  });

  it("checks a look's pairs, the shortfalls first, each with a colour that would do", () => {
    const checks = themeChecks(DARK, false);
    const short = checks.filter((c) => c.ratio !== null && c.ratio < c.needs);
    expect(short.map((c) => `${c.token} on ${c.on}`)).toEqual([
      "--elyra-muted on --elyra-bg",
      "--elyra-muted on --elyra-surface",
      "--elyra-muted on --elyra-raised",
      "--elyra-muted on --elyra-inset",
    ]);
    expect(checks.slice(0, 4)).toEqual(short);
    const fix = short[0]!.nearest!;
    for (const surface of ["--elyra-bg", "--elyra-surface", "--elyra-raised", "--elyra-inset"]) {
      expect(themeContrast(fix, DARK[surface as keyof typeof DARK])).toBeGreaterThanOrEqual(4.5);
    }
    // Under more contrast, borders and 7:1 text count too.
    const more = themeChecks(DARK, true);
    expect(more.some((c) => c.token === "--elyra-border" && c.needs === 3)).toBe(true);
    expect(more.find((c) => c.token === "--elyra-text")!.needs).toBe(7);
  });

  it("draws the draft for every look, and gives app.css only what changed", () => {
    const draft = {
      looks: {
        all: { "--elyra-radius": "10px" },
        dark: { "--elyra-accent": "#f97316" },
        light: { "--elyra-accent": "#c2410c" },
        "light-more": { "--elyra-muted": "#30333d" },
      },
    };
    const css = themeDraftCss(draft);
    expect(css).toContain(":root {\n  --elyra-radius: 10px;\n}");
    expect(css).toContain(':root:not([data-theme="light"]) {\n  --elyra-accent: #f97316;\n}');
    expect(css).toContain(':root[data-theme="light"] {\n  --elyra-accent: #c2410c;\n}');
    expect(css).toContain(':root[data-contrast="more"][data-theme="light"] {\n  --elyra-muted: #30333d;\n}');
    expect(css).toContain('@media (prefers-contrast: more) {\n  :root[data-theme="light"] {\n    --elyra-muted: #30333d;\n  }\n}');

    const app = themeAppCss(draft, 0);
    expect(app.startsWith("/* From the Theme Studio (RFC 0037): every pair it checks is readable. */\n")).toBe(true);
    // Dark is app.css's :root, as tokens.css's is.
    expect(app).toContain(":root {\n  --elyra-radius: 10px;\n  --elyra-accent: #f97316;\n}");
    expect(app).not.toContain(':root:not([data-theme="light"])');
    expect(themeAppCss({ looks: {} }, 0)).toBe("");
    expect(themeAppCss(draft, 2)).toContain("2 pair(s) fall short");
  });
});
