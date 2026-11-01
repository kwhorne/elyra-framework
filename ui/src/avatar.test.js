/** Avatar and AvatarGroup: a picture or initials, the same colour for the same name, a count of the rest. */
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import Avatar from "./Avatar.svelte";
import AvatarGroup from "./AvatarGroup.svelte";
import { avatarSize, hue, initials } from "./internal/people.js";

describe("people", () => {
  it("take the first letters of the first and last names", () => {
    expect(initials("Ada Lovelace")).toBe("AL");
    expect(initials("  ada   king  lovelace ")).toBe("AL");
    expect(initials("øyvind")).toBe("Ø");
    expect(initials("Anne-Marie Ødegård")).toBe("AØ");
    expect(initials("😀 Smile")).toBe("😀S");
    expect(initials("")).toBe("?");
    expect(initials(null)).toBe("?");
  });

  it("give the same name the same hue, and spread names apart", () => {
    expect(hue("Ada Lovelace")).toBe(hue("ada lovelace "));
    const hues = ["Ada", "Grace", "Linus", "Margaret", "Ken", "Barbara"].map(hue);
    expect(new Set(hues).size).toBe(hues.length);
    expect(hues.every((h) => h >= 0 && h < 360 && Number.isInteger(h))).toBe(true);
  });

  it("size by name or in pixels", () => {
    expect(avatarSize("sm")).toBe(20);
    expect(avatarSize("lg")).toBe(40);
    expect(avatarSize(64)).toBe(64);
    expect(avatarSize("huge")).toBe(28);
  });
});

describe("Avatar", () => {
  it("shows the picture, named", () => {
    render(Avatar, { name: "Ada Lovelace", src: "ada.png" });
    const img = screen.getByRole("img", { name: "Ada Lovelace" });
    expect(img.tagName).toBe("IMG");
    expect(img.getAttribute("src")).toBe("ada.png");
  });

  it("falls back to the initials, named, when there's no picture or it fails", async () => {
    const { container } = render(Avatar, { name: "Ada Lovelace", src: "gone.png" });
    await fireEvent.error(screen.getByRole("img"));
    const fallback = screen.getByRole("img", { name: "Ada Lovelace" });
    expect(fallback.tagName).toBe("SPAN");
    expect(fallback.textContent.trim()).toBe("AL");
    expect(container.querySelector(".elyra-avatar").style.getPropertyValue("--hue")).toBe(String(hue("Ada Lovelace")));
  });

  it("is sized", () => {
    const { container } = render(Avatar, { name: "Ada", size: "lg" });
    expect(container.querySelector(".elyra-avatar").style.getPropertyValue("--size")).toBe("40px");
  });
});

const PEOPLE = ["Ada Lovelace", "Grace Hopper", "Linus Torvalds", "Margaret Hamilton", "Ken Thompson", "Barbara Liskov"].map(
  (name) => ({ name }),
);

describe("AvatarGroup", () => {
  it("shows everyone when they fit", () => {
    render(AvatarGroup, { people: PEOPLE.slice(0, 4), max: 4, label: "Viewing" });
    const group = screen.getByRole("group", { name: "Viewing" });
    expect([...group.querySelectorAll("[role=img]")].map((a) => a.getAttribute("aria-label"))).toEqual(
      PEOPLE.slice(0, 4).map((p) => p.name),
    );
  });

  it("counts the rest in the last place, naming them", () => {
    render(AvatarGroup, { people: PEOPLE, max: 4 });
    const group = screen.getByRole("group", { name: "People" });
    const imgs = [...group.querySelectorAll("[role=img]")];
    expect(imgs).toHaveLength(4);
    const more = imgs.at(-1);
    expect(more.textContent.trim()).toBe("+3");
    expect(more.getAttribute("aria-label")).toBe("3 more: Margaret Hamilton, Ken Thompson, Barbara Liskov");
    expect(more.parentElement.getAttribute("title")).toBe("Margaret Hamilton, Ken Thompson, Barbara Liskov");
  });

  it("shows nothing for no one", () => {
    render(AvatarGroup, { people: [] });
    expect(screen.getByRole("group").children).toHaveLength(0);
  });
});
