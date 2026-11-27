// Report pages (RFC 0064): blocks on pages, tables and lists split between
// their rows, a header and a footer given the page and the pages.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { paginate, paperOf, px } from "./internal/pages.js";
import ReportFixture from "./test/ReportFixture.svelte";

const room = (h) => () => h;

describe("paginate", () => {
  it("puts each block whole where it fits, else on the next page", () => {
    expect(paginate([{ height: 40 }, { height: 50 }, { height: 30 }], room(100))).toEqual([
      [{ block: 0 }, { block: 1 }],
      [{ block: 2 }],
    ]);
  });

  it("a page's own room: a first page with a taller letterhead holds less", () => {
    const rooms = (page) => (page === 0 ? 60 : 100);
    expect(paginate([{ height: 50 }, { height: 50 }, { height: 50 }], rooms)).toEqual([
      [{ block: 0 }],
      [{ block: 1 }, { block: 2 }],
    ]);
  });

  it("splits a table between its rows, its head on each part", () => {
    const table = { height: 10 + 8 * 20, fixed: 10, units: Array(8).fill(20) };
    expect(paginate([{ height: 30 }, table, { height: 20 }], room(100))).toEqual([
      // 30 used: the head (10) and three rows (60) fit in the 70 left.
      [{ block: 0 }, { block: 1, from: 0, to: 3 }],
      [{ block: 1, from: 3, to: 7 }],
      [{ block: 1, from: 7, to: 8 }, { block: 2 }],
    ]);
  });

  it("starts a table on the next page when not one row fits", () => {
    const table = { height: 10 + 2 * 20, fixed: 10, units: [20, 20] };
    expect(paginate([{ height: 85 }, table], room(100))).toEqual([[{ block: 0 }], [{ block: 1, from: 0, to: 2 }]]);
  });

  it("a block taller than a page goes alone, clipped", () => {
    expect(paginate([{ height: 20 }, { height: 250 }, { height: 20 }], room(100))).toEqual([
      [{ block: 0 }],
      [{ block: 1, clipped: true }],
      [{ block: 2 }],
    ]);
  });

  it("no blocks is one empty page", () => {
    expect(paginate([], room(100))).toEqual([[]]);
  });
});

describe("paper", () => {
  it("A4 and letter in millimetres, turned for landscape, and as CSS pixels", () => {
    expect(paperOf()).toEqual({ width: 210, height: 297 });
    expect(paperOf("letter", true)).toEqual({ width: 279.4, height: 215.9 });
    expect(Math.round(px(210))).toBe(794);
  });
});

describe("Report", () => {
  beforeEach(() => {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function () {
      const height = Number(this.getAttribute?.("data-h") ?? 0);
      return { height, width: 0, top: 0, left: 0, right: 0, bottom: height, x: 0, y: 0, toJSON() {} };
    });
  });
  afterEach(() => vi.restoreAllMocks());

  const settle = async () => {
    for (let i = 0; i < 6; i++) await tick();
  };

  it("lays its body out on A4 pages, the table split between rows, its head on each part", async () => {
    // An A4 page with no margins holds 1122.5 px: 600, then 600 alone (522 left
    // for the table's head and four rows), then the other six rows.
    render(ReportFixture);
    await settle();
    const sheets = screen.getAllByRole("group");
    expect(sheets.map((s) => s.getAttribute("aria-label"))).toEqual(["Page 1 of 3", "Page 2 of 3", "Page 3 of 3"]);
    const body = (i) => sheets[i].querySelector(".body");
    expect(body(0).textContent).toBe("Ada Lovelace");
    expect(body(1).querySelectorAll("tbody tr")).toHaveLength(4);
    expect(body(2).querySelectorAll("tbody tr")).toHaveLength(6);
    expect(body(2).querySelector("thead").textContent).toBe("Item");
    expect(body(2).querySelector("tbody tr").textContent).toBe("Part 5");
    // Copies: no id twice on the page.
    expect(document.querySelectorAll("#first")).toHaveLength(1);
    expect(sheets.map((s) => s.querySelector(".foot").textContent)).toEqual(["Page 1 of 3", "Page 2 of 3", "Page 3 of 3"]);
    expect(sheets[1].querySelector(".head").textContent).toBe("Invoice 1042, continued");
  });

  it("is laid out again when the body changes, and says how many pages", async () => {
    let pages = 0;
    const { rerender } = render(ReportFixture, { rows: 2, get pages() { return pages; }, set pages(v) { pages = v; } });
    await settle();
    expect(screen.getAllByRole("group")).toHaveLength(2);
    await rerender({ rows: 20 });
    await new Promise((r) => setTimeout(r, 60));
    await settle();
    expect(screen.getAllByRole("group")).toHaveLength(4);
  });

  it("carries its paper for print and pdf (RFC 0062)", async () => {
    render(ReportFixture);
    await settle();
    expect(document.querySelector(".elyra-report").getAttribute("data-elyra-report")).toBe("a4 portrait");
  });
});
