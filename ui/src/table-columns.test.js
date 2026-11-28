/** Table: columns the user picks (RFC 0068). Shown, hidden, in their order, kept with the widths. */
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import Table from "./Table.svelte";
import { store } from "./test/runtime.js";
import { arrange, canHide, keptOf, moved, toggled } from "./internal/columns.js";

const customers = [
  { id: 1, name: "Ada", email: "ada@analytical.co", phone: "+47 1", org_number: "912345678", balance: 1200 },
  { id: 2, name: "Grace", email: "grace@cobol.dev", phone: "+47 2", org_number: "998877665", balance: 300 },
];
const columns = [
  { key: "name", required: true },
  "email",
  "phone",
  { key: "org_number", label: "Org. no.", hidden: true },
  { key: "balance", align: "end", hidden: true, total: "sum" },
];
const col = (key, extra = {}) => ({ key, label: key, ...extra });
const app = [col("name", { required: true }), col("email"), col("phone"), col("org", { hidden: true })];
const keys = (list) => list.map((c) => c.key);
const heads = () => [...document.querySelectorAll("thead th")].map((th) => th.textContent.trim());
const settle = async () => {
  for (let i = 0; i < 4; i++) await new Promise((r) => setTimeout(r, 0));
};
const openMenu = async () => {
  await userEvent.click(screen.getByRole("button", { name: "Columns" }));
  return screen.getByRole("menu");
};
const item = (name) => screen.getByRole("menuitemcheckbox", { name });

describe("columns.js", () => {
  it("the app's columns, its hidden ones hidden, until the user chooses", () => {
    expect(keys(arrange(app, null).shown)).toEqual(["name", "email", "phone"]);
    const chosen = { hidden: ["email"], order: ["phone", "name", "email", "org"] };
    const a = arrange(app, chosen);
    expect(keys(a.all)).toEqual(["phone", "name", "email", "org"]);
    expect(keys(a.shown)).toEqual(["phone", "name", "org"]);
  });

  it("a column the app adds later goes after the one before it; a key it took away is forgotten", () => {
    const later = [...app.slice(0, 2), col("mobile"), ...app.slice(2), col("notes", { hidden: true })];
    const a = arrange(later, { hidden: [], order: ["email", "name", "gone", "phone", "org"] });
    expect(keys(a.all)).toEqual(["email", "mobile", "name", "phone", "org", "notes"]);
    expect(keys(a.shown)).toEqual(["email", "mobile", "name", "phone", "org"]);
  });

  it("a required column is never hidden; the last shown can't be", () => {
    const a = arrange(app, { hidden: ["name", "email", "phone", "org"], order: keys(app) });
    expect(keys(a.shown)).toEqual(["name"]);
    expect(canHide(a, "name")).toBe(false);
    const b = arrange([col("a"), col("b")], { hidden: ["b"], order: ["a", "b"] });
    expect(canHide(b, "a")).toBe(false);
    expect(canHide(b, "b")).toBe(true);
  });

  it("toggled and moved give the choice to keep", () => {
    const a = arrange(app, null);
    expect(toggled(a, "org")).toEqual({ hidden: [], order: ["name", "email", "phone", "org"] });
    expect(toggled(a, "email")).toEqual({ hidden: ["email", "org"], order: ["name", "email", "phone", "org"] });
    expect(moved(a, "phone", { by: -1 })).toEqual({ hidden: ["org"], order: ["name", "phone", "email", "org"] });
    expect(moved(a, "name", { to: 3 }).order).toEqual(["email", "phone", "org", "name"]);
    expect(moved(a, "name", { by: -1 }).order).toEqual(["name", "email", "phone", "org"]);
  });

  it("what was kept: the new value, or 0.62's widths alone", () => {
    expect(keptOf({ name: 180, email: 220 })).toEqual({ widths: { name: 180, email: 220 }, choice: null });
    expect(keptOf({ widths: { name: 180 }, hidden: ["email"], order: ["email", "name"] })).toEqual({
      widths: { name: 180 },
      choice: { hidden: ["email"], order: ["email", "name"] },
    });
    expect(keptOf(null)).toEqual({ widths: {}, choice: null });
    expect(keptOf({ name: "wide" })).toEqual({ widths: {}, choice: null });
  });
});

