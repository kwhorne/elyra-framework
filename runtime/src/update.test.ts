/**
 * The update toast (RFC 0024): one step for a manifest URL ("Install &
 * restart"), two for an update from the backend ("Install", then "Restart"),
 * and words an app can replace or translate.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { encode } from "@msgpack/msgpack";
import { okResponse, stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const {
  __onUpdateEvent,
  checkForUpdate,
  __resetI18n,
  dismissUpdate,
  loadTranslations,
  restartToUpdate,
  setUpdateTexts,
  updateToastHtml,
} = await import("./index.js");

afterEach(() => {
  dismissUpdate();
  vi.unstubAllGlobals();
});

describe("the update toast", () => {
  it("offers install and restart as one step for a manifest URL", () => {
    const html = updateToastHtml({ phase: "available", version: "1.2.0", restart: "now" });
    expect(html).toContain("<strong>v1.2.0</strong>");
    expect(html).toContain(">Install &amp; restart</button>");
    expect(html).toContain('data-act="later"');
    expect(updateToastHtml({ phase: "ready", restart: "now" })).toContain("restarting");
  });

  it("installs and restarts as two steps for an update from the backend", () => {
    const offer = updateToastHtml({ phase: "available", version: "2.0.0", restart: "ask" });
    expect(offer).toContain('data-act="install">Install</button>');
    expect(offer).not.toContain("restart</button>");

    const ready = updateToastHtml({ phase: "ready", version: "2.0.0", restart: "ask" });
    expect(ready).toContain("<strong>v2.0.0</strong> is installed. Restart to use it.");
    expect(ready).toContain('data-act="restart">Restart</button>');
    expect(ready).toContain('data-act="later">Later</button>');
    expect(ready).not.toContain("restarting");
  });

  it("follows the phases, and an offer goes when there's nothing to offer", () => {
    __onUpdateEvent({ phase: "available", version: "2.0.0", notes: "- faster", restart: "ask" });
    __onUpdateEvent({ phase: "downloading", progress: 40 });
    expect(updateToastHtml(__onUpdateEvent(undefined)!)).toContain("40%");
    const ready = __onUpdateEvent({ phase: "ready", restart: "ask" });
    // The version is kept from the offer when `ready` doesn't repeat it.
    expect(ready).toMatchObject({ phase: "ready", version: "2.0.0", restart: "ask" });
    // An installed update stays through a later "nothing newer"…
    expect(__onUpdateEvent({ phase: "up-to-date" })?.phase).toBe("ready");
    dismissUpdate();
    // …an offer doesn't (signed out, another server).
    __onUpdateEvent({ phase: "available", version: "2.0.0", restart: "ask" });
    expect(__onUpdateEvent({ phase: "up-to-date" })).toBeNull();
  });

  it("escapes what the server says", () => {
    const html = updateToastHtml({ phase: "error", message: "<img src=x onerror=alert(1)>" });
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;img");
  });

  it("takes the app's words", () => {
    setUpdateTexts({
      install: "Installer",
      restart: "Start på nytt",
      installed: ":version er installert.",
      downloading: "Laster ned … :percent %",
    });
    expect(updateToastHtml({ phase: "available", version: "2.0.0", restart: "ask" })).toContain(">Installer</button>");
    const ready = updateToastHtml({ phase: "ready", version: "2.0.0", restart: "ask" });
    expect(ready).toContain("<strong>v2.0.0</strong> er installert.");
    expect(ready).toContain(">Start på nytt</button>");
    expect(updateToastHtml({ phase: "downloading", progress: 7 })).toContain("Laster ned … 7 %");
    setUpdateTexts({ install: undefined, restart: undefined, installed: undefined, downloading: undefined });
  });
});

describe("translated", () => {
  it("uses the app's elyra.update.* translations when its catalog is loaded", async () => {
    stubFetch(() =>
      okResponse({
        locale: "nb",
        fallback: "en",
        messages: { "elyra.update.restart": "Start på nytt", "elyra.update.later": "Senere" },
      }),
    );
    await loadTranslations();
    const ready = updateToastHtml({ phase: "ready", version: "2.0.0", restart: "ask" });
    expect(ready).toContain(">Start på nytt</button>");
    expect(ready).toContain(">Senere</button>");
    // What the catalog doesn't have stays English.
    expect(ready).toContain("is installed");
    __resetI18n();
  });
});

describe("checking and restarting", () => {
  it("shows an installed update as ready", async () => {
    stubFetch(() => okResponse({ available: true, version: "2.0.0", ready: true, restart: "ask" }));
    const check = await checkForUpdate();
    expect(check.ready).toBe(true);
    expect(__onUpdateEvent(undefined)).toMatchObject({ phase: "ready", version: "2.0.0", restart: "ask" });
  });

  it("asks the shell to restart into the installed version", async () => {
    const calls = stubFetch(() => okResponse(true));
    await restartToUpdate();
    expect(calls.map((c) => [c.url, c.init?.method])).toEqual([
      ["elyra://localhost/__update/restart", "POST"],
    ]);
  });

  it("fails when nothing is installed", async () => {
    stubFetch(
      () =>
        new Response(encode("no update has been installed yet"), {
          status: 200,
          headers: { "x-elyra-status": "error" },
        }),
    );
    await expect(restartToUpdate()).rejects.toThrow(/restart to update failed/);
  });
});
