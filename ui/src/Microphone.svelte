<!--
  Speak instead of type (RFC 0032): a button that records in the shell and
  hands back what was said. A click starts and stops; Space or Enter held
  past 300 ms records until let go; Esc throws the recording away. With
  `transcribe` (the default) the recording becomes text through the app's
  server, for `ontext`; `onrecorded` gets the recording itself.
-->
<script>
  import { microphone, onMicrophone } from "@elyra/runtime";

  let {
    transcribe = true,
    language = undefined,
    maxSeconds = undefined,
    disabled = false,
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
    hint: "Click, or hold Space, to talk",
  };
  /** Held longer than this, a key is push-to-talk; shorter, a click. */
  const HOLD_MS = 300;

  const words = $derived({ ...WORDS, ...labels });

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
    if (disabled || phase !== "idle") return;
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
    onrecorded?.(file);
    if (!transcribe) {
      phase = "idle";
      return;
    }
    phase = "transcribing";
    try {
      const text = await microphone.transcribe(file, { language });
      phase = "idle";
      if (text.trim()) ontext?.(text.trim());
    } catch (err) {
      fail(err);
    }
  }

  function click() {
    if (recording) stop();
    else start();
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
    const long = Date.now() - held.at >= HOLD_MS;
    held = null;
    // Held: push-to-talk ends. Pressed: it stays on, like a click.
    if (long) stop();
  }

  $effect(() =>
    onMicrophone((event) => {
      if (event.type === "level") {
        if (recording) level = event.level;
      } else if (event.type === "stopped") {
        // It reached its longest.
        if (recording) finish(event.file);
      } else if (event.type === "failed") {
        if (recording) fail({ message: event.message });
      }
    }),
  );

  // Leaving the page with the microphone on turns it off.
  $effect(() => () => {
    if (recording) cancel();
  });
</script>

<span class="elyra-microphone {className}" class:recording class:busy>
  <button
    type="button"
    class="mic"
    aria-label={words.button}
    aria-pressed={recording}
    aria-busy={busy || undefined}
    title={words.hint}
    disabled={disabled || busy}
    onclick={click}
    onkeydown={keydown}
    onkeyup={keyup}
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
  <span class="status" class:failure role={failure ? "alert" : "status"} aria-live="polite">{status}</span>
</span>

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
