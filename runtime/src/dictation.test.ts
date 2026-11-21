/**
 * Dictating into Point at it's note (RFC 0035): held and let go, a start
 * that fails, a press shorter than the start, and Esc while it records.
 */
import { describe, expect, it, vi } from "vitest";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
const { holdToDictate } = await import("./index.js");

function rig(start: () => Promise<void> = () => Promise.resolve()) {
  const mic = {
    start: vi.fn(start),
    stop: vi.fn(() => Promise.resolve({ path: "/tmp/a.wav", name: "a.wav", size: 1, mime: "audio/wav" } as never)),
    cancel: vi.fn(() => Promise.resolve()),
    transcribe: vi.fn(() => Promise.resolve("make it blue")),
  };
  const seen: string[] = [];
  const view = {
    state: (state: string) => seen.push(state),
    text: (text: string) => seen.push(`text: ${text}`),
    failed: (message: string) => seen.push(`failed: ${message}`),
  };
  return { mic, seen, dictation: holdToDictate(mic, view) };
}

describe("hold to dictate", () => {
  it("records while held, and hands the text over when let go", async () => {
    const { mic, seen, dictation } = rig();
    dictation.begin();
    dictation.begin(); // a key's repeat
    await dictation.end();
    expect(mic.start).toHaveBeenCalledTimes(1);
    expect(mic.start).toHaveBeenCalledWith({ maxSeconds: 60 });
    expect(seen).toEqual(["listening", "transcribing", "text: make it blue", "idle"]);
  });

  it("says why it can't start, and nothing else, even let go before it said", async () => {
    let fail!: (err: Error) => void;
    const { mic, seen, dictation } = rig(() => new Promise((_, reject) => (fail = reject)));
    dictation.begin();
    const ended = dictation.end();
    fail(new Error("not allowed"));
    await ended;
    expect(mic.stop).not.toHaveBeenCalled();
    expect(seen).toEqual(["listening", "failed: No microphone: not allowed", "idle"]);
  });

  it("let go before the start answered, still stops what it started", async () => {
    let started!: () => void;
    const { mic, seen, dictation } = rig(() => new Promise<void>((resolve) => (started = resolve)));
    dictation.begin();
    const ended = dictation.end();
    expect(mic.stop).not.toHaveBeenCalled();
    started();
    await ended;
    expect(mic.stop).toHaveBeenCalledTimes(1);
    expect(seen[seen.length - 1]).toBe("idle");
  });

  it("thrown away (Esc, Cancel, handed off) stops the microphone, one still starting too", async () => {
    const recording = rig();
    recording.dictation.begin();
    await Promise.resolve();
    recording.dictation.cancel();
    await vi.waitFor(() => expect(recording.mic.cancel).toHaveBeenCalledTimes(1));
    await recording.dictation.end();
    expect(recording.mic.stop).not.toHaveBeenCalled();
    expect(recording.seen).toEqual(["listening"]);

    // Let go, then thrown away while the start is still on its way.
    let started!: () => void;
    const late = rig(() => new Promise<void>((resolve) => (started = resolve)));
    late.dictation.begin();
    const ended = late.dictation.end();
    late.dictation.cancel();
    started();
    await ended;
    expect(late.mic.cancel).toHaveBeenCalledTimes(1);
    expect(late.mic.stop).not.toHaveBeenCalled();

    // Once thrown away, a press does nothing.
    late.dictation.begin();
    expect(late.mic.start).toHaveBeenCalledTimes(1);
  });
});
