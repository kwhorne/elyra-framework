// The gallery runs in a browser, without the Elyra shell: the real
// @elyra/runtime, with the catalog the reader picks (English or Norwegian)
// in place of the app's.
export * from "../../runtime/dist/index.js";
import { writable } from "svelte/store";
import nb from "../../framework/lang/nb.json";

const flat = (value, prefix = "", out = {}) => {
  for (const [key, v] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof v === "object") flat(v, path, out);
    else out[path] = v;
  }
  return out;
};
const NORWEGIAN = flat(nb);

const words = writable({ locale: "", message: () => undefined });
export const frameworkWords = { subscribe: words.subscribe };

/** The gallery's language: "en" (the charts' own English) or "nb". */
export function speak(locale) {
  words.set(locale === "nb" ? { locale: "nb", message: (key) => NORWEGIAN[key] } : { locale: "", message: () => undefined });
}
