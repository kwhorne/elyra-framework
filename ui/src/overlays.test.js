/** Modal, Drawer, Tooltip and Popover: roles, focus in and back, Esc. */
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import OverlayFixture from "./test/OverlayFixture.svelte";
import GuardedModalFixture from "./test/GuardedModalFixture.svelte";

describe("Modal", () => {
  it("is a labelled, described modal dialog that takes focus and gives it back", async () => {
    const onclose = vi.fn();
    render(OverlayFixture, { onclose });
    const opener = screen.getByRole("button", { name: "Open" });
    await userEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Delete customer" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(dialog.getAttribute("aria-describedby")).textContent).toBe("It can't be undone.");
    await vi.waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));

    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Delete customer" })).toBeNull();
    expect(onclose).toHaveBeenCalledOnce();
    await tick();
    expect(document.activeElement).toBe(opener);
  });

  it("keeps Tab inside", async () => {
    render(OverlayFixture, { open: true });
    const remove = screen.getByRole("button", { name: "Delete" });
    const close = screen.getByRole("button", { name: "Close" });
    // Tab past the last control wraps to the first, and Shift+Tab back. The
    // keys are dispatched as they are: user-event moves focus on its own.
    remove.focus();
    expect(await fireEvent.keyDown(remove, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(close);
    expect(await fireEvent.keyDown(close, { key: "Tab", shiftKey: true })).toBe(false);
    expect(document.activeElement).toBe(remove);
    // In between, Tab goes its own way.
    const reason = screen.getByRole("textbox", { name: "Reason" });
    reason.focus();
    expect(await fireEvent.keyDown(reason, { key: "Tab" })).toBe(true);
  });

  it("stays open when it isn't dismissible", async () => {
    render(OverlayFixture, { open: true, dismissible: false });
    const dialog = screen.getByRole("dialog", { name: "Delete customer" });
    dialog.focus();
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "Delete customer" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
  });
});

describe("Drawer", () => {
  it("is a dialog at the side", () => {
    render(OverlayFixture, { drawer: true });
    const drawer = screen.getByRole("dialog", { name: "Filters" });
    expect(drawer.className).toContain("right");
  });
});

describe("Tooltip", () => {
  it("describes what's inside, on focus, and Esc hides it", async () => {
    render(OverlayFixture);
    const copy = screen.getByRole("button", { name: "Copy" });
    const tip = document.getElementById(copy.getAttribute("aria-describedby"));
    expect(tip.getAttribute("role")).toBe("tooltip");
    expect(tip.textContent).toBe("Copies the link");
    expect(tip.hidden).toBe(true);
    copy.focus();
    await vi.waitFor(() => expect(tip.hidden).toBe(false));
    await userEvent.keyboard("{Escape}");
    expect(tip.hidden).toBe(true);
  });

  it("is placed once it's shown, not while it's hidden", async () => {
    render(OverlayFixture);
    const copy = screen.getByRole("button", { name: "Copy" });
    const tip = document.getElementById(copy.getAttribute("aria-describedby"));
    expect(tip.style.position).toBe("");
    copy.focus();
    await vi.waitFor(() => expect(tip.style.position).toBe("fixed"));
  });
});

describe("Popover", () => {
  it("opens a panel, focuses into it, and Esc gives focus back", async () => {
    render(OverlayFixture);
    const button = screen.getByRole("button", { name: "Filter" });
    await userEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    const panel = screen.getByRole("dialog", { name: "Filter" });
    await vi.waitFor(() => expect(document.activeElement.getAttribute("aria-label")).toBe("Search in filter"));
    expect(panel.contains(document.activeElement)).toBe(true);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Filter" })).toBeNull();
    expect(document.activeElement).toBe(button);
  });
});

describe("Modal: asking before it closes", () => {
  it("stays open when beforeclose says no, and closes when it says yes", async () => {
    const onclose = vi.fn();
    let answer = false;
    const beforeclose = vi.fn(async () => answer);
    render(GuardedModalFixture, { beforeclose, onclose });
    expect(screen.getByRole("dialog")).toBeTruthy();
    await userEvent.keyboard("{Escape}");
    await vi.waitFor(() => expect(beforeclose).toHaveBeenCalledTimes(1));
    await tick();
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(onclose).not.toHaveBeenCalled();

    answer = true;
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it("while it's asking, another Esc asks nothing more", async () => {
    let answer;
    const beforeclose = vi.fn(() => new Promise((resolve) => (answer = resolve)));
    render(GuardedModalFixture, { beforeclose });
    await userEvent.keyboard("{Escape}");
    await userEvent.keyboard("{Escape}");
    expect(beforeclose).toHaveBeenCalledTimes(1);
    answer(true);
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
