/**
 * The app's assistant (RFC 0011): the `assistant` store over the
 * `/__assistant/*` routes and the `elyra:assistant` channel, and its
 * questions answered before a command runs.
 */
import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  errorResponse,
  headersOf,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { assistant, __assistantConfirm } = await import("./index.js");
type State = Parameters<Parameters<typeof assistant.subscribe>[0]>[0];

// Once used, the assistant's channel is followed for good (a question must
// arrive with the panel closed too), so the pump runs across the tests: one
// stub for the whole file.
const pending: PendingCall[] = stubPendingFetch();
const unsubscribes: Array<() => void> = [];

afterEach(() => {
  __assistantConfirm(null);
});

afterAll(async () => {
  await drainPump(pending, unsubscribes);
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

function body(call: PendingCall): unknown {
  return decode(new Uint8Array(call.init!.body as ArrayBuffer));
}

function last<T>(list: T[]): T | undefined {
  return list[list.length - 1];
}

/** The shell sends `events` on the next poll. */
async function events(...batch: unknown[]): Promise<void> {
  (await request("/__events")).resolve(
    new Response(encode(batch.map((e) => ["elyra:assistant", e])), { status: 200 }),
  );
}

/** Subscribe, and start from an empty conversation. */
async function fresh(): Promise<() => State> {
  let state: State | undefined;
  unsubscribes.push(assistant.subscribe((s) => (state = s)));
  const reset = assistant.reset();
  (await request("/__assistant/reset")).resolve(okResponse(true));
  await reset;
  return () => state!;
}

describe("assistant", () => {
  it("sends the user's words and follows the turn to its answer", async () => {
    const state = await fresh();
    const asked = assistant.ask("  How many customers?  ");
    const prompt = await request("/__assistant/prompt");
    // Outside a window there's no route to send.
    expect(body(prompt)).toEqual({ text: "How many customers?", route: null });
    expect(headersOf(prompt).get("x-elyra-token")).toBe("test-token");
    expect(headersOf(prompt).get("x-elyra-client-id")).toBeTruthy();
    prompt.resolve(okResponse(true));
    await asked;
    expect(state().busy).toBe(true);
    expect(state().items).toEqual([{ kind: "user", text: "How many customers?" }]);

    await events({ type: "tool", tool: "customers_index", phase: "started", ok: null });
    await vi.waitFor(() =>
      expect(last(state().items)).toEqual({ kind: "tool", tool: "customers_index", ok: null }),
    );
    await events(
      { type: "tool", tool: "customers_index", phase: "finished", ok: true },
      { type: "answer", text: "You have 3." },
    );
    await vi.waitFor(() => expect(state().busy).toBe(false));
    expect(state().items).toEqual([
      { kind: "user", text: "How many customers?" },
      { kind: "tool", tool: "customers_index", ok: true },
      { kind: "answer", text: "You have 3." },
    ]);
  });

  it("sends the window's route with the words (RFC 0013)", async () => {
    // Only these, and taken back after: the other stubs stay.
    const g = globalThis as Record<string, unknown>;
    g.location = { hash: "#/customers/42?tab=notes&q=a b" };
    g.addEventListener = () => {};
    g.removeEventListener = () => {};
    try {
      await fresh();
      const asked = assistant.ask("Who's this?");
      const prompt = await request("/__assistant/prompt");
      expect(body(prompt)).toEqual({ text: "Who's this?", route: "/customers/42?tab=notes&q=a+b" });
      prompt.resolve(okResponse(true));
      await asked;
      await events({ type: "answer", text: "Ada." });
    } finally {
      delete g.location;
      delete g.addEventListener;
      delete g.removeEventListener;
    }
  });

  it("keeps a step's link into the app, and only that (RFC 0013)", async () => {
    const state = await fresh();
    const asked = assistant.ask("Update Ada, and the others");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await asked;
    const step = (tool: string, ok: boolean, link: string | null) => [
      { type: "tool", tool, phase: "started", ok: null },
      { type: "tool", tool, phase: "finished", ok, link },
    ];
    await events(
      ...step("customers_update", true, "/customers/42"),
      ...step("customers_show", true, "https://evil.example/x"),
      ...step("invoices_show", true, "//evil.example/x"),
      ...step("customers_destroy", false, "/customers/7"),
      ...step("customers_index", true, null),
      { type: "answer", text: "Done." },
    );
    await vi.waitFor(() => expect(state().busy).toBe(false));
    expect(state().items.slice(1, -1)).toEqual([
      { kind: "tool", tool: "customers_update", ok: true, link: "/customers/42" },
      { kind: "tool", tool: "customers_show", ok: true },
      { kind: "tool", tool: "invoices_show", ok: true },
      { kind: "tool", tool: "customers_destroy", ok: false },
      { kind: "tool", tool: "customers_index", ok: true },
    ]);
  });

  it("asks nothing while a turn runs, and nothing blank", async () => {
    const state = await fresh();
    await assistant.ask("   ");
    const asked = assistant.ask("One");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await asked;
    await assistant.ask("Two");
    await tick(10);
    expect(pending.some((p) => p.url.endsWith("/__assistant/prompt"))).toBe(false);
    expect(state().items).toEqual([{ kind: "user", text: "One" }]);
    await events({ type: "answer", text: "Done." });
    await vi.waitFor(() => expect(state().busy).toBe(false));
  });

  it("shows a refused prompt, a failed turn and a stop", async () => {
    const state = await fresh();
    const asked = assistant.ask("Hi");
    (await request("/__assistant/prompt")).resolve(
      errorResponse("The assistant is still answering in this window."),
    );
    await asked;
    expect(state().busy).toBe(false);
    expect(last(state().items)).toEqual({
      kind: "error",
      text: "The assistant is still answering in this window.",
    });

    const again = assistant.ask("Hi");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await again;
    await events({ type: "error", message: "Sign in to use the assistant." });
    await vi.waitFor(() =>
      expect(last(state().items)).toEqual({ kind: "error", text: "Sign in to use the assistant." }),
    );
    expect(state().busy).toBe(false);

    const third = assistant.ask("Delete them all");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await third;
    const stopped = assistant.stop();
    (await request("/__assistant/stop")).resolve(okResponse(true));
    expect(await stopped).toBe(true);
    await events({ type: "stopped" });
    await vi.waitFor(() => expect(last(state().items)).toEqual({ kind: "stopped" }));
    expect(state().busy).toBe(false);
  });

  it("asks the user before a command runs, and sends the answer", async () => {
    await fresh();
    const questions: unknown[] = [];
    __assistantConfirm((c) => {
      questions.push(c);
      return true;
    });
    const asked = assistant.ask("Delete customer 7");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await asked;
    await events({
      type: "confirm",
      id: "q1",
      tool: "customers_destroy",
      description: "Delete a customer.",
      arguments: { id: 7 },
    });
    const answer = await request("/__assistant/confirm");
    expect(body(answer)).toEqual({ id: "q1", accept: true });
    answer.resolve(okResponse(true));
    expect(questions).toEqual([
      { id: "q1", tool: "customers_destroy", description: "Delete a customer.", arguments: { id: 7 } },
    ]);
  });

  it("says no when there's nobody to ask", async () => {
    await fresh();
    // No dialog without a document, and a handler that fails is a no.
    const asked = assistant.ask("Delete customer 8");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await asked;
    await events({ type: "confirm", id: "q2", tool: "customers_destroy", description: "", arguments: {} });
    expect(body(await request("/__assistant/confirm"))).toEqual({ id: "q2", accept: false });

    __assistantConfirm(() => Promise.reject(new Error("closed")));
    await events({ type: "confirm", id: "q3", tool: "customers_destroy", description: "", arguments: {} });
    expect(body(await request("/__assistant/confirm"))).toEqual({ id: "q3", accept: false });
  });

  it("says what the window has selected (RFC 0013)", async () => {
    await fresh();
    const said = assistant.selected("customers", [4, 7]);
    const selected = await request("/__assistant/selected");
    expect(body(selected)).toEqual({ kind: "customers", ids: [4, 7], route: null });
    selected.resolve(okResponse(true));
    await said;
  });

  it("starts over, and reads its tools, its prompts, its source and the user's key", async () => {
    const state = await fresh();
    const asked = assistant.ask("Hi");
    (await request("/__assistant/prompt")).resolve(okResponse(true));
    await asked;
    await events({ type: "answer", text: "Hello." });
    await vi.waitFor(() => expect(state().items.length).toBe(2));
    const reset = assistant.reset();
    (await request("/__assistant/reset")).resolve(okResponse(true));
    await reset;
    expect(state().items).toEqual([]);

    const tools = assistant.tools();
    (await request("/__assistant/tools")).resolve(okResponse(["customers_index"]));
    expect(await tools).toEqual(["customers_index"]);
    const suggested = { name: "overdue", title: "Overdue invoices", text: "Find them.", arguments: [] };
    const prompts = assistant.prompts();
    (await request("/__assistant/prompts")).resolve(okResponse([suggested]));
    expect(await prompts).toEqual([suggested]);
    const source = assistant.source();
    (await request("/__assistant/source")).resolve(okResponse("keychain"));
    expect(await source).toBe("keychain");

    const kept = assistant.setKey("sk-ant-mine");
    const key = await request("/__assistant/key");
    expect(body(key)).toBe("sk-ant-mine");
    key.resolve(okResponse(true));
    expect(await kept).toBe(true);
    const forgot = assistant.setKey(null);
    const none = await request("/__assistant/key");
    expect(body(none)).toBeNull();
    none.resolve(okResponse(false));
    expect(await forgot).toBe(false);
  });
});
