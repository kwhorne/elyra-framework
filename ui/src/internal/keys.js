// Keys as Elyra writes them (`CmdOrCtrl+Shift+P`): for Kbd and MenuItem's shortcut.

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
