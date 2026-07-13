/**
 * @elyra/runtime — the frontend half of the Elyra bridge.
 *
 * Wire format (see docs/wire-format.md):
 *   - command request body: a *compact* MessagePack array of the arguments,
 *   - command response: MessagePack, structs encoded as named maps -> objects,
 *   - event stream: a long-poll of `/__events` returning a MessagePack array of
 *     `[channel, value]` pairs — batched per flush, binary, no base64.
 *
 * Everything is same-origin under `elyra://localhost`, so `fetch` needs no CORS.
 */
import { encode, decode } from "@msgpack/msgpack";

const ORIGIN = "elyra://localhost";
const CMD_BASE = `${ORIGIN}/__cmd/`;
const EVENTS_URL = `${ORIGIN}/__events`;

/** Thrown when a command returns an error status. */
export class CommandError extends Error {
  constructor(
    public readonly command: string,
    message: string,
  ) {
    super(`elyra command "${command}" failed: ${message}`);
    this.name = "CommandError";
  }
}

/**
 * Invoke a Rust `#[command]` by name.
 *
 * @example
 * const greeting = await invoke<string>("greet", "World");
 * const sum = await invoke<number>("add", 2, 3);
 */
export async function invoke<T = unknown>(
  command: string,
  ...args: unknown[]
): Promise<T> {
  const res = await fetch(CMD_BASE + command, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(args), // compact array of arguments
  });

  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new CommandError(command, await res.text());
  }

  const buf = new Uint8Array(await res.arrayBuffer());
  return decode(buf) as T;
}

// --- Event bus (Rust -> frontend), one multiplexed long-poll connection -----

type Handler = (value: unknown) => void;

const subscribers = new Map<string, Set<Handler>>();
const lastValue = new Map<string, unknown>();
let pumping = false;

function dispatch(name: string, value: unknown) {
  lastValue.set(name, value);
  const set = subscribers.get(name);
  if (set) for (const handler of set) handler(value);
}

async function pump() {
  pumping = true;
  let backoff = 0;
  while (subscribers.size > 0) {
    try {
      const res = await fetch(EVENTS_URL, { headers: { accept: "application/msgpack" } });
      if (!res.ok) throw new Error(`events ${res.status}`);
      const batch = decode(new Uint8Array(await res.arrayBuffer())) as [string, unknown][];
      for (const [name, value] of batch) dispatch(name, value);
      backoff = 0;
    } catch {
      // Window closing or transient failure — back off, then retry.
      backoff = Math.min(backoff ? backoff * 2 : 100, 2000);
      await new Promise((r) => setTimeout(r, backoff));
    }
  }
  pumping = false;
}

/**
 * A server-pushed event channel. The return value is a Svelte-readable store,
 * so `$channel("cursor")` works directly in a component; it is also usable
 * standalone via `.subscribe(handler)`.
 *
 * @example
 * // In a .svelte component:
 * const ticks = channel<number>("tick");
 * // ...then use `$ticks` in markup.
 *
 * @example
 * const unsubscribe = channel<number>("tick").subscribe((n) => console.log(n));
 */
export function channel<T = unknown>(name: string): {
  subscribe: (handler: (value: T | undefined) => void) => () => void;
} {
  return {
    subscribe(handler: (value: T | undefined) => void) {
      // Svelte store contract: emit the current value immediately.
      handler(lastValue.get(name) as T | undefined);

      let set = subscribers.get(name);
      if (!set) subscribers.set(name, (set = new Set()));
      set.add(handler as Handler);

      if (!pumping) void pump();

      return () => {
        const s = subscribers.get(name);
        if (!s) return;
        s.delete(handler as Handler);
        if (s.size === 0) subscribers.delete(name);
      };
    },
  };
}

// --- About dialog (framework built-in) --------------------------------------
//
// Metadata is set on the Rust `App::about(...)` builder and served at
// `/__about`. On macOS the standard "About <App>" menu item emits an
// `elyra:about` event that this module listens for; call `openAbout()` directly
// to open it from a button on any platform.

/** App metadata behind the About dialog (mirrors Rust `AboutInfo`). */
export interface AboutInfo {
  name: string;
  version: string;
  description?: string;
  website?: string | null;
  repository?: string | null;
  author?: string | null;
  author_url?: string | null;
  icon?: string | null;
}

const ABOUT_URL = `${ORIGIN}/__about`;

