// Pure chart logic (no Recharts, no DOM): series.test.ts checks it.
// - `chartRows`: the CPU and memory series on one shared time axis (so the
//   two charts' crosshairs line up index for index), with explicit empty
//   rows wherever consecutive points are more than 1.5 × step apart: Recharts
//   breaks the line at a null and the tooltip there says "no data".
// - `ticks`: local-time axis ticks on whole minutes, hours or days.
// - `chartMarkers`: the events worth marking (crash, OOM, failing health
//   check, fired alert), folded when
//   closer than 1.5 % of the width (`cluster_markers`).
// - `zoomTo`: a drag in pixels as a `from`/`to` window, or null for a tap.
import type { EventRow } from "../api/gen/EventRow.ts";
import type { SeriesSet } from "../api/gen/SeriesSet.ts";

export interface ChartRow {
  t: number;
  cpu: number | null;
  mem: number | null;
  /** [average, bucket maximum] for the band, or null. */
  cpuBand: [number, number] | null;
  memBand: [number, number] | null;
}

/** Most rows a chart gets: a long gap is filled with empty rows only up to this. */
export const MAX_ROWS = 600;
/** Most empty rows the gaps of one window get (see chartRows). */
export const FILL_ROWS = 120;

type Pts = readonly (readonly [number, number])[];

const band = (avg: number | undefined, max: number | undefined): [number, number] | null =>
  avg !== undefined && max !== undefined ? [Math.min(avg, max), max] : null;
const empty = (t: number): ChartRow => ({ t, cpu: null, mem: null, cpuBand: null, memBand: null });

function toMap(p: Pts | null | undefined): Map<number, number> {
  const m = new Map<number, number>();
  for (const [t, v] of p ?? []) if (Number.isFinite(t) && Number.isFinite(v)) m.set(t, v);
  return m;
}

/**
 * Both series on one axis. A row per timestamp that either series has;
 * between two timestamps more than 1.5 × step apart, empty rows every step
 * (one in the middle when that would be too many), so a hover inside a gap
 * reads "no data" and the lines break there.
 */
export function chartRows(s: Pick<SeriesSet, "step" | "cpu" | "mem" | "cpu_max" | "mem_max">): ChartRow[] {
  const cpu = toMap(s.cpu);
  const mem = toMap(s.mem);
  const cpuMax = toMap(s.cpu_max);
  const memMax = toMap(s.mem_max);
  const times = [...new Set([...cpu.keys(), ...mem.keys()])].toSorted((a, b) => a - b);
  const step = s.step > 0 ? s.step : medianStep(times);
  const row = (t: number): ChartRow => ({
    t,
    cpu: cpu.get(t) ?? null,
    mem: mem.get(t) ?? null,
    cpuBand: band(cpu.get(t), cpuMax.get(t)),
    memBand: band(mem.get(t), memMax.get(t)),
  });
  // Empty rows go every `fill` seconds: the step, or coarser when the
  // window would need more than FILL_ROWS of them (a chart's first render
  // costs per row; a "no data" read-out needs no finer grid than that).
  const first = times[0] ?? 0;
  const last = times[times.length - 1] ?? 0;
  const fill = Math.max(step, (last - first) / FILL_ROWS);
  let filler = 0;
  for (let i = 1; i < times.length; i++) {
    const d = (times[i] as number) - (times[i - 1] as number);
    if (d > 1.5 * step) filler += Math.max(1, Math.floor(d / fill) - 1);
  }
  const dense = times.length + filler <= MAX_ROWS;
  const out: ChartRow[] = [];
  for (let i = 0; i < times.length; i++) {
    const t = times[i] as number;
    if (i > 0) {
      const p = times[i - 1] as number;
      if (t - p > 1.5 * step) {
        if (dense) {
          for (let x = p + fill; x < t - fill / 2; x += fill) out.push(empty(x));
          if (out[out.length - 1]?.t === p) out.push(empty((p + t) / 2));
        } else out.push(empty((p + t) / 2));
      }
    }
    out.push(row(t));
  }
  return out;
}

function medianStep(times: readonly number[]): number {
  const d = times
    .slice(1)
    .map((t, i) => t - (times[i] as number))
    .toSorted((a, b) => a - b);
  return d.length ? (d[Math.floor(d.length / 2)] as number) : 60;
}

