/** DatePicker: ISO in and out, typing in the locale's order, the calendar's grid and its keys, min and max. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import DatePickerFixture from "./test/DatePickerFixture.svelte";
import {
  addDays,
  addMonths,
  clamp,
  formatNumeric,
  localeWeekStart,
  monthGrid,
  numericPattern,
  parseTyped,
  parts,
  startOfWeek,
  today,
} from "./internal/dates.js";

describe("dates", () => {
  it("know a real ISO date from one that isn't", () => {
    expect(parts("2026-02-28")).toEqual({ y: 2026, m: 2, d: 28 });
    expect(parts("2026-02-29")).toBeNull();
    expect(parts("2028-02-29")).toEqual({ y: 2028, m: 2, d: 29 });
    expect(parts("2026-13-01")).toBeNull();
    expect(parts("2026-1-01")).toBeNull();
    expect(parts(null)).toBeNull();
  });

  it("move by days and months across months, years and daylight saving", () => {
    expect(addDays("2026-03-28", 2)).toBe("2026-03-30");
    expect(addDays("2026-10-24", 7)).toBe("2026-10-31");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
    expect(addMonths("2026-01-31", 1)).toBe("2026-02-28");
    expect(addMonths("2028-01-31", 1)).toBe("2028-02-29");
    expect(addMonths("2026-12-15", 1)).toBe("2027-01-15");
    expect(addMonths("2026-01-15", -1)).toBe("2025-12-15");
    expect(addMonths("2026-03-31", -13)).toBe("2025-02-28");
  });

  it("start a week on the locale's day", () => {
    // 2026-11-04 is a Wednesday.
    expect(startOfWeek("2026-11-04", 1)).toBe("2026-11-02");
    expect(startOfWeek("2026-11-04", 0)).toBe("2026-11-01");
    expect(startOfWeek("2026-11-02", 1)).toBe("2026-11-02");
    expect(localeWeekStart("nb-NO")).toBe(1);
    expect(localeWeekStart("en-US")).toBe(0);
    // A locale it can't read: the ISO week.
    expect(localeWeekStart("x-invalid-")).toBe(1);
  });

  it("lay out six weeks covering the month", () => {
    const weeks = monthGrid("2026-11-17", 1);
    expect(weeks).toHaveLength(6);
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    expect(weeks[0][0]).toBe("2026-10-26");
    expect(weeks.flat()).toContain("2026-11-01");
    expect(weeks.flat()).toContain("2026-11-30");
    expect(weeks[5][6]).toBe("2026-12-06");
  });

  it("hold a date between min and max", () => {
    expect(clamp("2026-01-01", "2026-02-01", undefined)).toBe("2026-02-01");
    expect(clamp("2026-03-01", undefined, "2026-02-01")).toBe("2026-02-01");
    expect(clamp("2026-02-10", "2026-02-01", "2026-02-28")).toBe("2026-02-10");
  });

  it("write and read a date in the locale's order, or ISO", () => {
    expect(formatNumeric("2026-11-30", "nb-NO")).toBe("30.11.2026");
    expect(formatNumeric("2026-11-30", "en-US")).toBe("11/30/2026");
    expect(numericPattern("nb-NO")).toBe("DD.MM.YYYY");
    expect(numericPattern("en-US")).toBe("MM/DD/YYYY");
    expect(parseTyped("30.11.2026", "nb-NO")).toBe("2026-11-30");
    expect(parseTyped("1/2/26", "nb-NO")).toBe("2026-02-01");
    expect(parseTyped("11/30/2026", "en-US")).toBe("2026-11-30");
    expect(parseTyped("2026-11-30", "en-US")).toBe("2026-11-30");
    expect(parseTyped("31.02.2026", "nb-NO")).toBeNull();
    expect(parseTyped("2026-02-31", "nb-NO")).toBeNull();
    expect(parseTyped("tomorrow", "nb-NO")).toBeNull();
    expect(parseTyped("1.2", "nb-NO")).toBeNull();
    expect(parseTyped("  ", "nb-NO")).toBeNull();
  });

  it("know today where the user is", () => {
    expect(today(new Date(2026, 10, 30, 23, 59))).toBe("2026-11-30");
    expect(today(new Date(2026, 0, 1, 0, 1))).toBe("2026-01-01");
  });
});

const field = () => screen.getByRole("textbox", { name: "Due" });
const opener = () => screen.getByRole("button", { name: /^Choose date/ });
const dialog = () => screen.getByRole("dialog");
const focused = () => document.activeElement?.getAttribute("data-date");
const shown = () => screen.getByTestId("value").textContent;

describe("DatePicker", () => {
  it("shows an ISO value in the locale's numbers, and the field is labelled", () => {
    render(DatePickerFixture, { value: "2026-11-30", locale: "nb-NO" });
    expect(field().value).toBe("30.11.2026");
    expect(field().getAttribute("placeholder")).toBe("DD.MM.YYYY");
    expect(opener().getAttribute("aria-label")).toMatch(/^Choose date, .*2026/);
    expect(opener().getAttribute("aria-haspopup")).toBe("dialog");
  });

  it("takes a typed date, in the locale's order or ISO, as ISO", async () => {
    const onchange = vi.fn();
    render(DatePickerFixture, { locale: "nb-NO", onchange });
    await userEvent.type(field(), "1.2.2027");
    await userEvent.tab();
    expect(shown()).toBe('"2027-02-01"');
    expect(field().value).toBe("01.02.2027");
    expect(onchange).toHaveBeenLastCalledWith("2027-02-01");
    await userEvent.clear(field());
    await userEvent.type(field(), "2027-03-04{Enter}");
    expect(shown()).toBe('"2027-03-04"');
  });

  it("puts back the value when the typing isn't a date, and clears when emptied", async () => {
    const onchange = vi.fn();
    render(DatePickerFixture, { value: "2026-11-30", locale: "nb-NO", onchange });
    await userEvent.clear(field());
    await userEvent.type(field(), "31.02.2026");
    await userEvent.tab();
    expect(field().value).toBe("30.11.2026");
    expect(shown()).toBe('"2026-11-30"');
    expect(onchange).not.toHaveBeenCalled();
    await userEvent.clear(field());
    await userEvent.tab();
    expect(shown()).toBe("null");
    expect(onchange).toHaveBeenLastCalledWith(null);
  });

  it("opens a calendar on the value, with the week as the locale has it", async () => {
    render(DatePickerFixture, { value: "2026-11-18", locale: "nb-NO" });
    await userEvent.click(opener());
    expect(opener().getAttribute("aria-expanded")).toBe("true");
    expect(dialog().getAttribute("aria-modal")).toBe("true");
    const grid = screen.getByRole("grid");
    expect(grid.getAttribute("aria-labelledby")).toBe(dialog().getAttribute("aria-labelledby"));
    expect(document.getElementById(grid.getAttribute("aria-labelledby")).textContent).toMatch(/november 2026/i);
    // Monday first in Norway.
    expect(screen.getAllByRole("columnheader")[0].getAttribute("abbr")).toMatch(/mandag/i);
    expect(focused()).toBe("2026-11-18");
    expect(document.activeElement.getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement.getAttribute("tabindex")).toBe("0");
  });

  it("moves with the grid's keys, across months, and Enter picks", async () => {
    const onchange = vi.fn();
    render(DatePickerFixture, { value: "2026-11-30", locale: "nb-NO", onchange });
    await userEvent.click(opener());
    await userEvent.keyboard("{ArrowRight}");
    expect(focused()).toBe("2026-12-01");
    expect(screen.getByRole("dialog").textContent).toMatch(/desember 2026/i);
    await userEvent.keyboard("{ArrowLeft}{ArrowUp}");
    expect(focused()).toBe("2026-11-23");
    await userEvent.keyboard("{ArrowDown}");
    expect(focused()).toBe("2026-11-30");
    await userEvent.keyboard("{Home}");
    expect(focused()).toBe("2026-11-30");
    await userEvent.keyboard("{End}");
    expect(focused()).toBe("2026-12-06");
    await userEvent.keyboard("{PageUp}");
    expect(focused()).toBe("2026-11-06");
    await userEvent.keyboard("{Shift>}{PageDown}{/Shift}");
    expect(focused()).toBe("2027-11-06");
    await userEvent.keyboard("{Enter}");
    expect(shown()).toBe('"2027-11-06"');
    expect(onchange).toHaveBeenLastCalledWith("2027-11-06");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener());
  });

  it("closes on Esc without picking, and keeps Tab inside", async () => {
    render(DatePickerFixture, { value: "2026-11-30", locale: "nb-NO" });
    await userEvent.click(opener());
    await userEvent.keyboard("{ArrowRight}");
    await userEvent.tab();
    expect(dialog().contains(document.activeElement)).toBe(true);
    await userEvent.tab();
    await userEvent.tab();
    await userEvent.tab();
    expect(dialog().contains(document.activeElement)).toBe(true);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(shown()).toBe('"2026-11-30"');
    expect(document.activeElement).toBe(opener());
  });

  it("pages with its buttons, picks with a click, and a click outside closes", async () => {
    render(DatePickerFixture, { value: "2026-11-30", locale: "nb-NO" });
    await userEvent.click(opener());
    await userEvent.click(screen.getByRole("button", { name: "Next month" }));
    await userEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(dialog().textContent).toMatch(/januar 2027/i);
    await userEvent.click(screen.getByRole("gridcell", { name: /\s15\. januar 2027/i }));
    expect(shown()).toBe('"2027-01-15"');
    await userEvent.click(opener());
    await userEvent.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("won't pick or take a day outside min and max", async () => {
    render(DatePickerFixture, { value: "2026-11-10", min: "2026-11-05", max: "2026-11-20", locale: "nb-NO" });
    await userEvent.click(opener());
    const early = screen.getByRole("gridcell", { name: /\s4\. november 2026/i });
    expect(early.getAttribute("aria-disabled")).toBe("true");
    await userEvent.click(early);
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(shown()).toBe('"2026-11-10"');
    await userEvent.keyboard("{Escape}");
    await userEvent.clear(field());
    await userEvent.type(field(), "21.11.2026");
    await userEvent.tab();
    expect(shown()).toBe('"2026-11-10"');
    expect(field().value).toBe("10.11.2026");
  });

  it("opens on today, held in range, when there's no value; ↓ in the field opens it too", async () => {
    render(DatePickerFixture, { min: "2099-01-01", locale: "nb-NO" });
    field().focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(focused()).toBe("2099-01-01");
    expect(screen.getByRole("button", { name: "Today" }).disabled).toBe(true);
  });

  it("marks today, and Today picks it", async () => {
    render(DatePickerFixture, { locale: "nb-NO" });
    await userEvent.click(opener());
    expect(document.activeElement.getAttribute("aria-current")).toBe("date");
    await userEvent.click(screen.getByRole("button", { name: "Today" }));
    expect(shown()).toBe(JSON.stringify(today()));
  });

  it("clears with its button, and starts the week on Sunday where the locale does", async () => {
    render(DatePickerFixture, { value: "2026-11-30", locale: "en-US" });
    expect(field().value).toBe("11/30/2026");
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(shown()).toBe("null");
    expect(field().value).toBe("");
    await userEvent.click(opener());
    expect(screen.getAllByRole("columnheader")[0].getAttribute("abbr")).toBe("Sunday");
  });

  it("turns off the field and the calendar when disabled", () => {
    render(DatePickerFixture, { disabled: true });
    expect(field().disabled).toBe(true);
    expect(opener().disabled).toBe(true);
  });
});
