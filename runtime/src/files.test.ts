/**
 * Files the user chose (RFC 0009): a drop hands over `ChosenFile`s, which a
 * command takes as they are.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { onFileDrop, transfers } = await import("./index.js");
type ChosenFile = import("./index.js").ChosenFile;
type Transfer = import("./index.js").Transfer;

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

const nextPoll = () => request("/__events");

const transfer = (id: string, sent: number, state: Transfer["state"] = "running"): Transfer => ({
  id,
  direction: "upload",
  name: "contract.pdf",
  sent,
  total: 100,
  state,
  error: null,
});

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

describe("transfers", () => {
  it("is read once, then followed as they move", async () => {
    pending = stubPendingFetch();
    let seen: Transfer[] = [];
    unsubscribes.push(transfers.subscribe((t) => (seen = t)));
    expect(seen).toEqual([]);

    (await request("/__auth/transfers")).resolve(okResponse([transfer("t1", 10)]));
    await vi.waitFor(() => expect(seen[0]?.sent).toBe(10));

    (await nextPoll()).resolve(
      new Response(encode([["elyra:transfers", [transfer("t1", 60)]]]), { status: 200 }),
    );
    await vi.waitFor(() => expect(seen[0]?.sent).toBe(60));
  });

  it("cancels one through its route, and takes the answer", async () => {
    pending = stubPendingFetch();
    let seen: Transfer[] = [];
    unsubscribes.push(transfers.subscribe((t) => (seen = t)));
    (await request("/__auth/transfers")).resolve(okResponse([transfer("t1", 10)]));

    const done = transfers.cancel("t1");
    const call = await request("/__auth/transfer-cancel");
    expect(decode(new Uint8Array(call.init!.body as Uint8Array))).toEqual({ id: "t1" });
    call.resolve(okResponse([transfer("t1", 10, "cancelled")]));
    await done;
    expect(seen[0]?.state).toBe("cancelled");
  });
});
