/** Table: taking it out (RFC 0066). A menu, ⌘C on a row, and csv(), copy(), save(): numbers plain, as a spreadsheet takes them. */
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import Table from "./Table.svelte";
import { clipboard, files, menus, system, toast } from "./test/runtime.js";
import { speak } from "./test/runtime.js";
import { csvText, fileNameOf, plainNumber, separatorFor, tsvText } from "./internal/csv.js";

const invoices = [
  { id: 1, number: "2041", customer: "Ada Lovelace", due: "2026-11-05", amount: 12400.5, status: "paid" },
  { id: 2, number: "2042", customer: "Grace Hopper", due: "2026-11-07", amount: 860, status: "open" },
  { id: 3, number: "2043", customer: "Kristen \"KN\" Nygaard", due: "2026-11-12", amount: 3150, status: "open" },
];
const euros = (n) => `€${n}`;
const columns = [
  "number",
  "customer",
  "due",
  { key: "amount", label: "Amount", align: "end", format: euros },
  { key: "status", format: (s) => (s === "paid" ? "Paid" : "Open") },
];
const bodyRows = () => screen.getAllByRole("row").slice(1);
const settle = () => new Promise((r) => setTimeout(r, 0));

/** The component's own methods, as an app's `bind:this` has them. */
function mount(props) {
  const result = render(Table, { rows: invoices, columns, label: "Invoices", ...props });
  return result.component;
}

describe("csv.js", () => {
  it("CSV in the locale's separator, quoted where it must be; TSV with no tabs or breaks in a value", () => {
    expect(separatorFor("nb")).toBe(";");
    expect(csvText([["a", "b;c"], ['say "hi"', "x\ny"]], "nb")).toBe('a;"b;c"\r\n"say ""hi""";"x\ny"\r\n');
    expect(csvText([["a", "b;c"]], "en")).toBe("a,b;c\r\n");
    expect(tsvText([["a\tb", "c\r\nd"], ["e", null]])).toBe("a b\tc d\ne\t\n");
    expect(plainNumber("nb")(12400.5)).toBe("12400,5");
    expect(plainNumber("en")(1234567.25)).toBe("1234567.25");
    expect(fileNameOf({ label: "Invoices: November", extension: "csv", now: new Date(2026, 10, 28) })).toBe("Invoices 2026-11-28.csv");
    expect(fileNameOf({ extension: "csv", fallback: "table", now: new Date(2026, 10, 28) })).toBe("table 2026-11-28.csv");
  });
});

