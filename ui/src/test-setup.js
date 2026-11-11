import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/svelte";

// The runtime's stores talk to the shell; the components' tests stand in for
// them (src/test/runtime.js), keeping the runtime's own validationErrors.
vi.mock("@elyra/runtime", async (importOriginal) => {
  const real = await importOriginal();
  const fake = await import("./test/runtime.js");
  return {
    ...real,
    can: fake.can,
    confirm: fake.confirm,
    href: fake.href,
    route: fake.route,
    connection: fake.connection,
    outbox: fake.outbox,
    transfers: fake.transfers,
    assistant: fake.assistant,
    openAssistant: fake.openAssistant,
    dialog: fake.dialog,
    onFileDrop: fake.onFileDrop,
    microphone: fake.microphone,
    onMicrophone: fake.onMicrophone,
    store: fake.store,
    abilities: fake.abilities,
    inspector: fake.inspector,
    openInspector: fake.openInspector,
    presence: fake.presence,
    notifications: fake.notifications,
    navigate: fake.navigate,
    toast: fake.toast,
    accounts: fake.accounts,
    auth: fake.auth,
  };
});

beforeEach(async () => {
  const fake = await import("./test/runtime.js");
  fake.grant(["*"]);
  fake.confirm.mockReset();
  fake.confirm.mockResolvedValue(true);
  fake.resetStores();
  fake.resetMicrophone();
});

afterEach(() => cleanup());
