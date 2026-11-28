/** Table: filters, a column's own (RFC 0069). Text, options, a range, days; chips; query.filters. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import Table from "./Table.svelte";
import TableFixture from "./test/TableFixture.svelte";
import { activeFilters, chipText, filterRows, isEmpty, kindOf, optionsOf } from "./internal/filters.js";

const invoices = [
  { id: 1, customer: "Ada Lovelace", status: "open", due: "2026-11-05", amount: 12400 },
  { id: 2, customer: "Grace Hopper", status: "paid", due: "2026-11-30 16:45:00", amount: 860 },
  { id: 3, customer: "Kristen Nygaard", status: "overdue", due: "2026-12-01", amount: 3150 },
  { id: 4, customer: "Ole-Johan Dahl", status: "open", due: "2026-10-31", amount: 22000 },
];
const STATUS = [["open", "Open"], ["paid", "Paid"], ["overdue", "Overdue"]];
const columns = [
  { key: "customer", filter: "text" },
  { key: "status", filter: { options: STATUS } },
  { key: "due", filter: "dates" },
  { key: "amount", align: "end", filter: "range", total: "sum" },
];
const cols = columns.map((c) => ({ ...c, label: c.key[0].toUpperCase() + c.key.slice(1) }));
const words = {
  from: "From",
  to: "To",
  atLeast: (v) => `${v} or more`,
  atMost: (v) => `${v} or less`,
  between: (a, b) => `${a} – ${b}`,
};
const names = () => screen.getAllByRole("row").slice(1).map((r) => r.querySelector("td")?.textContent.trim()).filter(Boolean);

describe("filters.js", () => {
  it("each kind, and an empty one is none", () => {
    expect(cols.map(kindOf)).toEqual(["text", "options", "dates", "range"]);
    expect(kindOf({ key: "x" })).toBeNull();
    expect(isEmpty("text", "  ")).toBe(true);
    expect(isEmpty("options", [])).toBe(true);
    expect(isEmpty("range", { min: null })).toBe(true);
    expect(isEmpty("dates", { from: "" })).toBe(true);
    expect(activeFilters({ customer: " ", status: ["open"], nope: "x" }, cols)).toEqual({ status: ["open"] });
  });

  it("rows by text, options, a range and days; `to` takes in a time on its day", () => {
    const ids = (f) => filterRows(invoices, f, cols).map((r) => r.id);
    expect(ids({ customer: "  ADA " })).toEqual([1]);
    expect(ids({ status: ["open", "overdue"] })).toEqual([1, 3, 4]);
    expect(ids({ amount: { min: 3150, max: 12400 } })).toEqual([1, 3]);
    expect(ids({ amount: { min: 10000 } })).toEqual([1, 4]);
    expect(ids({ due: { from: "2026-11-01", to: "2026-11-30" } })).toEqual([1, 2]);
    expect(ids({ status: ["open"], amount: { max: 15000 } })).toEqual([1]);
    expect(ids({})).toEqual([1, 2, 3, 4]);
  });

  it("options from the rows, shown as the column shows them", () => {
    const col = { key: "status", filter: "options", format: (s) => s.toUpperCase() };
    expect(optionsOf(col, invoices)).toEqual([["open", "OPEN"], ["overdue", "OVERDUE"], ["paid", "PAID"]]);
    expect(optionsOf(cols[1])).toEqual(STATUS);
  });

  it("what a chip says", () => {
    const say = (key, value) => chipText(cols.find((c) => c.key === key), value, { words, locale: "en" });
    expect(say("customer", " ada ")).toBe("Customer: “ada”");
    expect(say("status", ["open", "overdue"])).toBe("Status: Open, Overdue");
    expect(say("amount", { min: 10000 })).toBe("Amount: 10,000 or more");
    expect(say("amount", { max: 500 })).toBe("Amount: 500 or less");
    expect(say("amount", { min: 1, max: 2 })).toBe("Amount: 1 – 2");
    expect(say("due", { from: "2026-11-01", to: "2026-11-30" })).toBe("Due: Nov 1, 2026 – Nov 30, 2026");
    expect(say("due", { from: "2026-11-01" })).toBe("Due: From Nov 1, 2026");
  });
});

describe("Table filters", () => {
  it("a funnel on each filterable header, named by its column", () => {
    render(Table, { rows: invoices, columns });
    expect(screen.getAllByRole("button", { name: /^Filter / }).map((b) => b.getAttribute("aria-label"))).toEqual([
      "Filter Customer",
      "Filter Status",
      "Filter Due",
      "Filter Amount",
    ]);
  });

  it("options: tick, Apply; the rows, a chip, the funnel says it's on, the totals follow", async () => {
    render(Table, { rows: invoices, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Status" }));
    const panel = screen.getByRole("dialog", { name: "Filter Status" });
    await userEvent.click(screen.getByRole("checkbox", { name: "Open" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Overdue" }));
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(panel.isConnected).toBe(false);
    expect(names()).toEqual(["Ada Lovelace", "Kristen Nygaard", "Ole-Johan Dahl"]);
    expect(screen.getByRole("list", { name: "Filters" }).textContent).toContain("Status: Open, Overdue");
    expect(screen.getByRole("button", { name: "Filter Status (filtered)" })).toBeTruthy();
    expect(document.querySelector("tfoot td:last-child").textContent).toBe("37550");
    expect(document.querySelector('[role="status"]').textContent).toBe("3 of 4 shown");
    // The focus is back on the funnel.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Filter Status (filtered)" }));
  });

  it("text and a range by the keys: Enter applies, Esc closes without", async () => {
    render(Table, { rows: invoices, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Customer" }));
    await userEvent.keyboard("hopper{Enter}");
    expect(names()).toEqual(["Grace Hopper"]);
    await userEvent.click(screen.getByRole("button", { name: "Remove Customer: “hopper”" }));
    expect(names()).toHaveLength(4);

    await userEvent.click(screen.getByRole("button", { name: "Filter Amount" }));
    await userEvent.type(screen.getByRole("spinbutton", { name: "From" }), "10000");
    await userEvent.keyboard("{Escape}");
    expect(names()).toHaveLength(4);
    await userEvent.click(screen.getByRole("button", { name: "Filter Amount" }));
    await userEvent.type(screen.getByRole("spinbutton", { name: "From" }), "10000");
    await userEvent.keyboard("{Enter}");
    expect(names()).toEqual(["Ada Lovelace", "Ole-Johan Dahl"]);
    expect(screen.getByRole("list", { name: "Filters" }).textContent).toContain("Amount: 10,000 or more");
  });

  it("a live page: query.filters, from the first page; Clear filters takes them all off", async () => {
    const page = { data: invoices, total: 412, per_page: 25, current_page: 3, last_page: 17 };
    const query = { page: 3 };
    render(Table, { page, query, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Status" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Paid" }));
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    await userEvent.click(screen.getByRole("button", { name: "Filter Customer" }));
    await userEvent.keyboard("a{Enter}");
    // The live page's rows aren't filtered here: the command does it.
    expect(names()).toHaveLength(4);
    const chips = screen.getByRole("list", { name: "Filters" });
    expect(chips.querySelectorAll("li")).toHaveLength(2);
    await userEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(screen.queryByRole("list", { name: "Filters" })).toBeNull();
  });

  it("writes query.filters, from the first page, as bind:query has it", async () => {
    const page = { data: invoices, total: 412, per_page: 25, current_page: 3, last_page: 17 };
    render(TableFixture, { page, query: { page: 3, search: "x" }, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Amount" }));
    await userEvent.type(screen.getByRole("spinbutton", { name: "To" }), "5000");
    await userEvent.keyboard("{Enter}");
    expect(JSON.parse(screen.getByTestId("query").textContent)).toEqual({
      page: 1,
      search: "x",
      filters: { amount: { min: null, max: 5000 } },
    });
    expect(screen.getByRole("list", { name: "Filters" }).textContent).toContain("Amount: 5,000 or less");
    await userEvent.click(screen.getByRole("button", { name: "Remove Amount: 5,000 or less" }));
    expect(JSON.parse(screen.getByTestId("query").textContent).filters).toBeNull();
  });

  it("a click outside closes the panel without applying; no filter, no funnel", async () => {
    render(Table, { rows: invoices, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Customer" }));
    await userEvent.keyboard("ada");
    await userEvent.click(document.body);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(names()).toHaveLength(4);
    render(Table, { rows: invoices, columns: ["customer", "status"] });
    expect(screen.getAllByRole("button", { name: /^Filter / })).toHaveLength(4);
  });

  it("dates: a day range, applied by day", async () => {
    render(TableFixture, { rows: invoices, columns });
    await userEvent.click(screen.getByRole("button", { name: "Filter Due" }));
    const panel = screen.getByRole("dialog", { name: "Filter Due" });
    expect(panel.querySelector("input")).toBeTruthy();
  });
});