describe("Table out", () => {
  it("csv(): the label, the head, every row as sorted; numbers plain, dates as they are, the rest by format", async () => {
    const table = mount({ perPage: 2, query: { sort: "amount", direction: "desc" } });
    expect(table.csv()).toBe(
      [
        "Invoices",
        "Number,Customer,Due,Amount,Status",
        "2041,Ada Lovelace,2026-11-05,12400.5,Paid",
        '2043,"Kristen ""KN"" Nygaard",2026-11-12,3150,Open',
        "2042,Grace Hopper,2026-11-07,860,Open",
      ].join("\r\n") + "\r\n",
    );
  });

  it("in Norwegian: a semicolon, and a decimal comma", async () => {
    speak("nb", {});
    await settle();
    const table = mount({ label: undefined });
    expect(table.csv().split("\r\n")[1]).toBe("2041;Ada Lovelace;2026-11-05;12400,5;Paid");
  });

  it("a search narrows what's taken; a live page gives its page", async () => {
    const searched = mount({ query: { search: "grace" } });
    expect(searched.csv().trim().split("\r\n")).toHaveLength(3);
    const page = { data: invoices.slice(0, 2), total: 40, per_page: 2, current_page: 1, last_page: 20 };
    const live = render(Table, { page, columns: ["number"] }).component;
    expect(live.csv()).toBe("Number\r\n2041\r\n2042\r\n");
  });

  it("copy(): the table with its head, or the rows given without it, tab-separated", async () => {
    const table = mount();
    await table.copy();
    expect(clipboard.writeText).toHaveBeenLastCalledWith(
      "Number\tCustomer\tDue\tAmount\tStatus\n2041\tAda Lovelace\t2026-11-05\t12400.5\tPaid\n2042\tGrace Hopper\t2026-11-07\t860\tOpen\n2043\tKristen \"KN\" Nygaard\t2026-11-12\t3150\tOpen\n",
    );
    await table.copy([invoices[1]]);
    expect(clipboard.writeText).toHaveBeenLastCalledWith("2042\tGrace Hopper\t2026-11-07\t860\tOpen\n");
  });

  it("save(): the CSV, named by the label and the date", async () => {
    const table = mount();
    expect(await table.save()).toBe("/tmp/table.csv");
    const [data, options] = files.save.mock.calls[0];
    expect(data).toBe(table.csv());
    expect(options.defaultName).toMatch(/^Invoices \d{4}-\d{2}-\d{2}\.csv$/);
    expect(options.filters).toEqual([{ name: "CSV", extensions: ["csv"] }]);
  });

  it("the menu on a row: Copy row, Copy table, Save as CSV…", async () => {
    mount();
    await fireEvent.contextMenu(screen.getByText("Grace Hopper"));
    await settle();
    expect(menus).toHaveLength(1);
    const items = menus[0].items;
    expect(items.map((i) => i.label ?? "—")).toEqual(["Copy row", "Copy table", "—", "Save as CSV…"]);
    await items[0].action();
    await settle();
    expect(clipboard.writeText).toHaveBeenLastCalledWith("2042\tGrace Hopper\t2026-11-07\t860\tOpen\n");
    expect(toast).toHaveBeenLastCalledWith("Copied", { variant: "success" });
  });

  it("with rows selected, Copy is theirs, on any page; outside the shell there's no Save", async () => {
    system.here = false;
    mount({ select: true, selected: [1, 3], perPage: 2 });
    await fireEvent.contextMenu(screen.getByText("Ada Lovelace"));
    await settle();
    const items = menus[0].items;
    expect(items.map((i) => i.label)).toEqual(["Copy 2 rows", "Copy table"]);
  });

  it("⌘C on a focused row copies it, or the selected rows; not while text is selected", async () => {
    mount({ select: true });
    const [first, second] = bodyRows();
    first.focus();
    await userEvent.keyboard("{Meta>}c{/Meta}");
    await settle();
    expect(clipboard.writeText).toHaveBeenLastCalledWith("2041\tAda Lovelace\t2026-11-05\t12400.5\tPaid\n");

    second.focus();
    await userEvent.keyboard(" ");
    first.focus();
    await userEvent.keyboard(" ");
    await userEvent.keyboard("{Control>}c{/Control}");
    await settle();
    expect(clipboard.writeText).toHaveBeenLastCalledWith(
      "2041\tAda Lovelace\t2026-11-05\t12400.5\tPaid\n2042\tGrace Hopper\t2026-11-07\t860\tOpen\n",
    );

    const calls = clipboard.writeText.mock.calls.length;
    const range = document.createRange();
    range.selectNodeContents(screen.getByText("Grace Hopper"));
    window.getSelection().removeAllRanges();
    window.getSelection().addRange(range);
    await userEvent.keyboard("{Meta>}c{/Meta}");
    await settle();
    expect(clipboard.writeText.mock.calls.length).toBe(calls);
    window.getSelection().removeAllRanges();
  });

  it("Shift+F10 on a row opens the menu there", async () => {
    mount({ onrowclick: () => {} });
    bodyRows()[2].focus();
    await userEvent.keyboard("{Shift>}{F10}{/Shift}");
    await settle();
    expect(menus.at(-1).items[0].label).toBe("Copy row");
    await menus.at(-1).items[0].action();
    await settle();
    expect(clipboard.writeText.mock.calls.at(-1)[0]).toMatch(/^2043\t/);
  });

  it("a failure says why; exportable={false} has no menu and no ⌘C", async () => {
    clipboard.writeText.mockRejectedValueOnce(new Error("no clipboard"));
    mount();
    await fireEvent.contextMenu(screen.getByText("Ada Lovelace"));
    await settle();
    await menus[0].items[1].action();
    await settle();
    expect(toast).toHaveBeenLastCalledWith("Couldn't copy or save the table: no clipboard", { variant: "danger" });

    menus.length = 0;
    const off = render(Table, { rows: invoices, columns, exportable: false, onrowclick: () => {} });
    await fireEvent.contextMenu(off.getAllByText("Ada Lovelace").at(-1));
    await settle();
    expect(menus).toHaveLength(0);
  });

  it("a right click in a cell's own field keeps the browser's menu", async () => {
    render(Table, { rows: invoices, columns: ["number"], cell: undefined });
    await tick();
    const input = document.createElement("input");
    screen.getByText("2041").appendChild(input);
    await fireEvent.contextMenu(input);
    await settle();
    expect(menus).toHaveLength(0);
  });
});
