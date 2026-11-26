/** The app's menu at runtime (RFC 0055): `menu.set` and `menu.state`. */
import { afterEach, describe, expect, it } from "vitest";
import { decode } from "@msgpack/msgpack";
import { drainPump, errorResponse, okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { menu } = await import("./index.js");

let pending: PendingCall[] = [];

afterEach(async () => {
  await drainPump(pending, []);
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const sent = (call: PendingCall) => decode(new Uint8Array(call.init!.body as ArrayBuffer));

describe("menu", () => {
  it("sends a change with its id, and says whether the item was there", async () => {
    pending = stubPendingFetch();
    const changed = menu.set("doc.save", { enabled: false, label: "Save Draft" });
    const call = await request("/__menu/set");
    expect(sent(call)).toEqual({ id: "doc.save", enabled: false, label: "Save Draft" });
    call.resolve(okResponse(true));
    expect(await changed).toBe(true);
  });

  it("reads an item as it is now", async () => {
    pending = stubPendingFetch();
    const read = menu.state("doc.track");
    const call = await request("/__menu/state");
    expect(sent(call)).toBe("doc.track");
    call.resolve(okResponse({ enabled: true, checked: true, label: "Track changes" }));
    expect(await read).toEqual({ enabled: true, checked: true, label: "Track changes" });
  });

  it("fails as a command does when it's refused", async () => {
    pending = stubPendingFetch();
    const changed = menu.set("doc.save", { checked: true });
    (await request("/__menu/set")).resolve(errorResponse("capability Menu is not granted", "forbidden", 403));
    await expect(changed).rejects.toThrow(/Menu/);
  });
});
