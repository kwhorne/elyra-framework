// The coding agent's view of a window (RFC 0026), in debug builds only: the
// shell adds this to each page's initialization scripts under
// `cfg(debug_assertions)`, so a release has none of it.
//
// `rata mcp dev` asks through the dev endpoint; the shell starts a job with
// `__elyraAgent.start(id, what, args)` and reads it back with
// `__elyraAgent.take(id)` (a webview's script callback has the script's value,
// not a promise's, so the answer is fetched once it's there).
(() => {
  if (window.__elyraAgent) return;

  // What went wrong since the last snapshot: console errors, uncaught errors
  // and rejections, from the page's start.
  const errors = [];
  const keep = (message) => {
    errors.push(String(message).slice(0, 300));
    if (errors.length > 20) errors.shift();
  };
  const consoleError = console.error;
  console.error = (...args) => {
    keep(args.map((a) => (a instanceof Error ? a.message : String(a))).join(" "));
    consoleError.apply(console, args);
  };
  window.addEventListener("error", (event) => keep(event.message));
  window.addEventListener("unhandledrejection", (event) => keep(event.reason?.message ?? event.reason));

  // Calls to the shell in flight, so an action can wait for the page to settle.
  let inFlight = 0;
  const realFetch = window.fetch;
  if (realFetch) {
    window.fetch = function (input, init) {
      const url = String(input?.url ?? input);
      const ours = url.startsWith("elyra://") || url.includes("://elyra.localhost");
      if (!ours) return realFetch.call(this, input, init);
      inFlight += 1;
      return realFetch.call(this, input, init).finally(() => (inFlight -= 1));
    };
  }

  const MAX_LINES = 1000;
  const MAX_TEXT = 200;
  const MAX_CHILDREN = 50;

  /** What can be acted on gets a ref. */
  const ACTIONABLE = new Set([
    "link", "button", "textbox", "searchbox", "checkbox", "radio", "switch", "combobox", "listbox",
    "option", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "slider", "spinbutton", "row", "treeitem",
  ]);
  /** Named by their content, and shown as one line unless something in them can be acted on. */
  const FROM_CONTENT = new Set([
    "button", "link", "heading", "tab", "option", "menuitem", "menuitemcheckbox", "menuitemradio",
    "cell", "columnheader", "rowheader", "listitem", "treeitem", "tooltip", "status", "alert",
  ]);
  const LANDMARK_INSIDE = "article, aside, main, nav, section";

  let refs = new Map();
  let nextRef = 1;

  const text = (s) => String(s ?? "").replace(/\s+/g, " ").trim();
  /** An element's words, with a space between its parts: a row's cells read
   * `1 Ada Delete`, not `1AdaDelete`. What's hidden isn't read. */
  function words(el) {
    const parts = [];
    const read = (node) => {
      for (const child of node.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) parts.push(child.textContent);
        else if (child.nodeType === Node.ELEMENT_NODE && !hidden(child)) read(child);
      }
    };
    read(el);
    return text(parts.join(" "));
  }
  const cut = (s) => (s.length > MAX_TEXT ? `${s.slice(0, MAX_TEXT)}…` : s);
  const quote = (s) => `"${cut(s).replace(/"/g, '\\"')}"`;

  function hidden(el) {
    if (el.hidden || el.inert || el.getAttribute("aria-hidden") === "true") return true;
    if (el.tagName === "TEMPLATE" || el.tagName === "SCRIPT" || el.tagName === "STYLE") return true;
    const style = window.getComputedStyle?.(el);
    return !!style && (style.display === "none" || style.visibility === "hidden");
  }

  function roleOf(el) {
    const explicit = el.getAttribute("role")?.split(/\s+/)[0];
    if (explicit && explicit !== "presentation" && explicit !== "none") return explicit;
    if (explicit) return undefined;
    const tag = el.tagName.toLowerCase();
    const type = (el.getAttribute("type") ?? "text").toLowerCase();
    switch (tag) {
      case "a": return el.hasAttribute("href") ? "link" : undefined;
      case "button": return "button";
      case "input":
        if (type === "hidden") return undefined;
        if (["button", "submit", "reset", "image"].includes(type)) return "button";
        if (type === "checkbox") return el.getAttribute("switch") !== null ? "switch" : "checkbox";
        if (type === "radio") return "radio";
        if (type === "range") return "slider";
        if (type === "number") return "spinbutton";
        if (type === "search") return "searchbox";
        if (type === "file") return "button";
        return "textbox";
      case "textarea": return "textbox";
      case "select": return el.multiple || el.size > 1 ? "listbox" : "combobox";
      case "option": return "option";
      case "h1": case "h2": case "h3": case "h4": case "h5": case "h6": return "heading";
      case "nav": return "navigation";
      case "main": return "main";
      case "aside": return "complementary";
      case "header": return el.closest(LANDMARK_INSIDE) ? undefined : "banner";
      case "footer": return el.closest(LANDMARK_INSIDE) ? undefined : "contentinfo";
      case "form": return nameOf(el, "form") ? "form" : undefined;
      case "section": return nameOf(el, "region") ? "region" : undefined;
      case "dialog": return "dialog";
      case "table": return "table";
      case "tr": return "row";
      case "th": return el.closest("thead") || el.getAttribute("scope") === "col" ? "columnheader" : "rowheader";
      case "td": return "cell";
      case "ul": case "ol": return "list";
      case "li": return "listitem";
      case "img": return el.getAttribute("alt") === "" ? undefined : "img";
      case "progress": return "progressbar";
      case "fieldset": return "group";
      case "hr": return "separator";
      case "label": return undefined;
      default: return undefined;
    }
  }

  /** A label's own words: without the controls in it (a select's options). */
  function ownText(label) {
    const copy = label.cloneNode(true);
    for (const control of copy.querySelectorAll("input, select, textarea, button")) control.remove();
    return text(copy.textContent);
  }

  function labelText(el) {
    if (el.id) {
      const label = document.querySelector(`label[for="${CSS.escape(el.id)}"]`);
      if (label) return ownText(label);
    }
    const wrapping = el.closest("label");
    return wrapping ? ownText(wrapping) : "";
  }

  function nameOf(el, role) {
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const named = by.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? "").join(" ");
      if (text(named)) return text(named);
    }
    const aria = el.getAttribute("aria-label");
    if (aria && text(aria)) return text(aria);
    const tag = el.tagName.toLowerCase();
    // What a <label> can name: before the element's own content.
    if (["button", "meter", "output", "progress"].includes(tag)) {
      const label = labelText(el);
      if (label) return label;
    }
    if (tag === "input" || tag === "textarea" || tag === "select") {
      const type = (el.getAttribute("type") ?? "").toLowerCase();
      if (["button", "submit", "reset"].includes(type)) return labelText(el) || text(el.value);
      return labelText(el) || text(el.getAttribute("placeholder")) || text(el.getAttribute("title"));
    }
    if (tag === "img") return text(el.getAttribute("alt")) || text(el.getAttribute("title"));
    if (tag === "fieldset") return text(el.querySelector("legend")?.textContent);
    if (tag === "table") return text(el.querySelector("caption")?.textContent);
    if (FROM_CONTENT.has(role) || role === "row") return words(el) || text(el.getAttribute("title"));
    return text(el.getAttribute("title"));
  }

  function valueOf(el, role) {
    const tag = el.tagName.toLowerCase();
    if (tag === "select") return text(el.options[el.selectedIndex]?.textContent ?? el.value);
    // A combobox that isn't a field shows its value as its text.
    if (role === "combobox" && tag !== "input") return text(el.textContent);
    if (tag !== "input" && tag !== "textarea") {
      const now = el.getAttribute("aria-valuetext") ?? el.getAttribute("aria-valuenow");
      return role === "slider" || role === "progressbar" || role === "spinbutton" ? now ?? undefined : undefined;
    }
    const type = (el.getAttribute("type") ?? "text").toLowerCase();
    if (["checkbox", "radio", "button", "submit", "reset", "file", "hidden"].includes(type)) return undefined;
    const secret = type === "password" || /password|one-time-code|cc-number|cc-csc/.test(el.getAttribute("autocomplete") ?? "");
    if (secret) return el.value ? "•••" : "";
    return el.value;
  }

  function statesOf(el, role) {
    const out = [];
    const flag = (name) => el.getAttribute(name);
    if (role === "heading") out.push(`level=${flag("aria-level") ?? el.tagName.slice(1)}`);
    const checked = el.type === "checkbox" || el.type === "radio" ? el.checked : flag("aria-checked") === "true";
    if (checked) out.push("checked");
    if (flag("aria-checked") === "mixed" || el.indeterminate) out.push("mixed");
    if (flag("aria-selected") === "true") out.push("selected");
    if (flag("aria-pressed") === "true") out.push("pressed");
    const current = flag("aria-current");
    if (current && current !== "false") out.push("current");
    const expanded = flag("aria-expanded");
    if (expanded === "true") out.push("expanded");
    if (expanded === "false") out.push("collapsed");
    if (el.disabled || flag("aria-disabled") === "true") out.push("disabled");
    if (el.required || flag("aria-required") === "true") out.push("required");
    if (flag("aria-invalid") === "true") out.push("invalid");
    if (el.readOnly || flag("aria-readonly") === "true") out.push("readonly");
    if (document.activeElement === el && el !== document.body) out.push("focused");
    return out;
  }

  /** Something in `el` that can be acted on, so it isn't one line. */
  function holdsActions(el) {
    return !!el.querySelector(
      'a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"]), [role="button"], [role="link"], [role="checkbox"], [role="switch"], [role="tab"], [role="option"], [role="menuitem"]',
    );
  }

  function render() {
    refs = new Map();
    nextRef = 1;
    const lines = [];
    let more = 0;
    const push = (line) => (lines.length < MAX_LINES ? lines.push(line) : (more += 1));

    function walk(node, depth) {
      let shown = 0;
      for (const child of node.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) {
          const t = text(child.textContent);
          if (t) push(`${"  ".repeat(depth)}- text ${quote(t)}`);
          continue;
        }
        if (child.nodeType !== Node.ELEMENT_NODE || hidden(child)) continue;
        shown += 1;
        if (shown > MAX_CHILDREN) {
          const rest = [...node.children].filter((c) => !hidden(c)).length - MAX_CHILDREN;
          push(`${"  ".repeat(depth)}- … ${rest} more`);
          return;
        }
        const role = roleOf(child);
        if (!role) {
          // A label's words are its control's name: only what else is in it.
          if (child.tagName === "LABEL") walkElements(child, depth);
          else walk(child, depth);
          continue;
        }
        let line = `${"  ".repeat(depth)}- ${role}`;
        const leaf = (FROM_CONTENT.has(role) || role === "img" || role === "textbox" || role === "searchbox" || role === "combobox") && !holdsActions(child);
        // Named by content that's shown below it anyway: not said twice.
        const fromContent = FROM_CONTENT.has(role) && !leaf && !child.hasAttribute("aria-label") && !child.hasAttribute("aria-labelledby");
        const name = fromContent ? "" : nameOf(child, role);
        if (name) line += ` ${quote(name)}`;
        const value = valueOf(child, role);
        if (value !== undefined && value !== "") line += ` value=${quote(value)}`;
        for (const state of statesOf(child, role)) line += ` [${state}]`;
        if (ACTIONABLE.has(role) || child.matches?.('[tabindex]:not([tabindex="-1"])')) {
          const ref = `e${nextRef++}`;
          refs.set(ref, child);
          line += ` [ref=${ref}]`;
        }
        if (role === "list" || role === "table" || role === "listbox" || role === "tablist") {
          const items = child.querySelectorAll(role === "table" ? "tr" : '[role="option"], li, [role="tab"]').length;
          if (items) line += ` (${items} ${role === "table" ? "rows" : "items"})`;
        }
        push(line);
        if (!leaf) walk(child, depth + 1);
      }
    }

    /** `walk`, without the text right inside `node`. */
    function walkElements(node, depth) {
      for (const child of node.children) walk({ childNodes: [child], children: [child] }, depth);
    }

    const route = (location.hash || "").replace(/^#/, "") || "/";
    walk(document.body, 0);
    const out = [`route: ${route}`, `title: ${quote(document.title)}`, ...lines];
    if (more) out.push(`- … cut at ${MAX_LINES} lines (${more} more)`);
    if (errors.length) out.push("errors since the last snapshot:", ...errors.map((e) => `- ${e}`));
    errors.length = 0;
    return out.join("\n");
  }

  const frame = () => new Promise((done) => requestAnimationFrame(() => done()));

  /** Until no call to the shell is in flight for two frames, at most two seconds. */
  async function settle() {
    const until = Date.now() + 2000;
    let quiet = 0;
    while (Date.now() < until && quiet < 2) {
      await frame();
      quiet = inFlight === 0 ? quiet + 1 : 0;
    }
  }

  function target(ref) {
    const el = refs.get(ref);
    if (!el) throw new Error(`no element ${ref}: take a snapshot first, and use a ref from it`);
    if (!el.isConnected) throw new Error(`${ref} is no longer on the page: take a new snapshot`);
    return el;
  }

  function setValue(el, value) {
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
    if (setter) setter.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  async function act({ action, ref, text: typed, append, key, modifiers = [], option, route, direction }) {
    switch (action) {
      case "click": {
        const el = target(ref);
        el.scrollIntoView?.({ block: "nearest" });
        el.focus?.({ preventScroll: true });
        el.click();
        break;
      }
      case "type": {
        const el = target(ref);
        el.focus?.({ preventScroll: true });
        if (el.isContentEditable) {
          el.textContent = append ? el.textContent + typed : typed;
          el.dispatchEvent(new Event("input", { bubbles: true }));
        } else {
          setValue(el, append ? `${el.value}${typed}` : String(typed ?? ""));
        }
        break;
      }
      case "press": {
        const el = ref ? target(ref) : document.activeElement ?? document.body;
        if (ref) el.focus?.({ preventScroll: true });
        const init = {
          key, bubbles: true, cancelable: true,
          ctrlKey: modifiers.includes("ctrl"), metaKey: modifiers.includes("meta"),
          altKey: modifiers.includes("alt"), shiftKey: modifiers.includes("shift"),
        };
        const proceed = el.dispatchEvent(new KeyboardEvent("keydown", init));
        // What a browser does itself on Enter in a form's field.
        if (proceed && key === "Enter" && el.form && el.tagName === "INPUT") el.form.requestSubmit?.();
        el.dispatchEvent(new KeyboardEvent("keyup", init));
        break;
      }
      case "select": {
        const el = target(ref);
        if (el.tagName === "SELECT") {
          const found = [...el.options].find((o) => o.value === option || text(o.textContent) === option);
          if (!found) throw new Error(`${ref} has no option ${quote(String(option))}`);
          el.value = found.value;
          el.dispatchEvent(new Event("input", { bubbles: true }));
          el.dispatchEvent(new Event("change", { bubbles: true }));
        } else {
          const options = [...document.querySelectorAll('[role="option"]')].filter((o) => !hidden(o));
          const found = options.find((o) => text(o.textContent) === option || o.getAttribute("data-value") === option);
          if (!found) throw new Error(`no option ${quote(String(option))} is shown: open the list first`);
          found.click();
        }
        break;
      }
      case "scroll": {
        if (ref) target(ref).scrollIntoView?.({ block: "center" });
        else window.scrollBy(0, (direction === "up" ? -1 : 1) * window.innerHeight * 0.8);
        break;
      }
      case "navigate": {
        location.hash = `#${String(route ?? "/").replace(/^#/, "")}`;
        break;
      }
      default:
        throw new Error(`unknown action ${quote(String(action))}: click, type, press, select, scroll or navigate`);
    }
    await settle();
    return render();
  }

  const jobs = new Map();

  window.__elyraAgent = {
    snapshot: render,
    act,
    /** Start `what` ("snapshot" or "act") as job `id`. */
    start(id, what, args) {
      jobs.set(id, { done: false });
      Promise.resolve()
        .then(() => (what === "act" ? act(args ?? {}) : render()))
        .then(
          (value) => jobs.set(id, { done: true, value }),
          (error) => jobs.set(id, { done: true, error: String(error?.message ?? error) }),
        );
      return true;
    },
    /** Job `id`'s answer once it's done, and forget it; `null` before. */
    take(id) {
      const job = jobs.get(id);
      if (!job?.done) return null;
      jobs.delete(id);
      return job;
    },
  };
})();
