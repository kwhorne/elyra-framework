// The shell's system calls, as the export tests see them (RFC 0060).
import { vi } from "vitest";

export const state = { system: true };
export const paths = vi.fn(async () => {
  if (!state.system) throw new Error("no system feature");
  return { temp: "/tmp" };
});
export const files = { save: vi.fn(async () => "/tmp/chart.png") };
export const clipboard = { writeImage: vi.fn(async () => {}), writeText: vi.fn(async () => {}) };
export const toast = vi.fn(() => ({ dismiss() {} }));

export function reset() {
  state.system = true;
  paths.mockClear();
  files.save.mockClear();
  clipboard.writeImage.mockClear();
  clipboard.writeText.mockClear();
  toast.mockClear();
}
