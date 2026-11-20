/**
 * Edit it in its own app (RFC 0038): the `editing` store, and what the user
 * does with a file being edited. The page names an editing by its id only.
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

const { editing } = await import("./index.js");
type Editing = import("./index.js").Editing;

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

const one = (state: Editing["state"], error: string | null = null): Editing => ({
  id: "e1",
  name: "Contract Acme.docx",
  state,
  savedAt: state === "saved" ? 1_763_600_000_000 : null,
  error,
});

describe("editing", () => {
  it("is read once, then followed as saves are sent", async () => {
    pending = stubPendingFetch();
    let seen: Editing[] = [];
    unsubscribes.push(editing.subscribe((e) => (seen = e)));
    expect(seen).toEqual([]);

    (await request("/__auth/editing")).resolve(okResponse([one("open")]));
    await vi.waitFor(() => expect(seen[0]?.state).toBe("open"));

    (await request("/__events")).resolve(
      new Response(encode([["elyra:editing", [one("saved")]]]), { status: 200 }),
    );
    await vi.waitFor(() => expect(seen[0]?.state).toBe("saved"));
    expect(seen[0]?.savedAt).toBe(1_763_600_000_000);
  });

  for (const [call, op] of [
    [() => editing.show("e1"), "editing-show"],
    [() => editing.reveal("e1"), "editing-reveal"],
    [() => editing.finish("e1"), "editing-finish"],
    [() => editing.finish("e1", { discard: true }), "editing-discard"],
    [() => editing.keepMine("e1"), "editing-keep-mine"],
    [() => editing.takeTheirs("e1"), "editing-take-theirs"],
  ] as const) {
    it(`sends ${op} with the editing's id, and takes the list it answers`, async () => {
      pending = stubPendingFetch();
      let seen: Editing[] = [];
      unsubscribes.push(editing.subscribe((e) => (seen = e)));
      (await request("/__auth/editing")).resolve(okResponse([one("conflict", "changed")]));
      await vi.waitFor(() => expect(seen[0]?.state).toBe("conflict"));

      const done = call();
      const sent = await request(`/__auth/${op}`);
      expect(decode(new Uint8Array(sent.init!.body as Uint8Array))).toEqual({ id: "e1" });
      sent.resolve(okResponse([one("saved")]));
      await done;
      expect(seen[0]?.state).toBe("saved");
    });
  }
});
