// @vitest-environment happy-dom
/**
 * Printing (RFC 0062): the shell's print dialog for the window, the page in
 * the light theme while it prints, a part of it on its own, `window.print()`
 * and File → Print… routed to it.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode } from "@msgpack/msgpack";
import { errorResponse, okResponse, stubFetch, tick } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
// The webview's own print, before the runtime takes the name.
const browser = vi.fn();
window.print = browser;
const { print, onPrint, pdf } = await import("./index.js");

const html = document.documentElement;
const menuPrint = () => (globalThis as { __ELYRA_PRINT__?: () => void }).__ELYRA_PRINT__!();
const printed = (calls: { url: string }[]) => calls.filter((c) => c.url.endsWith("/__window/print")).length;

afterEach(() => {
  window.dispatchEvent(new Event("afterprint"));
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
  html.removeAttribute("data-theme");
  html.removeAttribute("data-print-theme");
});

describe("print", () => {
  it("asks the shell for the window's print dialog, in the light theme until it's done", async () => {
    const calls = stubFetch(() => okResponse(true));
    const seen: string[] = [];
    window.addEventListener("elyra:beforeprint", () => seen.push(`before ${html.dataset.theme}`), { once: true });
    window.addEventListener("elyra:afterprint", () => seen.push(`after ${html.dataset.theme}`), { once: true });
    html.dataset.theme = "dark";
    await print();
    expect(calls.map((c) => [c.url, c.init?.method])).toEqual([["elyra://localhost/__window/print", "POST"]]);
    expect(html.dataset.theme).toBe("light");
    expect(html.hasAttribute("data-elyra-printing")).toBe(true);
    await tick();
    window.dispatchEvent(new Event("afterprint"));
    expect(html.dataset.theme).toBe("dark");
    expect(html.hasAttribute("data-elyra-printing")).toBe(false);
    expect(seen).toEqual(["before light", "after dark"]);
  });

  it("a theme with no attribute goes back to none; data-print-theme=current keeps it", async () => {
    stubFetch(() => okResponse(true));
    await print();
    await tick();
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(html.hasAttribute("data-theme")).toBe(false);
    html.dataset.printTheme = "current";
    await print();
    expect(html.hasAttribute("data-theme")).toBe(false);
  });

  it("a part of the page: it and its way up marked, the rest hidden by the print sheet", async () => {
    stubFetch(() => okResponse(true));
    document.body.innerHTML = `<nav>Menu</nav><main><aside>Side</aside><article id="invoice"><h1>Invoice</h1></article></main>`;
    const invoice = document.getElementById("invoice")!;
    await print({ element: invoice });
    expect(invoice.hasAttribute("data-elyra-print-target")).toBe(true);
    expect([...document.querySelectorAll("[data-elyra-print-path]")].map((e) => e.tagName)).toEqual(["HTML", "BODY", "MAIN"]);
    expect(html.hasAttribute("data-elyra-print-part")).toBe(true);
    const sheet = document.getElementById("elyra-print")!.textContent!;
    expect(sheet).toContain('[data-print="hide"]');
    expect(sheet).toContain("[data-elyra-print-path] > :not([data-elyra-print-path]):not([data-elyra-print-target])");
    await tick();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true }));
    expect(document.querySelectorAll("[data-elyra-print-path], [data-elyra-print-target]")).toHaveLength(0);
    expect(html.hasAttribute("data-elyra-print-part")).toBe(false);
  });

  it("puts the page back when the shell can't print", async () => {
    stubFetch(() => errorResponse("capability `window` not granted", "denied", 403));
    html.dataset.theme = "dark";
    await expect(print()).rejects.toThrow(/print.*not granted/);
    expect(html.dataset.theme).toBe("dark");
    expect(html.hasAttribute("data-elyra-printing")).toBe(false);
  });

  it("a second print undoes the first's marks", async () => {
    stubFetch(() => okResponse(true));
    document.body.innerHTML = `<p id="a">A</p><p id="b">B</p>`;
    await print({ element: document.getElementById("a") });
    await print({ element: document.getElementById("b") });
    expect([...document.querySelectorAll("[data-elyra-print-target]")].map((e) => e.id)).toEqual(["b"]);
  });
});

describe("window.print() and File → Print…", () => {
  it("window.print() is the shell's; the browser's own is kept for WebView2's dialog", async () => {
    const calls = stubFetch(() => okResponse(true));
    window.print();
    await vi.waitFor(() => expect(printed(calls)).toBe(1));
    const kept = (globalThis as { __ELYRA_BROWSER_PRINT__?: unknown }).__ELYRA_BROWSER_PRINT__;
    (kept as () => void)();
    expect(browser).toHaveBeenCalledTimes(1);
    expect(kept).not.toBe(window.print);
  });

  it("Print… prints the window, unless the page says what", async () => {
    const calls = stubFetch(() => okResponse(true));
    menuPrint();
    await vi.waitFor(() => expect(printed(calls)).toBe(1));
    const asked: string[] = [];
    const off = onPrint(() => asked.push("invoice"));
    menuPrint();
    await tick(20);
    expect(asked).toEqual(["invoice"]);
    expect(printed(calls)).toBe(1);
    off();
    menuPrint();
    await vi.waitFor(() => expect(printed(calls)).toBe(2));
  });
});

describe("pdf", () => {
  const PDF = new TextEncoder().encode("%PDF-1.7 …");
  const answer = () =>
    new Response(PDF, { status: 200, headers: { "content-type": "application/pdf", "x-elyra-status": "ok" } });

  it("asks the shell for the page as a PDF, the page ready for paper while it's made", async () => {
    let during: string[] = [];
    const calls = stubFetch(() => {
      during = [html.dataset.theme ?? "", String(document.getElementById("invoice")?.hasAttribute("data-elyra-print-target"))];
      return answer();
    });
    document.body.innerHTML = `<main><article id="invoice">Invoice</article></main>`;
    html.dataset.theme = "dark";
    const bytes = await pdf({ element: document.getElementById("invoice"), paper: "letter", landscape: true, margins: { top: 20, left: undefined } });
    expect(new TextDecoder().decode(bytes)).toBe("%PDF-1.7 …");
    expect(calls.map((c) => c.url)).toEqual(["elyra://localhost/__window/pdf"]);
    expect(decode(calls[0]!.init!.body as Uint8Array)).toEqual({ paper: "letter", landscape: true, margins: { top: 20 } });
    expect(during).toEqual(["light", "true"]);
    // Made: the page is as it was at once, no dialog to wait for.
    expect(html.dataset.theme).toBe("dark");
    expect(document.querySelectorAll("[data-elyra-print-target]")).toHaveLength(0);
  });

  it("sends nothing it wasn't told, and says why the shell made none", async () => {
    const calls = stubFetch(() => errorResponse("the PDF took too long"));
    await expect(pdf()).rejects.toThrow(/pdf failed: the PDF took too long/);
    expect(decode(calls[0]!.init!.body as Uint8Array)).toEqual({});
    expect(html.hasAttribute("data-elyra-printing")).toBe(false);
  });
});
