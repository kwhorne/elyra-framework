// What <Avatar> draws for someone without a picture: their initials, on a
// colour that is always the same for the same name.

/** The first letters of the first and last names: "Ada Lovelace" → "AL", "Øyvind" → "Ø". */
export function initials(name) {
  const words = String(name ?? "")
    .trim()
    .split(/[\s]+/)
    .filter(Boolean);
  if (words.length === 0) return "?";
  const first = (word) => Array.from(word)[0].toLocaleUpperCase();
  return words.length === 1 ? first(words[0]) : first(words[0]) + first(words[words.length - 1]);
}

/** A hue, 0–359, from the name: the same name always gets the same one. */
export function hue(name) {
  // FNV-1a over the code points.
  let h = 0x811c9dc5;
  for (const c of String(name ?? "").trim().toLowerCase()) {
    h ^= c.codePointAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h % 360;
}

const SIZES = { sm: 20, md: 28, lg: 40 };

/** An avatar's size in pixels: `"sm"`, `"md"` and `"lg"`, or a number. */
export function avatarSize(size) {
  return typeof size === "number" ? size : (SIZES[size] ?? SIZES.md);
}
