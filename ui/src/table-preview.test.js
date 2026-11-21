/** Table: Quick Look (RFC 0041). Space previews a row, ↓/↑ move it along, Space or Esc closes it. */
import { beforeEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import Table from "./Table.svelte";
import { quickLook } from "./test/runtime.js";

const rows = [
  { id: 1, name: "Ada" },
  { id: 2, name: "Grace" },
  { id: 3, name: "Linus" },
];
const preview = (row) => `/customers/${row.id}`;
const bodyRows = () => screen.getAllByRole("row").slice(1);

beforeEach(() => quickLook.reset());

describe("Table preview", () => {
  it("previews with Space, follows ↓/↑, and closes with Space or Esc", async () => {
    const opened = [];
    render(Table, { rows, columns: ["name"], preview, onrowclick: (r) => opened.push(r.id) });
    bodyRows()[0].focus();

    await userEvent.keyboard(" ");
    expect(quickLook.show).toHaveBeenLastCalledWith("/customers/1");
    expect(opened).toEqual([]);

    await userEvent.keyboard("{ArrowDown}");
    expect(quickLook.show).toHaveBeenLastCalledWith("/customers/2");
    await userEvent.keyboard("{ArrowDown}{ArrowUp}");
    expect(quickLook.show).toHaveBeenLastCalledWith("/customers/2");

    await userEvent.keyboard(" ");
    expect(quickLook.hide).toHaveBeenCalledTimes(1);

    await userEvent.keyboard(" ");
    await userEvent.keyboard("{Escape}");
    expect(quickLook.hide).toHaveBeenCalledTimes(2);

    // Enter still opens the row.
    await userEvent.keyboard("{Enter}");
    expect(opened).toEqual([2]);
  });

  it("Esc closes it with the focus elsewhere in the window, but not out of a dialog", async () => {
    render(Table, { rows, columns: ["name"], preview, search: true });
    bodyRows()[0].focus();
    await userEvent.keyboard(" ");
    expect(quickLook.show).toHaveBeenLastCalledWith("/customers/1");

    // In the search field: Esc still closes it.
    screen.getByRole("searchbox").focus();
    await userEvent.keyboard("{Escape}");
    expect(quickLook.hide).toHaveBeenCalledTimes(1);

    // Closed: Esc is the field's again.
    await userEvent.keyboard("{Escape}");
    expect(quickLook.hide).toHaveBeenCalledTimes(1);

    // A dialog open over it keeps its Esc.
    bodyRows()[0].focus();
    await userEvent.keyboard(" ");
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");
    const button = document.createElement("button");
    dialog.append(button);
    document.body.append(dialog);
    button.focus();
    await userEvent.keyboard("{Escape}");
    expect(quickLook.hide).toHaveBeenCalledTimes(1);
    dialog.remove();
  });

  it("doesn't follow the arrows when it isn't open", async () => {
    render(Table, { rows, columns: ["name"], preview });
    bodyRows()[0].focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(quickLook.show).not.toHaveBeenCalled();
  });

  it("uses ⌘Y where Space selects", async () => {
    render(Table, { rows, columns: ["name"], preview, select: true });
    bodyRows()[0].focus();
    await userEvent.keyboard(" ");
    expect(quickLook.show).not.toHaveBeenCalled();
    expect(screen.getAllByRole("checkbox")[1].checked).toBe(true);
    await userEvent.keyboard("{Meta>}y{/Meta}");
    expect(quickLook.show).toHaveBeenLastCalledWith("/customers/1");
  });

  it("without preview, Space opens the row as before", async () => {
    const opened = [];
    render(Table, { rows, columns: ["name"], onrowclick: (r) => opened.push(r.id) });
    bodyRows()[1].focus();
    await userEvent.keyboard(" ");
    expect(opened).toEqual([2]);
    expect(quickLook.show).not.toHaveBeenCalled();
  });
});
