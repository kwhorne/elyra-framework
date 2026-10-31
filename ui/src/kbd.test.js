/** Kbd: Elyra's accelerators as the platform writes them, and as a screen reader says them. */
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/svelte";
import Kbd, { ariaShortcut } from "./Kbd.svelte";

const drawn = (container) => [...container.querySelectorAll(".drawn kbd")].map((k) => k.textContent);
const spoken = (container) => container.querySelector(".spoken").textContent;
const plain = (container) => container.querySelector(".drawn").textContent;

describe("Kbd", () => {
  it("writes CmdOrCtrl as ⌘ on a Mac, with the symbols and no plus", () => {
    const { container } = render(Kbd, { keys: "CmdOrCtrl+Shift+p", platform: "mac" });
    expect(drawn(container)).toEqual(["⌘", "⇧", "P"]);
    expect(plain(container)).toBe("⌘⇧P");
    expect(spoken(container)).toBe("Command Shift P");
  });

  it("writes it as Ctrl elsewhere, joined by plus", () => {
    const { container } = render(Kbd, { keys: "CmdOrCtrl+Shift+P", platform: "other" });
    expect(drawn(container)).toEqual(["Ctrl", "Shift", "P"]);
    expect(plain(container)).toBe("Ctrl+Shift+P");
    expect(spoken(container)).toBe("Control Shift P");
  });

  it("knows the other modifiers and the named keys", () => {
    const mac = render(Kbd, { keys: ["Ctrl", "Alt", "Cmd", "Enter", "Esc", "Backspace", "Tab", "Up", "ArrowLeft", "Space"], platform: "mac" });
    expect(drawn(mac.container)).toEqual(["⌃", "⌥", "⌘", "↩", "Esc", "⌫", "⇥", "↑", "←", "Space"]);
    expect(spoken(mac.container)).toBe("Control Option Command Enter Escape Backspace Tab Up Left Space");
    mac.unmount();
    const pc = render(Kbd, { keys: "Option+Meta+Delete+PageDown", platform: "other" });
    expect(drawn(pc.container)).toEqual(["Alt", "Win", "Del", "PgDn"]);
    expect(spoken(pc.container)).toBe("Alt Windows Delete Page Down");
  });

  it("takes a list, any case, and keys it doesn't know as written", () => {
    const { container } = render(Kbd, { keys: ["shift", "F5"], platform: "other" });
    expect(drawn(container)).toEqual(["Shift", "F5"]);
  });

  it("has the plus key itself", () => {
    const one = render(Kbd, { keys: "CmdOrCtrl++", platform: "other" });
    expect(drawn(one.container)).toEqual(["Ctrl", "+"]);
    one.unmount();
    const alone = render(Kbd, { keys: "+", platform: "mac" });
    expect(drawn(alone.container)).toEqual(["+"]);
  });

  it("follows the platform the WebView reports", () => {
    const was = Object.getOwnPropertyDescriptor(globalThis.navigator, "platform");
    Object.defineProperty(globalThis.navigator, "platform", { configurable: true, value: "MacIntel" });
    try {
      const mac = render(Kbd, { keys: "CmdOrCtrl+K" });
      expect(plain(mac.container)).toBe("⌘K");
      mac.unmount();
      Object.defineProperty(globalThis.navigator, "platform", { configurable: true, value: "Win32" });
      const pc = render(Kbd, { keys: "CmdOrCtrl+K" });
      expect(plain(pc.container)).toBe("Ctrl+K");
      pc.unmount();
      // The newer userAgentData, where there is one, is asked first.
      Object.defineProperty(globalThis.navigator, "userAgentData", { configurable: true, value: { platform: "macOS" } });
      const both = render(Kbd, { keys: "CmdOrCtrl+K" });
      expect(plain(both.container)).toBe("⌘K");
    } finally {
      if (was) Object.defineProperty(globalThis.navigator, "platform", was);
      else delete globalThis.navigator.platform;
      delete globalThis.navigator.userAgentData;
    }
  });

  it("writes aria-keyshortcuts as ARIA does", () => {
    expect(ariaShortcut("CmdOrCtrl+Shift+p", true)).toBe("Meta+Shift+P");
    expect(ariaShortcut("CmdOrCtrl+Shift+p", false)).toBe("Control+Shift+P");
    expect(ariaShortcut(["Option", "Up"], false)).toBe("Alt+ArrowUp");
    expect(ariaShortcut("Ctrl+Esc+F5", true)).toBe("Control+Escape+F5");
  });

  it("is one kbd around a kbd per key, the drawn ones hidden from screen readers", () => {
    const { container } = render(Kbd, { keys: "Shift+Enter", platform: "other", class: "hint", title: "Send" });
    const outer = container.querySelector("kbd.elyra-kbd");
    expect(outer.classList.contains("hint")).toBe(true);
    expect(outer.getAttribute("title")).toBe("Send");
    expect(outer.querySelector(".drawn").getAttribute("aria-hidden")).toBe("true");
  });
});
