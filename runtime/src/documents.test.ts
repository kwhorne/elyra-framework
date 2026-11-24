/**
 * Documents the OS opens with the app (RFC 0052): the launch's files from
 * `documents.initial()`, later ones on `elyra:open-documents`.
 */
import { afterEach, describe, expect, it } from "vitest";
import { encode } from "@msgpack/msgpack";
import { drainPump, okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { documents } = await import("./index.js");
type ChosenFile = import("./index.js").ChosenFile;

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const file = (name: string): ChosenFile => ({
  id: `g-${name}`,
  path: `/Users/ada/${name}`,
  name,
  kind: "document",
});

describe("documents", () => {
  it("initial() asks once for the launch's files", async () => {
    pending = stubPendingFetch();
    const asked = documents.initial();
    const call = await request("/__documents/initial");
    call.resolve(okResponse([file("Plan.kver")]));
    expect(await asked).toEqual([file("Plan.kver")]);
    expect(call.init?.method).toBe("POST");
  });

  it("initial() is [] when the app was opened with none", async () => {
    pending = stubPendingFetch();
    const asked = documents.initial();
    (await request("/__documents/initial")).resolve(okResponse(null));
    expect(await asked).toEqual([]);
  });

  it("onOpen hears each batch opened while the app runs", async () => {
    pending = stubPendingFetch();
    const opened: ChosenFile[][] = [];
    unsubscribes.push(documents.onOpen((files) => opened.push(files)));

    const batch = [file("a.kver"), file("b.kver")];
    (await request("/__events")).resolve(
      new Response(encode([["elyra:open-documents", batch]]), { status: 200 }),
    );
    for (let i = 0; i < 20 && opened.length === 0; i++) await tick();

    expect(opened).toEqual([batch]);
    expect(opened[0]![0]!.kind).toBe("document");
  });
});
