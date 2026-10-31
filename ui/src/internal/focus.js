// Focus helpers for the overlays (Modal, Drawer, Popover, Dropdown).

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type=hidden])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/** The elements inside `root` the keyboard can reach, in order. */
export function focusables(root) {
  return root ? [...root.querySelectorAll(FOCUSABLE)].filter((el) => !el.closest("[hidden]")) : [];
}

/** Keep Tab and Shift+Tab inside `root`. Returns true when it moved focus. */
export function trapTab(event, root) {
  if (event.key !== "Tab") return false;
  const list = focusables(root);
  if (list.length === 0) {
    event.preventDefault();
    root.focus();
    return true;
  }
  const first = list[0];
  const last = list[list.length - 1];
  const active = root.ownerDocument.activeElement;
  if (event.shiftKey && (active === first || active === root)) {
    event.preventDefault();
    last.focus();
    return true;
  }
  if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
    return true;
  }
  return false;
}

/** Call `close` on a pointer press outside every element in `inside`. Returns the cleanup. */
export function onOutside(inside, close) {
  const handler = (event) => {
    if (inside().some((el) => el?.contains(event.target))) return;
    close();
  };
  document.addEventListener("pointerdown", handler, true);
  return () => document.removeEventListener("pointerdown", handler, true);
}
