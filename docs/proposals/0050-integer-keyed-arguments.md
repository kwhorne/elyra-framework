# RFC 0050 — Integer-keyed arguments: a map the page sends, as the command takes it

**Status:** implemented (2026-11-21) · **Target:** 0.52.0 · builds on [commands](../commands.md), [codegen](../codegen.md) · see [Decisions](#decisions)

## Summary

A command that takes a map keyed by integers can't be called from the
page:

```rust
#[command]
async fn set_quantities(ctx: Ctx, by_line: HashMap<i64, u32>) -> Result<()> { … }
```

```ts
await api.set_quantities({ 3: 2, 7: 1 });   // fails: "invalid type: string, expected i64"
```

The bindings type the argument `{ [key in number]: number }`, so this
compiles. But a JavaScript object's keys are strings, the runtime sends
`"3"` and `"7"`, and `rmp_serde` won't read a string as an `i64`. It fails
the same way with and without `codegen_bigint()`, for any integer key type,
and for a map nested in a struct. Since 0.52 `codegen.md` says to key such
a map by `String`; this RFC makes the map work as typed.

**The decoding of a command's arguments falls back, when it fails, to
reading a map key that's a string of digits as the integer the key's type
asks for.** Only map keys; only after the strict decoding failed; nothing
else is read more leniently:

```ts
await api.set_quantities({ 3: 2, 7: 1 });            // by_line: {3: 2, 7: 1}
await api.set_quantities({ "9007199254740993": 1 }); // past 2^53, exactly
```

## Motivation

- **The types say it works.** The bindings accept the call, and it fails
  at run time with a message about strings and integers the app never
  wrote.
- **Lookups keyed by id** (quantities per line, a setting per user, a
  sort order per item) are a common shape for a command's argument.
- **The page can't send it right**: `@msgpack/msgpack` encodes a JS
  object's keys as strings, and a `Map` as an empty object. Encoding
  integer keys from the page would need, for every command, codegen to say
  where its arguments' integer-keyed maps are.

## Current state (0.52)

- Generated code decodes the argument tuple with
  `rmp_serde::from_slice(args)` and returns `Error::decode` on failure.
- serde's integer map keys are read with `deserialize_i64` (and friends)
  on the key; `rmp_serde` refuses a string there.
- MCP tool calls carry JSON, and `serde_json` already reads a string map
  key as an integer when the key type is one; only the page's MessagePack
  path fails.

## Design

### Where

`elyra::__private::decode_args::<T>(bytes)`, called by the generated code
instead of `rmp::from_slice`:

1. `rmp_serde::from_slice::<T>(bytes)`. If it succeeds, that's the
   answer: every call that works today decodes exactly as today, at the
   same cost.
2. If it fails: decode the bytes to an `rmpv::Value`, then deserialize `T`
   from it through a deserializer that is `rmpv`'s, except for map keys.
   A map key that is a string, asked for as an integer
   (`deserialize_i8` … `deserialize_u128`), is parsed as one: digits, an
   optional `-`, within the type's range. Anything else about a key, and
   everything that isn't a key, is as strict as before.
3. If that fails too, the error is the first one, the strict decoding's.
   The message the app sees doesn't change for a call that was wrong.

### What it reads

- `HashMap<i64, T>`, `BTreeMap<u32, T>`, `HashMap<u64, T>` past 2^53 (the
  key is the string's digits, so nothing is lost), nested in a struct, a
  `Vec`, an `Option`.
- A key `"3.0"`, `"03"`, `" 3"` or `"three"` is still an error: only what
  `str::parse` reads as the integer type, nothing rounded or trimmed.
- A newtype key (`struct LineId(i64)`) reads through to its integer.

### Codegen

Nothing changes: the bindings already type such an argument
`{ [key in number]: T }` (or `string` keys with `codegen_bigint()`, since
0.52's map-key fix), and either is accepted. `codegen.md`'s note about
arguments is replaced by a line saying integer keys are read from the
page's string keys.

### Testing

Unit tests on `decode_args`: an `i64`, a `u32` and a wide `u64` key; a map
in a struct and in a `Vec`; a key that isn't digits, one out of range and
one with a sign on an unsigned type, each failing with the strict error; a
string *value* where an integer is expected still failing; a correct call
decoded on the first path. A command test through `TestApp::invoke` with
the bytes the runtime sends (`encode({3: 2})`).

## Implementation plan

1. `framework/src/args.rs`: `decode_args`, the key-lenient deserializer
   over `rmpv::Value`.
2. `macros`: the generated decode calls it.
3. Tests, `codegen.md` and `commands.md`, roadmap, CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **Strict first, lenient on failure, or always the lenient path?**
   Always would decode every call through `rmpv::Value`: an allocation per
   call for a case that's rare.
   *Recommendation:* strict first. A call that works today pays nothing,
   and its decoding can't change.
2. **Map keys only, or also a string where an integer field is
   expected?** The second would accept `{qty: "3"}` for a `u32` field,
   which the types reject and the app didn't mean.
   *Recommendation:* map keys only. They're the one place the page can't
   send an integer.
3. **Fix it in the page instead**: codegen emits where each argument's
   integer-keyed maps are, and the runtime encodes those keys as integers?
   The bytes would then be what Rust expects, but it's a shape per
   argument, an encoder that writes a map from an object with integer
   keys, and a hand-written `invoke` would still fail.
   *Recommendation:* no. The Rust side knows the types; reading the keys
   there fixes generated and hand-written calls alike.

## Not in this RFC

- **Events the page sends to Rust** with integer-keyed maps (none of the
  built-in channels take one).
- **Keys of other types**: `bool`, `char`, enums. Their string forms need
  their own rules.

## Alternatives considered

- **Document it and leave it** (0.52's note): the bindings keep accepting
  a call that fails.
- **A `Map` from the page**: `@msgpack/msgpack` encodes a `Map` as an
  empty object, so it would need an extension codec on the page and
  knowledge of the types there.

## As built

As designed, with these specifics:

- **`framework/src/args.rs`**: `decode_args` (re-exported for the derive
  as `elyra::__private::decode_args`), a `Lenient` deserializer over
  `rmpv::Value` that hands map keys to a `Key` deserializer, and `Key`,
  which parses a string for `deserialize_i8` … `deserialize_u128` with
  `str::parse` and is otherwise `Lenient`. Arrays, maps, options and
  newtypes are walked by `Lenient` so a map anywhere in the arguments is
  reached; enums, unit structs and MessagePack extensions are `rmpv`'s.
- **A map inside an enum's payload** is read strictly: `rmpv`'s enum
  access doesn't hand its content back. Not a case the page sends for
  commands today.
- **The error when both fail is the strict one**, unchanged: for
  `{"three": 2}`, "failed to decode command arguments: wrong msgpack
  marker FixStr(5)".
- **Tests**: unit tests in `args.rs` (`i64`, negative, `u32` sorted, a
  `u64` past 2^53; a map in a struct sent as a map and as an array, in an
  `Option`, in a `Vec`, a newtype key; `"3.0"`, `" 3"`, `"three"`,
  `"256"` and `"-1"` for a `u8`, and an empty key, each the strict error; a
  string for an integer field, argument or list item still an error; a
  correct call and a `String`-keyed map unchanged), and
  `framework/tests/integer_keyed_arguments.rs` through a command with the
  bytes the page sends. The command test fails without the derive's change.

## Decisions

1. **Strict first**; the key-lenient decoding only when it fails, and the
   strict error reported when both do.
2. **Map keys only**: a string where an integer field is expected stays
   an error.
3. **Fixed on the Rust side**, for generated and hand-written calls alike;
   the page sends what it sends today.
