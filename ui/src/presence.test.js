/** Presence: the other members of a channel, as avatars, joined while on screen. */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import Presence from "./Presence.svelte";
import { presenceEditing, presenceJoined, setPresence } from "./test/runtime.js";

const member = (id, name, avatar) => ({ id, info: avatar ? { name, avatar } : name ? { name } : {}, editing: false });
const editing = (m) => ({ ...m, editing: true });

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

  it("says who else is editing, and rings them", async () => {
    render(Presence, { channel: "customers.60" });
    setPresence("customers.60", {
      joined: true,
      me: "7",
      members: [editing(member("7", "Grete")), editing(member("9", "Ada")), member("12", "Linus")],
    });
    await tick();
    // The user editing isn't news to them.
    const status = document.querySelector("[aria-live=polite]");
    expect(status.textContent.trim()).toBe("✎ Ada is editing");
    const rings = [...screen.getByRole("group").querySelectorAll(".ring")];
    expect(rings.map((r) => r.getAttribute("title"))).toEqual(["Ada (editing)", "Linus"]);
    expect(rings.map((r) => r.classList.contains("noted"))).toEqual([true, false]);

    setPresence("customers.60", {
      joined: true,
      me: "7",
      members: [member("7", "Grete"), editing(member("9", "Ada")), editing(member("12", "Linus"))],
    });
    await tick();
    expect(status.textContent.trim()).toBe("✎ Ada and Linus are editing");
    setPresence("customers.60", {
      joined: true,
      me: "7",
      members: [editing(member("9", "Ada")), editing(member("12", "Linus")), editing(member("13"))],
    });
    await tick();
    expect(status.textContent.trim()).toBe("✎ 3 people are editing");
    setPresence("customers.60", { joined: true, me: "7", members: [member("9", "Ada")] });
    await tick();
    expect(status.textContent.trim()).toBe("");
    expect(screen.getByRole("group").querySelector(".noted")).toBeNull();
  });

  it("takes its words from labels", async () => {
    render(Presence, {
      channel: "customers.61",
      labels: { editing: (names) => `${names.join(" og ")} redigerer`, note: "redigerer" },
    });
    setPresence("customers.61", { joined: true, me: "7", members: [editing(member("9", "Ada"))] });
    await tick();
    expect(document.querySelector("[aria-live=polite]").textContent.trim()).toBe("✎ Ada redigerer");
    expect(screen.getByRole("group").querySelector(".ring").getAttribute("title")).toBe("Ada (redigerer)");
  });

  it("tells the channel when the user is editing, on the channel it shows", async () => {
    const { rerender } = render(Presence, { channel: "customers.62" });
    expect(presenceEditing).toEqual([["customers.62", false]]);
    await rerender({ channel: "customers.62", editing: true });
    await rerender({ channel: "customers.62", editing: false });
    await rerender({ channel: "customers.62", editing: true });
    // A new channel: told there (the old one's store stops when it goes).
    await rerender({ channel: "customers.63", editing: true });
    expect(presenceEditing).toEqual([
      ["customers.62", false],
      ["customers.62", true],
      ["customers.62", false],
      ["customers.62", true],
      ["customers.63", true],
    ]);
    expect(presenceJoined).toEqual(["customers.62", "customers.63"]);
  });
});
