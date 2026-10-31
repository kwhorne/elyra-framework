// The gallery runs in a browser, without the Elyra shell: the user's
// abilities are a fixed list (everything but deleting customers), and the
// rest is the real @elyra/runtime.
export * from "../../runtime/dist/index.js";

const granted = ["customers.view", "customers.create", "customers.update", "customers.export"];

export const can = {
  subscribe(run) {
    run((ability) => granted.includes(ability));
    return () => {};
  },
};
