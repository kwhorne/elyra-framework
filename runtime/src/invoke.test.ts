/**
 * The command path: framing, headers, and the error contract. `fetch` is stubbed,
 * so no app runs here — these pin the wire format described in docs/wire-format.md.
 */
import { afterEach, describe, expect, it } from "vitest";
import { decode } from "@msgpack/msgpack";
import {
  errorResponse,
  headersOf,
  okResponse,
  rejection,
  stubFetch,
} from "./test-support.js";
import { vi } from "vitest";

// The module reads `globalThis.__ELYRA__` at import time.
(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { invoke, invokeCancellable, CommandError, ValidationError, ForbiddenError, validationErrors } =
  await import("./index.js");

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("invoke — request framing", () => {
  it("posts a compact msgpack array of arguments and decodes the result", async () => {
    const calls = stubFetch(() => okResponse(42));
    const result = await invoke<number>("add", 2, 40);

    expect(result).toBe(42);
    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe("elyra://localhost/__cmd/add");
    expect(calls[0]!.init?.method).toBe("POST");
    expect(decode(calls[0]!.init?.body as Uint8Array)).toEqual([2, 40]);
  });

  it("sends an empty array when the command takes no arguments", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("ping");

    expect(decode(calls[0]!.init?.body as Uint8Array)).toEqual([]);
  });

  it("declares a msgpack content type, not JSON", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("ping");

    expect(headersOf(calls[0]).get("content-type")).toBe("application/msgpack");
  });

  it("preserves argument structure, including nulls and nested objects", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("save", { title: "hi", tags: ["a", "b"], parent: null }, 7);

    expect(decode(calls[0]!.init?.body as Uint8Array)).toEqual([
      { title: "hi", tags: ["a", "b"], parent: null },
      7,
    ]);
  });

  it("decodes a struct response as a plain object (named maps, not arrays)", async () => {
    stubFetch(() => okResponse({ id: 1, name: "Ada", admin: true }));

    await expect(invoke("get_user", 1)).resolves.toEqual({ id: 1, name: "Ada", admin: true });
  });

  it("decodes a unit return as null", async () => {
    stubFetch(() => okResponse(null));

    await expect(invoke("do_nothing")).resolves.toBeNull();
  });
});

describe("invoke — headers", () => {
  it("attaches the IPC token and a client id to every request", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("ping");

    const headers = headersOf(calls[0]);
    expect(headers.get("x-elyra-token")).toBe("test-token");
    expect(headers.get("x-elyra-client-id")).toMatch(/.+/);
  });

  it("reuses one client id for the document, so the shell keeps a single queue", async () => {
    const calls = stubFetch(() => okResponse(null));
    await invoke("one");
    await invoke("two");

    const first = headersOf(calls[0]).get("x-elyra-client-id");
    const second = headersOf(calls[1]).get("x-elyra-client-id");
    expect(first).toBe(second);
  });
});

describe("invoke — error contract", () => {
  it("throws a CommandError carrying the kind and detail", async () => {
    stubFetch(() => errorResponse("kaboom"));

    const error = await rejection(invoke("explode"), CommandError);
    expect(error.command).toBe("explode");
    expect(error.kind).toBe("command");
    expect(error.detail).toBe("kaboom");
    expect(error.message).toContain("kaboom");
  });

  it("treats a 200 marked x-elyra-status: error as a failure", async () => {
    // The shell answers 200 + an error status for command-level failures, so
    // res.ok alone is not enough to decide success.
    stubFetch(
      () =>
        new Response("nope", {
          status: 200,
          headers: { "x-elyra-status": "error", "x-elyra-error-kind": "command" },
        }),
    );

    await expect(invoke("explode")).rejects.toBeInstanceOf(CommandError);
  });

  it("treats a bare non-ok response as a failure even without the status header", async () => {
    stubFetch(() => new Response("gateway sad", { status: 502 }));

    const error = await rejection(invoke("explode"), CommandError);
    expect(error.detail).toBe("gateway sad");
  });

  it("defaults the kind to command when the shell sends no kind header", async () => {
    stubFetch(() => new Response("mystery", { status: 500 }));

    const error = await rejection(invoke("explode"), CommandError);
    expect(error.kind).toBe("command");
  });

  it("surfaces a panic as a CommandError of kind panic", async () => {
    stubFetch(() => errorResponse("command `x` panicked: boom", "panic"));

    const error = await rejection(invoke("x"), CommandError);
    expect(error.kind).toBe("panic");
  });

  it("throws a ValidationError with a parsed field bag", async () => {
    const bag = { email: ["The email must be a valid email address."] };
    stubFetch(() => errorResponse(JSON.stringify(bag), "validation"));

    const error = await rejection(invoke("create_account", {}), ValidationError);
    expect(error.errors).toEqual(bag);
    expect(error.kind).toBe("validation");
    expect(validationErrors(error)).toEqual(bag);
  });

  it("falls back to a plain CommandError when a validation body is not JSON", async () => {
    stubFetch(() => errorResponse("not json at all", "validation"));

    const error = await rejection(invoke("create_account", {}), CommandError);
    expect(error).not.toBeInstanceOf(ValidationError);
    expect(error.kind).toBe("validation");
  });
});

