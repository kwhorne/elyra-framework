// The chart's look from the design tokens (RFC 0058). ECharts takes colours
// as values, not CSS variables, so they're read off the chart's own element
// (an override on a section counts) and read again when the theme changes.

/** What the tokens say, as the chart uses them. `fallback` when one is unset. */
export function themeOf(element) {
  const style = element && typeof getComputedStyle === "function" ? getComputedStyle(element) : null;
  const token = (name, fallback) => style?.getPropertyValue(name).trim() || fallback;
  const ms = (text) => {
    const m = String(text).match(/^([\d.]+)(ms|s)$/);
    return m ? Number(m[1]) * (m[2] === "s" ? 1000 : 1) : 0;
  };
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  return {
    colors: [1, 2, 3, 4, 5, 6, 7, 8].map((n, i) => token(`--elyra-chart-${n}`, FALLBACK[i])),
    text: token("--elyra-text", "#c0caf5"),
    muted: token("--elyra-muted", "#8387a2"),
    border: token("--elyra-border", "#2a2b3c"),
    raised: token("--elyra-raised", "#1e1f2b"),
    surface: token("--elyra-surface", "#1a1b26"),
    inset: token("--elyra-inset", "#16161e"),
    accent: token("--elyra-accent", "#7aa2f7"),
    accentSoft: token("--elyra-accent-soft", "rgba(122, 162, 247, 0.2)"),
    // ECharts writes the family into an SVG's style="…" as it is: a name in
    // double quotes ("Segoe UI") would end the attribute, and the SVG with it.
    font: token("--elyra-font", "system-ui, sans-serif").replaceAll('"', "'"),
    duration: reduced ? 0 : ms(token("--elyra-duration-slow", "260ms")),
  };
}

/** The dark look's series, when the tokens aren't loaded. */
const FALLBACK = ["#7aa2f7", "#ff9e64", "#9ece6a", "#bb9af7", "#7dcfff", "#f7768e", "#e0af68", "#73daca"];

/** The parts of an ECharts option the theme sets. */
export function themed(theme) {
  const axis = {
    axisLine: { lineStyle: { color: theme.border } },
    axisTick: { lineStyle: { color: theme.border } },
    axisLabel: { color: theme.muted },
    splitLine: { lineStyle: { color: theme.border } },
    nameTextStyle: { color: theme.muted },
  };
  return {
    color: theme.colors,
    backgroundColor: "transparent",
    textStyle: { color: theme.text, fontFamily: theme.font },
    animation: theme.duration > 0,
    animationDuration: theme.duration,
    animationDurationUpdate: theme.duration,
    legend: { textStyle: { color: theme.muted } },
    tooltip: {
      backgroundColor: theme.raised,
      borderColor: theme.border,
      textStyle: { color: theme.text, fontFamily: theme.font },
    },
    axis,
  };
}

/**
 * Call `changed` when the look can have changed: `data-theme` or
 * `data-contrast` on `<html>`, its `style` (the Theme Studio), or the OS's
 * appearance, contrast or motion. Returns the way to stop.
 */
export function watchTheme(changed) {
  const stops = [];
  if (typeof MutationObserver === "function" && typeof document !== "undefined") {
    const observer = new MutationObserver(() => changed());
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-contrast", "style", "class"] });
    stops.push(() => observer.disconnect());
  }
  if (typeof matchMedia === "function") {
    for (const query of ["(prefers-color-scheme: dark)", "(prefers-contrast: more)", "(prefers-reduced-motion: reduce)"]) {
      const list = matchMedia(query);
      list.addEventListener?.("change", changed);
      stops.push(() => list.removeEventListener?.("change", changed));
    }
  }
  return () => stops.forEach((stop) => stop());
}
