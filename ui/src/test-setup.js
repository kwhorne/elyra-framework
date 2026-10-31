import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/svelte";

// The runtime's stores talk to the shell; the components' tests stand in for
// them (src/test/runtime.js), keeping the runtime's own validationErrors.
vi.mock("@elyra/runtime", async (importOriginal) => {
  const real = await importOriginal();
  const fake = await import("./test/runtime.js");
  return { ...real, can: fake.can, confirm: fake.confirm, href: fake.href };
});

beforeEach(async () => {
  const fake = await import("./test/runtime.js");
  fake.grant(["*"]);
  fake.confirm.mockReset();
  fake.confirm.mockResolvedValue(true);
});

afterEach(() => cleanup());
