/**
 * Signing in (RFC 0005): `auth` talks to `/__auth/*`, maps a 422 to a
 * `ValidationError` like any form, and keeps a store the UI can read. `fetch`
 * is stubbed; the token never appears on this side at all.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode } from "@msgpack/msgpack";
import { errorResponse, okResponse, stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { auth, ValidationError, CommandError } = await import("./index.js");

afterEach(() => {
  vi.unstubAllGlobals();
});

const ada = { signedIn: true, user: { id: 1, name: "Ada" }, reason: null };

describe("auth", () => {
  it("signs in with the email and password, and hands back the state", async () => {
    const calls = stubFetch(() => okResponse(ada));
    const state = await auth.signIn("ada@example.com", "secret");

    expect(state).toEqual(ada);
    expect(calls[0]!.url).toBe("elyra://localhost/__auth/sign-in");
    expect(decode(calls[0]!.init?.body as Uint8Array)).toEqual({
      email: "ada@example.com",
      password: "secret",
    });
  });

  it("turns a wrong password into a ValidationError, per field", async () => {
    stubFetch(() =>
      errorResponse(
        JSON.stringify({ email: ["The provided credentials are incorrect."] }),
        "validation",
      ),
    );
    const error = await auth.signIn("ada@example.com", "wrong").catch((e: unknown) => e);

    expect(error).toBeInstanceOf(ValidationError);
    expect((error as InstanceType<typeof ValidationError>).errors).toEqual({
      email: ["The provided credentials are incorrect."],
    });
  });

  it("keeps an error's kind, so offline can be told apart", async () => {
    stubFetch(() => errorResponse("can't reach the server: timed out", "offline"));
    const error = await auth.signIn("ada@example.com", "secret").catch((e: unknown) => e);

    expect(error).toBeInstanceOf(CommandError);
    expect((error as InstanceType<typeof CommandError>).kind).toBe("offline");
  });

  it("is a store: the current state now, and every change after", async () => {
    stubFetch((url) =>
      url.endsWith("/__auth/state")
        ? okResponse(ada)
        : url.endsWith("/__auth/sign-out")
          ? okResponse({ signedIn: false, user: null, reason: "signed-out" })
          : new Promise<Response>(() => {}), // the event poll: stays open
    );
    const seen: unknown[] = [];
    const stop = auth.subscribe((state) => seen.push(state));

    await vi.waitFor(() => expect(seen[seen.length - 1]).toEqual(ada));
    await auth.signOut();
    expect(seen[seen.length - 1]).toEqual({ signedIn: false, user: null, reason: "signed-out" });
    stop();
  });
});
