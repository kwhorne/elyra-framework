import { writable } from "svelte/store";

const catalog = writable({ locale: "", message: () => undefined });
export const frameworkWords = { subscribe: catalog.subscribe };

/** Load a catalog: `speak("nb", { "elyra.charts.empty": "Ingen data" })`. */
export function speak(locale, messages) {
  catalog.set({ locale, message: (key) => messages[key] });
}
