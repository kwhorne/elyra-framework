/** Sparkline, Stat, Progress and Breadcrumbs: what they draw and what a screen reader hears. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import Sparkline from "./Sparkline.svelte";
import Stat from "./Stat.svelte";
import Progress from "./Progress.svelte";
import Breadcrumbs from "./Breadcrumbs.svelte";

describe("Sparkline", () => {
  it("draws a line through the numbers, from the lowest at the bottom to the highest at the top", () => {
    const { container } = render(Sparkline, { values: [1, 3, 2], width: 100, height: 20 });
    const d = container.querySelector("path.stroke").getAttribute("d");
    const ys = [...d.matchAll(/[ML][\d.]+ ([\d.]+)/g)].map((m) => Number(m[1]));
    expect(ys[1]).toBeLessThan(ys[2]);
    expect(ys[2]).toBeLessThan(ys[0]);
    expect(ys[0]).toBeCloseTo(18.5);
    expect(ys[1]).toBeCloseTo(1.5);
  });

  it("says where the numbers went, or what it's given", () => {
    render(Sparkline, { values: [4, 9, 2, 7] });
    expect(screen.getByRole("img", { name: "From 4 to 7, between 2 and 9" })).toBeTruthy();
  });

  it("leaves out what isn't a number, and draws a flat series through the middle", () => {
    const { container } = render(Sparkline, { values: [5, null, 5, NaN, 5], height: 20 });
    const d = container.querySelector("path.stroke").getAttribute("d");
    expect(d.split("L")).toHaveLength(3);
    expect([...d.matchAll(/ ([\d.]+)/g)].every((m) => Number(m[1]) === 10)).toBe(true);
  });

  it("draws a bar for each number, and nothing for none", () => {
    const { container, unmount } = render(Sparkline, { values: [1, 2, 3], kind: "bars" });
    expect(container.querySelectorAll("rect")).toHaveLength(3);
    unmount();
    render(Sparkline, { values: [] });
    expect(screen.getByRole("img", { name: "No data" }).children).toHaveLength(0);
  });

  it("colours itself by which way the numbers went, with tone auto", () => {
    const { container: up } = render(Sparkline, { values: [1, 4], tone: "auto" });
    const { container: down } = render(Sparkline, { values: [4, 1], tone: "auto" });
    expect(up.querySelector("svg").classList.contains("success")).toBe(true);
    expect(down.querySelector("svg").classList.contains("danger")).toBe(true);
  });
});

describe("Stat", () => {
  it("shows the label, the value and how it changed", () => {
    render(Stat, { label: "Revenue", value: 48200, change: 0.12, caption: "vs. last month" });
    const group = screen.getByRole("group", { name: "Revenue" });
    expect(group.textContent).toContain((48200).toLocaleString());
    expect(screen.getByLabelText("Up 12% vs. last month").className).toContain("success");
  });

  it("colours a fall as good news where down is good", () => {
    render(Stat, { label: "Open tickets", value: 14, change: -0.25, good: "down" });
    expect(screen.getByLabelText("Down 25%").className).toContain("success");
  });

  it("formats the value, and draws its numbers", () => {
    const { container } = render(Stat, {
      label: "Latency",
      value: 182,
      format: (v) => `${v} ms`,
      values: [200, 190, 182],
    });
    expect(container.textContent).toContain("182 ms");
    expect(screen.getByRole("img", { name: "Latency: 200, 190, 182" })).toBeTruthy();
  });
});

describe("Progress", () => {
  it("is a progress bar of its value, kept within 0 and max", () => {
    render(Progress, { label: "Uploading", value: 150, max: 120 });
    const bar = screen.getByRole("progressbar", { name: "Uploading" });
    expect(bar.getAttribute("aria-valuenow")).toBe("120");
    expect(bar.getAttribute("aria-valuetext")).toBe("100 %");
  });

  it("is busy, without a value, while how long is unknown", () => {
    render(Progress, { label: "Syncing" });
    const bar = screen.getByRole("progressbar", { name: "Syncing" });
    expect(bar.getAttribute("aria-valuenow")).toBeNull();
    expect(bar.getAttribute("aria-busy")).toBe("true");
  });

  it("as a meter, turns warning and then danger as it fills", () => {
    const shade = (value) => {
      const { container, unmount } = render(Progress, { kind: "meter", label: "Disk", value, low: 70, high: 90 });
      const bar = container.querySelector(".bar");
      const out = ["accent", "warning", "danger"].find((t) => bar.classList.contains(t));
      unmount();
      return out;
    };
    expect([shade(40), shade(75), shade(95)]).toEqual(["accent", "warning", "danger"]);
  });

  it("as a meter where less is worse, the other way round", () => {
    const { container } = render(Progress, { kind: "meter", label: "Battery", value: 8, low: 30, high: 10 });
    expect(screen.getByRole("meter", { name: "Battery" })).toBeTruthy();
    expect(container.querySelector(".bar").classList.contains("danger")).toBe(true);
  });

  it("shows its label and value beside it, formatted", () => {
    const { container } = render(Progress, { label: "Storage", value: 3, max: 5, showValue: true, format: (v, m) => `${v} of ${m} GB` });
    expect(container.textContent).toContain("Storage");
    expect(container.textContent).toContain("3 of 5 GB");
  });
});

describe("Breadcrumbs", () => {
  const trail = [
    { label: "Customers", path: "/customers" },
    { label: "Acme", path: "/customers/7" },
    { label: "Invoices", href: "https://billing.example.com/acme" },
    { label: "2026-114", path: "/customers/7/invoices/114" },
    { label: "Lines" },
  ];

  it("links each step through the router or out, and the last is this page", () => {
    render(Breadcrumbs, { items: trail, max: 9 });
    const nav = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(screen.getByRole("link", { name: "Customers" }).getAttribute("href")).toBe("#/customers");
    expect(screen.getByRole("link", { name: "Invoices" }).getAttribute("href")).toBe("https://billing.example.com/acme");
    expect(nav.querySelector('[aria-current="page"]').textContent).toBe("Lines");
    expect(screen.queryByRole("link", { name: "Lines" })).toBeNull();
  });

  it("folds the middle of a long trail, and says what's folded", () => {
    const { container } = render(Breadcrumbs, { items: trail, max: 3 });
    const labels = [...container.querySelectorAll("li")].map((li) => li.textContent.trim());
    expect(labels).toEqual(["Customers", "…", "2026-114", "Lines"]);
    expect(container.querySelector(".folded").getAttribute("title")).toBe("Acme › Invoices");
  });
});
