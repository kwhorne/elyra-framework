// The spans a DateRangePicker offers a click away, from today: ISO dates in,
// `{ label, start, end }` out.

import { addDays, addMonths, startOfMonth } from "./dates.js";

/** The last day of `day`'s month. */
export const endOfMonth = (day) => addDays(addMonths(startOfMonth(day), 1), -1);

/** Today, the last 7 and 30 days (today among them), this and last month, this year. */
export function defaultPresets(today) {
  const year = today.slice(0, 4);
  const lastMonth = addMonths(startOfMonth(today), -1);
  return [
    { label: "Today", start: today, end: today },
    { label: "Last 7 days", start: addDays(today, -6), end: today },
    { label: "Last 30 days", start: addDays(today, -29), end: today },
    { label: "This month", start: startOfMonth(today), end: endOfMonth(today) },
    { label: "Last month", start: lastMonth, end: endOfMonth(lastMonth) },
    { label: "This year", start: `${year}-01-01`, end: `${year}-12-31` },
  ];
}
