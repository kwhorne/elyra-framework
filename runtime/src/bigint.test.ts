/**
 * `bigint` all the way (RFC 0045): with the shell's `bigint: true`, 64-bit
 * integers decode as `bigint`s, a `bigint` argument encodes as one, and
 * `wide(shape)` turns the small ones in wide fields into `bigint`s too.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import { stubFetch } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token", bigint: true };

const { bigintPolicy, invoke, liveWith, wide } = await import("./index.js");

const BIG = 9_007_199_254_740_993n; // 2^53 + 1: a number can't say it
const wire = (value: unknown) =>
  new Response(encode(value, { useBigInt64: true }), {
    status: 200,
    headers: { "content-type": "application/msgpack", "x-elyra-status": "ok" },
  });

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("bigint all the way", () => {
  it("decodes a wide value as a bigint, losslessly, and sends a bigint argument", async () => {
    const calls = stubFetch(() => wire({ id: BIG, small: 42, count: 3 }));
    const order = (await invoke("orders_show", BIG)) as { id: bigint; small: number; count: number };
    expect(order.id).toBe(BIG);
    // Small on the wire, so a number until a shape says otherwise.
    expect(order.small).toBe(42);
    const sent = decode(new Uint8Array(calls[0]!.init!.body as ArrayBuffer), { useBigInt64: true });
    expect(sent).toEqual([BIG]);
  });

  it("decodes a map keyed by wide integers, keyed by their exact digits", async () => {
    // rmp-serde writes a `HashMap<u64, T>` with integer keys; a key past 2^32
    // is a 64-bit integer on the wire, which decoded to a bigint key and threw.
    // fixmap(2): 1 => "small", int64 2^53+1 => "wide", as rmp-serde writes it.
    const bytes = new Uint8Array([
      0x82, 0x01, 0xa5, ...new TextEncoder().encode("small"),
      0xd3, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xa4, ...new TextEncoder().encode("wide"),
    ]);
    stubFetch(
      () =>
        new Response(bytes, {
          status: 200,
          headers: { "content-type": "application/msgpack", "x-elyra-status": "ok" },
        }),
    );
    const byId = (await invoke("by_id")) as Record<string, string>;
    expect(byId).toEqual({ "1": "small", "9007199254740993": "wide" });
    expect(byId[String(BIG)]).toBe("wide");
  });

  it("turns the integers a shape points at into bigints, and nothing else", () => {
    const value = {
      id: 7,
      already: BIG,
      none: null,
      count: 3,
      lines: [{ qty: 1, name: "bolt" }, { qty: 2, name: "nut" }],
      by_day: { mon: 1, tue: 2 },
      pair: [5, "x"],
      page: { data: [{ id: 1 }], total: 1 },
    };
    const out = wide<typeof value>({
      id: 1,
      already: 1,
      none: 1,
      lines: [{ qty: 1 }],
      by_day: { "*": 1 },
      pair: { "0": 1 },
      page: { data: [{ id: 1 }] },
    })(value) as unknown as Record<string, unknown>;
    expect(out.id).toBe(7n);
    expect(out.already).toBe(BIG);
    expect(out.none).toBeNull();
    expect(out.count).toBe(3);
    expect(out.lines).toEqual([{ qty: 1n, name: "bolt" }, { qty: 2n, name: "nut" }]);
    expect(out.by_day).toEqual({ mon: 1n, tue: 2n });
    expect(out.pair).toEqual([5n, "x"]);
    expect(out.page).toEqual({ data: [{ id: 1n }], total: 1 });
    // A shape that reaches nothing leaves it be.
    expect(wide({ id: 1 })(null)).toBeNull();
    expect(wide([1])("text")).toBe("text");
  });

  it("maps a live query's values through it", async () => {
    stubFetch((url) =>
      url.endsWith("/__events") ? new Promise<Response>(() => {}) as never : wire({ id: "s1", value: { n: 5 } }),
    );
    const store = liveWith(wide<{ n: bigint }>({ n: 1 }), "counter");
    const seen: unknown[] = [];
    const off = store.subscribe((s) => seen.push(s.value));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toEqual({ n: 5n }));
    off();
  });

  it("says once when the bindings and the app disagree", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    bigintPolicy(true);
    expect(warn).not.toHaveBeenCalled();
    bigintPolicy(false);
    bigintPolicy(false);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0]![0]).toContain("rata codegen");
  });
});
