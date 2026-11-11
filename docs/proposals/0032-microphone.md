# RFC 0032 — The microphone

**Status:** implemented (2026-11-11) · **Target:** 0.46.0 · follows [RFC 0009](0009-files.md), [RFC 0011](0011-in-app-assistant.md) and [RFC 0029](0029-installers.md) · see [Decisions](#decisions)

## Summary

An app can let its user speak instead of type. The shell records from the
microphone natively, hands the frontend the recording as a
[`ChosenFile`](0009-files.md), and turns it into text through the app's
Laravel server, which holds the key:

```rust
App::new()
    .microphone("Notes uses the microphone to let you dictate.")
    .backend(Backend::new(url).transcription())
```

```svelte
<Field label="Notes">
  <Textarea bind:value={notes} />
</Field>
<Microphone ontext={(text) => (notes = notes ? `${notes} ${text}` : text)} />
```

A click starts and stops it; held down (Space or Enter), it records until
let go. Esc throws the recording away.

## Design

### Recording in the shell, not the page

The page doesn't call `getUserMedia`. On macOS and Linux it's served from
`elyra://localhost`, which WKWebView and WebKitGTK don't treat as a secure
context, and neither asks the app before a page may listen; each webview
records in a format of its own (`audio/mp4` in WebKit, `audio/webm` in
WebView2). The shell records instead, with `cpal`, the same way on all
three, and writes one format: **WAV, 16 kHz, mono, 16-bit**, what speech
models take without converting, at about 1.9 MB a minute.

The recording is written under the app's cache folder and granted as a
`ChosenFile` (`kind: "open"`). The frontend can hand it to a command like a
file the user picked: attach it to a record, queue it in the outbox
(RFC 0007), or transcribe it. It never sees the samples or a path it could
change.

### Routes

`/__mic/*`, under a new capability, `Capability::Microphone`. It isn't
granted by default: `App::microphone(..)` grants it.

| Route | Body | Answers |
|---|---|---|
| `/__mic/start` | `{ max_seconds? }` | `true`, or an error (no device, denied, already recording) |
| `/__mic/stop` | — | the `ChosenFile` |
| `/__mic/cancel` | — | `true`; the samples are dropped, nothing is written |
| `/__mic/transcribe` | `{ file, language? }` | the text; `file` is a `ChosenFile` that may be read |

A recording of nothing but exact zeros isn't kept: no real input is that
quiet, and it's what macOS hands an app it won't let listen, with no
error. `stop` (or `failed`, at the longest) says so, and where to allow
the microphone.

One recording at a time for the whole app. Only the window that started
it can stop or cancel it, and it's cancelled when that window closes. It
stops by itself at `max_seconds`, 300 unless the app or the call says
less, and the window is told on `elyra:microphone`:

```ts
{ type: "level", level: 0.42 }        // ~15 a second, 0–1, for a meter
{ type: "stopped", file: ChosenFile } // at max_seconds
{ type: "failed", message: string }   // the device went away
```

### `App::microphone(why)`

One call says the app listens and why:

- it grants `Capability::Microphone` to the frontend;
- `rata bundle` asks the built app (`ELYRA_BUNDLE_INSPECT` answers
  `"microphone"`) and writes `why` as `NSMicrophoneUsageDescription` in
  the `.app`'s `Info.plist`. macOS shows it when it asks the user, and a
  bundled app that records without it is killed by the system;
- without elyra's `microphone` feature, or with an empty `why`, it's a
  [configuration problem](0025-commands-and-configuration.md), found
  before anything starts.

`App::microphone_max_seconds(n)` lowers the 300-second cap.

### Transcription through the server

`Backend::transcription()`, as `Backend::assistant()`: the shell sends
OpenAI's transcription request to
`{server}/api/elyra/transcription/v1/audio/transcriptions` with the
user's token, and Laravel adds the key. `Route::elyraTranscription()`
registers it; `config('elyra.transcription')` holds the key
(`OPENAI_API_KEY`), the models the desktop may ask for, the largest file
(25 MB, OpenAI's limit) and the requests per user per minute. Each one
fires `TranscriptionUsed($user, $model, $bytes)`.

Without `Backend::transcription()`, the container's `Ai` is used if there
is one (`AiProvider`, `OPENAI_API_KEY` from the environment): for
development, and for tests through `AiFake`. With neither,
`/__mic/transcribe` answers an error naming both.

### `@elyra/runtime`

```ts
microphone.start({ maxSeconds?: number }): Promise<void>
microphone.stop(): Promise<ChosenFile>
microphone.cancel(): Promise<void>
microphone.transcribe(file: ChosenFile, { language?: string }): Promise<string>
microphone.onLevel((level: number) => void): () => void
```

### `<Microphone>` in `@elyra/ui`

A button with three states: idle, recording (with a level meter) and
transcribing.

- **Click** starts; click again stops.
- **Space or Enter held** past 300 ms records until released, so a press
  is still a click.
- **Esc** while recording cancels.
- `aria-pressed` while recording, and a polite live region announces
  "Recording", "Transcribing" and failures.
- The meter is drawn with the theme's tokens and stands still under
  `prefers-reduced-motion`.

Props: `ontext(text)`, `onrecorded(file)` (with `transcribe={false}` it
stops there), `language`, `maxSeconds`, `disabled`, and `labels` for every
word it shows, English by default.

## Decisions

1. **Native recording, not `getUserMedia`.** Three webviews, three
   permission models and three formats, against one crate and one format.
   The shell also knows which window recorded, so closing it stops the
   recording.
2. **A file, not a stream.** A recording is a `ChosenFile`, so everything
   RFC 0009 gives files (commands, uploads, the outbox) works on it.
   Streaming transcription can come later on top of the same routes.
3. **WAV at 16 kHz mono.** Lossless, no encoder to bundle, and what speech
   models take natively. A five-minute recording is under 10 MB, inside
   OpenAI's 25 MB.
4. **The key stays on the server.** The same rule as the assistant's
   (RFC 0011). The environment's `Ai` is a fallback for development and
   tests, not a way to ship a key.
5. **Opt-in.** The capability isn't a default and the crate is behind a
   feature: an app that never listens neither asks the OS nor links
   `cpal`.
6. **Silence is an error.** macOS gives a denied app zeros, not a
   refusal, and a terminal's `cargo run` records as the terminal: without
   this, the first try in development is an empty recording and no reason.
7. **Press-and-hold past 300 ms.** Both gestures on one key, without a
   second button. Shorter than that, it's a click.

## Not in this RFC

- Streaming or live transcription (partial text while speaking).
- Text to speech in the shell (`SpeechRequest` stays in `elyra-ai`).
- Voice-filled forms (`prompt_as::<T>()` from a transcript), a global
  push-to-talk shortcut and offline voice notes through the outbox: each
  builds on this one.
- Choosing an input device; the system's default is used.
- The hardened-runtime entitlement `com.apple.security.device.audio-input`,
  needed only once `rata bundle` signs with a Developer ID.

## Testing

- `framework`: the WAV encoder (downmixing, resampling to 16 kHz, the
  header) on generated samples; the routes against a fake input device
  (`Microphone::fake(samples)`): start, the level events, stop to a
  `ChosenFile`, a recording of only zeros refused with why, cancel, `max_seconds`, only the starting window may stop,
  closing it cancels; the capability isn't a default; the problems;
  `ELYRA_BUNDLE_INSPECT` answers `"microphone"`.
- `elyra-ai`: `AiFake::transcript(..)` answers `transcribe`, and records
  what was sent.
- `ratatosk`: `Info.plist` has `NSMicrophoneUsageDescription` when the app
  says why, and not otherwise.
- Laravel (`framework/tests/laravel_transcription.rs`, CI's real app): the
  request reaches a stand-in for OpenAI with the server's key and not the
  user's token; a model not allowed, a file too large and too many
  requests are refused before it.
- `@elyra/runtime` and `@elyra/ui` (Vitest): the calls, click and hold,
  Esc, the labels, the ARIA, and `docs/ui/microphone.md` locked to the
  props.
- CI's Linux jobs need ALSA's headers (`libasound2-dev`) for `cpal`.
