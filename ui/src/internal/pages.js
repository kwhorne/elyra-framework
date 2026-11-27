// A report's pages (RFC 0064): its blocks' heights in, pages out. Each
// block goes whole on the page where it fits, else on the next; a table or
// a list taller than the room left is split between its rows or items, and
// goes on. Plain arithmetic, so a test can hold it without a layout.

/** Millimetres as CSS pixels, 96 to the inch. */
export const px = (mm) => (mm * 96) / 25.4;

/** A paper's size in millimetres, upright or turned. */
export function paperOf(paper = "a4", landscape = false) {
  const [w, h] = paper === "letter" ? [215.9, 279.4] : [210, 297];
  return landscape ? { width: h, height: w } : { width: w, height: h };
}

/**
 * The pages for `blocks`, each `{ height }`, or a splittable one
 * `{ height, fixed, units: [h, …] }` (`fixed` is its height without its
 * units: a table's head and frame, a list's own). `room(page)` is the body's
 * height on page `page` (0-based). Each page is a list of `{ block }`, or
 * `{ block, from, to }` for units `from` up to `to`, or `{ block, clipped }`
 * for a block taller than any page.
 */
export function paginate(blocks, room) {
  const pages = [[]];
  let used = 0;
  const page = () => pages.length - 1;
  const next = () => {
    pages.push([]);
    used = 0;
  };
  blocks.forEach((block, index) => {
    if (block.height <= room(page()) - used) {
      pages[page()].push({ block: index });
      used += block.height;
      return;
    }
    if (Array.isArray(block.units) && block.units.length > 0) {
      let from = 0;
      while (from < block.units.length) {
        let to = from;
        let height = block.fixed ?? 0;
        while (to < block.units.length && height + block.units[to] <= room(page()) - used) height += block.units[to++];
        if (to === from) {
          // Not one more row here: on the next page, or alone if none fits anywhere.
          if (used > 0) {
            next();
            continue;
          }
          height += block.units[to++];
        }
        pages[page()].push({ block: index, from, to });
        used += height;
        from = to;
        if (from < block.units.length) next();
      }
      return;
    }
    if (used > 0) next();
    const clipped = block.height > room(page());
    pages[page()].push(clipped ? { block: index, clipped: true } : { block: index });
    used += block.height;
  });
  return pages[pages.length - 1].length === 0 && pages.length > 1 ? pages.slice(0, -1) : pages;
}

/** What splits a block: its table's body rows, or its list's items; null if it doesn't. */
export function unitsOf(block) {
  const tag = block.tagName;
  const tables = tag === "TABLE" ? [block] : [...block.querySelectorAll("table")];
  if (tables.length === 1) {
    const rows = [...tables[0].querySelectorAll(":scope > tbody > tr")];
    return rows.length > 1 ? rows : null;
  }
  if (tables.length > 1) return null;
  const lists = tag === "UL" || tag === "OL" ? [block] : [...block.querySelectorAll("ul, ol")];
  if (lists.length === 1) {
    const items = [...lists[0].children].filter((c) => c.tagName === "LI");
    return items.length > 1 ? items : null;
  }
  return null;
}
