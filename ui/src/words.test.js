// The framework's own words (RFC 0056): the English here is the catalog's,
// Norwegian has every word with the same placeholders, and a component says
// them in the app's language, under its `labels`.
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { WORDS, englishOf } from "./internal/words.js";
import { speak } from "./test/runtime.js";
import DateRangePicker from "./DateRangePicker.svelte";
import Pagination from "./Pagination.svelte";
import StatusBar from "./StatusBar.svelte";
import { outbox } from "./test/runtime.js";

const LANG = `${import.meta.dirname}/../../framework/lang/`;
const lang = (locale) => JSON.parse(readFileSync(`${LANG}${locale}.json`, "utf8"));
const flat = (value, prefix = "", out = {}) => {
  for (const [k, v] of Object.entries(value)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "object") flat(v, key, out);
    else out[key] = v;
  }
  return out;
};
const placeholders = (m) => [...new Set([...m.matchAll(/:([A-Za-z_]\w*)/g)].map((x) => x[1].toLowerCase()))].sort();

const en = flat(lang("en"));
const nb = flat(lang("nb"));
const ui = Object.fromEntries(
  Object.entries(WORDS).flatMap(([comp, words]) => Object.entries(words).map(([k, v]) => [`elyra.${comp}.${k}`, englishOf(v)])),
);

describe("the framework's words", () => {
  it("are framework/lang/en.json's, word for word", () => {
    const inCatalog = Object.fromEntries(Object.entries(en).filter(([k]) => k.split(".")[1] in WORDS));
    expect(ui).toEqual(inCatalog);
  });

  it("are all in Norwegian, with the same placeholders", () => {
    expect(Object.keys(nb).sort()).toEqual(Object.keys(en).sort());
    for (const [key, message] of Object.entries(en)) {
      expect(placeholders(nb[key]), key).toEqual(placeholders(message));
      expect(nb[key].split("|").length, `${key}'s plural segments`).toBe(message.split("|").length);
    }
  });

  it("leave no word in a component's markup", () => {
    const dir = `${import.meta.dirname}/`;
    // The developer's tools stay English (RFC 0056, decision 4).
    const english = /Inspector|Simulating/;
    const found = [];
    for (const file of readdirSync(dir).filter((f) => f.endsWith(".svelte"))) {
      const markup = readFileSync(`${dir}${file}`, "utf8").replace(/<style>[\s\S]*<\/style>/, "").replace(/<!--[\s\S]*?-->/g, "");
      for (const m of markup.matchAll(/(?:aria-label|title|placeholder|confirm|confirmLabel|label)="([A-Z][^"{]*)"|>\s*([A-Z][a-z][^<{]*?)\s*</g)) {
        const word = m[1] ?? m[2];
        if (!english.test(word)) found.push(`${file}: ${word}`);
      }
    }
    expect(found).toEqual([]);
  });
});

describe("a component's words", () => {
  it("are English without a catalog", () => {
    render(DateRangePicker);
    expect(screen.getByRole("textbox", { name: "Start date" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Choose dates" })).toBeTruthy();
  });

  it("follow the app's catalog, and its locale for the dates", async () => {
    speak("nb", nb);
    render(DateRangePicker, { start: "2026-03-01", end: "2026-03-04" });
    await tick();
    expect(screen.getByRole("textbox", { name: "Fra dato" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Fra dato" }).value).toBe("01.03.2026");
    screen.getByRole("button", { name: /^Velg datoer, / }).click();
    await tick();
    expect(screen.getByRole("list", { name: "Perioder" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Siste 7 dager" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Forrige måned" }).getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByRole("button", { name: "Gå til forrige måned" })).toBeTruthy();
  });

  it("change when the locale does", async () => {
    render(Pagination, { page: 2, lastPage: 3 });
    expect(screen.getByRole("button", { name: "Next" })).toBeTruthy();
    speak("nb", nb);
    await tick();
    expect(screen.getByRole("button", { name: "Neste" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Side 3" })).toBeTruthy();
  });

  it("take the app's own message over the framework's, and labels over both", async () => {
    speak("nb", { ...nb, "elyra.pagination.next": "Videre" });
    render(Pagination, { page: 1, lastPage: 3, labels: { previous: "Tilbake" } });
    await tick();
    expect(screen.getByRole("button", { name: "Videre" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Tilbake" })).toBeTruthy();
  });

  it("pick the plural for the count", async () => {
    speak("nb", nb);
    outbox.set([
      { id: 1, status: "pending", method: "PUT", path: "/a", createdAt: Date.now() },
      { id: 2, status: "pending", method: "PUT", path: "/b", createdAt: Date.now() },
    ]);
    render(StatusBar, { assistant: false, inspector: false });
    await tick();
    expect(screen.getByRole("button", { name: "2 endringer venter" })).toBeTruthy();
    outbox.set([{ id: 1, status: "pending", method: "PUT", path: "/a", createdAt: Date.now() }]);
    await tick();
    expect(screen.getByRole("button", { name: "1 endring venter" })).toBeTruthy();
  });
});
