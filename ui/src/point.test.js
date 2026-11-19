/**
 * Pointing at part of a window for a coding agent (RFC 0035), in
 * `@elyra/runtime`: the overlay swallows the page's clicks, a rectangle
 * opens the note, and Enter hands the task to the shell with what the
 * agent script found under it. The runtime's source, in a DOM.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
// The runtime's own MessagePack, as the shell speaks it.
import { decode, encode } from "../../runtime/node_modules/@msgpack/msgpack/dist.esm/index.mjs";

let runtime;
let calls = [];
const answers = {
  abilities: [],
  point: { dictation: false },
  task: { id: 3, copied: true, markdown: "## UI task 3", file: ".elyra/tasks/3.md" },
};

beforeAll(async () => {
  globalThis.__ELYRA__ = { token: "test-token" };
  vi.stubGlobal("fetch", async (url, init) => {
    const op = String(url).split("/__inspector/")[1];
    calls.push({ url: String(url), body: init?.body ? decode(new Uint8Array(init.body)) : null });
    if (op in answers) return new Response(encode(answers[op]), { status: 200 });
    return new Response(encode(true), { status: 200 });
  });
  runtime = await import("../../runtime/src/index.ts");
});

afterEach(() => {
  runtime.stopPointing();
  document.body.innerHTML = "";
  calls = [];
  delete window.__elyraAgent;
});

function pointer(target, type, x, y) {
  target.dispatchEvent(new PointerEvent(type, { clientX: x, clientY: y, bubbles: true, pointerId: 1 }));
}

describe("pointing", () => {
  it("swallows the page's clicks, and a rectangle opens the note", async () => {
    const pressed = vi.fn();
    document.body.innerHTML = '<button id="b">Delete</button>';
    document.getElementById("b").addEventListener("click", pressed);
    await runtime.startPointing();
    const overlay = document.querySelector(".elyra-point");
    expect(overlay).toBeTruthy();
    overlay.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(pressed).not.toHaveBeenCalled();

    pointer(overlay, "pointerdown", 10, 20);
    pointer(overlay, "pointermove", 110, 70);
    pointer(overlay, "pointerup", 110, 70);
    const box = document.querySelector(".elyra-point-box");
    expect([box.style.left, box.style.top, box.style.width, box.style.height]).toEqual(["10px", "20px", "100px", "50px"]);
    expect(document.querySelector(".elyra-point-note textarea")).toBe(document.activeElement);
  });

  it("hands off the note, the rectangle and what's under it, after the overlay is gone", async () => {
    let seenOverlay = null;
    window.__elyraAgent = {
      describe: vi.fn((rect) => {
        seenOverlay = document.querySelector(".elyra-point");
        return { elements: [{ tree: '- button "Delete"', source: "src/App.svelte:3", stack: [] }], sources: true, rect };
      }),
    };
    await runtime.startPointing();
    const overlay = document.querySelector(".elyra-point");
    pointer(overlay, "pointerdown", 10, 20);
    pointer(overlay, "pointerup", 210, 120);
    const field = document.querySelector(".elyra-point-note textarea");
    field.value = "Make this a danger button";
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));

    await vi.waitFor(() => expect(calls.some((c) => c.url.endsWith("/__inspector/task"))).toBe(true));
    expect(seenOverlay).toBeNull();
    expect(window.__elyraAgent.describe).toHaveBeenCalledWith({ x: 10, y: 20, width: 200, height: 100 });
    const task = calls.find((c) => c.url.endsWith("/__inspector/task")).body;
    expect(task).toMatchObject({
      note: "Make this a danger button",
      rect: { x: 10, y: 20, width: 200, height: 100 },
      elements: [{ tree: '- button "Delete"', source: "src/App.svelte:3", stack: [] }],
      sources: true,
    });
    expect(task.viewport.width).toBe(window.innerWidth);
    await vi.waitFor(() =>
      expect(document.querySelector(".elyra-point-notice")?.textContent).toMatch(/^Task 3 copied for your agent/),
    );
  });

  it("is thrown away with Esc", async () => {
    await runtime.startPointing();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(document.querySelector(".elyra-point")).toBeNull();
    expect(calls.some((c) => c.url.endsWith("/__inspector/task"))).toBe(false);
  });

  it("starts while ⌘⌥ (Ctrl+Alt) is held, not for a shortcut or AltGr, and ends when they're let go", async () => {
    vi.useFakeTimers();
    try {
      const mac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
      const held = { altKey: true, metaKey: mac, ctrlKey: !mac, bubbles: true };
      /** A key event; `altGr` as Windows says AltGr (Ctrl+Alt) is down. */
      const key = (type, init, altGr = false) => {
        const e = new KeyboardEvent(type, init);
        Object.defineProperty(e, "getModifierState", { value: (k) => k === "AltGraph" && altGr });
        return e;
      };
      if (!mac) {
        // AltGr, typing `@` on a Norwegian keyboard: never pointing.
        document.dispatchEvent(key("keydown", { key: "Alt", ...held }, true));
        await vi.advanceTimersByTimeAsync(400);
        expect(document.querySelector(".elyra-point")).toBeNull();
      }
      document.dispatchEvent(key("keydown", { key: "Alt", ...held }));
      // A shortcut on its way (⌘⌥X): no crosshair.
      document.dispatchEvent(key("keydown", { key: "x", code: "KeyX", ...held }));
      await vi.advanceTimersByTimeAsync(400);
      expect(document.querySelector(".elyra-point")).toBeNull();

      document.dispatchEvent(key("keydown", { key: "Alt", ...held }));
      await vi.advanceTimersByTimeAsync(400);
      expect(document.querySelector(".elyra-point")).toBeTruthy();
      document.dispatchEvent(new KeyboardEvent("keyup", { key: "Alt", bubbles: true }));
      expect(document.querySelector(".elyra-point")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
