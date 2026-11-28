// Data out of the app as a spreadsheet reads it (RFC 0060, RFC 0066): CSV
// for a file, tab-separated text for the clipboard. A table's and a chart's
// are the same, written here.

/** The separator a spreadsheet in `locale` reads: `;` where the decimal sign is a comma. */
export function separatorFor(locale) {
  const decimal = new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === "decimal")?.value;
  return decimal === "," ? ";" : ",";
}

/** A field as RFC 4180 has it: quoted when it holds the separator, a quote or a line break. */
function field(value, separator) {
  const text = String(value ?? "");
  return text.includes(separator) || /["\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

/** Lines of cells as CSV, in `locale`'s separator, each line ended by `\r\n`. */
export function csvText(lines, locale) {
  const separator = separatorFor(locale);
  return lines.map((cells) => cells.map((c) => field(c, separator)).join(separator)).join("\r\n") + "\r\n";
}

/**
 * Lines of cells as tab-separated text, which pastes into a spreadsheet as
 * cells and into an email as lines. A tab or a line break in a value is a space.
 */
export function tsvText(lines) {
  return lines.map((cells) => cells.map((c) => String(c ?? "").replace(/[\t\r\n]+/g, " ")).join("\t")).join("\n") + "\n";
}

/** A number as a spreadsheet reads it: no grouping, `locale`'s decimal sign. */
export function plainNumber(locale) {
  const plain = new Intl.NumberFormat(locale, { useGrouping: false, maximumFractionDigits: 20 });
  return (n) => plain.format(n);
}

/** A name for the file: the label, else the summary's first words, and the date. */
export function fileNameOf({ label, summary, extension, now = new Date(), fallback = "data" }) {
  const words = (label || summary || "")
    .split(/[.:;,]/)[0]
    .replace(/[\\/?%*|"<>]/g, "")
    .trim()
    .split(/\s+/)
    .slice(0, 4)
    .join(" ");
  const pad = (n) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  return `${words || fallback} ${date}.${extension}`;
}
