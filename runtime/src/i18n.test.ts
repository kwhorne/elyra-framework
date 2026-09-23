/**
 * i18n: the catalog fetch, `$t` / `$tc` as stores, placeholders, and the plural
 * rules — which must match the Rust `Translator` exactly.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decode } from "@msgpack/msgpack";
import { okResponse, stubFetch, errorResponse } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { t, tc, locale, setLocale, translate, choice, loadTranslations, selectPlural, __resetI18n } =
  await import("./index.js");

const en = {
  locale: "en",
  fallback: "en",
  messages: {
    greeting: "Hello, :name!",
    shout: ":Name / :NAME",
    files: "{0} No files|{1} file|[2,*] files",
    ns: ":namespace then :name",
  },
};
const nb = {
  locale: "nb",
  fallback: "en",
  messages: { greeting: "Hei, :name!", files: "{0} Ingen filer|{1} fil|[2,*] filer", shout: "", ns: "" },
};

beforeEach(() => __resetI18n());
afterEach(() => vi.unstubAllGlobals());

/** Serve `/__i18n` and `/__i18n/locale`; park the event long-poll. */
function serve() {
  return stubFetch((url, init) => {
    if (url.endsWith("/__i18n/locale")) {
      const requested = decode(init!.body as Uint8Array);
      return okResponse(requested === "nb" ? nb : en);
    }
    if (url.endsWith("/__i18n")) return okResponse(en);
    return new Promise<Response>(() => {}) as unknown as Response; // /__events
  });
}

describe("translate / choice", () => {
  it("returns the key until the catalog is loaded, then the message", async () => {
    serve();
    expect(translate("greeting", { name: "Ada" })).toBe("greeting");
    await loadTranslations();
    expect(translate("greeting", { name: "Ada" })).toBe("Hello, Ada!");
    expect(translate("missing.key")).toBe("missing.key");
  });

  it("fetches with the token, and only once", async () => {
    const calls = serve();
    await Promise.all([loadTranslations(), loadTranslations()]);
    const i18n = calls.filter((c) => c.url.endsWith("/__i18n"));
    expect(i18n).toHaveLength(1);
    expect(new Headers(i18n[0]!.init?.headers).get("x-elyra-token")).toBe("test-token");
  });

  it("fills placeholders in the case they are written in, longest first", async () => {
    serve();
    await loadTranslations();
    expect(translate("shout", { name: "ada" })).toBe("Ada / ADA");
    expect(translate("ns", { name: "x", namespace: "app" })).toBe("app then x");
  });

  it("picks plurals with :count", async () => {
    serve();
    await loadTranslations();
    expect(choice("files", 0)).toBe("No files");
    expect(choice("files", 1)).toBe("file");
    expect(choice("files", 5)).toBe("files");
  });

  it("surfaces a missing I18nProvider as a rejected load", async () => {
    stubFetch(() => errorResponse("i18n unavailable (add I18nProvider)"));
    await expect(loadTranslations()).rejects.toThrow("add I18nProvider");
  });
});

describe("stores", () => {
  it("$t re-renders when the catalog loads and when the locale changes", async () => {
    serve();
    const seen: string[] = [];
    const stop = t.subscribe((tr) => seen.push(tr("greeting", { name: "Ada" })));
    await loadTranslations();
    await setLocale("nb");
    stop();
    expect(seen).toEqual(["greeting", "Hello, Ada!", "Hei, Ada!"]);
  });

  it("$tc and $locale follow the same catalog", async () => {
    serve();
    const counts: string[] = [];
    const locales: string[] = [];
    const a = tc.subscribe((c) => counts.push(c("files", 2)));
    const b = locale.subscribe((l) => locales.push(l));
    await loadTranslations();
    await setLocale("nb");
    a();
    b();
    expect(counts[counts.length - 1]).toBe("filer");
    expect(locales).toEqual(["", "en", "nb"]);
  });

  it("setLocale posts the locale to the shell", async () => {
    const calls = serve();
    await setLocale("nb");
    const post = calls.find((c) => c.url.endsWith("/__i18n/locale"))!;
    expect(post.init?.method).toBe("POST");
    expect(decode(post.init!.body as Uint8Array)).toBe("nb");
  });
});

describe("plural rules (mirror the Rust table)", () => {
  it("one/other, French, Japanese", () => {
    expect(selectPlural("file|files", 1, "en")).toBe("file");
    expect(selectPlural("file|files", 0, "en")).toBe("files");
    expect(selectPlural("fichier|fichiers", 0, "fr")).toBe("fichier");
    expect(selectPlural("ファイル|x", 5, "ja")).toBe("ファイル");
  });

  it("Russian and Polish three-form rules", () => {
    const ru = "файл|файла|файлов";
    expect([1, 2, 5, 11, 21, 22].map((n) => selectPlural(ru, n, "ru"))).toEqual([
      "файл", "файла", "файлов", "файлов", "файл", "файла",
    ]);
    const pl = "plik|pliki|plików";
    expect([1, 2, 5, 12, 22].map((n) => selectPlural(pl, n, "pl"))).toEqual([
      "plik", "pliki", "plików", "plików", "pliki",
    ]);
  });

  it("explicit conditions and ranges", () => {
    const msg = "[0,0] none|[1,9] a few|[10,*] lots";
    expect(selectPlural(msg, 0, "en")).toBe("none");
    expect(selectPlural(msg, 9, "en")).toBe("a few");
    expect(selectPlural(msg, 1000, "en")).toBe("lots");
    expect(selectPlural("only", 5, "ru")).toBe("only");
  });
});
