# The microphone

Let the user speak instead of type ([RFC 0032](proposals/0032-microphone.md)).
The shell records, not the page, and hands the frontend the recording as a
[`ChosenFile`](system.md): a 16 kHz mono WAV it can transcribe, attach to a
record or queue in the outbox.

```toml
# Cargo.toml
elyra = { version = "0.46", features = ["microphone", "ai", "backend"] }
```

```rust
App::new()
    .microphone("Notes uses the microphone to let you dictate.")
    .backend(Backend::new(url).transcription())
```

```svelte
<script>
  import { Microphone } from "@elyra/ui";
  let notes = $state("");
</script>

<textarea bind:value={notes}></textarea>
<Microphone language="en" ontext={(text) => (notes += ` ${text}`)} />
```

## Why the shell records

On macOS and Linux the page is served from `elyra://localhost`, which
WKWebView and WebKitGTK don't treat as a secure context, so
`getUserMedia` isn't there. Each webview would also record in a format of
its own. The shell records with `cpal` on all three, and writes one format.

## `App::microphone(why)`

- It grants `Capability::Microphone` (`/__mic/*`), which is off by default.
- `why` is what macOS shows when it asks the user. `rata bundle` writes it
  as `NSMicrophoneUsageDescription` in the `.app`'s `Info.plist`. A bundled
  app that records without it is ended by macOS.
- An empty `why`, or no `microphone` feature, is a
  [configuration problem](configuration.md): the app doesn't start.
- `App::microphone_max_seconds(n)` stops each recording sooner than 300
  seconds.

On Linux, building needs ALSA's headers (`libasound2-dev`). On Windows,
the user allows the microphone in Settings → Privacy. A refusal is the
error `start` gives.

## Recording

One recording at a time, for the whole app. Only the window that started it
can stop or cancel it, and it's cancelled when that window closes.

```ts
import { microphone, onMicrophone } from "@elyra/runtime";

await microphone.start({ maxSeconds: 60 });
const off = onMicrophone((e) => {
  if (e.type === "level") meter = e.level;      // 0–1, about 15 a second
  if (e.type === "stopped") keep(e.file);       // it reached its longest
  if (e.type === "failed") show(e.message);     // unplugged, taken away
});
const file = await microphone.stop();           // a ChosenFile
const text = await microphone.transcribe(file, { language: "no" });
```

`microphone.cancel()` stops and keeps nothing. Recordings are written under
the app's cache folder, and those older than a week are removed when the
next one starts.

## Transcription

`Backend::transcription()` sends the recording to
`POST /api/elyra/transcription/v1/audio/transcriptions` with the user's
token. Laravel adds the OpenAI key and decides what may be sent
([backend.md](backend.md#speech-to-text)). Signed out, nothing is
transcribed.

Without it, the container's `elyra::ai::Ai` is used (`AiProvider`, from
`OPENAI_API_KEY`): for development, not for shipping a key. With neither,
`transcribe` fails with what to add.

`transcribe` takes a `ChosenFile` that may be read, so an audio file the
user opened in a dialog or dropped works too, up to 25 MB. A path chosen in
a save dialog is refused.

## In Rust

`elyra::microphone::transcribe(&ctx, &file, Some("no"))` does what
`/__mic/transcribe` does, from a command.

## Testing

`Microphone::fake(rate, channels, samples)` hears `samples` at once on each
recording, and `Microphone::fake_unavailable("…")` can't be opened.
`AiFake::transcript("…")` answers the transcription:

```rust
let fake = AiFake::new().transcript("Send a reminder on these.");
let app = App::new()
    .microphone("Dictate notes.")
    .swap(Microphone::fake(48_000, 2, samples))
    .swap(Ai::fake(fake.clone()));
```

`fake.transcriptions()` lists what was sent: the file's name and size,
the model and the language.
