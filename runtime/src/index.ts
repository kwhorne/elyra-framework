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

// --- Native system integration (the `system` feature) -----------------------
//
// Thin wrappers over the shell's `/__sys/*` endpoints. Available when the app is
// built with elyra's `system` feature (dialogs, shell-open, clipboard,
// notifications, paths).

async function sys<T>(op: string, arg?: unknown): Promise<T> {
  const res = await fetch(`${ORIGIN}/__sys/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra system "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/** A name + extensions filter for the file dialogs. */
export interface DialogFilter {
  name: string;
  extensions: string[];
}

export interface OpenDialogOptions {
  title?: string;
  /** Pick directories instead of files. */
  directory?: boolean;
  /** Allow selecting more than one entry. */
  multiple?: boolean;
  filters?: DialogFilter[];
  /** Directory to open the dialog at. */
  startDir?: string;
}

export interface SaveDialogOptions {
  title?: string;
  defaultName?: string;
  filters?: DialogFilter[];
  startDir?: string;
}

/** Native open/save file dialogs. */
export const dialog = {
  /** Show an open dialog; resolves to the selected paths (empty if cancelled). */
  open(options: OpenDialogOptions = {}): Promise<string[]> {
    return sys<string[]>("dialog.open", {
      title: options.title ?? null,
      directory: options.directory ?? false,
      multiple: options.multiple ?? false,
      filters: options.filters ?? [],
      start_dir: options.startDir ?? null,
    });
  },
  /** Show a save dialog; resolves to the chosen path, or `null` if cancelled. */
  save(options: SaveDialogOptions = {}): Promise<string | null> {
    return sys<string | null>("dialog.save", {
      title: options.title ?? null,
      default_name: options.defaultName ?? null,
      filters: options.filters ?? [],
      start_dir: options.startDir ?? null,
    });
  },
};

/** Open URLs / files with the OS default handler. */
export const shell = {
  openExternal(target: string): Promise<void> {
    return sys<void>("shell.open", target);
  },
};

/** Read/write the system clipboard (text). */
export const clipboard = {
  readText(): Promise<string> {
    return sys<string>("clipboard.read");
  },
  writeText(text: string): Promise<void> {
    return sys<void>("clipboard.write", text);
  },
};

/** Show an OS notification. */
export function notify(title: string, body?: string): Promise<void> {
  return sys<void>("notify", { title, body: body ?? null });
}

/** Standard OS directories + the running executable (strings or null). */
export interface Paths {
  home: string | null;
  config: string | null;
  data: string | null;
  cache: string | null;
  temp: string | null;
  exe: string | null;
}

/** Resolve standard OS paths for this app. */
export function paths(): Promise<Paths> {
  return sys<Paths>("paths");
}

// --- UI components (framework built-ins) ------------------------------------
//
// Themed, dependency-free replacements for the things every desktop app needs:
// confirm/alert/prompt dialogs, toasts, a ⌘K command palette, and context
// menus. All read the app's CSS variables (--surface/--bg/--text/--muted/
// --accent/--border) with dark fallbacks, matching the About/Update components.

let uiStyleInjected = false;
function injectUiStyle(): void {
  if (uiStyleInjected || typeof document === "undefined") return;
  uiStyleInjected = true;
  const style = document.createElement("style");
  style.textContent = `
.elyra-modal-overlay{position:fixed;inset:0;z-index:2147483200;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,.5);backdrop-filter:blur(2px);font:14px/1.5 system-ui,-apple-system,sans-serif}
.elyra-modal-card{width:400px;max-width:calc(100vw - 40px);box-sizing:border-box;padding:22px 22px 18px;border-radius:16px;background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.08));box-shadow:0 24px 60px rgba(0,0,0,.5)}
.elyra-modal-title{font-size:15px;font-weight:700;margin-bottom:8px}
.elyra-modal-body{color:var(--text,#c0caf5);font-size:14px;white-space:pre-wrap}
.elyra-modal-input{width:100%;box-sizing:border-box;margin-top:14px;padding:8px 10px;border-radius:8px;border:1px solid var(--border,rgba(255,255,255,.14));background:var(--bg,#16161e);color:var(--text,#c0caf5);font:inherit}
.elyra-modal-input:focus{outline:none;border-color:var(--accent,#7aa2f7)}
.elyra-modal-actions{display:flex;justify-content:flex-end;gap:8px;margin-top:18px}
.elyra-modal-btn{padding:7px 16px;border-radius:8px;border:1px solid var(--border,rgba(255,255,255,.12));background:var(--bg,#16161e);color:var(--text,#c0caf5);font:inherit;cursor:pointer}
.elyra-modal-btn:hover{border-color:var(--accent,#7aa2f7)}
.elyra-modal-btn.primary{background:var(--accent,#7aa2f7);border-color:var(--accent,#7aa2f7);color:#0b0b12;font-weight:600}
.elyra-modal-btn.danger{background:#f7768e;border-color:#f7768e;color:#0b0b12;font-weight:600}
.elyra-toast-stack{position:fixed;right:20px;bottom:20px;z-index:2147483100;display:flex;flex-direction:column;gap:10px;font:13px/1.4 system-ui,-apple-system,sans-serif}
.elyra-toast{min-width:220px;max-width:360px;padding:11px 14px;border-radius:10px;background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.1));border-left:3px solid var(--accent,#7aa2f7);box-shadow:0 12px 30px rgba(0,0,0,.45);cursor:pointer;animation:elyra-toast-in .16s ease-out}
.elyra-toast.success{border-left-color:#9ece6a}
.elyra-toast.error{border-left-color:#f7768e}
@keyframes elyra-toast-in{from{opacity:0;transform:translateY(6px)}to{opacity:1;transform:none}}
.elyra-cmdk-overlay{position:fixed;inset:0;z-index:2147483300;display:flex;align-items:flex-start;justify-content:center;padding-top:14vh;background:rgba(0,0,0,.5);backdrop-filter:blur(2px);font:14px/1.5 system-ui,-apple-system,sans-serif}
.elyra-cmdk{width:560px;max-width:calc(100vw - 40px);background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.1));border-radius:14px;box-shadow:0 30px 70px rgba(0,0,0,.55);overflow:hidden}
.elyra-cmdk input{width:100%;box-sizing:border-box;padding:14px 16px;border:0;border-bottom:1px solid var(--border,rgba(255,255,255,.08));background:transparent;color:var(--text,#c0caf5);font:15px system-ui;outline:none}
.elyra-cmdk-list{max-height:340px;overflow:auto;padding:6px}
.elyra-cmdk-item{padding:9px 12px;border-radius:8px;cursor:pointer;display:flex;flex-direction:column;gap:1px}
.elyra-cmdk-item.active{background:var(--bg,#16161e)}
.elyra-cmdk-item .sub{font-size:12px;color:var(--muted,#787c99)}
.elyra-cmdk-empty{padding:16px;color:var(--muted,#787c99);text-align:center}
.elyra-ctx{position:fixed;z-index:2147483400;min-width:180px;padding:6px;border-radius:10px;background:var(--surface,var(--panel,#1e2030));color:var(--text,#c0caf5);border:1px solid var(--border,rgba(255,255,255,.1));box-shadow:0 16px 40px rgba(0,0,0,.5);font:13px/1.4 system-ui,-apple-system,sans-serif}
.elyra-ctx-item{padding:7px 10px;border-radius:6px;cursor:pointer}
.elyra-ctx-item:hover{background:var(--bg,#16161e)}
.elyra-ctx-item.disabled{opacity:.45;pointer-events:none}
.elyra-ctx-sep{height:1px;margin:4px 6px;background:var(--border,rgba(255,255,255,.1))}
`;
  document.head.appendChild(style);
}

interface ModalButton {
  label: string;
  value: () => unknown;
  primary?: boolean;
  danger?: boolean;
}

function buildModal(opts: {
  title?: string;
  message: string;
  input?: { value: string; placeholder?: string };
  buttons: ModalButton[];
  cancel: () => unknown;
}): Promise<unknown> {
  return new Promise((resolve) => {
    injectUiStyle();
    const overlay = document.createElement("div");
    overlay.className = "elyra-modal-overlay";
    const card = document.createElement("div");
    card.className = "elyra-modal-card";
    card.setAttribute("role", "dialog");
    card.setAttribute("aria-modal", "true");
    card.innerHTML =
      (opts.title ? `<div class="elyra-modal-title">${escapeHtml(opts.title)}</div>` : "") +
      `<div class="elyra-modal-body">${escapeHtml(opts.message)}</div>` +
      (opts.input ? `<input class="elyra-modal-input" type="text" />` : "") +
      `<div class="elyra-modal-actions"></div>`;

    const input = opts.input
      ? card.querySelector<HTMLInputElement>(".elyra-modal-input")
      : null;
    if (input && opts.input) {
      input.value = opts.input.value;
      if (opts.input.placeholder) input.placeholder = opts.input.placeholder;
    }

    let settled = false;
    const done = (value: unknown): void => {
      if (settled) return;
      settled = true;
      document.removeEventListener("keydown", onKey);
      overlay.remove();
      resolve(value);
    };

    const actions = card.querySelector(".elyra-modal-actions") as HTMLElement;
    for (const b of opts.buttons) {
      const btn = document.createElement("button");
      btn.className =
        "elyra-modal-btn" + (b.primary ? " primary" : "") + (b.danger ? " danger" : "");
      btn.textContent = b.label;
      btn.addEventListener("click", () => done(b.value()));
      actions.appendChild(btn);
    }

    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") done(opts.cancel());
      else if (e.key === "Enter") {
        const primary = opts.buttons.find((b) => b.primary);
        if (primary) done(primary.value());
      }
    };
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) done(opts.cancel());
    });
    document.addEventListener("keydown", onKey);

    overlay.appendChild(card);
    document.body.appendChild(overlay);
    (input ?? actions.querySelector<HTMLButtonElement>(".primary") ?? actions.lastElementChild as HTMLElement | null)?.focus();
  });
}

export interface ConfirmOptions {
  title?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

/** Themed confirmation dialog. Resolves `true` if confirmed. */
export function confirm(message: string, options: ConfirmOptions = {}): Promise<boolean> {
  return buildModal({
    title: options.title,
    message,
    cancel: () => false,
    buttons: [
      { label: options.cancelLabel ?? "Cancel", value: () => false },
      {
        label: options.confirmLabel ?? "OK",
        value: () => true,
        primary: true,
        danger: options.danger,
      },
    ],
  }) as Promise<boolean>;
}

export interface AlertOptions {
  title?: string;
  label?: string;
}

/** Themed alert dialog. */
export function alert(message: string, options: AlertOptions = {}): Promise<void> {
  return buildModal({
    title: options.title,
    message,
    cancel: () => undefined,
    buttons: [{ label: options.label ?? "OK", value: () => undefined, primary: true }],
  }) as Promise<void>;
}

export interface PromptOptions {
  title?: string;
  defaultValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  cancelLabel?: string;
}

/** Themed prompt dialog. Resolves the entered string, or `null` if cancelled. */
export function prompt(message: string, options: PromptOptions = {}): Promise<string | null> {
  let inputEl: HTMLInputElement | null = null;
  const p = buildModal({
    title: options.title,
    message,
    input: { value: options.defaultValue ?? "", placeholder: options.placeholder },
    cancel: () => null,
    buttons: [
      { label: options.cancelLabel ?? "Cancel", value: () => null },
      {
        label: options.confirmLabel ?? "OK",
        value: () => inputEl?.value ?? "",
        primary: true,
      },
    ],
  });
  // Grab the input that buildModal created so the OK button can read it.
  inputEl = document.querySelector<HTMLInputElement>(".elyra-modal-overlay .elyra-modal-input");
  return p as Promise<string | null>;
}

// --- Toasts -----------------------------------------------------------------

export type ToastVariant = "info" | "success" | "error";
export interface ToastOptions {
  variant?: ToastVariant;
  /** Auto-dismiss after N ms; `0` keeps it until clicked. Default 3500. */
  duration?: number;
}

let toastStack: HTMLElement | null = null;

/** Show an in-app toast. Returns a handle to dismiss it early. */
export function toast(message: string, options: ToastOptions = {}): { dismiss: () => void } {
  if (typeof document === "undefined") return { dismiss: () => {} };
  injectUiStyle();
  if (!toastStack) {
    toastStack = document.createElement("div");
    toastStack.className = "elyra-toast-stack";
    document.body.appendChild(toastStack);
  }
  const el = document.createElement("div");
  el.className = "elyra-toast" + (options.variant ? ` ${options.variant}` : "");
  el.textContent = message;
  let removed = false;
  const dismiss = (): void => {
    if (removed) return;
    removed = true;
    el.remove();
  };
  el.addEventListener("click", dismiss);
  toastStack.appendChild(el);
  const duration = options.duration ?? 3500;
  if (duration > 0) setTimeout(dismiss, duration);
  return { dismiss };
}

// --- Command palette (⌘K) ---------------------------------------------------

export interface Command {
  id: string;
  title: string;
  subtitle?: string;
  keywords?: string;
  action: () => void | Promise<void>;
}

let registeredCommands: Command[] = [];
let cmdkOverlay: HTMLElement | null = null;

/** Register (replace) the commands shown in the ⌘K palette. */
export function registerCommands(commands: Command[]): void {
  registeredCommands = commands;
}

/** Close the command palette if open. */
export function closeCommandPalette(): void {
  if (cmdkOverlay) {
    cmdkOverlay.remove();
    cmdkOverlay = null;
  }
}

/** Open the ⌘K command palette over the registered commands. */
export function openCommandPalette(commands: Command[] = registeredCommands): void {
  if (typeof document === "undefined" || cmdkOverlay) return;
  injectUiStyle();
  const overlay = document.createElement("div");
  overlay.className = "elyra-cmdk-overlay";
  const box = document.createElement("div");
  box.className = "elyra-cmdk";
  box.innerHTML = `<input type="text" placeholder="Type a command…" /><div class="elyra-cmdk-list"></div>`;
  overlay.appendChild(box);

  const input = box.querySelector("input") as HTMLInputElement;
  const list = box.querySelector(".elyra-cmdk-list") as HTMLElement;
  let filtered: Command[] = commands;
  let active = 0;

  const render = (): void => {
    if (filtered.length === 0) {
      list.innerHTML = `<div class="elyra-cmdk-empty">No matching commands</div>`;
      return;
    }
    list.innerHTML = filtered
      .map(
        (c, i) =>
          `<div class="elyra-cmdk-item${i === active ? " active" : ""}" data-i="${i}">` +
          `<span>${escapeHtml(c.title)}</span>` +
          (c.subtitle ? `<span class="sub">${escapeHtml(c.subtitle)}</span>` : "") +
          `</div>`,
      )
      .join("");
    list.querySelectorAll<HTMLElement>(".elyra-cmdk-item").forEach((item) => {
      item.addEventListener("click", () => run(Number(item.dataset.i)));
      item.addEventListener("mousemove", () => {
        active = Number(item.dataset.i);
        highlight();
      });
    });
  };
  const highlight = (): void => {
    list.querySelectorAll<HTMLElement>(".elyra-cmdk-item").forEach((item, i) => {
      item.classList.toggle("active", i === active);
    });
  };
  const run = (i: number): void => {
    const cmd = filtered[i];
    close();
    if (cmd) void cmd.action();
  };
  const close = (): void => {
    document.removeEventListener("keydown", onKey, true);
    closeCommandPalette();
  };
  const onKey = (e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      active = Math.min(active + 1, filtered.length - 1);
      highlight();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      active = Math.max(active - 1, 0);
      highlight();
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(active);
    }
  };

  input.addEventListener("input", () => {
    const q = input.value.trim().toLowerCase();
    filtered = q
      ? commands.filter((c) =>
          `${c.title} ${c.subtitle ?? ""} ${c.keywords ?? ""}`.toLowerCase().includes(q),
        )
      : commands;
    active = 0;
    render();
  });
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  document.addEventListener("keydown", onKey, true);

  document.body.appendChild(overlay);
  cmdkOverlay = overlay;
  render();
  input.focus();
}

// Auto-wire ⌘K / Ctrl+K when commands are registered.
if (typeof document !== "undefined") {
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k" && registeredCommands.length > 0) {
      e.preventDefault();
      if (cmdkOverlay) closeCommandPalette();
      else openCommandPalette();
    }
  });
}

// --- Context menu -----------------------------------------------------------

export interface MenuItem {
  label?: string;
  action?: () => void | Promise<void>;
  separator?: boolean;
  disabled?: boolean;
}

let ctxMenu: HTMLElement | null = null;
function closeContextMenu(): void {
  if (ctxMenu) {
    ctxMenu.remove();
    ctxMenu = null;
  }
}

/** Show a context menu at the pointer. Call from an `oncontextmenu` handler. */
export function contextMenu(event: MouseEvent, items: MenuItem[]): void {
  if (typeof document === "undefined") return;
  event.preventDefault();
  closeContextMenu();
  injectUiStyle();
  const menu = document.createElement("div");
  menu.className = "elyra-ctx";
  for (const item of items) {
    if (item.separator) {
      const sep = document.createElement("div");
      sep.className = "elyra-ctx-sep";
      menu.appendChild(sep);
      continue;
    }
    const el = document.createElement("div");
    el.className = "elyra-ctx-item" + (item.disabled ? " disabled" : "");
    el.textContent = item.label ?? "";
    el.addEventListener("click", () => {
      closeContextMenu();
      if (!item.disabled && item.action) void item.action();
    });
    menu.appendChild(el);
  }
  menu.style.left = `${event.clientX}px`;
  menu.style.top = `${event.clientY}px`;
  document.body.appendChild(menu);
  ctxMenu = menu;

  // Keep the menu on-screen.
  const rect = menu.getBoundingClientRect();
  if (rect.right > window.innerWidth) menu.style.left = `${window.innerWidth - rect.width - 8}px`;
  if (rect.bottom > window.innerHeight) menu.style.top = `${window.innerHeight - rect.height - 8}px`;

  const dismiss = (e: MouseEvent): void => {
    if (ctxMenu && !ctxMenu.contains(e.target as Node)) {
      closeContextMenu();
      document.removeEventListener("mousedown", dismiss, true);
    }
  };
  setTimeout(() => document.addEventListener("mousedown", dismiss, true), 0);
}

// --- Window control + file drop (framework built-ins, always available) -----

async function winCall(op: string, arg?: unknown): Promise<boolean> {
  const res = await fetch(`${ORIGIN}/__window/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra window "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as boolean;
}

/** Live window state, pushed on resize / move / focus. */
export interface WindowState {
  label: string;
  width: number;
  height: number;
  maximized: boolean;
  fullscreen: boolean;
  focused: boolean;
}

/**
 * Control the app window (min/maximize/fullscreen/close/…). Actions target the
 * focused window (or the primary one). Exported as `appWindow` to avoid clashing
 * with the global `window`.
 */
export const appWindow = {
  minimize: () => winCall("minimize"),
  toggleMaximize: () => winCall("toggle_maximize"),
  toggleFullscreen: () => winCall("toggle_fullscreen"),
  close: () => winCall("close"),
  focus: () => winCall("focus"),
  show: () => winCall("show"),
  hide: () => winCall("hide"),
  center: () => winCall("center"),
  setTitle: (title: string) => winCall("set_title", title),
  setSize: (width: number, height: number) => winCall("set_size", [width, height]),
  /** Subscribe to live window state (resize / move / focus). Returns an unsubscribe fn. */
  onState(handler: (state: WindowState) => void): () => void {
    return channel<WindowState>("elyra:window").subscribe((s) => {
      if (s) handler(s);
    });
  },
};

/**
 * Subscribe to native file drops onto the window. The handler receives the
 * dropped absolute paths. Returns an unsubscribe function.
 */
export function onFileDrop(handler: (paths: string[]) => void): () => void {
  return channel<string[]>("elyra:file-drop").subscribe((paths) => {
    if (paths) handler(paths);
  });
}

/**
 * Subscribe to an OS-level global shortcut firing. The handler receives the
 * accelerator string (e.g. `"CmdOrCtrl+Shift+P"`) registered via
 * `App::global_shortcut`. Requires the app's `shortcuts` feature.
 */
export function onShortcut(handler: (accelerator: string) => void): () => void {
  return channel<string>("elyra:shortcut").subscribe((accel) => {
    if (accel) handler(accel);
  });
}

/**
 * Subscribe to native application-menu item clicks. The handler receives the
 * item id set in `App::menu`. Returns an unsubscribe function.
 */
export function onMenu(handler: (id: string) => void): () => void {
  return channel<string>("elyra:menu").subscribe((id) => {
    if (id) handler(id);
  });
}

// --- Settings store (framework built-in, always available) ------------------

async function storeCall<T>(op: string, arg?: unknown): Promise<T> {
  const res = await fetch(`${ORIGIN}/__store/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra store "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/**
 * A persistent key-value settings store (JSON on disk, in the OS config dir).
 * Values are any JSON-serializable data.
 */
export const store = {
  get<T = unknown>(key: string): Promise<T | null> {
    return storeCall<T | null>("get", key);
  },
  set(key: string, value: unknown): Promise<void> {
    return storeCall<void>("set", { key, value });
  },
  delete(key: string): Promise<boolean> {
    return storeCall<boolean>("delete", key);
  },
  all(): Promise<Record<string, unknown>> {
    return storeCall<Record<string, unknown>>("all");
  },
  clear(): Promise<void> {
    return storeCall<void>("clear");
  },
};

// --- Autostart (the `autostart` feature) ------------------------------------

async function autostartCall<T>(op: string): Promise<T> {
  const res = await fetch(`${ORIGIN}/__autostart/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra autostart "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/** Launch-at-login control (requires the app's `autostart` feature). */
export const autostart = {
  enable(): Promise<void> {
    return autostartCall<void>("enable");
  },
  disable(): Promise<void> {
    return autostartCall<void>("disable");
  },
  isEnabled(): Promise<boolean> {
    return autostartCall<boolean>("status");
  },
};

// --- Sidecar processes (the `sidecar` feature) ------------------------------

async function sidecarCall<T>(op: string, arg?: unknown): Promise<T> {
  const res = await fetch(`${ORIGIN}/__sidecar/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra sidecar "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/** An event from a sidecar process (on the `elyra:sidecar` channel). */
export interface SidecarEvent {
  id: number;
  kind: "data" | "exit";
  stream?: "stdout" | "stderr";
  line?: string;
  code?: number | null;
}

/** Spawn and manage sidecar child processes (requires the `sidecar` feature). */
export const sidecar = {
  /** Spawn a process; resolves to its id (used by `write` / `kill` and events). */
  spawn(program: string, args: string[] = []): Promise<number> {
    return sidecarCall<number>("spawn", { program, args });
  },
  /** Write to a sidecar's stdin. */
  write(id: number, data: string): Promise<boolean> {
    return sidecarCall<boolean>("write", { id, data });
  },
  /** Ask a sidecar to terminate. */
  kill(id: number): Promise<boolean> {
    return sidecarCall<boolean>("kill", id);
  },
};

/** Subscribe to sidecar output + exit events. Returns an unsubscribe function. */
export function onSidecar(handler: (event: SidecarEvent) => void): () => void {
  return channel<SidecarEvent>("elyra:sidecar").subscribe((e) => {
    if (e) handler(e);
  });
}

// --- Single-instance + deep-linking -----------------------------------------

async function deeplinkInitial(): Promise<string | null> {
  const res = await fetch(`${ORIGIN}/__deeplink/initial`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra deeplink failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as string | null;
}

/** Deep-link (custom URL scheme) access. */
export const deepLink = {
  /** The `<scheme>://…` URL the app was launched with, if any. */
  initial(): Promise<string | null> {
    return deeplinkInitial();
  },
};

/** Subscribe to deep-link URLs delivered while running. Returns unsubscribe. */
export function onDeepLink(handler: (url: string) => void): () => void {
  return channel<string>("elyra:deep-link").subscribe((u) => {
    if (u) handler(u);
  });
}

/**
 * Subscribe to second-launch payloads (single-instance mode). Returns
 * unsubscribe. The primary window is focused automatically.
 */
export function onSecondInstance(handler: (payload: string) => void): () => void {
  return channel<string>("elyra:second-instance").subscribe((p) => {
    if (p) handler(p);
  });
}

// --- Cache facade (needs the app's CacheProvider) ---------------------------

async function cacheCall<T>(op: string, arg?: unknown): Promise<T> {
  const res = await fetch(`${ORIGIN}/__cache/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra cache "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/**
 * An in-process key-value cache with TTLs — the same surface as Laravel's
 * `Cache::` (and Askr's shared cache), shared with the Rust `Cache`.
 */
export const cache = {
  get<T = unknown>(key: string): Promise<T | null> {
    return cacheCall<T | null>("get", key);
  },
  /** Store `value`, optionally expiring after `ttlSeconds`. */
  put(key: string, value: unknown, ttlSeconds?: number): Promise<void> {
    return cacheCall<void>("put", { key, value, ttl: ttlSeconds ?? null });
  },
  /** Store only if absent (atomic). Returns whether it was stored. */
  add(key: string, value: unknown, ttlSeconds?: number): Promise<boolean> {
    return cacheCall<boolean>("add", { key, value, ttl: ttlSeconds ?? null });
  },
  has(key: string): Promise<boolean> {
    return cacheCall<boolean>("has", key);
  },
  forget(key: string): Promise<boolean> {
    return cacheCall<boolean>("forget", key);
  },
  increment(key: string, by = 1): Promise<number> {
    return cacheCall<number>("increment", { key, by });
  },
  decrement(key: string, by = 1): Promise<number> {
    return cacheCall<number>("decrement", { key, by });
  },
  flush(): Promise<void> {
    return cacheCall<void>("flush");
  },
};

// --- Storage facade (needs the app's StorageProvider) -----------------------

async function storageCall<T>(op: string, arg?: unknown): Promise<T> {
  const res = await fetch(`${ORIGIN}/__storage/${op}`, {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: encode(arg ?? null),
  });
  if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
    throw new Error(`elyra storage "${op}" failed: ${await res.text()}`);
  }
  return decode(new Uint8Array(await res.arrayBuffer())) as T;
}

/**
 * A local filesystem disk — the same surface as Laravel's `Storage::`, jailed to
 * the app's disk root. Text content; use the Rust `Storage` for binary.
 */
export const storage = {
  put(path: string, contents: string): Promise<void> {
    return storageCall<void>("put", { path, contents });
  },
  get(path: string): Promise<string> {
    return storageCall<string>("get", path);
  },
  exists(path: string): Promise<boolean> {
    return storageCall<boolean>("exists", path);
  },
  delete(path: string): Promise<void> {
    return storageCall<void>("delete", path);
  },
  size(path: string): Promise<number> {
    return storageCall<number>("size", path);
  },
  /** A `file://` URL for the path (to open in the OS). */
  url(path: string): Promise<string> {
    return storageCall<string>("url", path);
  },
  /** File names directly inside `dir` (non-recursive). */
  files(dir = ""): Promise<string[]> {
    return storageCall<string[]>("files", dir);
  },
};

// --- Queue facade (needs the app's QueueProvider; handlers are Rust-side) ----

/** Enqueue a background job. Handlers run in Rust; status arrives on `onQueue`. */
export const queue = {
  push(job: string, payload: unknown = null): Promise<void> {
    return (async () => {
      const res = await fetch(`${ORIGIN}/__queue/push`, {
        method: "POST",
        headers: { "content-type": "application/msgpack" },
        body: encode({ job, payload }),
      });
      if (res.headers.get("x-elyra-status") === "error" || !res.ok) {
        throw new Error(`elyra queue push failed: ${await res.text()}`);
      }
    })();
  },
};

/** A queue status event (on the `elyra:queue` channel). */
export interface QueueEvent {
  job: string;
  status: "processing" | "processed" | "failed" | "unhandled";
  error?: string;
}

/** Subscribe to queue status updates. Returns an unsubscribe function. */
export function onQueue(handler: (event: QueueEvent) => void): () => void {
  return channel<QueueEvent>("elyra:queue").subscribe((e) => {
    if (e) handler(e);
  });
}
