// A WireCharts docs example as a Svelte tag (RFC 0059): for the reference
// (scripts/components.mjs) and the gallery, the same way.

/** WireCharts' props that Elyra says its own way: the theme, Livewire, a button's words. */
export const NOT_PROPS = new Set(["height", "theme", "model", "label", "class"]);

export const nameOf = (slug) => slug.split("-").map((w) => w[0].toUpperCase() + w.slice(1)).join("") + "Chart";

const LONG = 20;
const shorten = (v) =>
  Array.isArray(v)
    ? v.length > LONG
      ? "@@points@@"
      : v.map(shorten)
    : v && typeof v === "object"
      ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, shorten(x)]))
      : v;

const svelteProp = ([key, value]) => {
  if (value === true) return key;
  if (typeof value === "string") return `${key}=${JSON.stringify(value)}`;
  const js = JSON.stringify(shorten(value))
    .replaceAll('"@@points@@"', "points")
    .replace(/"([A-Za-z_]\w*)":/g, "$1: ")
    .replace(/,(?=\S)/g, ", ");
  return `${key}={${js}}`;
};

/** The example's props, as the component takes them. */
export const propsOf = (fixture) =>
  Object.fromEntries(Object.entries(fixture.props).filter(([key]) => !NOT_PROPS.has(key)));

/** `<FunnelChart series={…} labels={…} />`, with a long list shown as `points`. */
export function exampleOf(fixture) {
  const props = Object.entries(propsOf(fixture));
  const usesPoints = JSON.stringify(shorten(Object.fromEntries(props))).includes("@@points@@");
  const tag = `<${nameOf(fixture.component)}${props.length ? "\n  " + props.map(svelteProp).join("\n  ") + "\n" : " "}/>`;
  return (usesPoints ? "<!-- points: a long list of numbers -->\n" : "") + tag;
}
