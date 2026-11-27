// @vitest-environment happy-dom
/** Printing outside the shell (RFC 0062): the browser's own print, and the page put back after it. */
import { describe, expect, it, vi } from "vitest";

const browser = vi.fn(() => {
  // A browser's print blocks while its dialog is up: the page is ready for paper.
  expect(document.documentElement.dataset.theme).toBe("light");
});
window.print = browser;
const { print } = await import("./index.js");

describe("print outside the shell", () => {
  it("is the browser's, and leaves window.print alone", async () => {
    document.documentElement.dataset.theme = "dark";
    await print();
    expect(browser).toHaveBeenCalledTimes(1);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.hasAttribute("data-elyra-printing")).toBe(false);
    expect(window.print).toBe(browser);
  });
});
