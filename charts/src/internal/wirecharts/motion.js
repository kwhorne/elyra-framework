// The WireCharts charts that move (RFC 0059), as WireCharts' Alpine glue
// moves them: the clock ticks, the races draw their points one by one, and
// the audio chart plays its series as pitch. Each returns how to stop.

const reduced = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The clock's hands at `now`, as WireCharts sets them. */
export function hands(now = new Date()) {
  const seconds = now.getSeconds();
  const minutes = now.getMinutes() + seconds / 60;
  const hours = (now.getHours() % 12) + minutes / 60;
  return [hours, minutes, seconds];
}

/** Tick every second; not while the window is hidden. */
export function clock(chart) {
  const tick = () => {
    if (typeof document !== "undefined" && document.hidden) return;
    chart.setOption({ series: hands().map((value) => ({ data: [{ value }] })) });
  };
  tick();
  const timer = setInterval(tick, 1000);
  return () => clearInterval(timer);
}

/** Reveal each series' points one by one, every `interval` ms; at once under reduced motion. */
export function race(chart, option, interval = 400) {
  let timer = null;
  const full = (option.series ?? []).map((s) => (Array.isArray(s.data) ? s.data.slice() : []));
  const longest = full.reduce((m, d) => Math.max(m, d.length), 0);
  const show = (step) => chart.setOption({ series: full.map((data) => ({ data: data.slice(0, step) })) });
  const play = () => {
    clearInterval(timer);
    if (reduced()) return show(longest);
    let step = 1;
    show(step);
    timer = setInterval(() => {
      step++;
      show(step);
      if (step >= longest) clearInterval(timer);
    }, interval);
  };
  play();
  return { play, stop: () => clearInterval(timer) };
}

/** A series' values, as the audio chart plays them. */
export function soundValues(option, track = 0) {
  return (option.series?.[track]?.data ?? [])
    .map((d) => (Array.isArray(d) ? (d[1] ?? d[0]) : d && typeof d === "object" ? d.value : d))
    .filter((v) => typeof v === "number");
}

/**
 * Play the series as pitch (higher value, higher note) over `duration` ms,
 * the tooltip following along; `done` is told when it ends.
 */
export function play(chart, option, audio, done) {
  const values = soundValues(option, audio.track ?? 0);
  if (values.length === 0) return () => {};
  const Context = globalThis.AudioContext ?? globalThis.webkitAudioContext;
  const ctx = Context ? new Context() : null;
  const duration = audio.duration ?? 5000;
  const step = duration / values.length;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const low = audio.minFreq ?? 220;
  const high = audio.maxFreq ?? 880;
  if (ctx) {
    const start = ctx.currentTime + 0.05;
    values.forEach((value, i) => {
      const t = start + (i * step) / 1000;
      const norm = max === min ? 0.5 : (value - min) / (max - min);
      const length = Math.min(step / 1000, 0.3);
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = audio.instrument || "sine";
      osc.frequency.value = low + norm * (high - low);
      osc.connect(gain);
      gain.connect(ctx.destination);
      gain.gain.setValueAtTime(0.0001, t);
      gain.gain.exponentialRampToValueAtTime(0.2, t + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + length);
      osc.start(t);
      osc.stop(t + length + 0.02);
    });
  }
  let i = 0;
  const stop = () => {
    clearInterval(timer);
    chart.dispatchAction({ type: "hideTip" });
    ctx?.close?.();
    done?.();
  };
  const timer = setInterval(() => {
    if (i >= values.length) return stop();
    chart.dispatchAction({ type: "showTip", seriesIndex: audio.track ?? 0, dataIndex: i });
    i++;
  }, step);
  return stop;
}
