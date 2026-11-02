/** Table: resizable columns, by drag and keyboard, kept in the settings store with `persist`. */
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import Table from "./Table.svelte";
import { store } from "./test/runtime.js";

const rows = [
  { id: 1, name: "Ada Lovelace", email: "ada@example.com", city: "London" },
  { id: 2, name: "Grace Hopper", email: "grace@example.com", city: "Arlington" },
];
const columns = ["name", "email", "city"];

const grip = (label) => screen.getByRole("separator", { name: `Resize ${label}` });
const table = () => screen.getByRole("table");
/** The `<col>` widths, in order ("" for one that takes the rest). */
const cols = () => [...table().querySelectorAll("colgroup col")].map((c) => c.style.width);

/** Drag `label`'s grip by `by` pixels. */
async function drag(label, by) {
  await fireEvent.pointerDown(grip(label), { button: 0, pointerId: 1, clientX: 300 });
  await fireEvent.pointerMove(grip(label), { pointerId: 1, clientX: 300 + by / 2 });
  await fireEvent.pointerMove(grip(label), { pointerId: 1, clientX: 300 + by });
  await fireEvent.pointerUp(grip(label), { pointerId: 1, clientX: 300 + by });
}

describe("Table resizable columns", () => {
  it("has no grips, and lays out as before, unless resizable", () => {
    render(Table, { rows, columns });
    expect(screen.queryAllByRole("separator")).toHaveLength(0);
    expect(table().querySelector("colgroup")).toBeNull();
    expect(table().classList.contains("fixed")).toBe(false);
  });

  it("gives each column but the last a labelled grip", () => {
    render(Table, { rows, columns, resizable: true });
    const grips = screen.getAllByRole("separator");
    expect(grips.map((g) => g.getAttribute("aria-label"))).toEqual(["Resize Name", "Resize Email"]);
    expect(grips[0].tabIndex).toBe(0);
    expect(grips[0].getAttribute("aria-orientation")).toBe("vertical");
    expect(grips[0].getAttribute("aria-valuemin")).toBe("48");
    expect(grips[0].hasAttribute("aria-valuenow")).toBe(false);
  });

  it("with actions, every column has one, and the actions take the rest", async () => {
    const { container } = render(Table, { rows, columns: ["name", { key: "email", width: 200 }], resizable: true, actions: (() => {}) });
    expect(screen.getAllByRole("separator")).toHaveLength(2);
    expect(cols()).toEqual(["", "200px", ""]);
    expect(container.querySelector("table").style.getPropertyValue("--elyra-table-width")).toBe("max(100%, 440px)");
  });

  it("doesn't give the last column a width, even its own", () => {
    render(Table, { rows, columns: [{ key: "name", width: 150 }, "email", { key: "city", width: 300 }], resizable: true });
    expect(cols()).toEqual(["150px", "", ""]);
    expect(table().style.getPropertyValue("--elyra-table-width")).toBe("max(100%, 390px)");
  });

  it("counts the checkboxes' column in the table's width", () => {
    render(Table, { rows, columns: [{ key: "name", width: 150 }, "city"], resizable: true, select: true });
    expect(cols()).toEqual(["36px", "150px", ""]);
    expect(table().style.getPropertyValue("--elyra-table-width")).toBe("max(100%, 306px)");
  });

  it("follows a drag, keeping the others as they were", async () => {
    render(Table, { rows, columns, resizable: true });
    await drag("Name", 152);
    // The others are measured when the drag starts (0 in jsdom, so the least).
    expect(cols()).toEqual(["200px", "48px", ""]);
    expect(grip("Name").getAttribute("aria-valuenow")).toBe("200");
    expect(table().classList.contains("fixed")).toBe(true);
    expect(table().style.getPropertyValue("--elyra-table-width")).toBe("max(100%, 368px)");
    await drag("Name", -500);
    expect(cols()[0]).toBe("48px");
  });

  it("ignores a move without a press, and another button", async () => {
    render(Table, { rows, columns: [{ key: "name", width: 150 }, "email", "city"], resizable: true });
    await fireEvent.pointerMove(grip("Name"), { clientX: 500 });
    await fireEvent.pointerDown(grip("Name"), { button: 2, clientX: 300 });
    await fireEvent.pointerMove(grip("Name"), { clientX: 400 });
    expect(cols()[0]).toBe("150px");
    // A drag on one grip doesn't move another column.
    await fireEvent.pointerDown(grip("Name"), { button: 0, pointerId: 1, clientX: 300 });
    await fireEvent.pointerMove(grip("Email"), { pointerId: 1, clientX: 400 });
    expect(cols()[1]).toBe("48px");
    await fireEvent.pointerUp(grip("Name"), { pointerId: 1, clientX: 300 });
    await fireEvent.pointerMove(grip("Name"), { pointerId: 1, clientX: 600 });
    expect(cols()[0]).toBe("150px");
  });

  it("moves with the keyboard, and Enter or a double click goes back", async () => {
    render(Table, { rows, columns: [{ key: "name", width: 160 }, "email", "city"], resizable: true });
    const name = grip("Name");
    await fireEvent.keyDown(name, { key: "ArrowRight" });
    expect(cols()[0]).toBe("176px");
    await fireEvent.keyDown(name, { key: "ArrowLeft", shiftKey: true });
    expect(cols()[0]).toBe("112px");
    await fireEvent.keyDown(name, { key: "Home" });
    expect(cols()[0]).toBe("48px");
    await fireEvent.keyDown(name, { key: "Enter" });
    expect(cols()[0]).toBe("160px");
    await fireEvent.keyDown(grip("Email"), { key: "ArrowRight" });
    expect(cols()[1]).toBe("64px");
    await fireEvent.dblClick(grip("Email"));
    expect(cols()[1]).toBe("");
    // Other keys are left alone.
    const tab = new KeyboardEvent("keydown", { key: "Tab", cancelable: true, bubbles: true });
    name.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(false);
  });

  it("back to no widths at all, lays out as before", async () => {
    render(Table, { rows, columns: ["name", "city"], resizable: true });
    await fireEvent.keyDown(grip("Name"), { key: "ArrowRight" });
    expect(table().classList.contains("fixed")).toBe(true);
    await fireEvent.dblClick(grip("Name"));
    expect(table().querySelector("colgroup")).toBeNull();
    expect(table().style.getPropertyValue("--elyra-table-width")).toBe("");
  });

  it("names the grips in the app's words", () => {
    render(Table, { rows, columns, resizable: true, labels: { resize: (label) => `Endre bredden på ${label}` } });
    expect(screen.getByRole("separator", { name: "Endre bredden på Name" })).toBeTruthy();
  });

  describe("persist", () => {
    it("keeps the widths in the settings store, and starts from them", async () => {
      const { unmount } = render(Table, { rows, columns, resizable: true, persist: "customers" });
      await vi.waitFor(() => expect(store.get).toHaveBeenCalledWith("elyra-ui.table.customers"));
      await drag("Name", 152);
      await vi.waitFor(() =>
        expect(store.set).toHaveBeenCalledWith("elyra-ui.table.customers", { name: 200, email: 48 }),
      );
      unmount();
      render(Table, { rows, columns, resizable: true, persist: "customers" });
      await vi.waitFor(() => expect(cols()).toEqual(["200px", "48px", ""]));
    });

    it("waits for the keys to stop, and ignores what isn't a width", async () => {
      await store.set("elyra-ui.table.invoices", { name: Infinity, email: 230.6, city: "wide" });
      store.set.mockClear();
      render(Table, { rows, columns, resizable: true, persist: "invoices" });
      await vi.waitFor(() => expect(cols()[1]).toBe("231px"));
      expect(cols()[0]).toBe("");
      expect(grip("Name").hasAttribute("aria-valuenow")).toBe(false);
      await fireEvent.keyDown(grip("Email"), { key: "ArrowRight" });
      await fireEvent.keyDown(grip("Email"), { key: "ArrowRight" });
      await new Promise((r) => setTimeout(r, 100));
      expect(store.set).not.toHaveBeenCalled();
      await vi.waitFor(() => expect(store.set).toHaveBeenCalledTimes(1));
      expect(store.set.mock.calls[0][1]).toMatchObject({ email: 263 });
    });

    it("starts afresh from a store that has nothing, or something else", async () => {
      await store.set("elyra-ui.table.notes", [300, 200]);
      render(Table, { rows, columns, resizable: true, persist: "notes" });
      await vi.waitFor(() => expect(store.get).toHaveBeenCalledWith("elyra-ui.table.notes"));
      await tick();
      expect(table().querySelector("colgroup")).toBeNull();
    });

    it("isn't kept without resizable, nor read", async () => {
      render(Table, { rows, columns, persist: "customers" });
      await tick();
      expect(store.get).not.toHaveBeenCalled();
    });
  });
});
