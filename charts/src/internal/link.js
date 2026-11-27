// Linked charts (RFC 0063): the page's charts by their `link` name. One moves
// its window or its pointer, and the others in its group follow, each by
// its own axis's values (zoom.js), so a month of days and a year of weeks
// line up where they meet.

/** name → the charts linked by it. */
const groups = new Map();

/**
 * Join `name`'s group. `member` is `{ follow(range), point(value), unpoint(),
 * changed() }`; `changed` is told when the group grows or shrinks. Returns
 * the way to leave.
 */
export function join(name, member) {
  let group = groups.get(name);
  if (!group) groups.set(name, (group = new Set()));
  group.add(member);
  for (const m of group) m.changed?.();
  return () => {
    group.delete(member);
    if (group.size === 0) groups.delete(name);
    for (const m of group) m.changed?.();
  };
}

/** The others in `member`'s group. */
export function others(name, member) {
  return [...(groups.get(name) ?? [])].filter((m) => m !== member);
}
