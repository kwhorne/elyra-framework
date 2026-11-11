/** Microphone (RFC 0032): click, hold to talk, Esc, the text, the level, and what's said aloud. */
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { RECORDING, micEvent, micListeners, microphone } from "./test/runtime.js";
import Microphone from "./Microphone.svelte";

const button = () => screen.getByRole("button", { name: "Dictate" });
const status = () => document.querySelector(".status");

describe("Microphone", () => {
  it("is a toggle button with a name, off until it records", () => {
    render(Microphone);
    expect(button().getAttribute("aria-pressed")).toBe("false");
    expect(button().getAttribute("title")).toBe("Click, or hold Space, to talk");
  });

  it("records on a click, and on the next one hands over the text", async () => {
    const ontext = vi.fn();
    const onrecorded = vi.fn();
    render(Microphone, { ontext, onrecorded, language: "no", maxSeconds: 60 });
    await userEvent.click(button());
    expect(microphone.start).toHaveBeenCalledWith({ maxSeconds: 60 });
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    expect(status().textContent).toBe("Recording");

    await userEvent.click(button());
    expect(microphone.stop).toHaveBeenCalled();
    expect(onrecorded).toHaveBeenCalledWith(RECORDING);
    expect(microphone.transcribe).toHaveBeenCalledWith(RECORDING, { language: "no" });
    await vi.waitFor(() => expect(ontext).toHaveBeenCalledWith("Send a reminder on these."));
    expect(button().getAttribute("aria-pressed")).toBe("false");
  });

  it("is busy while it transcribes", async () => {
    let answer;
    microphone.transcribe.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    render(Microphone);
    await userEvent.click(button());
    await userEvent.click(button());
    await vi.waitFor(() => expect(button().getAttribute("aria-busy")).toBe("true"));
    expect(button().disabled).toBe(true);
    expect(status().textContent).toBe("Transcribing…");
    answer("Done.");
    await vi.waitFor(() => expect(button().disabled).toBe(false));
  });

  it("with transcribe off, stops at the recording", async () => {
    const onrecorded = vi.fn();
    render(Microphone, { transcribe: false, onrecorded });
    await userEvent.click(button());
    await userEvent.click(button());
    await vi.waitFor(() => expect(onrecorded).toHaveBeenCalledWith(RECORDING));
    expect(microphone.transcribe).not.toHaveBeenCalled();
  });

  it("held down past 300 ms, records until let go", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      const ontext = vi.fn();
      render(Microphone, { ontext });
      button().focus();
      const user = userEvent.setup();
      await user.keyboard("[Space>]");
      expect(microphone.start).toHaveBeenCalledTimes(1);
      vi.setSystemTime(Date.now() + 500);
      await user.keyboard("[/Space]");
      await vi.waitFor(() => expect(microphone.stop).toHaveBeenCalledTimes(1));
      await vi.waitFor(() => expect(ontext).toHaveBeenCalled());
    } finally {
      vi.useRealTimers();
    }
  });

  it("pressed briefly, stays on like a click", async () => {
    render(Microphone);
    button().focus();
    await userEvent.keyboard("{Enter}");
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    expect(microphone.stop).not.toHaveBeenCalled();
    // Pressed again: it stops.
    await userEvent.keyboard("{Enter}");
    await vi.waitFor(() => expect(microphone.stop).toHaveBeenCalledTimes(1));
    expect(microphone.start).toHaveBeenCalledTimes(1);
  });

  it("Esc throws the recording away", async () => {
    const ontext = vi.fn();
    render(Microphone, { ontext });
    await userEvent.click(button());
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    await userEvent.keyboard("{Escape}");
    await vi.waitFor(() => expect(microphone.cancel).toHaveBeenCalled());
    expect(microphone.stop).not.toHaveBeenCalled();
    expect(ontext).not.toHaveBeenCalled();
    expect(button().getAttribute("aria-pressed")).toBe("false");
  });

  it("shows the level, and takes the recording that stopped by itself", async () => {
    const ontext = vi.fn();
    render(Microphone, { ontext });
    await userEvent.click(button());
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    micEvent({ type: "level", level: 0.5 });
    await vi.waitFor(() =>
      expect(document.querySelector(".meter span").getAttribute("style")).toContain("scaleX(0.5)"),
    );
    micEvent({ type: "stopped", file: RECORDING });
    await vi.waitFor(() => expect(ontext).toHaveBeenCalledWith("Send a reminder on these."));
    expect(microphone.stop).not.toHaveBeenCalled();
    expect(document.querySelector(".meter")).toBeNull();
  });

  it("says why it can't record, as an alert", async () => {
    microphone.start.mockRejectedValue({ detail: "the user didn't allow the microphone" });
    render(Microphone);
    await userEvent.click(button());
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toBe("the user didn't allow the microphone"));
    expect(button().getAttribute("aria-pressed")).toBe("false");
  });

  it("says when the input failed while recording", async () => {
    render(Microphone);
    await userEvent.click(button());
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    micEvent({ type: "failed", message: "the microphone stopped: unplugged" });
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toBe("the microphone stopped: unplugged"));
  });

  it("takes its words from labels", async () => {
    render(Microphone, {
      labels: { button: "Dikter", recording: "Tar opp", hint: "Klikk, eller hold mellomrom, for å snakke" },
    });
    const own = screen.getByRole("button", { name: "Dikter" });
    expect(own.getAttribute("title")).toBe("Klikk, eller hold mellomrom, for å snakke");
    await userEvent.click(own);
    await vi.waitFor(() => expect(status().textContent).toBe("Tar opp"));
  });

  it("does nothing disabled, and lets go of the microphone when it's gone", async () => {
    const { unmount } = render(Microphone, { disabled: true });
    await userEvent.click(button());
    expect(microphone.start).not.toHaveBeenCalled();
    expect(micListeners()).toBe(1);
    unmount();
    expect(micListeners()).toBe(0);
  });

  it("cancels a recording when it's taken off the page", async () => {
    const { unmount } = render(Microphone);
    await userEvent.click(button());
    await vi.waitFor(() => expect(button().getAttribute("aria-pressed")).toBe("true"));
    unmount();
    await vi.waitFor(() => expect(microphone.cancel).toHaveBeenCalled());
  });
});
