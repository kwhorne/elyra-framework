/** Table: totals in its foot (RFC 0067). A column's sum, mean or count, over what the table has; printed and copied with it. */
import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import Table from "./Table.svelte";
import ReportFixture from "./test/ReportFixture.svelte";
import { speak } from "./test/runtime.js";
import { hasTotal, totalOf, totalsOf, totalText } from "./internal/totals.js";

const lines = [
  { id: 1, item: "Bolt", qty: 4, price: 0.1, amount: 0.4, note: "" },
  { id: 2, item: "Nut", qty: 10, price: 0.2, amount: 2, note: "urgent" },
  { id: 3, item: "Gear", qty: null, price: 12.5, amount: 0, note: null },
  { id: 4, item: "Spring", qty: 1, price: "n/a", amount: 3.3, note: "back order" },
];
const kr = (n) => (typeof n === "number" ? `${n.toFixed(2)} kr` : String(n));
const columns = [
  "item",
  { key: "qty", align: "end", total: "sum" },
  { key: "price", align: "end", total: "max", format: kr },
  { key: "amount", align: "end", total: "sum", format: kr },
  { key: "note", total: "count" },
];
const foot = () => [...document.querySelectorAll("tfoot tr")].map((tr) => [...tr.children].map((c) => c.textContent.trim()));

describe("totals.js", () => {
  it("sum, avg, min, max and count; gaps and text left out; a sum to its values' decimals", () => {
    const col = (total, key = "price") => ({ key, total });
    expect(totalOf(lines, col("sum"))).toBe(12.8);
    expect(totalOf([{ v: 0.1 }, { v: 0.2 }], col("sum", "v"))).toBe(0.3);
    expect(totalOf(lines, col("avg"))).toBeCloseTo(4.2667, 3);
    expect(totalOf(lines, col("min"))).toBe(0.1);
    expect(totalOf(lines, col("max"))).toBe(12.5);
    expect(totalOf(lines, col("count", "note"))).toBe(2);
    expect(totalOf([], col("sum"))).toBe(0);
    expect(totalOf([], col("avg"))).toBeNull();
    expect(totalOf(lines, { key: "qty", total: (rows) => rows.length * 2 })).toBe(8);
    expect(hasTotal({ total: "median" })).toBe(false);
    expect(totalsOf(lines, [{ key: "qty", total: "sum" }, { key: "item" }])).toEqual({ qty: 15 });
  });

  it("as shown: by the column's format, except a count", () => {
    expect(totalText(5.7, { key: "amount", total: "sum", format: kr }, "en")).toBe("5.70 kr");
    expect(totalText(1200, { key: "note", total: "count", format: kr }, "en")).toBe("1,200");
    expect(totalText(null, { key: "x", total: "avg" }, "en")).toBe("");
    // A format that reads the row is given one with the total in it.
    expect(totalText(3, { key: "qty", total: "sum", format: (v, row) => `${row.qty} pcs` }, "en")).toBe("3 pcs");
  });
});

describe("Table totals", () => {
  it("a foot under the columns, over every row as searched, not the page", async () => {
    render(Table, { rows: lines, columns, perPage: 2 });
    expect(foot()).toEqual([["Total", "15", "12.50 kr", "5.70 kr", "2"]]);
    const th = document.querySelector("tfoot th");
    expect(th.getAttribute("scope")).toBe("row");
    expect(document.querySelector("tfoot td:nth-child(2)").classList.contains("end")).toBe(true);
  });

  it("a search totals what's found", async () => {
    render(Table, { rows: lines, columns, search: true, query: { search: "n" } });
    // "n" finds Nut and Spring: 10 + 1.
    expect(foot()[0].slice(0, 2)).toEqual(["Total", "11"]);
  });

  it("a live page: the server's totals, else its own, said as This page", () => {
    const page = { data: lines.slice(0, 2), total: 400, per_page: 2, current_page: 1, last_page: 200 };
    const { unmount } = render(Table, { page, columns: ["item", { key: "qty", total: "sum" }] });
    expect(foot()).toEqual([["This page", "14"]]);
    unmount();
    render(Table, { page: { ...page, totals: { qty: 1234 } }, columns: ["item", { key: "qty", total: "sum" }] });
    expect(foot()).toEqual([["Total", "1234"]]);
  });

  it("with rows selected, a line for them; the first column's own total goes after the words", async () => {
    render(Table, { rows: lines, columns: [{ key: "qty", total: "sum" }, "item"], select: true, selected: [1, 2] });
    await tick();
    expect(foot()).toEqual([
      ["", "Total: 15", ""],
      ["", "Selected (2): 14", ""],
    ]);
    expect(document.querySelectorAll("tfoot tr.selection")).toHaveLength(1);
  });

  it("no total, no foot; nothing loaded, no foot", () => {
    render(Table, { rows: lines, columns: ["item", "qty"] });
    expect(document.querySelector("tfoot")).toBeNull();
    render(Table, { rows: [], columns });
    expect(document.querySelectorAll("tfoot")).toHaveLength(0);
  });

  it("in Norwegian", async () => {
    speak("nb", { "elyra.table.totalRow": "Sum" });
    await new Promise((r) => setTimeout(r, 0));
    render(Table, { rows: lines, columns });
    expect(foot()[0][0]).toBe("Sum");
  });

  it("the CSV and Copy table end with it; a copied row doesn't", async () => {
    const { component } = render(Table, { rows: lines, columns, label: "Order" });
    const csv = component.csv().trim().split("\r\n");
    expect(csv.at(-1)).toBe("Total,15,12.5,5.7,2");
    expect(csv).toHaveLength(2 + lines.length + 1);
  });
});

describe("a Report's split table", () => {
  afterEach(() => vi.restoreAllMocks());

  it("keeps its foot for the last part", async () => {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function () {
      const height = Number(this.getAttribute?.("data-h") ?? 0);
      return { height, width: 0, top: 0, left: 0, right: 0, bottom: height, x: 0, y: 0, toJSON() {} };
    });
    render(ReportFixture, { foot: true });
    for (let i = 0; i < 6; i++) await tick();
    const sheets = screen.getAllByRole("group");
    const feet = sheets.map((s) => s.querySelectorAll(".body tfoot").length);
    expect(feet).toEqual([0, 0, 1]);
  });
});
