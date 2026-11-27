// @vitest-environment happy-dom
/**
 * The context menu (RFC 0060 opens it from the keyboard): a menu to a screen
 * reader, operable with the keys, focus going back where it was.
 */
import { describe, expect, it, vi } from "vitest";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
const { contextMenu } = await import("./index.js");

const key = (k: string) => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));

describe("contextMenu", () => {
  it("is a menu the keyboard works, and gives focus back", () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const chosen: string[] = [];
    contextMenu(new MouseEvent("contextmenu", { clientX: 10, clientY: 10 }), [
      { label: "Copy", action: () => void chosen.push("copy") },
      { label: "Off", disabled: true },
      { separator: true },
      { label: "Save", action: () => void chosen.push("save") },
    ]);
    const menu = document.querySelector('[role="menu"]')!;
    expect([...menu.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent)).toEqual(["Copy", "Off", "Save"]);
    expect(menu.querySelector('[aria-disabled="true"]')!.textContent).toBe("Off");
    expect(document.activeElement!.textContent).toBe("Copy");
    key("ArrowDown"); // past the disabled one
    expect(document.activeElement!.textContent).toBe("Save");
    key("ArrowDown"); // round again
    expect(document.activeElement!.textContent).toBe("Copy");
    key("End");
    key("Enter");
    expect(chosen).toEqual(["save"]);
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("closes on Esc without choosing", () => {
    const action = vi.fn();
    contextMenu(new MouseEvent("contextmenu"), [{ label: "Copy", action }]);
    key("Escape");
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(action).not.toHaveBeenCalled();
  });
});
