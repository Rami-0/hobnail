// Geometry for SVG sparklines and health strips. Null breaks a sparkline.
// A strip merges consecutive equal buckets and summarizes the time window.

export interface SparkPaths {
  /** Path data of the line (one M…L run per stretch without gaps). */
  line: string;
  /** The filled area under each stretch. */
  area: string;
}

const f1 = (n: number) => (Math.round(n * 10) / 10).toString();

/**
 * Sparkline paths in a `w` × `h` box, scaled from min(0, lowest) to `max`
 * (or the highest value). Values above `max` are drawn at the top. Null for
 * fewer than two numbers.
 */
export function sparkPaths(values: readonly (number | null)[], max?: number, w = 100, h = 24): SparkPaths | null {
  const nums = values.filter((v): v is number => typeof v === "number" && Number.isFinite(v));
  if (nums.length < 2 || values.length < 2) return null;
  const lo = Math.min(0, ...nums);
  const hi = max !== undefined && Number.isFinite(max) && max > lo ? max : Math.max(...nums);
  const span = hi - lo < 1e-9 ? 1 : hi - lo;
  const n = values.length;
  const x = (i: number) => (i / (n - 1)) * w;
  const y = (v: number) => h - 1 - ((Math.min(v, hi) - lo) / span) * (h - 2);
  let line = "";
  let area = "";
  let run: number[] = [];
  const end = () => {
    if (run.length >= 2) {
      const seg = run.map((i, k) => `${k ? "L" : "M"}${f1(x(i))} ${f1(y(values[i] as number))}`).join("");
      line += seg;
      area += `${seg}L${f1(x(run[run.length - 1] as number))} ${h}L${f1(x(run[0] as number))} ${h}Z`;
    }
    run = [];
  };
  values.forEach((v, i) => {
    if (typeof v === "number" && Number.isFinite(v)) run.push(i);
    else end();
  });
  end();
  return line ? { line, area } : null;
}

/** Health strip bucket codes (GET /api/v1/strips; db::strips::Cell order). */
export const STRIP = { unknown: 0, off: 1, ok: 2, warn: 3, bad: 4 } as const;

/** CSS class per code. */
export function stripClass(code: number): "u" | "f" | "o" | "w" | "b" {
  switch (code) {
    case STRIP.off:
      return "f";
    case STRIP.ok:
      return "o";
    case STRIP.warn:
      return "w";
    case STRIP.bad:
      return "b";
    default:
      return "u";
  }
}

/** Runs of equal buckets as [first, last, code]. */
export function stripRuns(codes: readonly number[]): [number, number, number][] {
  const out: [number, number, number][] = [];
  codes.forEach((c, i) => {
    const last = out[out.length - 1];
    if (last && last[2] === c) last[1] = i;
    else out.push([i, i, c]);
  });
  return out;
}

/** `3d 4h`, `5h 2m`, `7m`, `12s` (util::human_duration). */
export function humanDuration(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const d = Math.floor(s / 86400);
  const h = Math.floor(s / 3600) % 24;
  const m = Math.floor(s / 60) % 60;
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m`;
  return `${s}s`;
}

/** "24 h", "7 d" (ui::view::strip::window_label). */
export function windowLabel(secs: number): string {
  if (secs % 86400 === 0 && secs >= 2 * 86400) return `${secs / 86400} d`;
  return `${Math.floor((secs + 1800) / 3600)} h`;
}

/** "running 98% of the last 24 h · 2 problems · 1 restart" (ui::view::strip::summary). */
export function stripSummary(codes: readonly number[], step: number): string {
  const n = Math.max(1, codes.length);
  const win = windowLabel(codes.length * step);
  const known = codes.filter((c) => c !== STRIP.unknown).length;
  if (known === 0) return `no history in the last ${win}`;
  const up = codes.filter((c) => c === STRIP.ok || c === STRIP.warn || c === STRIP.bad).length;
  const runs = (want: number) => {
    let k = 0;
    let prev: number | null = null;
    for (const c of codes) {
      if (c === want && prev !== want) k++;
      prev = c;
    }
    return k;
  };
  const bad = runs(STRIP.bad);
  const warn = runs(STRIP.warn);
  const p = Math.floor((up * 100 + Math.floor(known / 2)) / known);
  let out =
    known * 2 < n
      ? `running ${p}% of ${humanDuration(Math.floor((codes.length * step * known) / n))} with data`
      : `running ${p}% of the last ${win}`;
  if (bad > 0) out += ` · ${bad} problem${bad === 1 ? "" : "s"}`;
  if (warn > 0) out += ` · ${warn} restart${warn === 1 ? "" : "s"}`;
  return out;
}

/** Threshold of a percentage (ui::shell::level): "warn" from 80, "bad" from 90. */
export function level(pct: number | null | undefined): "" | "warn" | "bad" {
  if (pct === null || pct === undefined || !Number.isFinite(pct)) return "";
  if (pct >= 90) return "bad";
  if (pct >= 80) return "warn";
  return "";
}
