// Where a popup goes (Dropdown, Popover, Select, Combobox, DatePicker,
// Tooltip, NotificationBell): beside what opened it, inside the window.
// It's `position: fixed`, so a scrolling or clipping container (a Table's
// last rows, a Card with `overflow: hidden`) doesn't cut it off; it opens
// on the other side when there's more room there, keeps inside the window's
// edges, and scrolls itself when neither side has room for all of it.

/** The space kept between a popup and the window's edge. */
export const MARGIN = 8;

/**
 * Where a popup of `size` goes beside `anchor` in a `viewport`, both
 * measured as the window sees them.
 *
 * - `side`: `"bottom"` (the default) or `"top"`, the side it prefers. It
 *   opens on the other one when it doesn't fit and there's more room there.
 * - `align`: `"start"` (left edges lined up), `"end"` (right edges) or
 *   `"center"`.
 * - `gap`: the space between the anchor and the popup.
 * - `match`: as wide as the anchor (a Select's list).
 */
export function position(anchor, size, viewport, { side = "bottom", align = "start", gap = 4, match = false } = {}) {
  const width = match ? anchor.width : size.width;
  const below = Math.max(viewport.height - anchor.bottom - gap - MARGIN, 0);
  const above = Math.max(anchor.top - gap - MARGIN, 0);
  let placed = side === "top" ? "top" : "bottom";
  if (placed === "bottom" && size.height > below && above > below) placed = "top";
  else if (placed === "top" && size.height > above && below > above) placed = "bottom";
  const room = placed === "bottom" ? below : above;
  const height = Math.min(size.height, room);
  const top = placed === "bottom" ? anchor.bottom + gap : anchor.top - gap - height;
  let left =
    align === "end"
      ? anchor.right - width
      : align === "center"
        ? anchor.left + (anchor.width - width) / 2
        : anchor.left;
  left = Math.max(MARGIN, Math.min(left, viewport.width - width - MARGIN));
  return { top, left, width: match ? width : null, maxHeight: room, side: placed, clipped: size.height > room };
}

/**
 * `use:place={{ anchor, side, align, gap, match }}` on a popup: placed as
 * {@link position} says, and again when the window resizes, anything
 * scrolls, or the popup or its anchor changes size. A `hidden` popup
 * (Tooltip's, between hovers) is left alone until it's shown.
 */
export function place(node, options) {
  let opts = options;
  const win = node.ownerDocument.defaultView;
  let retrying = false;

  function update() {
    const anchor = opts?.anchor;
    if (!anchor) return;
    if (node.hidden) {
      // Shown in the same update that told us so: Svelte sets `hidden`
      // after this runs. Once it has, measure.
      if (!retrying) {
        retrying = true;
        queueMicrotask(() => {
          retrying = false;
          if (!node.hidden) update();
        });
      }
      return;
    }
    const style = node.style;
    style.position = "fixed";
    style.top = "0px";
    style.left = "0px";
    style.right = "auto";
    style.bottom = "auto";
    style.transform = "none";
    style.maxHeight = "";
    style.overflowY = "";
    if (opts.match) {
      style.boxSizing = "border-box";
      style.width = `${anchor.getBoundingClientRect().width}px`;
    }
    // Its own size, unconstrained. The layout's, not as drawn: an opening
    // animation (a slide, a slight scale) mustn't be measured into it.
    const size = { width: node.offsetWidth, height: node.offsetHeight };
    const origin = fixedOrigin();
    const at = position(anchor.getBoundingClientRect(), size, { width: win.innerWidth, height: win.innerHeight }, opts);
    style.top = `${at.top - origin.top}px`;
    style.left = `${at.left - origin.left}px`;
    if (at.width !== null) style.width = `${at.width}px`;
    if (at.clipped) {
      style.maxHeight = `${at.maxHeight}px`;
      style.overflowY = "auto";
    }
    node.dataset.side = at.side;
  }

  /**
   * Where `fixed` puts 0,0 for the popup: the window's corner, or inside a
   * transformed or filtered ancestor (a Modal's backdrop) that ancestor's.
   * A sibling measures it: the popup's own animation doesn't move it.
   */
  function fixedOrigin() {
    const probe = node.ownerDocument.createElement("div");
    probe.style.cssText = "position:fixed;top:0;left:0;width:0;height:0;visibility:hidden;pointer-events:none";
    node.parentNode.insertBefore(probe, node);
    const at = probe.getBoundingClientRect();
    probe.remove();
    return { top: at.top, left: at.left };
  }

  update();
  win.addEventListener("resize", update);
  // Any container scrolling moves the anchor: capture sees them all.
  win.addEventListener("scroll", update, { capture: true, passive: true });
  const resized = typeof win.ResizeObserver === "function" ? new win.ResizeObserver(() => update()) : null;
  resized?.observe(node);
  if (opts?.anchor) resized?.observe(opts.anchor);

  return {
    update(next) {
      if (next?.anchor !== opts?.anchor && resized) {
        if (opts?.anchor) resized.unobserve(opts.anchor);
        if (next?.anchor) resized.observe(next.anchor);
      }
      opts = next;
      update();
    },
    destroy() {
      win.removeEventListener("resize", update);
      win.removeEventListener("scroll", update, { capture: true });
      resized?.disconnect();
    },
  };
}
