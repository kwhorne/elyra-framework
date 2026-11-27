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
    getWidth: () => 400,
    getHeight: () => 300,
    renderToSVGString: () => '<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><path d="M0 0"/></svg>',
    getDataURL: () => "data:image/png;base64,iVBORw0KGgo=",
    dispose() {
      this.disposed = true;
    },
  };
  charts.push(chart);
  return chart;
});
