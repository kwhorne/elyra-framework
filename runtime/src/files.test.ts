/**
 * Files the user chose (RFC 0009): a drop hands over `ChosenFile`s, which a
 * command takes as they are.
 */
import { afterEach, describe, expect, it } from "vitest";
import { encode } from "@msgpack/msgpack";
import { drainPump, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { onFileDrop } = await import("./index.js");
type ChosenFile = import("./index.js").ChosenFile;

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
});

async function nextPoll(): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith("/__events"));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

describe("onFileDrop", () => {
  it("hands over the dropped files as ChosenFiles", async () => {
    pending = stubPendingFetch();
    const dropped: ChosenFile[][] = [];
    unsubscribes.push(onFileDrop((files) => dropped.push(files)));

    const file: ChosenFile = { id: "g1", path: "/Users/ada/a.pdf", name: "a.pdf", kind: "open" };
    (await nextPoll()).resolve(new Response(encode([["elyra:file-drop", [file]]]), { status: 200 }));
    for (let i = 0; i < 20 && dropped.length === 0; i++) await tick();

    expect(dropped).toEqual([[file]]);
    expect(dropped[0]![0]!.id).toBe("g1");
  });
});
