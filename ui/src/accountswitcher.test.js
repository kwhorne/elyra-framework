/** AccountSwitcher: who the user is where, switching, signing out, and the changes that haven't been sent. */
import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import AccountSwitcher from "./AccountSwitcher.svelte";
import { accounts, auth, confirm, toast } from "./test/runtime.js";

const ACME = { server: "https://acme.example.com", userId: "7", user: { name: "Ada Lovelace", avatar_url: "/ada.png" }, active: true };
const KUNDE = { server: "https://kunde.example.com:8443", userId: "41", user: { email: "ada@kunde.no" }, active: false };
const BARE = { server: "https://third.example.com", userId: "3", user: null, active: false };

const signedIn = (...list) => accounts.set({ accounts: list, active: list.find((a) => a.active)?.server ?? null, signedOutOf: null, reason: null });
const pending = () => Object.assign(new Error("2 change(s) haven't been sent yet"), { kind: "pending_changes" });

async function openMenu(user) {
  await user.click(screen.getByRole("button", { expanded: false }));
  return screen.getByRole("menu");
}

describe("AccountSwitcher", () => {
  it("shows nothing while the user isn't signed in", () => {
    const { container } = render(AccountSwitcher);
    expect(container.querySelector("button")).toBeNull();
  });

  it("shows who the user is on the active server, and the server when there's more than one", async () => {
    signedIn(ACME);
    render(AccountSwitcher);
    const button = screen.getByRole("button", { name: "Ada Lovelace" });
    expect(button.getAttribute("aria-haspopup")).toBe("menu");
    expect(button.querySelector("img").getAttribute("src")).toBe("/ada.png");
    signedIn(ACME, KUNDE);
    await tick();
    expect(screen.getByRole("button", { name: "Ada Lovelace acme.example.com" })).toBeTruthy();
  });

  it("lists each server, the active one checked, named by the user or the server", async () => {
    const user = userEvent.setup();
    signedIn(ACME, KUNDE, BARE);
    render(AccountSwitcher);
    const menu = await openMenu(user);
    const radios = within(menu).getAllByRole("menuitemradio");
    expect(radios.map((r) => r.textContent.replace(/\s+/g, " ").trim())).toEqual([
      "Ada Lovelace acme.example.com",
      "A ada@kunde.no kunde.example.com:8443",
      "T third.example.com third.example.com",
    ]);
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["true", "false", "false"]);
    expect(within(menu).getByRole("menuitem", { name: "Sign out of acme.example.com" })).toBeTruthy();
    expect(within(menu).getByRole("menuitem", { name: "Sign out of all" })).toBeTruthy();
    // The keyboard reaches the accounts as it does the rest.
    expect(document.activeElement).toBe(radios[0]);
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(radios[1]);
  });

  it("switches to another server, and does nothing for the active one", async () => {
    const user = userEvent.setup();
    signedIn(ACME, KUNDE);
    render(AccountSwitcher);
    await user.click(within(await openMenu(user)).getByRole("menuitemradio", { name: /kunde/ }));
    expect(auth.switchTo).toHaveBeenCalledWith("https://kunde.example.com:8443");
    expect(screen.queryByRole("menu")).toBeNull();
    await user.click(within(await openMenu(user)).getByRole("menuitemradio", { name: /acme/ }));
    expect(auth.switchTo).toHaveBeenCalledTimes(1);
  });

  it("signs out of the active server, and of all", async () => {
    const user = userEvent.setup();
    signedIn(ACME, KUNDE);
    render(AccountSwitcher);
    await user.click(within(await openMenu(user)).getByRole("menuitem", { name: "Sign out of acme.example.com" }));
    expect(auth.signOutOf).toHaveBeenCalledWith("https://acme.example.com");
    await user.click(within(await openMenu(user)).getByRole("menuitem", { name: "Sign out of all" }));
    expect(auth.signOutAll).toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it("asks before dropping changes that haven't been sent, and drops them only if the user says so", async () => {
    const user = userEvent.setup();
    signedIn(ACME);
    auth.signOutOf.mockRejectedValue(pending());
    render(AccountSwitcher);
    confirm.mockResolvedValueOnce(false);
    await user.click(within(await openMenu(user)).getByRole("menuitem", { name: "Sign out of acme.example.com" }));
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    expect(confirm.mock.calls[0]).toEqual([
      "Changes made on acme.example.com haven't been sent yet. Sign out and discard them?",
      { title: "Sign out of acme.example.com", confirmLabel: "Sign out and discard", danger: true },
    ]);
    expect(auth.signOutOfDiscarding).not.toHaveBeenCalled();

    await user.click(within(await openMenu(user)).getByRole("menuitem", { name: "Sign out of acme.example.com" }));
    await waitFor(() => expect(auth.signOutOfDiscarding).toHaveBeenCalledWith("https://acme.example.com"));
  });

  it("asks the same before signing out of all", async () => {
    const user = userEvent.setup();
    signedIn(ACME, KUNDE);
    auth.signOutAll.mockRejectedValue(pending());
    render(AccountSwitcher);
    await user.click(within(await openMenu(user)).getByRole("menuitem", { name: "Sign out of all" }));
    await waitFor(() => expect(auth.signOutAllDiscarding).toHaveBeenCalled());
  });

  it("says what went wrong in a toast", async () => {
    const user = userEvent.setup();
    signedIn(ACME, KUNDE);
    auth.switchTo.mockRejectedValue(new Error("Not signed in to that server"));
    render(AccountSwitcher);
    await user.click(within(await openMenu(user)).getByRole("menuitemradio", { name: /kunde/ }));
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Not signed in to that server", { variant: "error" }));
  });

  it("offers to add an account with onadd, and leaves Sign out of all off with one server", async () => {
    const user = userEvent.setup();
    const onadd = vi.fn();
    signedIn(ACME);
    render(AccountSwitcher, { onadd });
    const menu = await openMenu(user);
    expect(within(menu).queryByRole("menuitem", { name: "Sign out of all" })).toBeNull();
    await user.click(within(menu).getByRole("menuitem", { name: "Add an account…" }));
    expect(onadd).toHaveBeenCalled();
  });

  it("takes the name and picture from person, and its words from labels", async () => {
    const user = userEvent.setup();
    signedIn(ACME);
    render(AccountSwitcher, {
      person: (a) => ({ name: `Ada (${a.userId})` }),
      labels: { signOut: (host) => `Logg ut av ${host}` },
    });
    const menu = await openMenu(user);
    expect(screen.getByRole("button", { name: "Ada (7)" })).toBeTruthy();
    expect(within(menu).getByRole("menuitem", { name: "Logg ut av acme.example.com" })).toBeTruthy();
  });
});
