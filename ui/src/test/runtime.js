// What the components use from @elyra/runtime, for the tests: the user's
// abilities, the confirm dialog and the router stand-ins; the real
// validationErrors.
import { vi } from "vitest";

let granted = ["*"];
const listeners = new Set();
const covers = (g, a) => (g.endsWith("*") ? a.startsWith(g.slice(0, -1)) : g === a);

/** Set what the signed-in user may do. */
export function grant(abilities) {
  granted = abilities;
  for (const run of listeners) run((a) => granted.some((g) => covers(g, a)));
}

export const can = {
  subscribe(run) {
    listeners.add(run);
    run((a) => granted.some((g) => covers(g, a)));
    return () => listeners.delete(run);
  },
};

export const confirm = vi.fn(async () => true);
export const href = (path) => `#${path}`;
