/**
 * Signed in to more than one server (RFC 0022): `accounts` is the list,
 * read once and followed on `elyra:accounts`; `auth` switches between the
 * servers and signs out of one, or all.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  okResponse,
  stubFetch,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { accounts, auth, onSignedOutOf, onSignedOutOfWorkspace } = await import("./index.js");

function batch(pairs: [string, unknown][]): Response {
  return new Response(encode(pairs), { status: 200 });
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const ACME = "https://acme.example.com";
const KUNDE = "https://kunde.example.com";

const account = (server: string, active: boolean) => ({
  server,
  userId: "7",
  user: null,
  active,
});

const both = (active: string) => ({
  accounts: [account(ACME, active === ACME), account(KUNDE, active === KUNDE)],
  active,
  signedOutOf: null,
  reason: null,
});

describe("accounts", () => {
  it("is read once, then followed", async () => {
    pending = stubPendingFetch();
    const seen: (string | null)[] = [];
    unsubscribes.push(accounts.subscribe((s) => seen.push(s.active)));
    expect(seen).toEqual([null]);

    (await request("/__auth/accounts")).resolve(okResponse(both(KUNDE)));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe(KUNDE));

    (await request("/__events")).resolve(batch([["elyra:accounts", both(ACME)]]));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe(ACME));
  });

  it("says which server was left, and why", async () => {
    pending = stubPendingFetch();
    const left: string[] = [];
    unsubscribes.push(onSignedOutOf((server, reason) => left.push(`${server}:${reason}`)));
    (await request("/__events")).resolve(
      batch([
        ["elyra:accounts", both(ACME)],
        [
          "elyra:accounts",
          { accounts: [account(ACME, true)], active: ACME, signedOutOf: KUNDE, reason: "expired" },
        ],
      ]),
    );
    await vi.waitFor(() => expect(left).toEqual([`${KUNDE}:expired`]));
  });

  it("tells a workspace left from a server left (RFC 0031)", async () => {
    pending = stubPendingFetch();
    const servers: string[] = [];
    const workspaces: string[] = [];
    unsubscribes.push(onSignedOutOf((server, reason) => servers.push(`${server}:${reason}`)));
    unsubscribes.push(
      onSignedOutOfWorkspace((server, workspace, reason) =>
        workspaces.push(`${server}#${workspace}:${reason}`),
      ),
    );
    (await request("/__events")).resolve(
      batch([
        [
          "elyra:accounts",
          { ...both(ACME), signedOutOf: ACME, reason: "expired", signedOutOfWorkspace: "7" },
        ],
        [
          "elyra:accounts",
          { accounts: [account(ACME, true)], active: ACME, signedOutOf: KUNDE, reason: "signed-out" },
        ],
      ]),
    );
    await vi.waitFor(() => expect(servers).toEqual([`${KUNDE}:signed-out`]));
    expect(workspaces).toEqual([`${ACME}#7:expired`]);
  });
});

describe("auth across servers", () => {
  it("switches, and signs out of one or all, by server", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/__events")
        ? batch([])
        : okResponse({ signedIn: true, user: null, reason: null, server: ACME }),
    );
    const state = await auth.switchTo(ACME);
    expect(state.server).toBe(ACME);
    await auth.signOutOf(KUNDE);
    await auth.signOutOfDiscarding(KUNDE);
    await auth.signOutAll();
    await auth.signOutAllDiscarding();
    const ops = calls
      .filter((c) => c.url.includes("/__auth/"))
      .map((c) => c.url.split("/__auth/")[1]);
    expect(ops).toEqual([
      "switch",
      "sign-out-of",
      "sign-out-of-discarding",
      "sign-out-all",
      "sign-out-all-discarding",
    ]);
    const body = (op: string) =>
      decode(new Uint8Array(calls.find((c) => c.url.endsWith(`/${op}`))!.init!.body as Uint8Array));
    expect(body("switch")).toEqual({ server: ACME });
    expect(body("sign-out-of")).toEqual({ server: KUNDE });
    expect(body("sign-out-all")).toBeNull();
  });

  it("switches and leaves a workspace, by server and id (RFC 0031)", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/__events")
        ? batch([])
        : okResponse({ signedIn: true, user: null, reason: null, server: ACME }),
    );
    await auth.switchWorkspace(ACME, "12");
    await auth.signOutOfWorkspace(ACME, "7");
    const body = (op: string) =>
      decode(new Uint8Array(calls.find((c) => c.url.endsWith(`/${op}`))!.init!.body as Uint8Array));
    expect(body("switch-workspace")).toEqual({ server: ACME, workspace: "12" });
    expect(body("sign-out-of-workspace")).toEqual({ server: ACME, workspace: "7" });
  });
});