describe("Table choosable", () => {
  it("a Columns menu of checkboxes: hide one, show one, a required one can't be hidden", async () => {
    render(Table, { rows: customers, columns, choosable: true });
    expect(heads()).toEqual(["Name", "Email", "Phone"]);
    await openMenu();
    expect(screen.getAllByRole("menuitemcheckbox").map((el) => [el.textContent.trim(), el.getAttribute("aria-checked")])).toEqual([
      ["Name", "true"],
      ["Email", "true"],
      ["Phone", "true"],
      ["Org. no.", "false"],
      ["Balance", "false"],
    ]);
    expect(item("Name").disabled).toBe(true);
    await userEvent.click(item("Email"));
    await userEvent.click(item("Org. no."));
    // The menu stays open while the user picks.
    expect(screen.getByRole("menu")).toBeTruthy();
    expect(heads()).toEqual(["Name", "Phone", "Org. no."]);
  });

  it("Alt+↑/↓ on an item moves its column, and says where", async () => {
    render(Table, { rows: customers, columns, choosable: true });
    await openMenu();
    item("Phone").focus();
    await userEvent.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(heads()).toEqual(["Name", "Phone", "Email"]);
    expect(document.querySelector('[role="status"]').textContent).toBe("Phone moved to 2 of 5");
    // The focus stays on the item that moved.
    expect(document.activeElement).toBe(item("Phone"));
    await userEvent.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(heads()).toEqual(["Phone", "Name", "Email"]);
  });

  it("Reset columns: the app's again", async () => {
    render(Table, { rows: customers, columns, choosable: true });
    await openMenu();
    await userEvent.click(item("Email"));
    await userEvent.click(screen.getByRole("menuitem", { name: "Reset columns" }));
    expect(heads()).toEqual(["Name", "Email", "Phone"]);
  });

  it("persist keeps the choice with the widths, and the table opens as the user left it", async () => {
    await store.set("elyra-ui.table.customers", {
      widths: { email: 220 },
      hidden: ["phone", "org_number", "balance"],
      order: ["email", "name", "phone", "org_number", "balance"],
    });
    const first = render(Table, { rows: customers, columns, choosable: true, resizable: true, persist: "customers" });
    await settle();
    expect(heads()).toEqual(["Email", "Name"]);
    await openMenu();
    await userEvent.click(item("Balance"));
    await settle();
    expect(store.set).toHaveBeenLastCalledWith("elyra-ui.table.customers", {
      widths: { email: 220 },
      hidden: ["phone", "org_number"],
      order: ["email", "name", "phone", "org_number", "balance"],
    });
    first.unmount();
    render(Table, { rows: customers, columns, choosable: true, resizable: true, persist: "customers" });
    await settle();
    expect(heads()).toEqual(["Email", "Name", "Balance"]);
  });

  it("0.62's widths alone are read as the widths", async () => {
    await store.set("elyra-ui.table.old", { email: 260 });
    render(Table, { rows: customers, columns, choosable: true, resizable: true, persist: "old" });
    await settle();
    expect(heads()).toEqual(["Name", "Email", "Phone"]);
    expect(document.querySelector("colgroup col:nth-child(2)").style.width).toBe("260px");
  });

  it("the search goes on over a hidden column; a sort by one is said", async () => {
    render(Table, { rows: customers, columns, choosable: true, search: true, query: { search: "998877", sort: "balance", direction: "asc" } });
    expect(screen.getAllByRole("row").slice(1).map((r) => r.textContent)).toEqual([expect.stringContaining("Grace")]);
    expect(screen.getByRole("table").getAttribute("aria-description")).toBe("Sorted by Balance");
  });

  it("the export and the totals are the shown columns; csv({ all: true }) every one", async () => {
    const { component } = render(Table, { rows: customers, columns, choosable: true });
    expect(component.csv().split("\r\n")[0]).toBe("Name,Email,Phone");
    expect(document.querySelector("tfoot")).toBeNull();
    expect(component.csv({ all: true }).split("\r\n").slice(0, 1)[0]).toBe("Name,Email,Phone,Org. no.,Balance");
    expect(component.csv({ all: true }).trim().split("\r\n").at(-1)).toBe("Total,,,,1500");
  });

  it("a right click on the head opens the menu", async () => {
    render(Table, { rows: customers, columns, choosable: true });
    await fireEvent.contextMenu(screen.getByRole("columnheader", { name: "Email" }));
    await tick();
    expect(screen.getByRole("menu")).toBeTruthy();
  });

  it("a header dragged to another's place moves there, and the click that ends it doesn't sort", async () => {
    const query = { sort: undefined };
    render(Table, { rows: customers, columns, choosable: true, sortable: true, query });
    const ths = [...document.querySelectorAll("thead th")];
    ths.forEach((th, i) => {
      th.getBoundingClientRect = () => ({ left: i * 100, right: (i + 1) * 100, top: 0, bottom: 30, width: 100, height: 30 });
    });
    const sortButton = ths[0].querySelector("button");
    await fireEvent.pointerDown(sortButton, { button: 0, clientX: 50 });
    await fireEvent.pointerMove(window, { clientX: 250 });
    await fireEvent.pointerUp(window, { clientX: 250 });
    await fireEvent.click(sortButton);
    expect(heads()).toEqual(["Email", "Phone", "Name"]);
    expect(screen.getByRole("columnheader", { name: "Name" }).getAttribute("aria-sort")).toBe("none");
  });

  it("without choosable there's no menu; hidden columns are still hidden", () => {
    render(Table, { rows: customers, columns });
    expect(screen.queryByRole("button", { name: "Columns" })).toBeNull();
    expect(heads()).toEqual(["Name", "Email", "Phone"]);
  });
});
