// The spans a DateRangePicker offers a click away, from today: ISO dates in,
// `{ label, start, end }` out.

import { addDays, addMonths, startOfMonth } from "./dates.js";

/** The last day of `day`'s month. */
export const endOfMonth = (day) => addDays(addMonths(startOfMonth(day), 1), -1);

const ENGLISH = {
  today: "Today",
  last7: "Last 7 days",
  last30: "Last 30 days",
  thisMonth: "This month",
  lastMonth: "Last month",
  thisYear: "This year",
};

/**
 * Today, the last 7 and 30 days (today among them), this and last month, this
 * year; labelled by `words` (the picker's, in the app's language).
 */
export function defaultPresets(today, words = ENGLISH) {
  const year = today.slice(0, 4);
  const lastMonth = addMonths(startOfMonth(today), -1);
  return [
    { label: words.today, start: today, end: today },
    { label: words.last7, start: addDays(today, -6), end: today },
    { label: words.last30, start: addDays(today, -29), end: today },
    { label: words.thisMonth, start: startOfMonth(today), end: endOfMonth(today) },
    { label: words.lastMonth, start: lastMonth, end: endOfMonth(lastMonth) },
    { label: words.thisYear, start: `${year}-01-01`, end: `${year}-12-31` },
  ];
}
