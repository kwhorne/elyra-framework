/**
 * The Theme Studio in a page (RFC 0037), from `@elyra/runtime`'s source: a
 * draft the shell hands a new window is drawn before anything else, the
 * tokens are read from the page's stylesheets, and a pair that falls short
 * is listed with a colour that would do, one click from being tried.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
// The runtime's own MessagePack, as the shell speaks it.
import { decode, encode } from "../../runtime/node_modules/@msgpack/msgpack/dist.esm/index.mjs";

let runtime;
let calls = [];
let draft = { looks: { dark: { "--elyra-accent": "#f97316" } } };

beforeAll(async () => {
  globalThis.__ELYRA__ = { token: "test-token" };
  // A debug build: the agent script, and the draft as the shell hands it.
  globalThis.__elyraAgent = {};
  globalThis.__ELYRA_THEME_DRAFT__ = draft;
  const tokens = document.createElement("style");
  tokens.textContent = `
    :root { --elyra-bg: #16161e; --elyra-surface: #1a1b26; --elyra-raised: #1e1f2b; --elyra-inset: #1f2030;
            --elyra-text: #c0caf5; --elyra-muted: #787c99; --elyra-border: #2a2b3d; --elyra-focus: #7aa2f7;
            --elyra-accent: #7aa2f7; --elyra-on-accent: #16161e; --elyra-danger: #f7768e;
            --elyra-success: #9ece6a; --elyra-warning: #e0af68; --elyra-radius: 8px; }
    :root[data-theme="light"] { --elyra-bg: #ffffff; --elyra-muted: #676b7b; }`;
  document.head.append(tokens);
  vi.stubGlobal("fetch", async (url, init) => {
    const body = init?.body ? decode(new Uint8Array(init.body)) : null;
    calls.push({ url: String(url), body });
    if (String(url).endsWith("/__inspector/theme")) {
      if (body?.token) {
        const look = draft.looks[body.look] ?? {};
        if (body.value === null) delete look[body.token];
        else look[body.token] = body.value;
        draft = { looks: { ...draft.looks, [body.look]: look } };
      }
      return new Response(encode(draft), { status: 200 });
    }
    return new Response(encode(true), { status: 200 });
  });
  runtime = await import("../../runtime/src/index.ts");
});

afterEach(() => {
  runtime.closeThemeStudio();
  calls = [];
});

describe("the theme draft", () => {
  it("is drawn in a new window before anything else, last in <head>", () => {
    const style = document.getElementById("elyra-theme-draft");
    expect(style.textContent).toBe(':root:not([data-theme="light"]) {\n  --elyra-accent: #f97316;\n}');
    expect(document.head.lastElementChild).toBe(style);
  });

  it("reads each look's tokens from the page's stylesheets, the draft over them", () => {
    const dark = runtime.themeTokens("dark", draft);
    expect(dark["--elyra-muted"]).toBe("#787c99");
    expect(dark["--elyra-accent"]).toBe("#f97316");
    const light = runtime.themeTokens("light", draft);
    expect(light["--elyra-bg"]).toBe("#ffffff");
    // The dark draft isn't light's.
    expect(light["--elyra-accent"]).toBe("#7aa2f7");
  });
});

describe("the studio", () => {
  it("lists what falls short with a colour that would do, and tries it in one click", async () => {
    await runtime.openThemeStudio();
    const panel = document.querySelector(".elyra-studio");
    expect(panel).toBeTruthy();
    const short = [...panel.querySelectorAll(".elyra-studio-check.short")].map((p) => p.textContent);
    expect(short[0]).toMatch(/^4 of \d+ pairs fall short$/);
    expect(short[1]).toMatch(/^--elyra-muted on --elyra-bg: 4\.40:1, needs 4\.5:1 — #[0-9a-f]{6} would do/);
    const use = [...panel.querySelectorAll("button")].find((b) => b.textContent === "Use it");
    use.click();
    await vi.waitFor(() => expect(calls.some((c) => c.body?.token === "--elyra-muted")).toBe(true));
    const sent = calls.find((c) => c.body?.token === "--elyra-muted").body;
    expect(sent.look).toBe("dark");
    expect(sent.value).toMatch(/^#[0-9a-f]{6}$/);
    await vi.waitFor(() =>
      expect(document.querySelector(".elyra-studio .elyra-studio-check").textContent).toMatch(/^All \d+ pairs are readable$/),
    );
  });

  it("sets the shape for every look", async () => {
    await runtime.openThemeStudio();
    const radius = document.querySelector('.elyra-studio input[aria-label="--elyra-radius"]');
    radius.value = "12";
    radius.dispatchEvent(new Event("input"));
    await vi.waitFor(() => expect(calls.some((c) => c.body?.token === "--elyra-radius")).toBe(true));
    expect(calls.find((c) => c.body?.token === "--elyra-radius").body).toEqual({
      look: "all",
      token: "--elyra-radius",
      value: "12px",
    });
  });
});
