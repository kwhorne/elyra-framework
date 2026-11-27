// Taking a chart out of the app (RFC 0060): the menu, the files, the
// clipboard, the CSV in the app's locale, and the methods for an app's button.
import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { csvOf, fileNameOf, separatorFor, svgOf } from "./internal/export.js";
import { themeOf } from "./internal/theme.js";
import { files, clipboard, state, toast } from "./test/system.js";
import { speak } from "./test/words.js";
import { LineChart, FunnelChart } from "./index.js";
import Harness from "./test/ExportHarness.svelte";

const revenue = { series: [{ name: "Revenue", data: [1200.5, 1800] }], categories: ["Jan", "Feb"] };
const settle = async () => {
  for (let i = 0; i < 5; i++) await tick();
  await new Promise((r) => setTimeout(r, 0));
  await tick();
};
// A menu left open by one test isn't the next one's.
afterEach(() => document.querySelectorAll(".elyra-ctx").forEach((m) => m.remove()));
const menuItems = () => [...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent);
const choose = (label) => [...document.querySelectorAll('[role="menuitem"]')].find((e) => e.textContent === label).click();

describe("the CSV", () => {
  it("separates as the locale's spreadsheet reads it", () => {
    expect(separatorFor("en")).toBe(",");
    expect(separatorFor("nb")).toBe(";");
  });

  it("is the caption, the head and the rows, quoted where it must be", () => {
    const table = { columns: ["Revenue", 'Cost "net"'], rows: [["Jan", ["1200.5", "3,4"]]] };
    expect(csvOf(table, { caption: "Q1", head: "Category", locale: "en" })).toBe(
      'Q1\r\nCategory,Revenue,"Cost ""net"""\r\nJan,1200.5,"3,4"\r\n',
    );
  });

  it("names its file by the label or the summary, and the date", () => {
    const now = new Date(2026, 10, 27);
    expect(fileNameOf({ label: "Revenue by month", extension: "png", now })).toBe("Revenue by month 2026-11-27.png");
    expect(fileNameOf({ summary: "Revenue: from 1 to 2, between", extension: "csv", now })).toBe("Revenue 2026-11-27.csv");
    expect(fileNameOf({ extension: "svg", now })).toBe("chart 2026-11-27.svg");
  });

  it("the font's names are in single quotes, so an SVG's style attribute holds them", () => {
    document.documentElement.style.setProperty("--elyra-font", '-apple-system, "Segoe UI", sans-serif');
    expect(themeOf(document.documentElement).font).toBe("-apple-system, 'Segoe UI', sans-serif");
    document.documentElement.style.removeProperty("--elyra-font");
  });

  it("an SVG gets the surface behind it", () => {
    const chart = { renderToSVGString: () => '<svg width="4"><path/></svg>' };
    expect(svgOf(chart, "#1a1b26")).toBe('<svg width="4"><rect width="100%" height="100%" fill="#1a1b26"/><path/></svg>');
  });
});

describe("the menu", () => {
  it("offers images, the data and files where the shell can", async () => {
    render(LineChart, { ...revenue });
    await settle();
    screen.getByRole("group").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    expect(menuItems()).toEqual(["Copy as image", "Save as PNG…", "Save as SVG…", "Copy data", "Save data as CSV…"]);
    choose("Save data as CSV…");
    await settle();
    const [data, options] = files.save.mock.calls[0];
    // The summary is the caption, quoted for its commas; the numbers are plain.
    expect(data).toBe(
      '"Revenue: from 1,200.5 to 1,800, between 1,200.5 and 1,800, over 2 points"\r\nCategory,Revenue\r\nJan,1200.5\r\nFeb,1800\r\n',
    );
    expect(options.filters).toEqual([{ name: "CSV", extensions: ["csv"] }]);
    expect(options.defaultName).toMatch(/^Revenue \d{4}-\d\d-\d\d\.csv$/);
  });

  it("offers only the data without the system feature", async () => {
    state.system = false;
    render(FunnelChart, { series: [3, 1], labels: ["Open", "Closed"] });
    await settle();
    screen.getByRole("group").dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true }));
    await settle();
    expect(menuItems()).toEqual(["Copy data"]);
  });

  it("isn't there with exportable={false}", async () => {
    render(LineChart, { ...revenue, exportable: false });
    await settle();
    screen.getByRole("group").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    expect(menuItems()).toEqual([]);
  });

  it("Norwegian: its words, and a spreadsheet's semicolons and decimal commas", async () => {
    speak("nb", {
      "elyra.charts.copyData": "Kopier data",
      "elyra.charts.saveCsv": "Lagre data som CSV…",
      "elyra.charts.copied": "Kopiert",
    });
    render(LineChart, { ...revenue, label: "Omsetning" });
    await settle();
    screen.getByRole("group").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    choose("Kopier data");
    await settle();
    expect(clipboard.writeText.mock.calls[0][0]).toBe("Omsetning\r\nCategory;Revenue\r\nJan;1200,5\r\nFeb;1800\r\n");
    expect(toast).toHaveBeenCalledWith("Kopiert", { variant: "success" });
  });

  it("says so when it can't", async () => {
    clipboard.writeImage.mockRejectedValueOnce(new Error("no clipboard"));
    render(LineChart, { ...revenue, renderer: "canvas" });
    await settle();
    screen.getByRole("group").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();
    expect(menuItems()).not.toContain("Save as SVG…");
    choose("Copy as image");
    await settle();
    expect(toast).toHaveBeenCalledWith("Couldn't export the chart: no clipboard", { variant: "danger" });
  });
});

describe("for the app's own button", () => {
  it("save, copy and csv", async () => {
    const { component } = render(Harness, { ...revenue });
    await settle();
    await component.chart.save("svg");
    const [svg, options] = files.save.mock.calls[0];
    expect(svg).toContain('<rect width="100%" height="100%"');
    expect(options.defaultName).toMatch(/\.svg$/);
    expect(component.chart.csv()).toContain("Jan,1200.5");
  });

  it("copy puts a PNG on the clipboard", async () => {
    const { component } = render(Harness, { ...revenue, renderer: "canvas" });
    await settle();
    await component.chart.copy();
    const png = clipboard.writeImage.mock.calls[0][0];
    expect([...png.slice(0, 4)]).toEqual([137, 80, 78, 71]);
  });
});
