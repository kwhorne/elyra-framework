// Calendar dates for <DatePicker>: ISO `YYYY-MM-DD` strings, reckoned in UTC
// so that no time zone or daylight saving moves a day. ISO strings compare
// as dates, so `<` and `>` are enough for min and max.

const ISO = /^(\d{4})-(\d{2})-(\d{2})$/;

/** `{ y, m (1–12), d }` of an ISO date, or null when it isn't a real one. */
export function parts(iso) {
  const match = ISO.exec(iso ?? "");
  if (!match) return null;
  const [y, m, d] = match.slice(1).map(Number);
  const date = new Date(Date.UTC(y, m - 1, d));
  if (date.getUTCFullYear() !== y || date.getUTCMonth() !== m - 1 || date.getUTCDate() !== d) return null;
  return { y, m, d };
}

/** The ISO date of a UTC `Date`. */
export function iso(date) {
  const y = String(date.getUTCFullYear()).padStart(4, "0");
  const m = String(date.getUTCMonth() + 1).padStart(2, "0");
  const d = String(date.getUTCDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function utc(value) {
  const p = parts(value);
  return new Date(Date.UTC(p.y, p.m - 1, p.d));
}

/** Today, where the user is. */
export function today(now = new Date()) {
  return iso(new Date(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate())));
}

/** `n` days later (or earlier). */
export function addDays(value, n) {
  const date = utc(value);
  date.setUTCDate(date.getUTCDate() + n);
  return iso(date);
}

/** `n` months later, on the same day, or the month's last when it's shorter (31 Jan + 1 = 28 Feb). */
export function addMonths(value, n) {
  const { y, m, d } = parts(value);
  const index = y * 12 + (m - 1) + n;
  const year = Math.floor(index / 12);
  const month = index - year * 12;
  const last = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  return iso(new Date(Date.UTC(year, month, Math.min(d, last))));
}

/** The day of the week, 0 for Sunday. */
export function weekday(value) {
  return utc(value).getUTCDay();
}

/** The first day of `value`'s week, which starts on `weekStart` (0 Sunday, 1 Monday, …). */
export function startOfWeek(value, weekStart) {
  return addDays(value, -((weekday(value) - weekStart + 7) % 7));
}

/** The first of `value`'s month. */
export function startOfMonth(value) {
  return `${value.slice(0, 8)}01`;
}

/** Six weeks of days, covering `value`'s month: the rows of the calendar. */
export function monthGrid(value, weekStart) {
  let day = startOfWeek(startOfMonth(value), weekStart);
  const weeks = [];
  for (let w = 0; w < 6; w++) {
    const week = [];
    for (let i = 0; i < 7; i++) {
      week.push(day);
      day = addDays(day, 1);
    }
    weeks.push(week);
  }
  return weeks;
}

/** `value` held between `min` and `max` (either may be missing). */
export function clamp(value, min, max) {
  if (min && value < min) return min;
  if (max && value > max) return max;
  return value;
}

/** The week's first day where `locale` is, as 0 (Sunday) to 6; Monday when it can't tell. */
export function localeWeekStart(locale) {
  try {
    const l = new Intl.Locale(locale ?? Intl.DateTimeFormat().resolvedOptions().locale);
    const info = l.getWeekInfo?.() ?? l.weekInfo;
    if (info?.firstDay) return info.firstDay % 7;
  } catch {
    // An unknown locale: the ISO week.
  }
  return 1;
}

/** The order the locale writes a numeric date in: `["day", "month", "year"]` for `nb`. */
function order(locale) {
  return new Intl.DateTimeFormat(locale, { year: "numeric", month: "2-digit", day: "2-digit" })
    .formatToParts(new Date(Date.UTC(2026, 10, 30)))
    .filter((p) => p.type === "day" || p.type === "month" || p.type === "year")
    .map((p) => p.type);
}

/** How the locale writes a date in numbers, as a hint: `DD.MM.YYYY`, `MM/DD/YYYY`. */
export function numericPattern(locale) {
  const names = { day: "DD", month: "MM", year: "YYYY" };
  return new Intl.DateTimeFormat(locale, { year: "numeric", month: "2-digit", day: "2-digit", timeZone: "UTC" })
    .formatToParts(new Date(Date.UTC(2026, 10, 30)))
    .map((p) => names[p.type] ?? p.value)
    .join("");
}

/** `value` as the locale writes a date in numbers: `30.11.2026`, `11/30/2026`. */
export function formatNumeric(value, locale) {
  if (!parts(value)) return "";
  return new Intl.DateTimeFormat(locale, { year: "numeric", month: "2-digit", day: "2-digit", timeZone: "UTC" }).format(
    utc(value),
  );
}

/**
 * What the user typed, as an ISO date, or null: ISO itself (`2026-11-30`),
 * or three numbers in the locale's order with any separator (`30.11.2026`,
 * `30/11/2026`, `11/30/2026` in `en-US`). A two-digit year is this century.
 */
export function parseTyped(text, locale) {
  const t = (text ?? "").trim();
  if (!t) return null;
  if (ISO.test(t)) return parts(t) ? t : null;
  const numbers = t.split(/[^\d]+/).filter(Boolean);
  if (numbers.length !== 3) return null;
  const at = Object.fromEntries(order(locale).map((type, i) => [type, numbers[i]]));
  let year = Number(at.year);
  if (at.year.length <= 2) year += 2000;
  const candidate = `${String(year).padStart(4, "0")}-${at.month.padStart(2, "0")}-${at.day.padStart(2, "0")}`;
  return parts(candidate) ? candidate : null;
}
