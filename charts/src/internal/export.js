// A chart taken out of the app (RFC 0060): as SVG or PNG with the theme's
// surface behind it, on the clipboard, or its data as CSV in the app's locale.
// The CSV is `@elyra/ui`'s, as a table's is (RFC 0066).

import { csvText, fileNameOf as fileNameOfData, separatorFor } from "@elyra/ui/internal/csv.js";

export { separatorFor };

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

/** The chart's table as CSV: its caption, the head, then the rows, as the table writes them. */
export function csvOf(table, { caption, head, locale }) {
  const lines = [[caption], [table.head ?? head, ...table.columns]];
  for (const [name, values] of table.rows) lines.push([name, ...values]);
  return csvText(lines, locale);
}

/** A name for the file: the chart's label, else its summary's first words, and the date. */
export const fileNameOf = (what) => fileNameOfData({ fallback: "chart", ...what });