async function fetchAbout(): Promise<AboutInfo> {
  const res = await fetch(ABOUT_URL, { headers: { accept: "application/msgpack" } });
  if (!res.ok) throw new Error(`about ${res.status}`);
  return decode(new Uint8Array(await res.arrayBuffer())) as AboutInfo;
}

const DEFAULT_ICON = `<svg class="elyra-about-icon" viewBox="0 0 72 72" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
  <defs><linearGradient id="elyra-mark" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#fdba74"/><stop offset="1" stop-color="#f97316"/>
  </linearGradient></defs>
  <rect width="72" height="72" rx="18" fill="#1a1b26"/>
  <g stroke="url(#elyra-mark)" stroke-width="3.5" fill="url(#elyra-mark)">
    <line x1="36" y1="22" x2="22" y2="48"/><line x1="36" y1="22" x2="50" y2="48"/><line x1="22" y1="48" x2="50" y2="48"/>
    <circle cx="36" cy="22" r="6.5"/><circle cx="22" cy="48" r="6.5"/><circle cx="50" cy="48" r="6.5"/>
  </g>
</svg>`;

let aboutStyleInjected = false;
function injectAboutStyle(): void {
  if (aboutStyleInjected) return;
  aboutStyleInjected = true;
  const style = document.createElement("style");
  style.textContent = `
.elyra-about-overlay{position:fixed;inset:0;z-index:2147483000;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,.55);backdrop-filter:blur(2px);font:14px/1.5 system-ui,-apple-system,sans-serif}
.elyra-about-card{width:360px;max-width:calc(100vw - 40px);box-sizing:border-box;padding:28px 24px 20px;border-radius:18px;text-align:center;background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.08));box-shadow:0 24px 60px rgba(0,0,0,.5)}
.elyra-about-icon{width:72px;height:72px;border-radius:16px;display:block;margin:0 auto 14px}
.elyra-about-name{font-size:20px;font-weight:700;letter-spacing:-.01em}
.elyra-about-version{margin-top:2px;color:var(--muted,#787c99);font-size:13px}
.elyra-about-desc{margin:14px 4px 18px;color:var(--muted,#9aa0bd);font-size:13px}
.elyra-about-rows{display:flex;flex-direction:column;gap:8px;text-align:center}
.elyra-about-row{padding:10px 12px;border-radius:10px;background:var(--bg,#16161e);border:1px solid var(--border,rgba(255,255,255,.06))}
.elyra-about-label{font-size:10px;font-weight:600;letter-spacing:.09em;text-transform:uppercase;color:var(--muted,#787c99)}
.elyra-about-author{margin-top:3px;color:var(--text,#c0caf5);font-size:13px}
.elyra-about-link{margin-top:2px;background:none;border:0;padding:0;cursor:pointer;font:inherit;color:var(--accent,#7aa2f7)}
.elyra-about-link:hover{text-decoration:underline}
.elyra-about-close{margin-top:20px;padding:8px 22px;border-radius:9px;border:1px solid var(--border,rgba(255,255,255,.1));background:var(--bg,#16161e);color:var(--text,#c0caf5);font:inherit;cursor:pointer}
.elyra-about-close:hover{border-color:var(--accent,#7aa2f7)}
`;
  document.head.appendChild(style);
}

let currentAbout: HTMLElement | null = null;

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string,
  );
}

function linkButton(value: string): string {
  return `<button class="elyra-about-link" type="button" data-copy="${escapeHtml(value)}">${escapeHtml(value)}</button>`;
}

function row(label: string, valueHtml: string): string {
  return `<div class="elyra-about-row"><div class="elyra-about-label">${escapeHtml(label)}</div>${valueHtml}</div>`;
}

function onAboutKeydown(e: KeyboardEvent): void {
  if (e.key === "Escape") closeAbout();
}

/** Close the About dialog if open. */
export function closeAbout(): void {
  if (!currentAbout) return;
  document.removeEventListener("keydown", onAboutKeydown);
  currentAbout.remove();
  currentAbout = null;
}

/**
 * Open the framework's built-in About dialog. Without an argument it fetches
 * the app's metadata from the Rust side (`App::about(...)`).
 */
