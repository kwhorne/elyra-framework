# RFC 0045 — `bigint` all the way: a wide integer that is one in the page

**Status:** implemented (2026-11-21) · **Target:** 0.51.0 · builds on [codegen](../codegen.md) · see [Decisions](#decisions)

## Summary

`App::codegen_bigint()` makes `rata codegen` type every 64-bit-and-wider
integer as `bigint`. The runtime doesn't follow: it decodes MessagePack
with `@msgpack/msgpack`'s defaults, so a value typed `bigint` arrives as a
`number`, a value past 2^53 arrives as a `number` that has already lost
its last digits, and a `bigint` passed as an argument can't be encoded
at all. The types say one thing and the values another.

This RFC makes the opt-in true end to end:

```rust
App::new().codegen_bigint()
```

```ts
const order = await api.orders_show(9_007_199_254_740_993n);   // a bigint goes out
order.id;            // 9007199254740993n — every i64 field is a bigint, small or not
order.lines.length;  // a number, as before: only the wide integers change
```

- **The page knows the policy**: the shell tells the runtime, as it tells
  it the token, so `codegen_bigint()` is one switch, not two.
- **Wide values decode as `bigint`**: the runtime decodes and encodes with
  `useBigInt64`, so an `int64`/`uint64` on the wire is a `bigint`, and a
  `bigint` argument is sent as one.
- **Small ones too**: MessagePack sends `42` in one byte whatever its Rust
  type, so `rata codegen` writes, beside each command, where its result's
  wide integers are, and the generated `api.*`, `live.*` and typed channels
  turn those into `bigint`s.
- **Without `codegen_bigint()` nothing changes.**

## Motivation

- **The types lie today.** With `codegen_bigint()` on, `order.id === 42n`
  is `false`, because `order.id` is `42`. `order.id + 1n` throws a
  `TypeError` mixing `bigint` and `number`. Code written to the types
  fails at runtime.
- **The wide values are the ones that matter.** Apps turn it on for ids
  from another system (Twitter-style snowflakes, 64-bit hashes), byte
  counts and money in minor units. A snowflake past 2^53 decoded as a
  `number` is a different id.
- **Arguments fail.** `api.orders_show(9007199254740993n)` throws in the
  encoder ("Unrecognized object"), because `bigint` isn't encoded without
  `useBigInt64`.
- The roadmap lists it under Codegen as "an optional `bigint` transport";
  only the typing half shipped.

## Current state (0.50)

- `NumberPolicy::BigInt` (`App::codegen_bigint()`) renders `i64`, `u64`,
  `i128`, `u128`, `isize`, `usize` as `bigint` in `bindings.ts`, struct
  fields included. `serde_json::Value` renders as `JsonValue`, whose
  numbers are `number`.
- Rust encodes with `rmp-serde`, which writes each integer in its smallest
  format: `42` is a positive fixint, `5_000_000_000` a `uint64`, whatever
  the field's type.
- The runtime's `invoke`, `invokeCancellable`, `live`, the event poll and
  `channel()` decode with `decode(buf)` and encode with `encode(args)`:
  `int64`/`uint64` become `number`, and a `bigint` can't be encoded.
- The page doesn't know the app's number policy; only `rata codegen` does.

## Design

### The page knows

With `codegen_bigint()`, the shell adds `bigint: true` to the
`globalThis.__ELYRA__` it injects before any page script (beside `token`
and `origin`). The runtime reads it once. `bindings.ts` also carries the
policy it was generated with, and the runtime warns once in the console
when the two disagree (bindings from before the switch, say).

### On the wire

With `bigint` on, every `decode` and `encode` in the runtime passes
`useBigInt64: true`:

- an `int64` or `uint64` on the wire decodes as a `bigint`, losslessly. A
  Rust integer that needs 64 bits to say is always one of those, so a
  value past 2^53 is never a lossy `number`;
- a `bigint` argument encodes as an `int64`/`uint64`, which `rmp-serde`
  reads into any Rust integer it fits;
- values that fit 32 bits are encoded small by Rust whatever their type,
  and still decode as `number`. That's what the next part is for.

Narrower integers (`i32`, `u32`, …) never need 64 bits on the wire, so
they stay `number` either way.

### Where the wide integers are

`rata codegen` knows each result's type, so with `bigint` on it writes a
**shape** beside each command whose result has a wide integer somewhere,
and one per named type that has one:

```ts
// bindings.ts (generated)
const $Order = { id: 1, customer_id: 1, lines: [$Line] };      // 1: a wide integer
const $Line = { qty: 1 };

export const api = {
  orders_show(id: bigint): Promise<Order> {
    return invoke("orders_show", id).then(wide($Order));
  },
};
export const live = {
  orders_show: (id: bigint) => live_("orders_show", id).map(wide($Order)),
};
```

- **A shape** is `1` for a wide integer, `[shape]` for each item of an
  array, `{ field: shape }` for an object's fields, and a named type's
  shape by reference. `Option<T>` is `T`'s shape (`null` is left alone);
  an enum's variants merge their fields' shapes; a map's values,
  `{ "*": shape }`.
- **`wide(shape)(value)`** (in `@elyra/runtime`) walks the value along the
  shape and turns each `number` it finds there into a `bigint`, in place.
  A `bigint` (a wide one, decoded so already) is left as it is.
- **Only what has one gets one**: a command whose result has no wide
  integer is generated as today, so an app with a few `u64` fields pays
  for those alone.
- **The same for live queries and typed channels** (`App::event::<T>`):
  `live.*` maps its values through the shape, and `channel<T>()` from the
  bindings gets its type's.

### `JsonValue`

A `serde_json::Value` can hold a number past 2^53, which now decodes as a
`bigint`. With `bigint` on, `JsonValue` is generated as `null | boolean |
number | bigint | string | …`, so the type says it can be one. Its small
numbers stay `number`: a JSON value has no field types to go by.

### What doesn't change

- Without `codegen_bigint()`, no `bigint: true`, no `useBigInt64`, no
  shapes: the bytes and the values are 0.50's.
- Rust's side: `rmp-serde` reads an `int64` into an `i64` and a positive
  fixint into an `i64` alike; nothing changes in commands.
- The store (`store.get`), the cache and the other facades aren't typed
  by codegen; a wide integer in them is a `bigint`, a small one a
  `number`, as with any untyped value. The docs say so.

### Testing

- Runtime unit tests: `wide` over objects, arrays, options, maps and
  nested named shapes; a `bigint` left alone; `null` left alone.
- `invoke`, `live` and the event poll decoding a `uint64` as a `bigint`
  only with the policy on, and encoding a `bigint` argument.
- Codegen tests: the shapes for a struct, a `Vec`, an `Option`, a
  `Page<T>`, an enum and a map; nothing emitted without the policy, or for
  a command with no wide integer.
- An end-to-end test over the protocol (`TestShell`): `u64::MAX` and
  `42u64` both arrive as `bigint`s through a generated call.

## Implementation plan

1. Shell: `bigint: true` in `__ELYRA__` with the policy.
2. Runtime: one codec with `useBigInt64` behind the flag, used by every
   decode and encode; `wide(shape)`; the mismatch warning.
3. Codegen: the shapes, the wrapped `api.*`, `live.*` and channels, and
   `JsonValue` with `bigint`.
4. Docs: codegen's number policy; the CHANGELOG's Upgrading for apps that
   already call `codegen_bigint()` (their values now match their types).

## Not in this RFC

- **Map keys that are wide integers** (`HashMap<u64, T>`):
  `@msgpack/msgpack` only takes `string` and `number` keys; such a map
  still decodes with `number` keys, and a key past 2^53 is an error. The
  docs say to key it by `String`.
- **Shapes for hand-written `invoke` calls**: `wide($Order)` is exported
  from the bindings, so a hand-written call can use it, but nothing does
  it for them.
- **`bigint` per field** (`#[specta(type = bigint)]` on one field while the
  rest stay `number`): the policy is the app's.

## Alternatives considered

- **Every integer as a `bigint`** (a reviver over every decoded value):
  no shapes needed, but `i32` fields would be `bigint`s too, against their
  `number` type, and every count and index in the app would need
  converting.
- **Rust always writing `int64` for a wide field** (no small encodings):
  `rmp-serde` has no such switch; a custom serializer for every `i64` in
  every type the app sends is far more code than shapes, and still wrong
  for values inside `serde_json::Value`.
- **Strings for wide integers** (`#[serde(with = "string")]`): what JSON
  APIs do, and what an app can already choose per field; not what a
  binary protocol that carries 64-bit integers natively should need.
- **Keep the types and drop the promise** (document that a `bigint` field
  may be a `number`): what 0.50 does implicitly, and the reason for this
  RFC.

## As built

- **The shell.** `Policy` carries the number policy (`with_bigint`, set from
  `App::codegen_bigint()`), and its init script adds `bigint: true` to the
  frozen `__ELYRA__` only then.
- **The runtime.** Every `encode`/`decode` in `@elyra/runtime` goes through
  `pack`/`unpack`, which pass `{ useBigInt64: true }` with the policy and
  nothing without it. `wide(shape)`, the `WideShape` type, `liveWith(map,
  command, ...args)` (what `live` is built on now) and `bigintPolicy(..)`
  are exported. `wide` converts in place, only integer `number`s, and
  leaves `null`, `bigint`s and anything the shape doesn't reach.
- **Codegen.** The shapes come from the types as they're exported
  (`ElyraFormat` after `specta-serde`), where each wide integer is already a
  `bigint` generic, so serde's renames, tags and flattening are what the
  shapes follow. A shape is `1`, `[..]` or `{ "key": .. }`, rendered inline
  beside each call; named types are expanded where they're used, generic
  ones (`Page<T>`) with their arguments' shapes, and a recursive type is cut
  off where it comes round again. An enum's variants are merged both under
  the variant's name (externally tagged) and beside the tag (internally
  tagged or untagged); an adjacently tagged enum's `content` isn't reached.
  A newtype is its field. `rawLive` is imported only when a live command
  has no shape, so the bindings pass `noUnusedLocals`.
