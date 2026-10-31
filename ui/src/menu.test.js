/** Dropdown and MenuItem: a menu button, the keyboard, abilities. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import MenuFixture from "./test/MenuFixture.svelte";
import { grant } from "./test/runtime.js";

describe("Dropdown", () => {
  it("opens a menu from the keyboard, moves through it, and closes", async () => {
    render(MenuFixture, { onedit: vi.fn(), ondelete: vi.fn() });
    const button = screen.getByRole("button", { name: "Actions" });
    expect(button.getAttribute("aria-haspopup")).toBe("menu");
    button.focus();
    await userEvent.keyboard("{Enter}");
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("menu").getAttribute("aria-labelledby")).toBe(button.id);
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Edit" }));
    // Archive is disabled: skipped.
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement.textContent.trim()).toBe("Export");
    await userEvent.keyboard("{ArrowUp}{ArrowUp}");
    expect(document.activeElement.textContent.trim()).toBe("Delete");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it("runs a choice and closes", async () => {
    const onedit = vi.fn();
    render(MenuFixture, { onedit, ondelete: vi.fn() });
    await userEvent.click(screen.getByRole("button", { name: "Actions" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(onedit).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("hides, or disables, what the user may not do", async () => {
    grant(["customers.view"]);
    const ondelete = vi.fn();
    render(MenuFixture, { onedit: vi.fn(), ondelete });
    await userEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(screen.queryByRole("menuitem", { name: "Export" })).toBeNull();
    const remove = screen.getByRole("menuitem", { name: "Delete" });
    expect(remove.disabled).toBe(true);
    await userEvent.click(remove);
    expect(ondelete).not.toHaveBeenCalled();
  });

  it("closes on a click elsewhere", async () => {
    render(MenuFixture, { onedit: vi.fn(), ondelete: vi.fn() });
    await userEvent.click(screen.getByRole("button", { name: "Actions" }));
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("MenuItem's shortcut", () => {
  it("shows the key, and says it as aria-keyshortcuts rather than in the name", async () => {
    render(MenuFixture, {});
    await userEvent.click(screen.getByRole("button", { name: "Actions" }));
    const edit = screen.getByRole("menuitem", { name: "Edit" });
    expect(["Meta+E", "Control+E"]).toContain(edit.getAttribute("aria-keyshortcuts"));
    const key = edit.querySelector(".elyra-kbd");
    expect(key.getAttribute("aria-hidden")).toBe("true");
    expect(screen.getByRole("menuitem", { name: "Archive" }).hasAttribute("aria-keyshortcuts")).toBe(false);
  });
});
