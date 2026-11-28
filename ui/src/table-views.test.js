/** Table: saved views (RFC 0070). The app's views and the user's own, picked, changed, saved, renamed, deleted. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import TableFixture from "./test/TableFixture.svelte";
import Table from "./Table.svelte";
import { prompt, store } from "./test/runtime.js";
import { keptViews, matches, queryOf, viewOf } from "./internal/views.js";

const invoices = [
  { id: 1, customer: "Ada Lovelace", status: "open", phone: "1", amount: 12400 },
  { id: 2, customer: "Grace Hopper", status: "paid", phone: "2", amount: 860 },
  { id: 3, customer: "Kristen Nygaard", status: "overdue", phone: "3", amount: 3150 },
  { id: 4, customer: "Ole-Johan Dahl", status: "open", phone: "4", amount: 22000 },
];
const STATUS = [["open", "Open"], ["paid", "Paid"], ["overdue", "Overdue"]];
const columns = [
  { key: "customer", filter: "text", sortable: true },
  { key: "status", filter: { options: STATUS } },
  "phone",
  { key: "amount", align: "end", filter: "range", sortable: true },
];
const VIEWS = [
  { name: "Overdue", filters: { status: ["overdue"] } },
  { name: "Large", filters: { amount: { min: 10000 } }, sort: "amount", direction: "desc", columns: { hidden: ["phone"], order: ["customer", "status", "phone", "amount"] } },
];
const names = () => screen.getAllByRole("row").slice(1).map((r) => r.querySelector("td")?.textContent.trim()).filter(Boolean);
const query = () => JSON.parse(screen.getByTestId("query").textContent);
const settle = async () => {
  for (let i = 0; i < 4; i++) await new Promise((r) => setTimeout(r, 0));
};
const menu = async (label) => userEvent.click(screen.getByRole("button", { name: label }));
const pick = async (name) => userEvent.click(screen.getByRole("menuitemradio", { name }));

describe("views.js", () => {
  const base = { sort: "id", direction: "asc" };

  it("a view's query: its filters, search and sort, else the list's own, from the first page", () => {
    expect(queryOf(VIEWS[1], { page: 4, search: "x", per_page: 25 }, base)).toEqual({
      page: 1,
      per_page: 25,
      search: null,
      filters: { amount: { min: 10000 } },
      sort: "amount",
      direction: "desc",
    });
    expect(queryOf(null, { page: 2, filters: { a: "b" } }, base)).toEqual({ page: 1, filters: null, search: null, sort: "id", direction: "asc" });
  });

  it("whether the list still asks what its view does", () => {
    const q = queryOf(VIEWS[0], {}, base);
    expect(matches(VIEWS[0], q, null, base)).toBe(true);
    expect(matches(VIEWS[0], { ...q, search: "ada" }, null, base)).toBe(false);
    expect(matches(VIEWS[0], { ...q, sort: "amount" }, null, base)).toBe(false);
    expect(matches(VIEWS[0], { ...q, filters: { status: ["overdue"], amount: { min: 1 } } }, null, base)).toBe(false);
    // The columns count when the view has them.
    const large = queryOf(VIEWS[1], {}, base);
    expect(matches(VIEWS[1], large, VIEWS[1].columns, base)).toBe(true);
    expect(matches(VIEWS[1], large, { hidden: [], order: VIEWS[1].columns.order }, base)).toBe(false);
  });

  it("a view of what the list asks now; the kept ones, named and each once", () => {
    expect(viewOf("Mine", { filters: { status: ["open"] }, search: " ada ", sort: "amount" }, { hidden: ["phone"], order: ["a"] })).toEqual({
      name: "Mine",
      filters: { status: ["open"] },
      search: "ada",
      sort: "amount",
      direction: "asc",
      columns: { hidden: ["phone"], order: ["a"] },
    });
    expect(keptViews([{ name: "A" }, { name: "A" }, { name: " " }, null, "x"])).toEqual([{ name: "A" }]);
    expect(keptViews(null)).toEqual([]);
  });
});

describe("Table views", () => {
  it("the app's views in a menu: one picked filters, sorts and sets the columns; All goes back", async () => {
    render(TableFixture, { rows: invoices, columns, views: VIEWS, choosable: true });
    await menu("All");
    expect(screen.getAllByRole("menuitemradio").map((el) => [el.textContent.trim(), el.getAttribute("aria-checked")])).toEqual([
      ["All", "true"],
      ["Overdue", "false"],
      ["Large", "false"],
    ]);
    await pick("Large");
    expect(names()).toEqual(["Ole-Johan Dahl", "Ada Lovelace"]);
    expect(screen.queryByRole("columnheader", { name: "Phone" })).toBeNull();
    expect(screen.getByRole("table").getAttribute("aria-description")).toBe("View: Large");
    await menu("Large");
    await pick("All");
    expect(names()).toHaveLength(4);
    expect(query().filters).toBeNull();
  });

  it("a changed view says so; the user saves it as their own, and it's kept", async () => {
    render(TableFixture, { rows: invoices, columns, views: VIEWS, persist: "invoices" });
    await menu("All");
    await pick("Overdue");
    // Another status: the list no longer asks what Overdue does.
    await userEvent.click(screen.getByRole("button", { name: "Filter Status (filtered)" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Open" }));
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(screen.getByRole("button", { name: "Overdue (changed)" })).toBeTruthy();
    await menu("Overdue (changed)");
    expect(screen.queryByRole("menuitem", { name: "Save" })).toBeNull();
    prompt.mockResolvedValueOnce("Open or overdue");
    await userEvent.click(screen.getByRole("menuitem", { name: "Save as new view…" }));
    await settle();
    expect(screen.getByRole("button", { name: "Open or overdue" })).toBeTruthy();
    expect(store.set).toHaveBeenLastCalledWith("elyra-ui.table.invoices.views", [
      { name: "Open or overdue", filters: { status: ["overdue", "open"] } },
    ]);
  });

  it("the user's own: Save over it, Rename, Delete", async () => {
    await store.set("elyra-ui.table.mine.views", [{ name: "Ada", filters: { customer: "ada" } }]);
    render(TableFixture, { rows: invoices, columns, views: [], persist: "mine" });
    await settle();
    await menu("All");
    await pick("Ada");
    expect(names()).toEqual(["Ada Lovelace"]);
    await userEvent.click(screen.getByRole("button", { name: "Filter Customer (filtered)" }));
    await userEvent.clear(screen.getByRole("searchbox", { name: "Customer" }));
    await userEvent.keyboard("o{Enter}");
    await menu("Ada (changed)");
    await userEvent.click(screen.getByRole("menuitem", { name: "Save" }));
    await settle();
    expect(store.set).toHaveBeenLastCalledWith("elyra-ui.table.mine.views", [{ name: "Ada", filters: { customer: "o" } }]);
    expect(screen.getByRole("button", { name: "Ada" })).toBeTruthy();

    prompt.mockResolvedValueOnce("Has an o");
    await menu("Ada");
    await userEvent.click(screen.getByRole("menuitem", { name: "Rename view…" }));
    await settle();
    expect(screen.getByRole("button", { name: "Has an o" })).toBeTruthy();
    await menu("Has an o");
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete view" }));
    await settle();
    expect(screen.getByRole("button", { name: "All" })).toBeTruthy();
    expect(store.set).toHaveBeenLastCalledWith("elyra-ui.table.mine.views", []);
  });

  it("the app's views can't be saved over, renamed or deleted; a user's can't take an app's name", async () => {
    render(TableFixture, { rows: invoices, columns, views: VIEWS, persist: "names" });
    await menu("All");
    await pick("Overdue");
    await menu("Overdue");
    expect(screen.queryByRole("menuitem", { name: "Rename view…" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Delete view" })).toBeNull();
    prompt.mockResolvedValueOnce("Overdue");
    await userEvent.click(screen.getByRole("menuitem", { name: "Save view…" }));
    await settle();
    expect(screen.getByRole("button", { name: "Overdue 2" })).toBeTruthy();
  });

  it("bind:view opens the list in a view, once the user's are read; a name that isn't one is All", async () => {
    await store.set("elyra-ui.table.linked.views", [{ name: "Paid", filters: { status: ["paid"] } }]);
    const { rerender } = render(Table, { rows: invoices, columns, views: VIEWS, persist: "linked", view: "Paid" });
    await settle();
    expect(names()).toEqual(["Grace Hopper"]);
    await rerender({ rows: invoices, columns, views: VIEWS, persist: "linked", view: "Nope" });
    await settle();
    expect(names()).toHaveLength(4);
    expect(screen.getByRole("button", { name: "All" })).toBeTruthy();
  });

  it("without views there's no menu; without persist, no saving", async () => {
    render(Table, { rows: invoices, columns });
    expect(screen.queryByRole("button", { name: "All" })).toBeNull();
    render(Table, { rows: invoices, columns, views: VIEWS });
    await menu("All");
    expect(screen.queryByRole("menuitem", { name: "Save view…" })).toBeNull();
  });
});
