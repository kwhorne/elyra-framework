/** Presence with `follow` (RFC 0042): ask to follow, follow, and stop; being followed. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import Presence from "./Presence.svelte";
import { followAsked, followers, following, setPresence } from "./test/runtime.js";

const here = (channel) =>
  setPresence(channel, {
    joined: true,
    me: "7",
    members: [
      { id: "7", info: { name: "Grete" }, editing: false },
      { id: "9", info: { name: "Ada" }, editing: false },
    ],
  });

describe("Presence follow", () => {
  it("asks the one chosen in Follow…", async () => {
    here("lobby");
    render(Presence, { channel: "lobby", follow: true });
    await userEvent.click(screen.getByRole("button", { name: "Follow…" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Ada" }));
    expect(followAsked).toContainEqual(["lobby", "9"]);
  });

  it("says whom it follows, with Stop, and where they can't be followed", async () => {
    here("office");
    following.set({ channel: "office", leader: "9", allowed: true });
    render(Presence, { channel: "office", follow: true });
    expect(await screen.findByText("Following Ada")).toBeTruthy();
    following.set({ channel: "office", leader: "9", allowed: true, away: true });
    expect(await screen.findByText("Ada is somewhere you can't follow")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(following.stop).toHaveBeenCalled();
  });

  it("says who follows the user, with Stop", async () => {
    here("room");
    following.set(null);
    followers.set([{ channel: "room", id: "9" }]);
    render(Presence, { channel: "room", follow: true });
    expect(await screen.findByText("Ada is following you")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(following.stopFollowers).toHaveBeenCalled();
  });

  it("offers nothing without follow", () => {
    here("quiet");
    render(Presence, { channel: "quiet" });
    expect(screen.queryByRole("button", { name: "Follow…" })).toBeNull();
  });
});
