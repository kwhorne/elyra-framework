/** Presence: the other members of a channel, as avatars, joined while on screen. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import Presence from "./Presence.svelte";
import { presenceJoined, setPresence } from "./test/runtime.js";

const member = (id, name, avatar) => ({ id, info: avatar ? { name, avatar } : name ? { name } : {} });

describe("Presence", () => {
  it("shows the others in the channel, not the user, named", async () => {
    render(Presence, { channel: "customers.42" });
    expect(presenceJoined).toEqual(["customers.42"]);
    setPresence("customers.42", {
      joined: true,
      me: "7",
      members: [member("7", "Grete"), member("9", "Ada Lovelace", "/ada.png"), member("12", "Linus")],
    });
    await tick();
    const group = screen.getByRole("group", { name: "Also here" });
    const names = [...group.querySelectorAll("[role=img], img")].map((a) => a.getAttribute("aria-label") ?? a.getAttribute("alt"));
    expect(names).toEqual(["Ada Lovelace", "Linus"]);
    expect(group.querySelector("img").getAttribute("src")).toBe("/ada.png");
  });

  it("shows nothing while nobody else is there, or it isn't joined", async () => {
    render(Presence, { channel: "customers.43" });
    setPresence("customers.43", { joined: true, me: "7", members: [member("7", "Grete")] });
    await tick();
    expect(screen.queryByRole("group")).toBeNull();
    // Offline: what was there isn't shown.
    setPresence("customers.43", { joined: false, me: "7", members: [member("9", "Ada")] });
    await tick();
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("shows the user too with self, names someone without a name by id, and counts past max", async () => {
    render(Presence, { channel: "customers.44", self: true, max: 3, label: "Viewing" });
    setPresence("customers.44", {
      joined: true,
      me: "7",
      members: [member("7", "Grete"), member("9"), member("10", "B"), member("11", "C")],
    });
    await tick();
    const group = screen.getByRole("group", { name: "Viewing" });
    const labels = [...group.querySelectorAll("[role=img]")].map((a) => a.getAttribute("aria-label"));
    expect(labels).toEqual(["Grete", "#9", "2 more: B, C"]);
  });

  it("follows its channel when it changes", async () => {
    const { rerender } = render(Presence, { channel: "customers.45" });
    await rerender({ channel: "customers.46" });
    expect(presenceJoined).toEqual(["customers.45", "customers.46"]);
    setPresence("customers.45", { joined: true, me: "7", members: [member("9", "Ada")] });
    await tick();
    expect(screen.queryByRole("group")).toBeNull();
    setPresence("customers.46", { joined: true, me: "7", members: [member("9", "Ada")] });
    await tick();
    expect(screen.getByRole("group")).toBeTruthy();
  });
});
