// Every example in the reference (ui/docs/examples/) mounts, with the
// runtime's stand-ins: what the docs show runs.
import { render } from "@testing-library/svelte";
import { expect, it } from "vitest";

const examples = import.meta.glob("../docs/examples/**/*.svelte", { eager: true });

it("has examples", () => {
  expect(Object.keys(examples).length).toBeGreaterThan(0);
});

for (const [path, module] of Object.entries(examples)) {
  it(`${path.replace("../docs/examples/", "")} mounts`, () => {
    const { container } = render(module.default);
    expect(container.innerHTML.trim()).not.toBe("");
  });
}