describe("invoke — rejected at the security gate", () => {
  it("throws ForbiddenError when the shell rejects the token", async () => {
    stubFetch(() => errorResponse("missing or invalid x-elyra-token", "forbidden", 403));

    const error = await rejection(invoke("add", 1, 2), ForbiddenError);
    expect(error.name).toBe("ForbiddenError");
    // The message has to be actionable: a hand-written page hitting this needs
    // to know it is the token, not the command.
    expect(error.message).toContain("elyra://localhost/__cmd/add");
    expect(error.message).toContain("IPC token");
  });

  it("reports an ungranted command ability instead of blaming the token", async () => {
    // `#[command(can = "posts.delete")]` without `App::allow_ability` — the same
    // 403 + `forbidden` shape as a bad token, so the body is the only thing that
    // tells the two apart. Saying "IPC token" here sends you hunting in the
    // wrong place entirely.
    stubFetch(() =>
      errorResponse(
        "command `delete_post` requires the `posts.delete` ability, which is not granted " +
          "to the frontend (grant it with App::allow_ability)",
        "forbidden",
        403,
      ),
    );

    const error = await rejection(invoke("delete_post", 7), ForbiddenError);
    expect(error.message).toContain("posts.delete");
    expect(error.message).toContain("App::allow_ability");
    expect(error.message).not.toContain("IPC token");
    expect(error.detail).toContain("requires the `posts.delete` ability");
  });

  it("reports an ungranted capability instead of blaming the token", async () => {
    stubFetch(() =>
      errorResponse(
        "capability StoreClear is not granted to the frontend (grant it with App::allow_frontend)",
        "forbidden",
        403,
      ),
    );

    const error = await rejection(invoke("wipe", null), ForbiddenError);
    expect(error.message).toContain("StoreClear");
    expect(error.message).not.toContain("IPC token");
  });

  it("does not claim forbidden for a 403 with a different error kind", async () => {
    // Only `x-elyra-error-kind: forbidden` is a gate refusal; anything else on a
    // 403 is an ordinary command failure.
    stubFetch(() => errorResponse("upstream said no", "command", 403));

    const error = await rejection(invoke("read_file", "/etc/passwd"), CommandError);
    expect(error).not.toBeInstanceOf(ForbiddenError);
    expect(error.kind).toBe("command");
  });
});

describe("validationErrors", () => {
  it("returns null for non-validation errors", () => {
    expect(validationErrors(new Error("nope"))).toBeNull();
    expect(validationErrors("not an error")).toBeNull();
    expect(validationErrors(new CommandError("x", "plain message"))).toBeNull();
  });

  it("recovers a bag from a plain CommandError whose detail is JSON", async () => {
    const bag = { age: ["The age must be at least 18."] };
    stubFetch(() => errorResponse(JSON.stringify(bag)));

    const error = await rejection(invoke("create_account", {}), CommandError);
    expect(validationErrors(error)).toEqual(bag);
  });

  it("returns null when the detail is JSON but not an object", async () => {
    stubFetch(() => errorResponse(JSON.stringify(["a", "b"])));

    const error = await rejection(invoke("x"), CommandError);
    expect(validationErrors(error)).toBeNull();
  });
});

describe("invokeCancellable", () => {
  it("sends a request id and can cancel it", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/__cancel") ? okResponse(true) : okResponse(1),
    );

    const job = invokeCancellable<number>("slow");
    expect(await job.result).toBe(1);
    expect(headersOf(calls[0]).get("x-elyra-request-id")).toBe(job.id);

    await job.cancel();
    const cancelCall = calls.find((c) => c.url.endsWith("/__cancel"));
    expect(cancelCall).toBeDefined();
    expect(decode(cancelCall!.init?.body as Uint8Array)).toBe(job.id);
  });

  it("gives concurrent jobs distinct ids", async () => {
    stubFetch(() => okResponse(1));

    const a = invokeCancellable("slow");
    const b = invokeCancellable("slow");
    await Promise.all([a.result, b.result]);

    expect(a.id).not.toBe(b.id);
  });

  it("authenticates the cancel request too", async () => {
    const calls = stubFetch(() => okResponse(true));

    const job = invokeCancellable("slow");
    await job.result;
    await job.cancel();

    const cancelCall = calls.find((c) => c.url.endsWith("/__cancel"))!;
    expect(cancelCall.url).toBe("elyra://localhost/__cancel");
    expect(headersOf(cancelCall).get("x-elyra-token")).toBe("test-token");
  });

  it("rejects result with a CommandError when the command fails", async () => {
    stubFetch(() => errorResponse("boom"));

    const job = invokeCancellable("slow");
    const error = await rejection(job.result, CommandError);
    expect(error.command).toBe("slow");
  });
});
