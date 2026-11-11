# Microphone

Speak instead of type. The button records in the shell, not the page, and
hands back what was said as text, through the app's server
([RFC 0032](../proposals/0032-microphone.md)).

<!-- example: microphone/basic -->
```svelte
<script>
  import { Field, Microphone, Textarea } from "@elyra/ui";

  let notes = $state("");
  const add = (text) => (notes = notes ? `${notes} ${text}` : text);
</script>

<Field label="Notes" description="Type, or dictate.">
  <Textarea bind:value={notes} rows={3} />
</Field>
<Microphone language="en" ontext={add} />
```

- **A click** starts recording, and the next one stops it.
- **Space or Enter held** past 300 ms records until let go: push-to-talk.
  A shorter press works like a click.
- **Esc** while recording throws the recording away.
- **`ontext(text)`** gets what was said, once it's transcribed. Nothing is
  sent for silence.
- **`onrecorded(file)`** gets the recording itself, a
  [`ChosenFile`](../backend.md#files) (a 16 kHz mono WAV), before it's
  transcribed. With `transcribe={false}` it stops there: attach it to a
  record, or queue it in the outbox, like a file the user chose.
- **`language`** tells the model what's spoken (`"no"`, `"en"`). Without
  it, the model guesses.
- **`maxSeconds`** stops a recording sooner than the app's longest (300
  seconds unless `App::microphone_max_seconds` says less).

The app needs `App::microphone(why)` and elyra's `microphone` feature, and
`Backend::transcription()` for the text:

```rust
App::new()
    .microphone("Notes uses the microphone to let you dictate.")
    .backend(Backend::new(url).transcription())
```

`why` is what macOS shows when it asks the user. `rata bundle` writes it
in the `.app`. On the server, `Route::elyraTranscription()` adds the key
([docs/backend.md](../backend.md)).

## Words

Every word the button shows or says is in `labels`, English by default:

<!-- example: microphone/labels -->
```svelte
<script>
  import { Microphone } from "@elyra/ui";

  let said = $state("");
</script>

<Microphone
  language="no"
  labels={{
    button: "Dikter",
    recording: "Tar opp",
    transcribing: "Skriver ut…",
    hint: "Klikk, eller hold mellomrom, for å snakke",
  }}
  ontext={(text) => (said = text)}
/>
<p>{said}</p>
```

| Label | Default | Where |
|---|---|---|
| `button` | `"Dictate"` | The button's name. |
| `recording` | `"Recording"` | Said, and shown, while it records. |
| `transcribing` | `"Transcribing…"` | Said, and shown, while it transcribes. |
| `hint` | `"Click, or hold Space, to talk"` | The button's tooltip. |

## Accessibility

- It's a toggle button: `aria-pressed` while it records, with a name that
  stays the same (`labels.button`).
- What it's doing is said in a polite live region ("Recording",
  "Transcribing…"). A failure, such as the user not allowing the
  microphone, is a `role="alert"`.
- While it transcribes, it's busy (`aria-busy`) and can't be pressed.
- The level meter is decoration (`aria-hidden`). With less motion asked
  for, it stands still while it records.
- Taken off the page while recording, it cancels the recording.

## API

### Microphone

| Prop | Type | Default | Description |
|---|---|---|---|
| `transcribe` | `boolean` | `true` | Turn the recording into text, for `ontext`. |
| `language` | `string` | — | What's spoken: `"no"`, `"en"`. |
| `maxSeconds` | `number` | — | Stop by itself after this long. |
| `disabled` | `boolean` | `false` | |
| `labels` | `object` | `{}` | Its words: `button`, `recording`, `transcribing`, `hint`. |
| `ontext` | `(text) => void` | — | Told what was said. |
| `onrecorded` | `(file) => void` | — | Told the recording, a `ChosenFile`. |
| `class` | `string` | `""` | Added to the outer element. |

Other attributes go to the button.
