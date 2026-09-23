/**
 * The router: parsing, matching and precedence are pure; the `route` store and
 * `navigate` run against a fake window built on Node's own EventTarget.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { parseRoute, href, matchRoute, resolveRoute, route, navigate, deepLinkPath } =
  await import("./index.js");

/** A window whose `location.hash` fires `hashchange`, like a browser's. */
function fakeWindow(initial = "") {
  const target = new EventTarget();
  let hash = initial;
  const location = {
    get hash() {
      return hash;
    },
    set hash(value: string) {
      const next = value.startsWith("#") ? value : `#${value}`;
      if (next === hash) return;
      hash = next;
      target.dispatchEvent(new Event("hashchange"));
    },
  };
  const history = {
    entries: [] as string[],
    replaceState(_d: unknown, _u: string, url?: string) {
      hash = url ?? hash;
      this.entries.push(`replace:${url}`);
    },
  };
  vi.stubGlobal("location", location);
  vi.stubGlobal("history", history);
  vi.stubGlobal("addEventListener", target.addEventListener.bind(target));
  vi.stubGlobal("removeEventListener", target.removeEventListener.bind(target));
  return { location, history, target };
}

afterEach(() => vi.unstubAllGlobals());

describe("parsing and hrefs", () => {
  it("reads the path and query from the hash", () => {
    expect(parseRoute("")).toEqual({ path: "/", query: {} });
    expect(parseRoute("#/")).toEqual({ path: "/", query: {} });
    expect(parseRoute("#/customers/12?tab=notes&page=2")).toEqual({
      path: "/customers/12",
      query: { tab: "notes", page: "2" },
    });
    // Doubled and trailing slashes normalise; segments are decoded.
    expect(parseRoute("#//customers//Ada%20L/").path).toBe("/customers/Ada L");
  });

  it("builds hrefs that round-trip", () => {
    expect(href("/customers")).toBe("#/customers");
    expect(href("customers/12", { tab: "notes" })).toBe("#/customers/12?tab=notes");
    expect(parseRoute(href("/files/a b", { q: "x&y" }))).toEqual({
      path: "/files/a b",
      query: { q: "x&y" },
    });
  });
});

describe("matching", () => {
  it("captures :params and a trailing *", () => {
    expect(matchRoute("/customers/:id", "/customers/12")).toEqual({ id: "12" });
    expect(matchRoute("/customers/:id/edit", "/customers/12/edit")).toEqual({ id: "12" });
    expect(matchRoute("/customers/:id", "/customers")).toBeNull();
    expect(matchRoute("/customers/:id", "/customers/12/edit")).toBeNull();
    expect(matchRoute("/files/*", "/files/a/b/c")).toEqual({ "*": "a/b/c" });
    expect(matchRoute("/", "/")).toEqual({});
  });

  it("prefers the most specific pattern, whatever the table order", () => {
    const routes = { "/customers/:id": "show", "/*": "notFound", "/customers/new": "create", "/": "home" };
    expect(resolveRoute(routes, "/customers/new")?.value).toBe("create");
    expect(resolveRoute(routes, "/customers/12")).toEqual({
      pattern: "/customers/:id",
      value: "show",
      params: { id: "12" },
    });
    expect(resolveRoute(routes, "/")?.value).toBe("home");
    expect(resolveRoute(routes, "/nope/deep")?.value).toBe("notFound");
    expect(resolveRoute({ "/a": 1 }, "/b")).toBeNull();
    // A wildcard deeper in the tree still loses to an exact match there.
    const nested = { "/docs/*": "any", "/docs/intro": "intro", "/docs/:page": "page" };
    expect(resolveRoute(nested, "/docs/intro")?.value).toBe("intro");
    expect(resolveRoute(nested, "/docs/setup")?.value).toBe("page");
    expect(resolveRoute(nested, "/docs/a/b")?.value).toBe("any");
  });
});

describe("the route store and navigate", () => {
  beforeEach(() => fakeWindow("#/"));

  it("emits the current route, then every navigation", () => {
    const seen: string[] = [];
    const stop = route.subscribe((r) => seen.push(r.path));
    navigate("/customers");
    navigate("/customers/12", { query: { tab: "notes" } });
    stop();
    navigate("/after-unsubscribe");
    expect(seen).toEqual(["/", "/customers", "/customers/12"]);
  });

  it("replace swaps the history entry and still notifies", () => {
    const { history } = fakeWindow("#/login");
    const seen: string[] = [];
    const stop = route.subscribe((r) => seen.push(r.path));
    navigate("/dashboard", { replace: true });
    stop();
    expect(history.entries).toEqual(["replace:#/dashboard"]);
    expect(seen).toEqual(["/login", "/dashboard"]);
  });

  it("follows a hand-edited hash (back/forward fire the same event)", () => {
    const { location } = fakeWindow("#/");
    const seen: string[] = [];
    const stop = route.subscribe((r) => seen.push(r.path));
    location.hash = "#/settings";
    stop();
    expect(seen).toEqual(["/", "/settings"]);
  });

  it("stops listening when the last subscriber leaves", () => {
    const { target } = fakeWindow("#/");
    const remove = vi.fn(target.removeEventListener.bind(target));
    vi.stubGlobal("removeEventListener", remove);
    const a = route.subscribe(() => {});
    const b = route.subscribe(() => {});
    a();
    expect(remove).not.toHaveBeenCalled();
    b();
    expect(remove).toHaveBeenCalledWith("hashchange", expect.any(Function));
  });
});

describe("without a window", () => {
  it("is inert rather than throwing (SSR, tests, workers)", () => {
    const seen: string[] = [];
    const stop = route.subscribe((r) => seen.push(r.path));
    navigate("/anywhere");
    stop();
    expect(seen).toEqual(["/"]);
  });
});

describe("deep links", () => {
  it("maps a custom-scheme URL onto a router path", () => {
    expect(deepLinkPath("myapp://customers/12?tab=notes")).toBe("/customers/12?tab=notes");
    expect(deepLinkPath("myapp:customers")).toBe("/customers");
    expect(deepLinkPath("myapp://")).toBe("/");
  });
});
