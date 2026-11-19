// @vitest-environment node
/** WCAG 2's contrast ratio (RFC 0037). */
import { describe, expect, it } from "vitest";
import { contrast, luminance, nearest, parseColor } from "./contrast.js";

describe("contrast", () => {
  it("is WCAG's ratio, in either order", () => {
    expect(contrast("#000", "#fff")).toBeCloseTo(21, 5);
    expect(contrast("#ffffff", "#ffffff")).toBe(1);
    // WCAG's own borderline grey: just under 4.5:1 on white.
    expect(contrast("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
    expect(contrast("#fff", "#767676")).toBeCloseTo(contrast("#767676", "#fff"), 10);
    expect(contrast("#767676", "#ffffff")).toBeGreaterThanOrEqual(4.5);
  });

  it("reads hex, short hex and rgb(), and nothing else", () => {
    expect(parseColor("#1F2030")).toEqual([31, 32, 48]);
    expect(parseColor("#abc")).toEqual([170, 187, 204]);
    expect(parseColor("rgb(31, 32, 48)")).toEqual([31, 32, 48]);
    expect(parseColor("rgba(31 32 48 / 0.5)")).toEqual([31, 32, 48]);
    expect(parseColor("#1f2030cc")).toEqual([31, 32, 48]);
    expect(parseColor("tomato")).toBeNull();
    expect(() => luminance("tomato")).toThrow(/not a colour/);
  });

  it("finds the nearest colour that meets a ratio, lighter on dark and darker on light", () => {
    const dark = ["#16161e", "#1f2030"];
    const lighter = nearest("#787c99", dark, 4.5);
    expect(dark.every((bg) => contrast(lighter, bg) >= 4.5)).toBe(true);
    expect(luminance(lighter)).toBeGreaterThan(luminance("#787c99"));
    const darker = nearest("#6b7080", ["#ffffff", "#eceef3"], 4.5);
    expect(luminance(darker)).toBeLessThan(luminance("#6b7080"));
    // Already enough: itself.
    expect(nearest("#c0caf5", dark, 4.5)).toBe("#c0caf5");
    // No lightness of anything has 21:1 against mid grey.
    expect(nearest("#777777", ["#777777"], 21)).toBeNull();
  });
});