- **Typed channels** get an `eventShapes` table and a `channel()` that
  widens each payload, only when a payload has a wide integer.
- **Tested**: runtime unit tests (a wide value decoded losslessly, a
  `bigint` argument encoded, `wide` over objects, arrays, maps, tuples,
  `null` and nested pages, `liveWith`, the policy warning); codegen tests
  (the exact shapes for a struct with a `Vec`, `Option`, map, tuple, tagged
  enum and `JsonValue`, `Page<T>`, a bare `i64` result, a result with none,
  a typed channel, and nothing without the policy); the init script, and
  the wire premise (`42i64` is one byte, `u64::MAX` a `uint64`). The
  generated bindings were type-checked with `tsc --strict
  --noUnusedLocals` against the runtime. Not tested: a real page in a
  webview decoding them, which the runtime tests stand in for.

## Decisions

1. **The page learns the policy from the shell** (`__ELYRA__.bigint`), since
   the runtime decodes before any bindings run; the bindings carry their
   own, and the runtime warns once in the console when they differ.
2. **Shapes beside each command in the bindings**, not a registry in the
   runtime: the generated code says what it converts, and the runtime knows
   nothing of the app's types.
3. **Commands, `live.*` and typed channels**, all three.
4. **Upgrading** says that an app already calling `codegen_bigint()` gets
   `bigint`s where it got `number`s, and names the patterns to look for
   (`Number(order.id)`, comparisons with a `number` literal).
