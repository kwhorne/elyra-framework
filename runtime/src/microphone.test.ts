/**
 * The microphone (RFC 0032): the shell records, the page asks it to start
 * and stop, and gets a `ChosenFile` it can transcribe. The payloads are
 * snake_case for Rust, and a failure is the shell's message.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  errorResponse,
  okResponse,
  stubFetch,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { microphone, onMicrophone } = await import("./index.js");
type ChosenFile = import("./index.js").ChosenFile;
type MicrophoneEvent = import("./index.js").MicrophoneEvent;

const recording: ChosenFile = {
  id: "g1",
  path: "/cache/notes/recordings/recording-1.wav",
  name: "recording-1.wav",
  kind: "open",
};

function bodyOf(call: { init?: RequestInit } | undefined): unknown {
  return decode(call?.init?.body as Uint8Array);
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  pending = [];
  vi.unstubAllGlobals();
});

describe("microphone", () => {
  it("starts, stops to a ChosenFile, and transcribes it", async () => {
    const calls = stubFetch((url) => {
      if (url.endsWith("/__mic/stop")) return okResponse(recording);
      if (url.endsWith("/__mic/transcribe")) return okResponse("Send a reminder on these.");
      return okResponse(true);
    });

    await microphone.start({ maxSeconds: 30 });
    const file = await microphone.stop();
    const text = await microphone.transcribe(file, { language: "no" });

    expect(calls.map((c) => c.url)).toEqual([
      "elyra://localhost/__mic/start",
      "elyra://localhost/__mic/stop",
      "elyra://localhost/__mic/transcribe",
    ]);
    expect(bodyOf(calls[0])).toEqual({ max_seconds: 30 });
    expect(file).toEqual(recording);
    expect(bodyOf(calls[2])).toEqual({ file: recording, language: "no" });
    expect(text).toBe("Send a reminder on these.");
    // Each call says which window it comes from.
    expect(new Headers(calls[0]!.init?.headers).get("x-elyra-client-id")).toBeTruthy();
  });

  it("sends nulls for what isn't given", async () => {
    const calls = stubFetch(() => okResponse(true));
    await microphone.start();
    await microphone.cancel();
    expect(bodyOf(calls[0])).toEqual({ max_seconds: null });
    expect(calls[1]!.url).toBe("elyra://localhost/__mic/cancel");
  });

  it("fails with the shell's reason", async () => {
    stubFetch(() => errorResponse("the user didn't allow the microphone"));
    await expect(microphone.start()).rejects.toMatchObject({
      detail: "the user didn't allow the microphone",
    });
  });
});

describe("onMicrophone", () => {
  it("hears the level and the end of this window's recording", async () => {
    pending = stubPendingFetch();
    const heard: MicrophoneEvent[] = [];
    unsubscribes.push(onMicrophone((e) => heard.push(e)));

    const poll = async () => {
      for (;;) {
        await tick();
        const at = pending.findIndex((p) => p.url.endsWith("/__events"));
        if (at >= 0) return pending.splice(at, 1)[0]!;
      }
    };
    (await poll()).resolve(
      new Response(
        encode([
          ["elyra:microphone", { type: "level", level: 0.4 }],
          ["elyra:microphone", { type: "stopped", file: recording }],
        ]),
        { status: 200 },
      ),
    );
    for (let i = 0; i < 20 && heard.length < 2; i++) await tick();

    expect(heard).toEqual([
      { type: "level", level: 0.4 },
      { type: "stopped", file: recording },
    ]);
  });
});
