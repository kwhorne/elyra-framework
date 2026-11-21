# RFC 0051 — Citations: the sources a web-search answer cites

**Status:** accepted (2026-11-21) · **Target:** 0.53.0 · builds on [AI SDK](../ai.md), [RFC 0049](0049-openai-web-search.md) · see [Decisions](#decisions)

## Summary

A chat with `.web_search(…)` answers with text that rests on pages it
found, and both labs say which pages back which part of the answer.
Anthropic puts `citations` on its text blocks, and OpenAI puts
`url_citation` annotations on its `output_text`. The SDK reads neither:
`Response` holds the text, so an app that wants to show its sources has to
ask the model to write them into the text, and then parse them back out.

This RFC adds `Response::citations()`. It works the same on both labs,
reads only what each lab already sends, and changes no request:

```rust
let answer = ai.chat()
    .provider(Provider::Anthropic)
    .failover([Provider::OpenAI])
    .web_search(WebSearch::new())
    .prompt("What changed in the latest Rust release?")
    .await?;

println!("{}", answer.text());
for c in answer.citations() {
    // `span` is a byte range into `answer.text()`: the words this source backs.
    println!("  [{}] {} — {}", &answer.text()[c.span.clone()], c.title.as_deref().unwrap_or(""), c.url);
}
```

- **One `Citation` type for both labs**, with `url`, `title`, `cited_text`
  (the quote from the page, which only Anthropic sends) and `span` (the part
  of the answer it backs).
- **An answer without citations** has `citations() == []`. This covers
  every chat without web search, every other provider, and the cache.
- **Nothing else changes**: `text()`, the requests, structured output and
  streaming stay as they are.

## Motivation

- **The roadmap's AI SDK line** has "citations from web search" next.
  RFC 0049 deferred it to its own design (its open question 3).
- **Showing sources is most of why an app searches.** An answer about
  today's prices, a law or a release is only as good as the link behind it,
  and the user wants to click that link.
- **The data already arrives and is thrown away.** The Anthropic stream
  reader collects `citations_delta` into its blocks
  (`ai/src/anthropic.rs`), and nothing reads them after that.
- **Asking the model to write the sources into the text** costs tokens,
  makes structured output harder, and gives URLs the model may have
  rewritten.

## Current state (0.52)

- `Response { text, usage, steps }` (`ai/src/response.rs`) has no field
  for sources.
- **Anthropic** (`anthropic::run`): the final answer's `text` blocks are
  joined into `text`. Their `citations` arrays are
  `[{type: "web_search_result_location", url, title, cited_text,
  encrypted_index}]`, one per block that rests on a source. Anthropic
  gives no offsets: the block itself is the cited part. A whole answer and
  a streamed step (`read_stream`, with `citations_delta`) end up with the
  same blocks.
- **OpenAI** (`openai_responses::run`): each `output_text` part's `text`
  is appended to the answer, and its `annotations`
  (`[{type: "url_citation", url, title, start_index, end_index}]`) are
  ignored. The API reference says only that the indices are "the index of
  the first/last character … in the message". It does not say which unit
  a character is.
- The **cache** only stores chats without provider tools, so no cached
  answer ever had citations.
- `AiFake` scripts text, tool calls, refusals and errors. It cannot script
  citations.

## Design

### The type

```rust
/// A source a web-search answer cites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Citation {
    /// The page.
    pub url: String,
    /// The page's title, when the lab gives one.
    pub title: Option<String>,
    /// The words quoted from the page (Anthropic; up to about 150 characters).
    pub cited_text: Option<String>,
    /// The part of `Response::text()` this source backs, as a byte range
    /// on `char` boundaries.
    pub span: std::ops::Range<usize>,
}

impl Response {
    /// The sources the answer cites, in the order they appear in the text.
    /// Empty when the answer cites none.
    pub fn citations(&self) -> &[Citation];
}
```

`Citation` is `#[non_exhaustive]`, so a field can be added later, and it
derives `Serialize`, so a command can return it to the page as it is.
`Citation::new(url, span)` with `.title(…)` and `.cited_text(…)` builds
one for tests and for `AiFake`.

### Anthropic

While the final answer's `text` blocks are joined, the SDK notes where
each block starts and ends in the text. Each of its
`web_search_result_location` citations becomes a `Citation` with that
range as `span`. Several sources on one block share the block's span. A
citation of another type, such as a document or fetched page location, is
skipped (open question 3). `encrypted_index` stays in the raw content that
is sent back on later steps; it is not exposed.

### OpenAI

As each `output_text` part is appended, its `url_citation` annotations
become citations. Their indices are moved by the part's offset in the
answer and converted to byte offsets, under the reading in open
question 2. An index past the part's end is clamped to the end. An
annotation whose range is then empty is dropped.

### Order and duplicates

Citations are sorted by `span.start` and keep each lab's order within a
span. The same URL backing two parts of the answer gives two citations.
`docs/ai.md` shows how to collapse them to a list of sources.

### Testing

The tests run offline against the local server in `ai/tests/common`.

- **Anthropic:** a whole answer with two cited blocks and an uncited one,
  checking spans, titles and `cited_text`; the same answer streamed with
  `on_text` and `citations_delta`; and a citation of another type, which
  is skipped.
- **OpenAI:** annotations on two parts with non-ASCII text before them, so
  the conversion is tested; an index past the end; and an empty range.
- **Failover:** from Anthropic to OpenAI, with the citations coming from
  the provider that answered.
- **Without a source:** a chat without web search and a cached answer both
  give `[]`.
- **`AiFake`:** a scripted cited answer.
- The opt-in live test (`ELYRA_AI_LIVE=1`) also checks that every span
  slices `text()` on `char` boundaries. That test is what will settle open
  question 2.

## Implementation plan

1. `Citation` in `ai/src/response.rs`, the `citations` field on
   `Response`, and the export from `elyra_ai` and `elyra::ai`.
2. Reading citations in `anthropic::run` and `openai_responses::run`.
   Chat Completions, the cache and the fake set `citations` to empty.
3. `AiFake::text_citing`, the tests, the docs (`ai.md`: a "Citations"
   section under provider tools), the roadmap and the CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **`span` as a byte range into `text()`, or no position at all** (just
   the list of sources)?
   *Recommendation:* a byte range. It slices the text as Rust does
   (`&text[span]`), so an app can show a footnote where the source applies.
   An app that wants only the list ignores it. Both labs send a position,
   explicitly on OpenAI and by block on Anthropic, so dropping it throws
   away something they give.
2. **OpenAI's `start_index`/`end_index`: which unit, and is the end
   exclusive?** The docs say "character" and nothing more.
   *Recommendation:* read them as Unicode scalar values (Rust `char`s,
   Python's string indices) with an exclusive end, like a slice. Clamp to
   the text, and let the live test confirm it. If it turns out to be UTF-16
   or an inclusive end, the fix is the conversion in one function, and no
   API changes.
3. **Web fetch citations** (Anthropic, with `citations: {enabled: true}`
   on the fetch tool): include them now?
   *Recommendation:* not in this RFC. They take a new option on
   `WebFetch`, and their citations point into a fetched document (with
   character offsets and a document index) rather than at a URL, so
   mapping them to `url` needs the fetch result as well. The type leaves
   room for them later.
4. **How the fake scripts citations:** `AiFake::new().text_citing(text,
   [Citation::new(url, span)])`?
   *Recommendation:* yes. It mirrors `tool_call_saying`, and the span is
   checked against the text when the script is built, so a test with a
   wrong span fails where it is written.
5. **A `sources()` helper** that returns the distinct URLs with their
   titles?
   *Recommendation:* no, not now. It is three lines with `citations()`
   (shown in the docs), and which title or order to keep is the app's
   choice.

## Not in this RFC

- **Citations in `Chat::stream`.** It sends no provider tools, so there
  is nothing to cite. A prompt with `on_text` does get citations on its
  `Response`.
- **Web fetch citations** (open question 3) and citations for documents
  the app sends in.
- **Citations from Chat Completions providers.** Their search models and
  online variants, such as OpenRouter's `:online`, each have their own
  shape.
- **Sending citations back** in a later conversation (`Message` has no
  place for them). Anthropic's `encrypted_index` is kept only within one
  prompt's tool loop.

## Alternatives considered

- **The raw content blocks or output items on `Response`** (`raw() ->
  &Value`): no design is needed and nothing is lost, but every app writes
  two parsers, one per lab, and failover gives it whichever shape it gets.
- **Footnote markers written into `text()`** (`… shipped [1]`): ready to
  show, but it changes the text an app may parse or store, and the marker
  style is the app's choice.

## Decisions

1. **`span` is a byte range into `text()`**, on `char` boundaries.
2. **OpenAI's indices are read as Unicode scalar values with an exclusive
   end**, clamped to the text; the live test confirms the reading.
3. **No web fetch citations in this RFC.**
4. **`AiFake::text_citing(text, citations)`**, with each span checked
   against the text when the script is built.
5. **No `sources()` helper**; `docs/ai.md` shows how to collapse
   citations into sources.
