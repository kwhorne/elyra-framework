/** EditingDrawer (RFC 0038), and the StatusBar's line that opens it. */
import { afterEach, describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import EditingDrawer from "./EditingDrawer.svelte";
import StatusBar from "./StatusBar.svelte";
import { confirm, editing } from "./test/runtime.js";

const SAVED = Date.UTC(2026, 10, 26, 9, 42);
const file = (id, name, state, extra = {}) => ({ id, name, state, savedAt: null, error: null, ...extra });

const CONFLICT = file("a", "contract.docx", "conflict", { error: "It changed on the server too." });
const SAVED_ONE = file("b", "budget.xlsx", "saved", { savedAt: SAVED });
const WAITING = file("c", "plan.pages", "waiting", { error: "The server isn't answering." });

const item = (name) => screen.getByRole("listitem", { name });

afterEach(() => {
  editing.set([]);
  confirm.mockReset();
  confirm.mockImplementation(async () => true);
});

describe("EditingDrawer", () => {
  it("says so when nothing is open", () => {
    render(EditingDrawer, { open: true });
    expect(screen.getByRole("dialog", { name: "Editing" }).textContent).toContain("Nothing open");
  });

  it("lists what needs the user first, then the rest, with what each said", () => {
    editing.set([SAVED_ONE, CONFLICT, WAITING]);
    render(EditingDrawer, { open: true });
    const needs = screen.getByRole("region", { name: /Needs you/ });
    const rest = screen.getByRole("region", { name: /Open/ });
    expect(within(needs).getAllByRole("listitem").map((li) => li.getAttribute("aria-label"))).toEqual(["contract.docx"]);
    expect(within(rest).getAllByRole("listitem").map((li) => li.getAttribute("aria-label"))).toEqual([
      "budget.xlsx",
      "plan.pages",
    ]);
    expect(item("contract.docx").textContent).toContain("Changed on the server");
    expect(item("contract.docx").textContent).toContain("It changed on the server too.");
    expect(item("budget.xlsx").textContent).toContain("Last saved to the server");
    expect(item("plan.pages").textContent).toContain("Waiting");
  });

  it("offers Keep mine and Take theirs for a conflict alone", async () => {
    editing.set([CONFLICT, SAVED_ONE]);
    render(EditingDrawer, { open: true });
    expect(within(item("budget.xlsx")).queryByRole("button", { name: "Keep mine" })).toBeNull();
    await userEvent.click(within(item("contract.docx")).getByRole("button", { name: "Keep mine" }));
    expect(editing.keepMine).toHaveBeenCalledWith("a");
  });

  it("opens a file in its app again, and shows it in its folder", async () => {
    editing.set([SAVED_ONE]);
    render(EditingDrawer, { open: true });
    const row = item("budget.xlsx");
    await userEvent.click(within(row).getByRole("button", { name: "Open" }));
    expect(editing.show).toHaveBeenCalledWith("b");
    await userEvent.click(within(row).getByRole("button", { name: /Show in/ }));
    expect(editing.reveal).toHaveBeenCalledWith("b");
  });

  it("asks before finishing a file whose last save hasn't arrived, and gives it up only when told", async () => {
    editing.set([WAITING]);
    const pending = Object.assign(new Error("a save hasn't reached the server"), { kind: "pending_changes" });
    editing.finish.mockImplementationOnce(async () => {
      throw pending;
    });
    confirm.mockImplementationOnce(async () => false);
    render(EditingDrawer, { open: true });
    await userEvent.click(within(item("plan.pages")).getByRole("button", { name: "Done" }));
    expect(confirm).toHaveBeenCalledOnce();
    expect(confirm.mock.calls[0][0]).toContain("plan.pages");
    // Said no: still there, and nothing discarded.
    expect(editing.finish).toHaveBeenCalledTimes(1);
    expect(item("plan.pages")).toBeTruthy();

    editing.finish.mockImplementationOnce(async () => {
      throw pending;
    });
    await userEvent.click(within(item("plan.pages")).getByRole("button", { name: "Done" }));
    expect(editing.finish).toHaveBeenLastCalledWith("c", { discard: true });
  });

  it("says what went wrong under the file", async () => {
    editing.set([SAVED_ONE]);
    editing.show.mockImplementationOnce(async () => {
      throw new Error("It's gone from the editing folder.");
    });
    render(EditingDrawer, { open: true });
    await userEvent.click(within(item("budget.xlsx")).getByRole("button", { name: "Open" }));
    expect(within(item("budget.xlsx")).getByRole("alert").textContent).toContain("gone from the editing folder");
  });
});

describe("StatusBar's editing", () => {
  it("says nothing while no file is open", () => {
    render(StatusBar);
    expect(screen.queryByRole("button", { name: /Editing|need|Saving|saves? waiting/ })).toBeNull();
  });

  it("names the one file open, and opens the drawer", async () => {
    editing.set([SAVED_ONE]);
    render(StatusBar);
    await userEvent.click(screen.getByRole("button", { name: "Editing budget.xlsx" }));
    expect(screen.getByRole("dialog", { name: "Editing" })).toBeTruthy();
  });

  it("puts what needs the user before the rest", () => {
    editing.set([SAVED_ONE, WAITING, CONFLICT]);
    render(StatusBar);
    expect(screen.getByRole("button", { name: "1 file needs you" })).toBeTruthy();
  });

  it("says a save on its way, then saves waiting, then how many are open", () => {
    editing.set([SAVED_ONE, file("d", "notes.txt", "saving")]);
    const { unmount } = render(StatusBar);
    expect(screen.getByRole("button", { name: /Saving notes\.txt/ })).toBeTruthy();
    unmount();
    editing.set([SAVED_ONE, WAITING]);
    const again = render(StatusBar);
    expect(screen.getByRole("button", { name: "1 save waiting" })).toBeTruthy();
    again.unmount();
    editing.set([SAVED_ONE, file("e", "deck.key", "open")]);
    render(StatusBar);
    expect(screen.getByRole("button", { name: "Editing 2 files" })).toBeTruthy();
  });

  it("turns off", () => {
    editing.set([SAVED_ONE]);
    render(StatusBar, { editing: false });
    expect(screen.queryByRole("button", { name: /Editing/ })).toBeNull();
  });
});
