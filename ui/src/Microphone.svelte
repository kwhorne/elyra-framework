<!--
  Speak instead of type (RFC 0032): a button that records in the shell and
  hands back what was said. A click starts and stops; the button, or Space
  or Enter, held past 300 ms records until let go (or focus leaves); Esc
  throws the
  recording away. With
  `transcribe` (the default) the recording becomes text through the app's
  server, for `ontext`; `onrecorded` gets the recording itself.

  `can="notes.dictate"` hides it from a user without the ability (or, with
  `canMode="disable"`, disables it). In a `<Field>` with `transcribe={false}`
  the recording is the field's value, and the button its control: named by
  the field's label, described by its description or its error.
-->
<script>
  import { getContext, untrack } from "svelte";
  import { can as canStore, microphone, onMicrophone } from "@elyra/runtime";
  import { FIELD, FORM } from "./internal/context.js";

  let {
    transcribe = true,
    language = undefined,
    maxSeconds = undefined,
    disabled = false,
    can = undefined,
    canMode = "hide",
    labels = {},
    ontext = undefined,
    onrecorded = undefined,
    class: className = "",
    ...rest
  } = $props();

  const WORDS = {
    button: "Dictate",
    recording: "Recording",
    transcribing: "Transcribing…",
    hint: "Click, or hold, to talk",
  };
  /** Held longer than this, a key is push-to-talk; shorter, a click. */
  const HOLD_MS = 300;

  const words = $derived({ ...WORDS, ...labels });
  const allowed = $derived(can ? $canStore(can) : true);
  const off = $derived(disabled || !allowed);
  // Dictating into a field's text, the text control is the field's; only a
  // recording kept as it is (`transcribe={false}`) is the field's value.
  const field = getContext(FIELD);
  const control = $derived(transcribe ? null : field);
  // What it hands over is the user's change: a `<Form>`'s `dirty`.
  const form = getContext(FORM);
  const own = $props.id();

  /** "idle", "starting", "recording" or "transcribing". */
  let phase = $state("idle");
  let level = $state(0);
  let failure = $state(null);
  let starting = null;
  /** The key held down, and since when, for push-to-talk. */
  let held = null;

  const recording = $derived(phase === "starting" || phase === "recording");
  const busy = $derived(phase === "transcribing");
  const status = $derived(failure ?? (recording ? words.recording : busy ? words.transcribing : ""));

  function fail(err) {
    failure = err?.detail ?? err?.message ?? String(err);
    phase = "idle";
    level = 0;
  }

  function start() {
    if (off || phase !== "idle") return;
    failure = null;
    phase = "starting";
    starting = microphone
      .start({ maxSeconds })
      .then(() => {
        if (phase === "starting") phase = "recording";
        return true;
      })
      .catch((err) => {
        fail(err);
        return false;
      });
  }

  async function stop() {
    if (!recording) return;
    // A stop before the start answered waits for it.
    if (!(await starting)) return;
    let file;
    try {
      file = await microphone.stop();
    } catch (err) {
      return fail(err);
    }
    await finish(file);
  }

  async function cancel() {
    if (!recording) return;
    phase = "idle";
    level = 0;
    if (await starting) {
      try {
        await microphone.cancel();
      } catch {
        // Already over: nothing was kept either way.
      }
    }
  }

  /** What was heard: handed over, then made text. */
  async function finish(file) {
    level = 0;
    if (!transcribe) form?.changed();
    onrecorded?.(file);
    if (!transcribe) {
      phase = "idle";
      return;
    }
    phase = "transcribing";
    try {
      const text = await microphone.transcribe(file, { language });
      phase = "idle";
      if (text.trim()) {
        form?.changed();
        ontext?.(text.trim());
      }
    } catch (err) {
      fail(err);
    }
  }

  /** A press of the pointer began here: the click after it is the press's. */
  let pressed = false;

  function click() {
    if (pressed) {
      pressed = false;
      return;
    }
    if (recording) stop();
    else start();
  }

  /** The button held down with the mouse, a finger or a pen: push-to-talk, as Space is. */
  function pointerdown(event) {
    if (event.button !== 0 || recording || held || off || busy) return;
    pressed = true;
    // Let go anywhere, and it's still this button's.
    event.currentTarget.setPointerCapture?.(event.pointerId);
    held = { pointer: event.pointerId, at: Date.now() };
    start();
  }

  function pointerup(event) {
    if (held?.pointer === event.pointerId) letGo();
  }

  /** Taken by the OS (a scroll, a gesture): let go, and no click follows. */
  function pointercancel(event) {
    if (held?.pointer !== event.pointerId) return;
    pressed = false;
    letGo();
  }

  function keydown(event) {
    if (event.key === "Escape" && recording) {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key !== " " && event.key !== "Enter") return;
    // The button's own click would come too; the keys are handled here.
    event.preventDefault();
    if (event.repeat || held) return;
    if (recording) {
      stop();
      return;
    }
    held = { key: event.key, at: Date.now() };
    start();
  }

  function keyup(event) {
    if (event.key === " ") event.preventDefault();
    if (!held || event.key !== held.key) return;
    letGo();
  }

  /** The held key or button is let go. Held: push-to-talk ends. Pressed: it stays on, like a click. */
  function letGo() {
    if (!held) return;
    const long = Date.now() - held.at >= HOLD_MS;
    held = null;
    if (long) stop();
  }

  // The events are the window's, not this button's: another <Microphone>
  // on the screen may be the one recording. Until the shell said yes to
  // this one's start, its level and its recording aren't this one's.
  $effect(() =>
    onMicrophone((event) => {
      if (event.type === "level") {
        if (phase === "recording") level = event.level;
      } else if (event.type === "stopped") {
        // It reached its longest.
        if (phase === "recording") finish(event.file);
      } else if (event.type === "failed") {
        if (recording) fail({ message: event.message });
      }
    }),
  );

  // Disabled while recording (the form is being saved, say, or the ability
  // taken away): the button can't be clicked or take Esc any more, so the
  // recording is thrown away rather than left running with no way to stop it.
  $effect(() => {
    if (off && recording) untrack(cancel);
  });

  // Leaving the page with the microphone on turns it off.
  $effect(() => () => {
    if (recording) cancel();
  });
