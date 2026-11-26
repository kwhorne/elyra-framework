/** DateRangePicker: two ends typed or picked, in order; the span between; presets; min and max. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import Fixture from "./test/DateRangePickerFixture.svelte";
import { defaultPresets, endOfMonth } from "./internal/ranges.js";

const from = () => screen.getByRole("textbox", { name: "Start date" });
const to = () => screen.getByRole("textbox", { name: "End date" });
const opener = () => screen.getByRole("button", { name: /^Choose dates/ });
const dialog = () => screen.getByRole("dialog");
const day = (iso) => dialog().querySelector(`[data-date="${iso}"]`);
const focused = () => document.activeElement.getAttribute("data-date");
const shown = () => JSON.parse(screen.getByTestId("value").textContent);

describe("ranges", () => {
  it("offer today, the last 7 and 30 days, this and last month, and this year", () => {
    expect(endOfMonth("2028-02-10")).toBe("2028-02-29");
    expect(defaultPresets("2026-03-04")).toEqual([
      { label: "Today", start: "2026-03-04", end: "2026-03-04" },
      { label: "Last 7 days", start: "2026-02-26", end: "2026-03-04" },
      { label: "Last 30 days", start: "2026-02-03", end: "2026-03-04" },
      { label: "This month", start: "2026-03-01", end: "2026-03-31" },
      { label: "Last month", start: "2026-02-01", end: "2026-02-28" },
      { label: "This year", start: "2026-01-01", end: "2026-12-31" },
    ]);
  });
});

describe("DateRangePicker", () => {
  it("shows both ends in the locale's numbers, in a group named by its field", () => {
    render(Fixture, { start: "2026-11-02", end: "2026-11-30", locale: "nb-NO" });
    expect(from().value).toBe("02.11.2026");
    expect(to().value).toBe("30.11.2026");
    expect(screen.getByRole("group", { name: "Period" })).toBeTruthy();
    // The label's click goes to the first field.
    expect(screen.getByText("Period").closest("label").htmlFor).toBe(from().id);
    expect(opener().getAttribute("aria-label")).toMatch(/^Choose dates, .*2026 to .*2026/);
  });

  it("takes typed ends as ISO, swaps them when the start is after the end, and leaves an end open", async () => {
    const onchange = vi.fn();
    render(Fixture, { locale: "nb-NO", onchange });
    await userEvent.type(from(), "1.3.2027");
    await userEvent.tab();
    expect(shown()).toEqual({ start: "2027-03-01", end: null });
    expect(onchange).toHaveBeenLastCalledWith({ start: "2027-03-01", end: null });
    await userEvent.type(to(), "2027-02-10{Enter}");
    expect(shown()).toEqual({ start: "2027-02-10", end: "2027-03-01" });
    expect(from().value).toBe("10.02.2027");
    expect(to().value).toBe("01.03.2027");
  });

  it("puts an end back when the typing isn't a date, and empties it when cleared", async () => {
    render(Fixture, { start: "2026-11-02", end: "2026-11-30", locale: "nb-NO" });
    await userEvent.clear(to());
    await userEvent.type(to(), "31.02.2026");
    await userEvent.tab();
    expect(to().value).toBe("30.11.2026");
    await userEvent.clear(to());
    await userEvent.tab();
    expect(shown()).toEqual({ start: "2026-11-02", end: null });
  });

  it("opens on two months, the span marked, and picks a first and a last day by click", async () => {
    const onchange = vi.fn();
    render(Fixture, { start: "2026-11-10", end: "2026-11-12", locale: "nb-NO", onchange });
    await userEvent.click(opener());
    expect(screen.getAllByRole("grid").map((g) => g.getAttribute("aria-label"))).toEqual([
      expect.stringMatching(/november 2026/i),
      expect.stringMatching(/desember 2026/i),
    ]);
    expect(focused()).toBe("2026-11-10");
    expect(["2026-11-10", "2026-11-11", "2026-11-12", "2026-11-13"].map((d) => day(d).getAttribute("aria-selected"))).toEqual([
      "true",
      "true",
      "true",
      "false",
    ]);
    // A day of another month isn't in this one's grid twice.
    expect(dialog().querySelectorAll('[data-date="2026-12-01"]')).toHaveLength(1);

    await userEvent.click(day("2026-12-03"));
    expect(within(dialog()).getByText("Pick the last day")).toBeTruthy();
    await userEvent.hover(day("2026-11-28"));
    expect(day("2026-11-30").getAttribute("aria-selected")).toBe("true");
    expect(day("2026-11-10").getAttribute("aria-selected")).toBe("false");
    await userEvent.click(day("2026-11-28"));
    expect(shown()).toEqual({ start: "2026-11-28", end: "2026-12-03" });
    expect(onchange).toHaveBeenLastCalledWith({ start: "2026-11-28", end: "2026-12-03" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener());
  });

  it("picks with the keys, the months moving on past the second", async () => {
    render(Fixture, { start: "2026-11-30", end: "2026-11-30", locale: "nb-NO" });
    await userEvent.click(opener());
    await userEvent.keyboard("{Enter}");
    await userEvent.keyboard("{PageDown}{PageDown}");
    expect(focused()).toBe("2027-01-30");
    expect(screen.getAllByRole("grid")[1].getAttribute("aria-label")).toMatch(/januar 2027/i);
    expect(day("2026-12-15").getAttribute("aria-selected")).toBe("true");
    await userEvent.keyboard("{ArrowRight}{Enter}");
    expect(shown()).toEqual({ start: "2026-11-30", end: "2027-01-31" });
  });

  it("closes on Esc without picking, and a click outside closes it", async () => {
    render(Fixture, { start: "2026-11-02", end: "2026-11-30", locale: "nb-NO" });
    await userEvent.click(opener());
    await userEvent.keyboard("{ArrowRight}{Enter}{ArrowRight}{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(shown()).toEqual({ start: "2026-11-02", end: "2026-11-30" });
    await userEvent.click(opener());
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("chooses a preset, held in min and max, and marks the one it is", async () => {
    const presets = [
      { label: "Q1", start: "2026-01-01", end: "2026-03-31" },
      { label: "Since March", start: "2026-03-01" },
    ];
    render(Fixture, { locale: "nb-NO", presets, min: "2026-02-01" });
    await userEvent.click(opener());
    await userEvent.click(within(dialog()).getByRole("button", { name: "Q1" }));
    expect(shown()).toEqual({ start: "2026-02-01", end: "2026-03-31" });
    await userEvent.click(opener());
    await userEvent.click(within(dialog()).getByRole("button", { name: "Since March" }));
    expect(shown()).toEqual({ start: "2026-03-01", end: null });
    await userEvent.click(opener());
    expect(within(dialog()).getByRole("button", { name: "Since March" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("offers the default presets from today, and none when given none", async () => {
    const { unmount } = render(Fixture, { locale: "nb-NO" });
    await userEvent.click(opener());
    expect(within(dialog()).getAllByRole("button", { name: /days|month|year|Today/ }).map((b) => b.textContent.trim())).toEqual(
      expect.arrayContaining(["Today", "Last 7 days", "This year"]),
    );
    unmount();
    render(Fixture, { locale: "nb-NO", presets: [] });
    await userEvent.click(opener());
    expect(within(dialog()).queryByRole("list", { name: "Ranges" })).toBeNull();
  });

  it("won't pick a day outside min and max, and clears with its button", async () => {
    render(Fixture, { start: "2026-11-10", end: "2026-11-12", locale: "nb-NO", min: "2026-11-05", max: "2026-11-20" });
    await userEvent.click(opener());
    expect(day("2026-11-04").getAttribute("aria-disabled")).toBe("true");
    await userEvent.click(day("2026-11-04"));
    expect(within(dialog()).getByText("Pick the first day")).toBeTruthy();
    await userEvent.keyboard("{Escape}");
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(shown()).toEqual({ start: null, end: null });
  });

  it("turns off both fields and the calendar when disabled", () => {
    render(Fixture, { start: "2026-11-10", disabled: true });
    expect(from().disabled).toBe(true);
    expect(to().disabled).toBe(true);
    expect(opener().disabled).toBe(true);
  });
});
