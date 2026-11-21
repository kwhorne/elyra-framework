/**
 * The Theme Studio's pieces that need no page (RFC 0037): WCAG contrast,
 * the checks of a look, and the draft as the windows' stylesheet and as
 * blocks for app.css.
 */
import { describe, expect, it } from "vitest";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
const { themeAppCss, themeChecks, themeContrast, themeDraftCss, themeTokens } = await import("./index.js");

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

  it("draws the draft for every look, and gives app.css the same rules", () => {
    const draft = {
      looks: {
        all: { "--elyra-radius": "10px" },
        dark: { "--elyra-accent": "#f97316" },
        light: { "--elyra-accent": "#c2410c", "--elyra-muted": "#222222" },
        "light-more": { "--elyra-muted": "#30333d" },
      },
    };
    const more = new Set(["--elyra-muted"]);
    const css = themeDraftCss(draft, more);
    expect(css).toContain(":root {\n  --elyra-radius: 10px;\n}");
    // Dark's only, and light's: not each other's.
    expect(css).toContain(':root:not([data-theme="light"]) {\n  --elyra-accent: #f97316;\n}');
    expect(css).toContain(':root[data-theme="light"] {\n  --elyra-accent: #c2410c;\n}');
    // A token more contrast sets: light's only without it.
    expect(css).toContain(
      '@media not all and (prefers-contrast: more) {\n  :root[data-theme="light"]:not([data-contrast="more"]) {\n    --elyra-muted: #222222;\n  }\n}',
    );
    expect(css).toContain(':root[data-contrast="more"][data-theme="light"] {\n  --elyra-muted: #30333d;\n}');
    expect(css).toContain('@media (prefers-contrast: more) {\n  :root[data-theme="light"] {\n    --elyra-muted: #30333d;\n  }\n}');

    const app = themeAppCss(draft, 0, more);
    expect(app.startsWith("/* From the Theme Studio (RFC 0037): every pair it checks is readable. */\n")).toBe(true);
    // What the windows drew is what app.css gets.
    expect(app).toBe(`/* From the Theme Studio (RFC 0037): every pair it checks is readable. */\n${css}\n`);
    expect(themeAppCss({ looks: {} }, 0, more)).toBe("");
    expect(themeAppCss(draft, 2, more)).toContain("2 pair(s) fall short");
  });

  it("resolves a look's tokens as the cascade does, with the draft as the windows draw it", () => {
    const rule = (selectorText: string, tokens: Record<string, string>) => {
      const names = Object.keys(tokens);
      const style = Object.assign([...names], { getPropertyValue: (name: string) => ` ${tokens[name]}` });
      return { selectorText, style };
    };
    const media = (mediaText: string, rules: unknown[]) => ({ media: { mediaText }, cssRules: rules });
    // tokens.css, then an app.css that sets a token on :root after it.
    const tokensCss = [
      rule(":root", { "--elyra-muted": "#787c99", "--elyra-accent": "#7aa2f7" }),
      rule(':root[data-theme="light"]', { "--elyra-muted": "#6b6f80", "--elyra-accent": "#2e5cb8" }),
      media("(prefers-contrast: more)", [
        rule(':root:not([data-theme="light"])', { "--elyra-muted": "#a9b1d6" }),
        rule(':root[data-theme="light"]', { "--elyra-muted": "#3b3e4a" }),
      ]),
      rule(':root[data-contrast="more"]:not([data-theme="light"])', { "--elyra-muted": "#a9b1d6" }),
      rule(':root[data-contrast="more"][data-theme="light"]', { "--elyra-muted": "#3b3e4a" }),
    ];
    const appCss = [rule(":root", { "--elyra-muted": "#999999" })];
    (globalThis as Record<string, unknown>).document = {
      styleSheets: [
        { ownerNode: null, cssRules: tokensCss },
        { ownerNode: null, cssRules: appCss },
      ],
    };
    try {
      // app.css's :root is later than tokens.css's, but light's is more specific.
      expect(themeTokens("dark")["--elyra-muted"]).toBe("#999999");
      expect(themeTokens("light")["--elyra-muted"]).toBe("#6b6f80");
      expect(themeTokens("dark-more")["--elyra-muted"]).toBe("#a9b1d6");

      // A dark change isn't light's, though light doesn't set it.
      expect(themeTokens("light", { looks: { dark: { "--elyra-radius": "2px" } } })["--elyra-radius"]).toBeUndefined();
      const draft = {
        looks: {
          dark: { "--elyra-muted": "#111111", "--elyra-accent": "#f97316" },
          light: { "--elyra-muted": "#222222", "--elyra-accent": "#c2410c" },
        },
      };
      expect(themeTokens("dark", draft)).toMatchObject({ "--elyra-muted": "#111111", "--elyra-accent": "#f97316" });
      // More contrast keeps its own muted, as the windows and app.css draw it.
      expect(themeTokens("dark-more", draft)).toMatchObject({ "--elyra-muted": "#a9b1d6", "--elyra-accent": "#f97316" });
      expect(themeTokens("light", draft)).toMatchObject({ "--elyra-muted": "#222222", "--elyra-accent": "#c2410c" });
      expect(themeTokens("light-more", draft)).toMatchObject({ "--elyra-muted": "#3b3e4a", "--elyra-accent": "#c2410c" });
      // Unless more contrast is changed itself.
      const more = { looks: { ...draft.looks, "dark-more": { "--elyra-muted": "#eeeeee" } } };
      expect(themeTokens("dark-more", more)["--elyra-muted"]).toBe("#eeeeee");
    } finally {
      delete (globalThis as Record<string, unknown>).document;
    }
  });
});
