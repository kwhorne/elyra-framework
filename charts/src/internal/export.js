// A chart taken out of the app (RFC 0060): as SVG or PNG with the theme's
// surface behind it, on the clipboard, or its data as CSV in the app's locale.

/** The chart's SVG, with `background` behind it so it reads on any page. */
export function svgOf(chart, background) {
  const svg = chart.renderToSVGString();
  return svg.replace(/<svg\b[^>]*>/, (open) => `${open}<rect width="100%" height="100%" fill="${background}"/>`);
}

/** An SVG drawn onto a canvas at `scale`: PNG bytes. */
export async function pngFromSvg(svg, width, height, scale = 2) {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const image = new Image();
    image.width = width;
    image.height = height;
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = () => reject(new Error("the chart's image didn't load"));
      image.src = url;
    });
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    const context = canvas.getContext("2d");
    context.scale(scale, scale);
    context.drawImage(image, 0, 0, width, height);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("the chart couldn't be drawn as PNG");
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A canvas-drawn chart's own PNG, at `scale`. */
export async function pngOfCanvas(chart, background, scale = 2) {
  const url = chart.getDataURL({ type: "png", pixelRatio: scale, backgroundColor: background });
  return new Uint8Array(await (await fetch(url)).arrayBuffer());
}

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

/** The chart's table as CSV: its caption, the head, then the rows, as the table writes them. */
export function csvOf(table, { caption, head, locale }) {
  const separator = separatorFor(locale);
  const line = (cells) => cells.map((c) => field(c, separator)).join(separator);
  const rows = [line([caption]), line([table.head ?? head, ...table.columns])];
  for (const [name, values] of table.rows) rows.push(line([name, ...values]));
  return `${rows.join("\r\n")}\r\n`;
}

/** A name for the file: the chart's label, else its summary's first words, and the date. */
export function fileNameOf({ label, summary, extension, now = new Date() }) {
  const words = (label || summary || "")
    .split(/[.:;,]/)[0]
    .replace(/[\\/?%*|"<>]/g, "")
    .trim()
    .split(/\s+/)
    .slice(0, 4)
    .join(" ");
  const pad = (n) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  return `${words || "chart"} ${date}.${extension}`;
}
