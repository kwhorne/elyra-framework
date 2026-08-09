/**
 * The `/__sys/*` and `/__update/*` bridges.
 *
 * The payloads matter more than they look: Rust deserializes them into structs
 * with snake_case fields, so a camelCase slip here is a runtime error in the
 * shell that no type-check catches. These tests pin the translation.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode } from "@msgpack/msgpack";
import { errorResponse, headersOf, okResponse, stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { dialog, shell, clipboard, notify, paths, checkForUpdate } = await import("./index.js");

/** Decode the msgpack argument of the first recorded call. */
function bodyOf(call: { init?: RequestInit } | undefined): unknown {
  return decode(call?.init?.body as Uint8Array);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("dialog", () => {
  it("posts open defaults with snake_case keys the Rust side expects", async () => {
    const calls = stubFetch(() => okResponse([]));
    await dialog.open();

    expect(calls[0]!.url).toBe("elyra://localhost/__sys/dialog.open");
    expect(calls[0]!.init?.method).toBe("POST");
    expect(bodyOf(calls[0])).toEqual({
      title: null,
      directory: false,
      multiple: false,
      filters: [],
      start_dir: null,
    });
  });

  it("maps open options, renaming startDir to start_dir", async () => {
    const calls = stubFetch(() => okResponse(["/tmp/a.txt"]));
    const picked = await dialog.open({
      title: "Pick",
      directory: true,
      multiple: true,
      filters: [{ name: "Text", extensions: ["txt", "md"] }],
      startDir: "/tmp",
    });

    expect(picked).toEqual(["/tmp/a.txt"]);
    expect(bodyOf(calls[0])).toEqual({
      title: "Pick",
      directory: true,
      multiple: true,
      filters: [{ name: "Text", extensions: ["txt", "md"] }],
      start_dir: "/tmp",
    });
  });

  it("maps save options, renaming defaultName to default_name", async () => {
    const calls = stubFetch(() => okResponse("/tmp/out.md"));
    const path = await dialog.save({ title: "Save", defaultName: "out.md", startDir: "/tmp" });

    expect(path).toBe("/tmp/out.md");
    expect(bodyOf(calls[0])).toEqual({
      title: "Save",
      default_name: "out.md",
      filters: [],
      start_dir: "/tmp",
    });
  });

  it("resolves null when a save dialog is cancelled", async () => {
    stubFetch(() => okResponse(null));

    await expect(dialog.save()).resolves.toBeNull();
  });

  it("resolves an empty list when an open dialog is cancelled", async () => {
    stubFetch(() => okResponse([]));

    await expect(dialog.open()).resolves.toEqual([]);
  });
});

describe("shell, clipboard, notify, paths", () => {
  it("sends the target as a bare string for shell.openExternal", async () => {
    const calls = stubFetch(() => okResponse(null));
    await shell.openExternal("https://elyracode.com");

    expect(calls[0]!.url).toBe("elyra://localhost/__sys/shell.open");
    expect(bodyOf(calls[0])).toBe("https://elyracode.com");
  });

  it("reads the clipboard with a null argument", async () => {
    const calls = stubFetch(() => okResponse("copied"));

    await expect(clipboard.readText()).resolves.toBe("copied");
    expect(calls[0]!.url).toBe("elyra://localhost/__sys/clipboard.read");
    expect(bodyOf(calls[0])).toBeNull();
  });

  it("writes the clipboard as a bare string", async () => {
    const calls = stubFetch(() => okResponse(null));
    await clipboard.writeText("hello");

    expect(calls[0]!.url).toBe("elyra://localhost/__sys/clipboard.write");
    expect(bodyOf(calls[0])).toBe("hello");
  });

  it("sends notify with an explicit null body rather than omitting it", async () => {
    const calls = stubFetch(() => okResponse(null));
    await notify("Done");

    // `body: Option<String>` on the Rust side — the key has to be present.
    expect(bodyOf(calls[0])).toEqual({ title: "Done", body: null });
  });

  it("sends notify with a body when given one", async () => {
    const calls = stubFetch(() => okResponse(null));
    await notify("Done", "Export finished");

    expect(bodyOf(calls[0])).toEqual({ title: "Done", body: "Export finished" });
  });

  it("resolves OS paths, passing nulls through", async () => {
    stubFetch(() =>
      okResponse({
        home: "/Users/ada",
        config: "/Users/ada/.config",
        data: null,
        cache: null,
        temp: "/tmp",
        exe: "/Applications/App.app",
      }),
    );

    const p = await paths();
    expect(p.home).toBe("/Users/ada");
    expect(p.data).toBeNull();
  });

  it("attaches the IPC token to system calls", async () => {
    const calls = stubFetch(() => okResponse(null));
    await notify("Done");

    expect(headersOf(calls[0]).get("x-elyra-token")).toBe("test-token");
  });
});

describe("system errors", () => {
  it("names the failing operation in the thrown error", async () => {
    stubFetch(() => errorResponse("no clipboard on this platform"));

    await expect(clipboard.readText()).rejects.toThrow(/clipboard\.read/);
    stubFetch(() => errorResponse("no clipboard on this platform"));
    await expect(clipboard.readText()).rejects.toThrow(/no clipboard on this platform/);
  });

  it("fails when the system feature is not compiled in (404)", async () => {
    stubFetch(() => new Response("not found", { status: 404 }));

    await expect(paths()).rejects.toThrow(/paths/);
  });
});

describe("checkForUpdate", () => {
  it("returns the shell's verdict when no update is available", async () => {
    const calls = stubFetch(() => okResponse({ available: false }));

    await expect(checkForUpdate()).resolves.toEqual({ available: false });
    expect(calls[0]!.url).toBe("elyra://localhost/__update/check");
    expect(headersOf(calls[0]).get("accept")).toBe("application/msgpack");
  });

  it("surfaces a manifest error reported by the shell", async () => {
    stubFetch(() => okResponse({ available: false, error: "manifest unreachable" }));

    const result = await checkForUpdate();
    expect(result.error).toBe("manifest unreachable");
  });

  it("throws when the update endpoint is unavailable", async () => {
    stubFetch(() => new Response("not found", { status: 404 }));

    await expect(checkForUpdate()).rejects.toThrow(/update check 404/);
  });
});
