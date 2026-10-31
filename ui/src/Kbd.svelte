<!--
  A key, or a combination: `<Kbd keys="CmdOrCtrl+K" />` shows ⌘K on macOS
  and Ctrl+K elsewhere. `keys` is written as Elyra's shortcuts and menus
  write them (`CmdOrCtrl+Shift+P`), or as a list (`["Shift", "Enter"]`).
  A screen reader hears the keys' names.
-->
<script module>
  /** Whether this is a Mac, from what the WebView says. */
  export function onMac() {
    const nav = globalThis.navigator;
    const name = nav?.userAgentData?.platform ?? nav?.platform ?? "";
    return /mac/i.test(name);
  }

  /** "CmdOrCtrl+Shift+P" → ["CmdOrCtrl", "Shift", "P"]; a "+" at the end is the plus key. */
  export function split(text) {
    return text.split(/\+(?!$)/).filter(Boolean);
  }

  const ARIA = {
    cmdorctrl: (mac) => (mac ? "Meta" : "Control"),
    commandorcontrol: (mac) => (mac ? "Meta" : "Control"),
    cmd: () => "Meta", command: () => "Meta", meta: () => "Meta", super: () => "Meta",
    ctrl: () => "Control", control: () => "Control",
    alt: () => "Alt", option: () => "Alt",
    shift: () => "Shift",
    enter: () => "Enter", return: () => "Enter",
    esc: () => "Escape", escape: () => "Escape",
    up: () => "ArrowUp", down: () => "ArrowDown", left: () => "ArrowLeft", right: () => "ArrowRight",
    space: () => "Space",
  };

  /** The keys as `aria-keyshortcuts` writes them: `Meta+Shift+P`. */
  export function ariaShortcut(keys, mac = onMac()) {
    const list = Array.isArray(keys) ? keys : split(String(keys));
    return list.map((k) => ARIA[k.toLowerCase()]?.(mac) ?? (k.length === 1 ? k.toUpperCase() : k)).join("+");
  }
</script>

<script>
  let { keys = "", platform = undefined, class: className = "", ...rest } = $props();

  const mac = $derived(platform ? platform === "mac" : onMac());

  // name: [macOS, elsewhere, spoken]
  const NAMES = {
    cmdorctrl: ["⌘", "Ctrl", (m) => (m ? "Command" : "Control")],
    commandorcontrol: ["⌘", "Ctrl", (m) => (m ? "Command" : "Control")],
    cmd: ["⌘", "Win", (m) => (m ? "Command" : "Windows")],
    command: ["⌘", "Win", (m) => (m ? "Command" : "Windows")],
    meta: ["⌘", "Win", (m) => (m ? "Command" : "Windows")],
    super: ["⌘", "Win", (m) => (m ? "Command" : "Windows")],
    ctrl: ["⌃", "Ctrl", () => "Control"],
    control: ["⌃", "Ctrl", () => "Control"],
    alt: ["⌥", "Alt", (m) => (m ? "Option" : "Alt")],
    option: ["⌥", "Alt", (m) => (m ? "Option" : "Alt")],
    shift: ["⇧", "Shift", () => "Shift"],
    enter: ["↩", "Enter", () => "Enter"],
    return: ["↩", "Enter", () => "Enter"],
    escape: ["Esc", "Esc", () => "Escape"],
    esc: ["Esc", "Esc", () => "Escape"],
    backspace: ["⌫", "Backspace", () => "Backspace"],
    delete: ["⌦", "Del", () => "Delete"],
    tab: ["⇥", "Tab", () => "Tab"],
    space: ["Space", "Space", () => "Space"],
    up: ["↑", "↑", () => "Up"],
    down: ["↓", "↓", () => "Down"],
    left: ["←", "←", () => "Left"],
    right: ["→", "→", () => "Right"],
    arrowup: ["↑", "↑", () => "Up"],
    arrowdown: ["↓", "↓", () => "Down"],
    arrowleft: ["←", "←", () => "Left"],
    arrowright: ["→", "→", () => "Right"],
    pageup: ["PgUp", "PgUp", () => "Page Up"],
    pagedown: ["PgDn", "PgDn", () => "Page Down"],
    home: ["Home", "Home", () => "Home"],
    end: ["End", "End", () => "End"],
  };

  const list = $derived(Array.isArray(keys) ? keys : split(String(keys)));
  const shown = $derived(
    list.map((k) => {
      const known = NAMES[k.toLowerCase()];
      if (known) return { text: mac ? known[0] : known[1], spoken: known[2](mac) };
      const text = k.length === 1 ? k.toUpperCase() : k;
      return { text, spoken: text };
    }),
  );
  const spoken = $derived(shown.map((k) => k.spoken).join(" "));
</script>

<!-- A <kbd> has no role to name, so the spoken keys are text, and the drawn ones are hidden. -->
<kbd class="elyra-kbd {className}" class:mac {...rest}>
  <span class="spoken">{spoken}</span>
  <span class="drawn" aria-hidden="true">{#each shown as key, i}{#if i > 0 && !mac}<span class="plus">+</span>{/if}<kbd>{key.text}</kbd>{/each}</span>
</kbd>

<style>
  .spoken { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .drawn { display: inline-flex; align-items: center; gap: 2px; }
  .elyra-kbd {
    display: inline-flex; align-items: center; gap: 2px; vertical-align: baseline;
    font: 500 11px/1 var(--elyra-font); color: var(--elyra-muted); white-space: nowrap;
  }
  .elyra-kbd kbd {
    display: inline-grid; place-items: center; min-width: 18px; height: 18px; padding: 0 4px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-bottom-width: 2px; border-radius: 4px;
    background: var(--elyra-surface); color: var(--elyra-text); font: inherit;
  }
  .mac kbd { min-width: 18px; padding: 0 3px; }
  .plus { opacity: .7; }
</style>