/** Retain the endpoints, extrema and gaps that a plot can distinguish at its width. */
export function rowsForWidth(rows: ChartRow[], from: number, to: number, width: number): ChartRow[] {
  if (width <= 0 || rows.length <= width || to <= from) return rows;
  const result: ChartRow[] = [];
  const buckets = Math.max(1, Math.floor(width / 8));
  const bucket = (row: ChartRow) => Math.floor(((row.t - from) / (to - from)) * buckets);
  const mask = (row: ChartRow) =>
    Number(row.cpu !== null) + 2 * Number(row.mem !== null)
      + 4 * Number(row.cpuBand !== null) + 8 * Number(row.memBand !== null);
  for (let first = 0; first < rows.length;) {
    const initial = rows[first];
    if (!initial) break;
    const bin = bucket(initial);
    const present = mask(initial);
    let end = first + 1;
    let cpuMin = first, cpuMax = first, memMin = first, memMax = first;
    let cpuPeak = first, memPeak = first;
    while (end < rows.length) {
      const row = rows[end];
      if (!row) break;
      if (bucket(row) !== bin || mask(row) !== present) break;
      if (row.cpu !== null && row.cpu < (rows[cpuMin]?.cpu ?? Infinity)) cpuMin = end;
      if (row.cpu !== null && row.cpu > (rows[cpuMax]?.cpu ?? -Infinity)) cpuMax = end;
      if (row.mem !== null && row.mem < (rows[memMin]?.mem ?? Infinity)) memMin = end;
      if (row.mem !== null && row.mem > (rows[memMax]?.mem ?? -Infinity)) memMax = end;
      if (row.cpuBand && row.cpuBand[1] > (rows[cpuPeak]?.cpuBand?.[1] ?? -Infinity)) cpuPeak = end;
      if (row.memBand && row.memBand[1] > (rows[memPeak]?.memBand?.[1] ?? -Infinity)) memPeak = end;
      end++;
    }
    for (const i of [...new Set([first, cpuMin, cpuMax, memMin, memMax, cpuPeak, memPeak, end - 1])].toSorted((a, b) => a - b)) {
      const row = rows[i];
      if (row) result.push(row);
    }
    first = end;
  }
  return result;
}

/** Lone points (no neighbour on either side) need a dot: a line of one point draws nothing. */
export function lonePoints(rows: readonly ChartRow[], key: "cpu" | "mem"): number[] {
  const out: number[] = [];
  rows.forEach((r, i) => {
    if (r[key] === null) return;
    const before = rows[i - 1]?.[key] ?? null;
    const after = rows[i + 1]?.[key] ?? null;
    if (before === null && after === null) out.push(i);
  });
  return out;
}

const MIN = 60;
const HOUR = 3600;
const DAY = 86400;
const STEPS = [5 * MIN, 10 * MIN, 15 * MIN, 30 * MIN, HOUR, 2 * HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, DAY, 2 * DAY, 7 * DAY];

/**
 * About `count` ticks in [from, to], on whole local units: a tick is a
 * multiple of the step in local time. `offset(t)` is the zone's offset from
 * UTC in seconds at t (east positive), so tests can pin a zone.
 */
export function ticks(from: number, to: number, count = 6, offset: (t: number) => number = localOffset): number[] {
  const span = Math.max(1, to - from);
  const step = STEPS.find((s) => span / s <= count) ?? 7 * DAY;
  const out: number[] = [];
  const off0 = offset(from);
  let local = Math.ceil((from + off0) / step) * step;
  for (let guard = 0; guard < 400; guard++) {
    // Back to UTC with the offset at that moment (DST moves it).
    let t = local - off0;
    const off = offset(t);
    t = local - off;
    if (t > to) break;
    if (t >= from && !out.includes(t)) out.push(t);
    local += step;
  }
  return out;
}

export function localOffset(t: number): number {
  return -new Date(t * 1000).getTimezoneOffset() * 60;
}

// One formatter each, made once: Intl formatting is slow to set up, and
// Recharts asks for every tick label several times while laying out.
let HM: Intl.DateTimeFormat | null = null;
let DM: Intl.DateTimeFormat | null = null;
const labels = new Map<string, string>();

/** Tick text: "14:00" within a day, "17 Nov" on a midnight of a longer window, else "17 Nov 06:00". */
export function tickLabel(t: number, span: number): string {
  const key = `${t}|${span <= DAY ? 0 : 1}`;
  const hit = labels.get(key);
  if (hit !== undefined) return hit;
  HM ??= new Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  DM ??= new Intl.DateTimeFormat([], { day: "numeric", month: "short" });
  const d = new Date(t * 1000);
  const hm = HM.format(d);
  const out = span <= DAY ? hm : d.getHours() === 0 && d.getMinutes() === 0 ? DM.format(d) : `${DM.format(d)} ${hm}`;
  if (labels.size > 500) labels.clear();
  labels.set(key, out);
  return out;
}

