// ECharts, as the tests see it: every chart made, with each option it was given.
import { vi } from "vitest";

export const charts = [];

export function reset() {
  charts.length = 0;
}

export const use = vi.fn();
export const init = vi.fn((element, _theme, opts) => {
  const chart = {
    element,
    opts,
    options: [],
    actions: [],
    handlers: {},
    disposed: false,
    setOption(option, how) {
      this.options.push({ option, how });
    },
    on(event, handler) {
      this.handlers[event] = handler;
    },
    dispatchAction(action) {
      this.actions.push(action);
    },
    resize() {},
    dispose() {
      this.disposed = true;
    },
  };
  charts.push(chart);
  return chart;
});
