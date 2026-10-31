/** Table selection: checkboxes, Shift and the keyboard, across pages, rows that go, bulk actions, the assistant. */
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { writable } from "svelte/store";
import { assistant } from "./test/runtime.js";
import SelectTableFixture from "./test/SelectTableFixture.svelte";

const PEOPLE = [
  { id: 1, name: "Ada", city: "London" },
  { id: 2, name: "Grace", city: "Arlington" },
  { id: 3, name: "Kristen", city: "Oslo" },
  { id: 4, name: "Ole-Johan", city: "Oslo" },
  { id: 5, name: "Margaret", city: "Boston" },
];

const selected = () => JSON.parse(screen.getByTestId("selected").textContent);
const box = (name) => screen.getByRole("checkbox", { name: `Select ${name}` });
const all = () => screen.getByRole("checkbox", { name: "Select all on this page" });
const row = (name) => screen.getByRole("cell", { name }).closest("tr");
const page = (data, current = 1, last = 1) => ({ data, total: 5, per_page: 2, current_page: current, last_page: last });
const live = (value) => writable({ value, error: null, loading: false });

describe("Table selection", () => {
  it("has no checkboxes without select", () => {
    render(SelectTableFixture, { rows: PEOPLE, select: false });
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("selects rows by their checkboxes, and shows how many with the bulk actions", async () => {
    const ondelete = vi.fn();
    render(SelectTableFixture, { rows: PEOPLE, select: true, ondelete });
    expect(screen.queryByRole("region")).toBeNull();
    await userEvent.click(box("Ada"));
    await userEvent.click(box("Kristen"));
    expect(selected()).toEqual([1, 3]);
    expect(box("Ada").checked).toBe(true);
    expect(row("Ada").classList.contains("selected")).toBe(true);
    const bar = screen.getByRole("region", { name: "2 selected" });
    expect(within(bar).getByText("2 selected")).toBeTruthy();
    await userEvent.click(within(bar).getByRole("button", { name: "Delete 2" }));
    expect(ondelete).toHaveBeenCalledWith([1, 3]);
    expect(selected()).toEqual([]);
    expect(screen.queryByRole("region")).toBeNull();
  });

  it("clears from the bar", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, selected: [2, 4] });
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(selected()).toEqual([]);
    expect(box("Grace").checked).toBe(false);
  });

  it("selects a range with Shift, from the last row clicked", async () => {
    const user = userEvent.setup();
    render(SelectTableFixture, { rows: PEOPLE, select: true });
    await user.click(box("Grace"));
    await user.keyboard("{Shift>}");
    await user.click(box("Margaret"));
    await user.keyboard("{/Shift}");
    expect(selected().sort()).toEqual([2, 3, 4, 5]);
    // Shift on a selected one takes the range off again.
    await user.keyboard("{Shift>}");
    await user.click(box("Kristen"));
    await user.keyboard("{/Shift}");
    expect(selected().sort()).toEqual([2]);
  });

  it("selects all on the page from the header, which shows part of it", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, perPage: 3 });
    await userEvent.click(box("Ada"));
    expect(all().indeterminate).toBe(true);
    expect(all().checked).toBe(false);
    await userEvent.click(all());
    expect(selected().sort()).toEqual([1, 2, 3]);
    expect(all().checked).toBe(true);
    expect(all().indeterminate).toBe(false);
    await userEvent.click(all());
    expect(selected()).toEqual([]);
  });

  it("shows a page that's all selected as checked, not partly", () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, perPage: 2, selected: [1, 2] });
    expect(all().checked).toBe(true);
    expect(all().indeterminate).toBe(false);
  });

  it("leaves the keys in a cell's own input alone", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, editable: true });
    const input = screen.getByRole("textbox", { name: "City of Ada" });
    await userEvent.clear(input);
    await userEvent.type(input, "New York");
    expect(input.value).toBe("New York");
    expect(selected()).toEqual([]);
  });

  it("keeps the selection across pages and searches", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, perPage: 2 });
    await userEvent.click(box("Ada"));
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    await userEvent.click(box("Kristen"));
    expect(selected()).toEqual([1, 3]);
    // This page has one of its two selected.
    expect(all().indeterminate).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(box("Ada").checked).toBe(true);
  });

  it("works from the keyboard: Space, ⌘A or Ctrl+A, Esc", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true, perPage: 3 });
    expect(row("Ada").tabIndex).toBe(0);
    row("Ada").focus();
    await userEvent.keyboard(" ");
    expect(selected()).toEqual([1]);
    await userEvent.keyboard("{ArrowDown} ");
    expect(selected()).toEqual([1, 2]);
    await userEvent.keyboard(" ");
    expect(selected()).toEqual([1]);
    await userEvent.keyboard("{Control>}a{/Control}");
    expect(selected().sort()).toEqual([1, 2, 3]);
    await userEvent.keyboard("{Escape}");
    expect(selected()).toEqual([]);
    await userEvent.keyboard("{Meta>}a{/Meta}");
    expect(selected().sort()).toEqual([1, 2, 3]);
  });

  it("selects a range with Shift+Space", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true });
    row("Ada").focus();
    await userEvent.keyboard(" {ArrowDown}{ArrowDown}{Shift>} {/Shift}");
    expect(selected().sort()).toEqual([1, 2, 3]);
  });

  it("toggles a row on a click when rows don't open, and opens it when they do", async () => {
    const onrowclick = vi.fn();
    const { unmount } = render(SelectTableFixture, { rows: PEOPLE, select: true });
    await userEvent.click(row("Kristen"));
    expect(selected()).toEqual([3]);
    unmount();
    render(SelectTableFixture, { rows: PEOPLE, select: true, onrowclick });
    await userEvent.click(row("Kristen"));
    expect(onrowclick).toHaveBeenCalledWith(PEOPLE[2]);
    expect(selected()).toEqual([]);
    // The checkbox selects; it doesn't open the row.
    await userEvent.click(box("Grace"));
    expect(onrowclick).toHaveBeenCalledTimes(1);
    expect(selected()).toEqual([2]);
    // Enter on the row opens it, Space selects it.
    row("Ada").focus();
    await userEvent.keyboard("{Enter} ");
    expect(onrowclick).toHaveBeenLastCalledWith(PEOPLE[0]);
    expect(selected()).toEqual([2, 1]);
  });

  it("leaves Space on the checkbox to the checkbox", async () => {
    render(SelectTableFixture, { rows: PEOPLE, select: true });
    box("Ada").focus();
    await userEvent.keyboard(" ");
    expect(selected()).toEqual([1]);
  });

  it("says its words in the app's language", async () => {
    render(SelectTableFixture, {
      rows: PEOPLE,
      select: true,
      labels: { selected: (n) => `${n} valgt`, clear: "Fjern valg", selectAll: "Velg alle", selectRow: (r) => `Velg ${r.name}` },
    });
    await userEvent.click(screen.getByRole("checkbox", { name: "Velg Ada" }));
    expect(screen.getByRole("region", { name: "1 valgt" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Fjern valg" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Velg alle" })).toBeTruthy();
  });

  it("drops rows the array no longer has", async () => {
    const { rerender } = render(SelectTableFixture, { rows: PEOPLE, select: true, selected: [1, 3] });
    await rerender({ rows: PEOPLE.filter((p) => p.id !== 3) });
    await vi.waitFor(() => expect(selected()).toEqual([1]));
  });

  describe("over a live page", () => {
    it("drops a row the live page no longer has, and keeps those on other pages", async () => {
      const store = live(page([PEOPLE[0], PEOPLE[1]], 1, 3));
      render(SelectTableFixture, { page: store, select: true, selected: [1, 2, 5] });
      expect(box("Ada").checked).toBe(true);
      // Grace is deleted elsewhere: the same page and query come back without her.
      store.set({ value: page([PEOPLE[0], PEOPLE[2]], 1, 3), error: null, loading: false });
      await vi.waitFor(() => expect(selected()).toEqual([1, 5]));
    });

    it("keeps the selection when the page changes", async () => {
      const store = live(page([PEOPLE[0], PEOPLE[1]], 1, 3));
      render(SelectTableFixture, { page: store, select: true, selected: [1, 2] });
      await userEvent.click(screen.getByRole("button", { name: "Next" }));
      // The page re-runs for page 2: a different page, not rows that went.
      store.set({ value: page([PEOPLE[2], PEOPLE[3]], 2, 3), error: null, loading: false });
      await new Promise((r) => setTimeout(r, 0));
      expect(selected()).toEqual([1, 2]);
    });

    it("keeps the selection while the page loads", async () => {
      const store = live(page([PEOPLE[0], PEOPLE[1]], 1, 3));
      render(SelectTableFixture, { page: store, select: true, selected: [1, 2] });
      store.set({ value: page([], 1, 3), error: null, loading: true });
      await new Promise((r) => setTimeout(r, 0));
      expect(selected()).toEqual([1, 2]);
    });
  });

  describe("telling the assistant", () => {
    it("says what's selected under select's name, once the clicks settle", async () => {
      render(SelectTableFixture, { rows: PEOPLE, select: "customers" });
      await userEvent.click(box("Ada"));
      await userEvent.click(box("Grace"));
      await vi.waitFor(() => expect(assistant.selected).toHaveBeenCalledWith("customers", [1, 2]));
      expect(assistant.selected).toHaveBeenCalledTimes(1);
      await userEvent.click(screen.getByRole("button", { name: "Clear" }));
      await vi.waitFor(() => expect(assistant.selected).toHaveBeenLastCalledWith("customers", []));
    });

    it("says nothing until something is selected, and nothing with select alone", async () => {
      const { unmount } = render(SelectTableFixture, { rows: PEOPLE, select: "customers" });
      await new Promise((r) => setTimeout(r, 200));
      expect(assistant.selected).not.toHaveBeenCalled();
      unmount();
      render(SelectTableFixture, { rows: PEOPLE, select: true });
      await userEvent.click(box("Ada"));
      await new Promise((r) => setTimeout(r, 200));
      expect(assistant.selected).not.toHaveBeenCalled();
    });

    it("clears it when the table goes", async () => {
      const { rerender } = render(SelectTableFixture, { rows: PEOPLE, select: "invoices" });
      await userEvent.click(box("Ada"));
      await vi.waitFor(() => expect(assistant.selected).toHaveBeenCalledWith("invoices", [1]));
      await rerender({ shown: false });
      expect(assistant.selected).toHaveBeenLastCalledWith("invoices", []);
    });

    it("doesn't break the table on a runtime without assistant.selected", async () => {
      const had = assistant.selected;
      delete assistant.selected;
      try {
        render(SelectTableFixture, { rows: PEOPLE, select: "customers" });
        await userEvent.click(box("Ada"));
        await new Promise((r) => setTimeout(r, 200));
        await userEvent.click(box("Grace"));
        expect(selected()).toEqual([1, 2]);
      } finally {
        assistant.selected = had;
      }
    });

    it("doesn't break the table when there's no assistant", async () => {
      assistant.selected.mockRejectedValue(new Error("no assistant"));
      render(SelectTableFixture, { rows: PEOPLE, select: "customers" });
      await userEvent.click(box("Ada"));
      await vi.waitFor(() => expect(assistant.selected).toHaveBeenCalled());
      expect(selected()).toEqual([1]);
    });
  });

  it("has a checkbox cell in the loading rows too", () => {
    render(SelectTableFixture, { page: writable({ value: undefined, error: null, loading: true }), select: true });
    const first = screen.getAllByRole("row")[1];
    expect(first.querySelectorAll("td")).toHaveLength(3);
    expect(all().disabled).toBe(true);
    expect(all().checked).toBe(false);
    fireEvent.click(all());
    expect(selected()).toEqual([]);
  });
});
