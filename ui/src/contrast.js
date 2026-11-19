/**
 * WCAG 2's contrast between two colours (RFC 0037): what the Theme Studio
 * checks as tokens change, what `tokens.css` is held to in its tests, and
 * what an app's own tests can hold its `app.css` to.
 *
 *   contrast("#787c99", "#1f2030")   // 3.93
 */

/** `#rgb`, `#rrggbb` or `rgb()`/`rgba()` as `[r, g, b]` (0–255), or `null`. Alpha is ignored. */
export function parseColor(text) {
  const value = String(text ?? "").trim().toLowerCase();
  let m = value.match(/^#([0-9a-f]{3})$/);
  if (m) return [...m[1]].map((c) => parseInt(c + c, 16));
  m = value.match(/^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/);
  if (m) return [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16));
  m = value.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/);
  if (m) return [m[1], m[2], m[3]].map((n) => Math.max(0, Math.min(255, Math.round(Number(n)))));
  return null;
}

/** A colour's relative luminance, 0 (black) to 1 (white). */
export function luminance(color) {
  const rgb = Array.isArray(color) ? color : parseColor(color);
  if (!rgb) throw new Error(`not a colour: ${color}`);
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** The contrast ratio of `a` and `b`, from 1 to 21; in either order. */
export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const hex = (rgb) => `#${rgb.map((c) => Math.round(c).toString(16).padStart(2, "0")).join("")}`;

function toHsl([r, g, b]) {
  const [rr, gg, bb] = [r / 255, g / 255, b / 255];
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === rr ? (gg - bb) / d + (gg < bb ? 6 : 0) : max === gg ? (bb - rr) / d + 2 : (rr - gg) / d + 4;
  return [h / 6, s, l];
}

function fromHsl([h, s, l]) {
  if (s === 0) return [l, l, l].map((c) => c * 255);
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const channel = (t) => {
    const u = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
    if (u < 1 / 6) return p + (q - p) * 6 * u;
    if (u < 1 / 2) return q;
    if (u < 2 / 3) return p + (q - p) * (2 / 3 - u) * 6;
    return p;
  };
  return [channel(h + 1 / 3), channel(h), channel(h - 1 / 3)].map((c) => c * 255);
}

/**
 * The colour nearest `color` that has at least `ratio` against every one
 * of `on`: its hue and saturation kept, its lightness moved away from the
 * backgrounds. `color` itself when it already does; `null` when no
 * lightness does.
 */
export function nearest(color, on, ratio) {
  const rgb = parseColor(color);
  if (!rgb) return null;
  const meets = (c) => on.every((bg) => contrast(c, bg) >= ratio);
  if (meets(rgb)) return hex(rgb);
  const [h, s, l] = toHsl(rgb);
  // Away from the backgrounds: lighter on dark ones, darker on light ones.
  const darkBackgrounds = on.reduce((sum, bg) => sum + luminance(bg), 0) / on.length < 0.18;
  const step = darkBackgrounds ? 0.002 : -0.002;
  for (let next = l + step; next >= 0 && next <= 1; next += step) {
    const candidate = fromHsl([h, s, next]).map((c) => Math.round(c));
    if (meets(candidate)) return hex(candidate);
  }
  return null;
}