export async function openAbout(info?: AboutInfo): Promise<void> {
  if (typeof document === "undefined") return;
  const data = info ?? (await fetchAbout());
  closeAbout();
  injectAboutStyle();

  const rows: string[] = [];
  if (data.website) rows.push(row("Website", linkButton(data.website)));
  if (data.repository) rows.push(row("GitHub", linkButton(data.repository)));
  if (data.author) {
    const url = data.author_url ? ` · ${linkButton(data.author_url)}` : "";
    rows.push(row("Developed by", `<div class="elyra-about-author">${escapeHtml(data.author)}${url}</div>`));
  }

  const overlay = document.createElement("div");
  overlay.className = "elyra-about-overlay";
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) closeAbout();
  });

  const card = document.createElement("div");
  card.className = "elyra-about-card";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-modal", "true");
  card.innerHTML = `
    ${data.icon ? `<img class="elyra-about-icon" src="${escapeHtml(data.icon)}" alt="" />` : DEFAULT_ICON}
    <div class="elyra-about-name">${escapeHtml(data.name)}</div>
    <div class="elyra-about-version">Version ${escapeHtml(data.version)}</div>
    ${data.description ? `<p class="elyra-about-desc">${escapeHtml(data.description)}</p>` : ""}
    ${rows.length ? `<div class="elyra-about-rows">${rows.join("")}</div>` : ""}
    <button class="elyra-about-close" type="button">Close</button>
  `;

  card.querySelector(".elyra-about-close")?.addEventListener("click", closeAbout);
  card.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((el) => {
    el.addEventListener("click", async () => {
      const value = el.getAttribute("data-copy") ?? "";
      try {
        await navigator.clipboard.writeText(value);
        const original = el.textContent;
        el.textContent = "Copied";
        setTimeout(() => {
          el.textContent = original;
        }, 1200);
      } catch {
        /* clipboard unavailable — ignore */
      }
    });
  });

  overlay.appendChild(card);
  document.body.appendChild(overlay);
  currentAbout = overlay;
  document.addEventListener("keydown", onAboutKeydown);
}

// Auto-wire the macOS "About <App>" menu item: open the dialog when the shell
// emits `elyra:about`. Importing @elyra/runtime is enough to make it work.
if (typeof document !== "undefined") {
  channel<AboutInfo>("elyra:about").subscribe((info) => {
    if (info) void openAbout(info);
  });
}

// --- Update toast (framework built-in) --------------------------------------
//
// Enabled by `App::updater(...)` (the `updater` feature). The shell checks the
// manifest silently on startup and streams progress on the `elyra:update`
// channel; this module renders a toast (available -> install -> download ->
// restart). Call `checkForUpdate()` from a menu/button for a manual check.

/** Result of an update check (mirrors Rust `UpdateCheck`). */
export interface UpdateCheck {
  available: boolean;
  version?: string;
  notes?: string;
  error?: string;
}

interface UpdatePhaseEvent {
  phase: "available" | "downloading" | "ready" | "error" | "up-to-date";
  version?: string;
  notes?: string;
  progress?: number;
  message?: string;
}

interface UpdateState {
  phase: "available" | "downloading" | "ready" | "error";
  version?: string;
  notes?: string;
  progress?: number;
  message?: string;
  notesOpen?: boolean;
}

/** Ask the Rust side whether a newer release exists. Shows the toast if so. */
export async function checkForUpdate(): Promise<UpdateCheck> {
  const res = await fetch(`${ORIGIN}/__update/check`, {
    headers: { accept: "application/msgpack" },
  });
  if (!res.ok) throw new Error(`update check ${res.status}`);
  const data = decode(new Uint8Array(await res.arrayBuffer())) as UpdateCheck;
  if (data.available) showUpdate({ phase: "available", version: data.version, notes: data.notes });
  return data;
}

/** Start downloading + installing the update (progress arrives via events). */
export async function installUpdate(): Promise<void> {
  showUpdate({ phase: "downloading", progress: 0 });
  await fetch(`${ORIGIN}/__update/install`, { method: "POST" });
}