</script>

<!-- Focus gone with the key down (another window, a click elsewhere): its
     keyup never comes here, so losing focus lets go. -->
<svelte:window onblur={letGo} />

{#if allowed || canMode === "disable"}
<span class="elyra-microphone {className}" class:recording class:busy class:invalid={control?.invalid}>
  <button
    type="button"
    class="mic"
    id={control?.id}
    aria-label={words.button}
    aria-labelledby={control ? `${control.labelId} ${control.id}` : undefined}
    aria-pressed={recording}
    aria-busy={busy || undefined}
    aria-describedby={[status ? `${own}-status` : null, control?.describedBy].filter(Boolean).join(" ") || undefined}
    title={words.hint}
    disabled={off || busy}
    onclick={click}
    onpointerdown={pointerdown}
    onpointerup={pointerup}
    onpointercancel={pointercancel}
    onkeydown={keydown}
    onkeyup={keyup}
    onblur={letGo}
    {...rest}
  >
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" fill="none" />
    </svg>
  </button>
  {#if recording}
    <span class="meter" aria-hidden="true"><span style="transform: scaleX({Math.max(level, 0.04)})"></span></span>
  {/if}
  <span id="{own}-status" class="status" class:failure role={failure ? "alert" : "status"} aria-live="polite">{status}</span>
</span>
{/if}

<style>
  .elyra-microphone { display: inline-flex; align-items: center; gap: 8px; font: 12px var(--elyra-font); }
  .mic {
    display: inline-flex; align-items: center; justify-content: center;
    width: 32px; height: 32px; padding: 0; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: 50%;
    background: var(--elyra-inset); color: var(--elyra-text); cursor: pointer;
    transition: background-color var(--elyra-duration-fast), border-color var(--elyra-duration-fast), color var(--elyra-duration-fast);
  }
  .mic svg { fill: currentColor; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .mic:hover:not(:disabled) { border-color: var(--elyra-accent); }
  .mic:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .mic:disabled { opacity: .5; cursor: default; }
  .invalid .mic { border-color: var(--elyra-danger); }
  .recording .mic { background: var(--elyra-danger); border-color: var(--elyra-danger); color: var(--elyra-on-accent); }
  .meter { display: inline-block; width: 48px; height: 4px; border-radius: 2px; background: var(--elyra-inset); overflow: hidden; }
  .meter span {
    display: block; height: 100%; background: var(--elyra-danger); transform-origin: left;
    transition: transform var(--elyra-duration-fast) var(--elyra-ease);
  }
  .status { color: var(--elyra-muted); }
  .status:empty { display: none; }
  .status.failure { color: var(--elyra-danger); }
  /* Less motion: the meter stands still, full, while it records. */
  @media (prefers-reduced-motion: reduce) {
    .meter span { transform: none !important; transition: none; }
  }
</style>
