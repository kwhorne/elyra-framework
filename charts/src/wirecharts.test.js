// Every WireCharts chart, held to WireCharts (RFC 0059): the option each of
// ours builds for a docs example's props is the option WireCharts renders for
// them, with functions compared as functions.
import { describe, expect, it } from "vitest";
import fixtures from "./test/wirecharts/fixtures.json";
import { CALLS } from "./internal/wirecharts/calls.js";

/** An option as JSON sees it: functions as "@@function@@", undefined left out. */
export function comparable(value) {
  if (typeof value === "function") return "@@function@@";
  if (Array.isArray(value)) return value.map(comparable);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, v]) => v !== undefined)
        .map(([k, v]) => [k, comparable(v)]),
    );
  }
  return value;
}

describe("held to WireCharts", () => {
  for (const fixture of fixtures) {
    const call = CALLS[fixture.component];
    it.skipIf(!call)(`${fixture.component}: the option WireCharts builds`, () => {
      const props = { ...fixture.defaults, ...fixture.props };
      expect(comparable(call(props))).toEqual(fixture.option);
    });
  }

  it("every WireCharts chart has a call", () => {
    const missing = fixtures.map((f) => f.component).filter((c) => !CALLS[c]);
    expect(missing).toEqual([]);
  });
});
