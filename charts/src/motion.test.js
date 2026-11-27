// The WireCharts charts that move (RFC 0059): the clock ticks, the races
// draw their points one by one and replay, the audio chart plays and stops.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { charts } from "./test/echarts.js";
import { ClockChart, LineRaceChart, AudioChart } from "./index.js";
import { hands, soundValues } from "./internal/wirecharts/motion.js";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("what moves", () => {
  it("the clock's hands are the time", () => {
    expect(hands(new Date(2026, 0, 1, 15, 30, 0))).toEqual([3.5, 30, 0]);
  });

  it("the clock ticks every second, and stops with the chart", async () => {
    const { unmount } = render(ClockChart);
    await tick();
    const ticks = () => charts[0].options.filter(({ option }) => option.series?.length === 3 && !option.color).length;
    const before = ticks();
    vi.advanceTimersByTime(3000);
    expect(ticks()).toBe(before + 3);
    unmount();
    vi.advanceTimersByTime(3000);
    expect(ticks()).toBe(before + 3);
  });

  it("a race draws its points one by one, and Replay starts it over", async () => {
    render(LineRaceChart, { series: [{ name: "A", data: [1, 2, 3, 4] }], categories: ["a", "b", "c", "d"], interval: 100 });
    await tick();
    const shown = () => charts[0].options.at(-1).option.series[0].data.length;
    expect(shown()).toBe(1);
    vi.advanceTimersByTime(300);
    expect(shown()).toBe(4);
    screen.getByRole("button", { name: "Replay" }).click();
    expect(shown()).toBe(1);
  });

  it("the audio chart plays its series as notes, and stops", async () => {
    const notes = [];
    globalThis.AudioContext = class {
      currentTime = 0;
      destination = {};
      createOscillator() {
        const osc = { frequency: {}, connect() {}, start() {}, stop() {} };
        notes.push(osc);
        return osc;
      }
      createGain() {
        return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} };
      }
      close() {}
    };
    render(AudioChart, { series: [{ name: "Temp", data: [10, 20, 30] }], categories: ["a", "b", "c"], duration: 300 });
    await tick();
    screen.getByRole("button", { name: "Play the chart as sound" }).click();
    await tick();
    expect(notes.map((n) => Math.round(n.frequency.value))).toEqual([220, 550, 880]);
    expect(screen.getByRole("button", { name: "Stop" }).getAttribute("aria-pressed")).toBe("true");
    vi.advanceTimersByTime(500);
    await tick();
    expect(charts.length, "one chart").toBe(1);
    expect(charts[0].actions.map((a) => a.type)).toContain("showTip");
    expect(screen.getByRole("button", { name: "Play the chart as sound" })).toBeTruthy();
    delete globalThis.AudioContext;
  });

  it("plays a series' values however they're written", () => {
    expect(soundValues({ series: [{ data: [1, [0, 2], { value: 3 }, "x"] }] })).toEqual([1, 2, 3]);
  });
});