let updateStyleInjected = false;
function injectUpdateStyle(): void {
  if (updateStyleInjected) return;
  updateStyleInjected = true;
  const style = document.createElement("style");
  style.textContent = `
.elyra-update-toast{position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:2147482000;box-sizing:border-box;max-width:min(560px,calc(100vw - 32px));padding:12px 16px;border-radius:12px;font:13px/1.5 system-ui,-apple-system,sans-serif;background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.1));box-shadow:0 16px 40px rgba(0,0,0,.5)}
.elyra-update-toast .eu-row{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.elyra-update-toast .eu-spacer{flex:1}
.elyra-update-toast .eu-err{color:#f7768e}
.elyra-update-toast .eu-btn{padding:6px 12px;border-radius:8px;border:1px solid var(--border,rgba(255,255,255,.12));background:var(--bg,#16161e);color:var(--text,#c0caf5);font:inherit;cursor:pointer}
.elyra-update-toast .eu-btn:hover{border-color:var(--accent,#7aa2f7)}
.elyra-update-toast .eu-primary{background:var(--accent,#7aa2f7);border-color:var(--accent,#7aa2f7);color:#0b0b12;font-weight:600}
.elyra-update-toast .eu-link{background:none;border:0;padding:0;color:var(--accent,#7aa2f7);cursor:pointer;font:inherit}
.elyra-update-toast .eu-link:hover{text-decoration:underline}
.elyra-update-toast .eu-bar{margin-top:8px;height:6px;border-radius:3px;background:var(--bg,#16161e);overflow:hidden}
.elyra-update-toast .eu-bar-fill{height:100%;background:var(--accent,#7aa2f7);transition:width .2s}
.elyra-update-toast .eu-notes{margin-top:10px;max-height:220px;overflow:auto;background:var(--bg,#16161e);border:1px solid var(--border,rgba(255,255,255,.08));border-radius:8px;padding:8px 12px;font-size:12px;white-space:pre-wrap}
`;
  document.head.appendChild(style);
}

let updateState: UpdateState | null = null;
let updateEl: HTMLElement | null = null;

/** Dismiss the update toast. */
export function dismissUpdate(): void {
  updateState = null;
  if (updateEl) {
    updateEl.remove();
    updateEl = null;
  }
}

function showUpdate(next: Partial<UpdateState> & { phase: UpdateState["phase"] }): void {
  if (typeof document === "undefined") return;
  updateState = { ...(updateState ?? {}), ...next };
  renderUpdate();
}

function renderUpdate(): void {
  if (!updateState) return;
  injectUpdateStyle();
  if (!updateEl) {
    updateEl = document.createElement("div");
    updateEl.className = "elyra-update-toast";
    updateEl.setAttribute("role", "status");
    document.body.appendChild(updateEl);
  }
  const s = updateState;
  let html: string;
  if (s.phase === "downloading") {
    const pct = s.progress ?? 0;
    html = `<div class="eu-row"><span>\u2193 Downloading update\u2026 ${pct}%</span></div><div class="eu-bar"><div class="eu-bar-fill" style="width:${pct}%"></div></div>`;
  } else if (s.phase === "ready") {
    html = `<div class="eu-row"><span>\u2713 Update ready \u2014 restarting\u2026</span></div>`;
  } else if (s.phase === "error") {
    html = `<div class="eu-row"><span class="eu-err">Update failed: ${escapeHtml(s.message ?? "")}</span><button class="eu-btn" data-act="dismiss">Dismiss</button></div>`;
  } else {
    html =
      `<div class="eu-row"><span>\u2191 Update available: <strong>v${escapeHtml(s.version ?? "")}</strong></span>` +
      (s.notes ? `<button class="eu-link" data-act="notes">${s.notesOpen ? "Hide notes" : "What's new"}</button>` : "") +
      `<span class="eu-spacer"></span><button class="eu-btn eu-primary" data-act="install">Install &amp; restart</button><button class="eu-btn" data-act="later">Later</button></div>` +
      (s.notesOpen && s.notes ? `<div class="eu-notes">${escapeHtml(s.notes)}</div>` : "");
  }
  updateEl.innerHTML = html;
  updateEl.querySelectorAll<HTMLElement>("[data-act]").forEach((el) => {
    el.addEventListener("click", () => {
      switch (el.getAttribute("data-act")) {
        case "install":
          void installUpdate();
          break;
        case "later":
        case "dismiss":
          dismissUpdate();
          break;
        case "notes":
          if (updateState) {
            updateState.notesOpen = !updateState.notesOpen;
            renderUpdate();
          }
          break;
      }
    });
  });
}

if (typeof document !== "undefined") {
  channel<UpdatePhaseEvent>("elyra:update").subscribe((ev) => {
    if (!ev) return;
    switch (ev.phase) {
      case "available":
        showUpdate({ phase: "available", version: ev.version, notes: ev.notes, notesOpen: false });
        break;
      case "downloading":
        showUpdate({ phase: "downloading", progress: ev.progress ?? 0 });
        break;
      case "ready":
        showUpdate({ phase: "ready" });
        break;
      case "error":
        showUpdate({ phase: "error", message: ev.message });
        break;
      // "up-to-date": only surfaced through checkForUpdate()'s return value.
    }
  });
}