export interface Marker {
  /** The event a click opens (the worst of a cluster). */
  id: number;
  at: number;
  level: "bad" | "warn";
  /** Accessible name and tooltip: "web: die, exit code 1" or "3 events: …". */
  label: string;
  /** Every event folded into this mark. */
  ids: number[];
}

/** chart.rs `markers_from_events`: only events worth seeing on a chart. */
export function markerLevel(e: Pick<EventRow, "kind" | "level" | "message">): "bad" | "warn" | null {
  if (e.kind === "container.die") return e.level !== "info" ? "bad" : null;
  if (e.kind === "container.oom") return "bad";
  if (e.kind === "container.health") return /\bunhealthy\b/.test(e.message) ? "warn" : null;
  if (e.kind.startsWith("alert.fired")) return "bad";
  return null;
}

export function markerLabel(e: Pick<EventRow, "kind" | "subject" | "message">): string {
  const what = e.kind.split(".").pop() ?? e.kind;
  return e.message ? `${e.subject}: ${what}, ${e.message}` : `${e.subject}: ${what}`;
}

/** The markers inside [from, to], clustered within 1.5 % of the width, worst first. */
export function chartMarkers(events: readonly EventRow[], from: number, to: number): Marker[] {
  const span = Math.max(1, to - from);
  const ms = events
    .map((e) => ({ e, level: markerLevel(e) }))
    .filter((x): x is { e: EventRow; level: "bad" | "warn" } => x.level !== null && x.e.at >= from && x.e.at <= to)
    .map((x) => ({ x: ((x.e.at - from) / span) * 100, e: x.e, level: x.level }))
    .toSorted((a, b) => a.x - b.x || a.e.id - b.e.id);
  const groups: { x: number; items: typeof ms }[] = [];
  for (const m of ms) {
    const last = groups[groups.length - 1];
    if (last && m.x - last.x < 1.5) last.items.push(m);
    else groups.push({ x: m.x, items: [m] });
  }
  return groups.map((g) => {
    const first = g.items[0] as (typeof ms)[number];
    if (g.items.length === 1) {
      return { id: first.e.id, at: first.e.at, level: first.level, label: markerLabel(first.e), ids: [first.e.id] };
    }
    const worst = g.items.find((m) => m.level === "bad") ?? first;
    const shown = g.items.slice(0, 4).map((m) => markerLabel(m.e));
    const label = `${g.items.length} events: ${shown.join("; ")}${g.items.length > 4 ? "; …" : ""}`;
    return { id: worst.e.id, at: worst.e.at, level: worst.level, label, ids: g.items.map((m) => m.e.id) };
  });
}

/** Pixels a pointer must travel before a press is a drag (a tap below this never zooms). */
export const DRAG_MIN_PX = 8;

/**
 * The window a drag from `x0` to `x1` (pixels from the plot's left edge,
 * plot `width` wide) selects on [from, to]; null for a tap or a plot with
 * no width. Clamped to the plot, whole seconds, at least one minute.
 */
export function zoomTo(x0: number, x1: number, width: number, from: number, to: number): { from: number; to: number } | null {
  if (!(width > 0) || Math.abs(x1 - x0) < DRAG_MIN_PX) return null;
  const at = (x: number) => Math.round(from + (Math.min(width, Math.max(0, x)) / width) * (to - from));
  let a = at(Math.min(x0, x1));
  let b = at(Math.max(x0, x1));
  if (b - a < 60) {
    const mid = (a + b) / 2;
    a = Math.max(from, Math.round(mid - 30));
    b = Math.min(to, a + 60);
  }
  return { from: a, to: b };
}

/** The y axis top: a "nice" ceiling over the line and the band (chart.rs `y_range`). */
export function yTop(rows: readonly ChartRow[], key: "cpu" | "mem", withBand: boolean): number {
  let max = 0;
  for (const r of rows) {
    const v = r[key];
    if (v !== null && v > max) max = v;
    const b = key === "cpu" ? r.cpuBand : r.memBand;
    if (withBand && b && b[1] > max) max = b[1];
  }
  const want = max * 1.1;
  if (key === "mem") return niceBytes(want);
  return Math.max(5, nice(want));
}

function nice(x: number): number {
  if (!(Number.isFinite(x) && x > 0)) return 1;
  const e = 10 ** Math.floor(Math.log10(x));
  const m = x / e;
  return ([1, 2, 4, 5, 8, 10].find((s) => m <= s + 1e-9) ?? 10) * e;
}

function niceBytes(x: number): number {
  if (!(Number.isFinite(x) && x > 0)) return 1;
  const k = Math.max(0, Math.floor(Math.log2(x) / 10));
  return nice(x / 1024 ** k) * 1024 ** k;
}
