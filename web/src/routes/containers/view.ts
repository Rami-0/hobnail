// The Containers table's view: filter, sort and grouping, held in the URL
// (`?q=&state=&stack=&sort=&dir=&group=`), the parts worth keeping (sort,
// direction, grouping) remembered per browser. Pure (no DOM): view.test.ts.
import type { ContainerRow } from "../../api/gen/ContainerRow.ts";
import type { GroupRow } from "../../api/gen/GroupRow.ts";

export type StateFilter = "" | "running" | "stopped" | "problem";
export type SortKey = "name" | "state" | "cpu" | "mem" | "uptime" | "stack";
export type Dir = "asc" | "desc";
export type Grouping = "stack" | "flat";

export interface View {
  q: string;
  state: StateFilter;
  /** A stack key, "" for all. */
  stack: string;
  sort: SortKey;
  /** null: the column's natural direction. */
  dir: Dir | null;
  /** null: the default (grouped by stack). */
  group: Grouping | null;
}

export const SORTS: readonly { key: SortKey; label: string }[] = [
  { key: "name", label: "Name" },
  { key: "stack", label: "Stack" },
  { key: "state", label: "State" },
  { key: "uptime", label: "Uptime" },
  { key: "cpu", label: "CPU" },
  { key: "mem", label: "Memory" },
];

const SORT_KEYS = new Set<string>(SORTS.map((s) => s.key));
const STATES = new Set<string>(["running", "stopped", "problem"]);

export const DEFAULT_VIEW: View = { q: "", state: "", stack: "", sort: "name", dir: null, group: null };

/** Numbers read best biggest first. */
export function defaultDesc(sort: SortKey): boolean {
  return sort === "cpu" || sort === "mem";
}

export function isDesc(v: View): boolean {
  return v.dir === null ? defaultDesc(v.sort) : v.dir === "desc";
}

export function isGrouped(v: View): boolean {
  return v.group !== "flat";
}

/** What a browser remembers: sort, direction and grouping (filters are not kept, OV-28). */
export interface Remembered {
  sort?: string;
  dir?: string;
  group?: string;
}

export const VIEW_KEYS = ["q", "state", "stack", "sort", "dir", "group"] as const;

/**
 * The view from a query string. Without any view key in the URL the
 * remembered sort and grouping apply. Junk falls back to the defaults.
 */
export function parseView(search: string, remembered: Remembered | null = null): View {
  const p = new URLSearchParams(search);
  const fromUrl = VIEW_KEYS.some((k) => p.has(k));
  const get = (k: string): string => {
    const v = p.get(k);
    if (v !== null || fromUrl || !remembered) return v ?? "";
    const r = (remembered as Record<string, unknown>)[k];
    return typeof r === "string" ? r : "";
  };
  const sort = get("sort");
  const dir = get("dir");
  const group = get("group");
  const state = get("state");
  return {
    q: [...p.get("q")?.trim() ?? ""].slice(0, 100).join(""),
    state: STATES.has(state) ? (state as StateFilter) : "",
    stack: p.get("stack") ?? "",
    sort: SORT_KEYS.has(sort) ? (sort as SortKey) : "name",
    dir: dir === "asc" || dir === "desc" ? dir : null,
    group: group === "stack" || group === "flat" ? group : null,
  };
}

/** Canonical query string (defaults left out), without the "?". */
export function viewSearch(v: View): string {
  const p = new URLSearchParams();
  if (v.q) p.set("q", v.q);
  if (v.state) p.set("state", v.state);
  if (v.stack) p.set("stack", v.stack);
  if (v.sort !== "name") p.set("sort", v.sort);
  if (v.dir !== null && (v.dir === "desc") !== defaultDesc(v.sort)) p.set("dir", v.dir);
  if (v.group !== null) p.set("group", v.group);
  return p.toString();
}

export function remember(v: View): Remembered {
  const r: Remembered = {};
  if (v.sort !== "name") r.sort = v.sort;
  if (v.dir !== null && (v.dir === "desc") !== defaultDesc(v.sort)) r.dir = v.dir;
  if (v.group !== null) r.group = v.group;
  return r;
}

/** A header click: the same column flips, another starts in its natural direction. */
export function sortBy(v: View, key: SortKey): View {
  const desc = v.sort === key ? !isDesc(v) : defaultDesc(key);
  return { ...v, sort: key, dir: desc === defaultDesc(key) ? null : desc ? "desc" : "asc" };
}

export function flipDir(v: View): View {
  const desc = !isDesc(v);
  return { ...v, dir: desc === defaultDesc(v.sort) ? null : desc ? "desc" : "asc" };
}

export function isRunning(r: Pick<ContainerRow, "state">): boolean {
  return r.state === "running";
}

export function isProblem(r: Pick<ContainerRow, "attention">): boolean {
  return r.attention !== null;
}

function matchesState(f: StateFilter, r: ContainerRow): boolean {
  switch (f) {
    case "running":
      return isRunning(r);
    case "stopped":
      return !isRunning(r);
    case "problem":
      return isProblem(r);
    default:
      return true;
  }
}

/** Lower-cased search text of a row: display name, name, stack, image. */
export function haystack(r: ContainerRow): string {
  const terms: string[] = [];
  for (const t of [r.title, r.name, r.stack, r.image]) if (t && !terms.includes(t)) terms.push(t);
  return terms.join(" ").toLowerCase();
}

export function matches(v: View, r: ContainerRow): boolean {
  if (!matchesState(v.state, r)) return false;
  if (v.stack && r.stack !== v.stack) return false;
  const hay = haystack(r);
  return v.q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((t) => hay.includes(t));
}

/** unhealthy, restarting, dead first; then starting, paused; then running; then the rest. */
export function statusRank(label: string): number {
  switch (label) {
    case "unhealthy":
    case "restarting":
    case "dead":
      return 0;
    case "starting":
    case "paused":
      return 1;
    case "healthy":
    case "running":
      return 2;
    default:
      return 3;
  }
}

/** Seconds up (null: not running or unknown). */
export function uptime(r: ContainerRow, now: number): number | null {
  return isRunning(r) && r.started_at !== null && r.started_at > 0 ? Math.max(0, now - r.started_at) : null;
}

const cmpStr = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

export interface SortCtx {
  sort: SortKey;
  desc: boolean;
  now: number;
  /** Stack key -> title, for sorting by stack. */
  stackTitle: (k: string) => string;
  /** The live numbers of a row (the stats store); default: the row's own. */
  stat?: (name: string) => { cpu_pct: number | null; mem_bytes: number | null };
}

const name = (r: ContainerRow) => r.title.toLowerCase();

/** containers.rs `cmp_card`: rows without a number always sink; ties by name. */
export function compareRows(a: ContainerRow, b: ContainerRow, c: SortCtx): number {
  const num = (x: number | null, y: number | null): number => {
    if (x !== null && y !== null) return c.desc ? y - x : x - y;
    if (x !== null) return -1;
    if (y !== null) return 1;
    return 0;
  };
  let o: number;
  switch (c.sort) {
    case "cpu":
      o = c.stat ? num(c.stat(a.name).cpu_pct, c.stat(b.name).cpu_pct) : num(a.cpu_pct, b.cpu_pct);
      break;
    case "mem":
      o = c.stat ? num(c.stat(a.name).mem_bytes, c.stat(b.name).mem_bytes) : num(a.mem_bytes, b.mem_bytes);
      break;
    case "uptime":
      o = num(uptime(a, c.now), uptime(b, c.now));
      break;
    case "stack":
      o = cmpStr(c.stackTitle(a.stack).toLowerCase(), c.stackTitle(b.stack).toLowerCase());
      if (c.desc) o = -o;
      break;
    case "state":
      o = statusRank(a.label) - statusRank(b.label);
      if (c.desc) o = -o;
      break;
    default:
      o = cmpStr(name(a), name(b));
      if (c.desc) o = -o;
  }
  return o || cmpStr(name(a), name(b)) || cmpStr(a.name, b.name);
}

export interface Section {
  /** The stack (grouped), or null for the flat list. */
  group: GroupRow | null;
  rows: ContainerRow[];
}

const NUMERIC = new Set<SortKey>(["cpu", "mem", "uptime"]);

/**
 * Rows visible under the view, in table order. Grouped: the stacks keep
 * their configured order, except under a numeric sort, where they follow
 * their top row (OV-22). Flat: one section.
 */
export function select(groups: readonly GroupRow[], v: View, now: number, stat?: SortCtx["stat"]): Section[] {
  const titles = new Map(groups.map((g) => [g.name, g.title]));
  const ctx: SortCtx = { sort: v.sort, desc: isDesc(v), now, stackTitle: (k) => titles.get(k) ?? k };
  if (stat) ctx.stat = stat;
  const cmp = (a: ContainerRow, b: ContainerRow) => compareRows(a, b, ctx);
  const out: Section[] = [];
  for (const g of groups) {
    const rows = g.containers.filter((r) => matches(v, r)).toSorted(cmp);
    if (rows.length) out.push({ group: g, rows });
  }
  if (!isGrouped(v)) return [{ group: null, rows: out.flatMap((s) => s.rows).toSorted(cmp) }];
  if (!NUMERIC.has(v.sort)) return out;
  const order = new Map(out.map((s, i) => [s, i]));
  return out.toSorted((a, b) => {
    const ra = a.rows[0];
    const rb = b.rows[0];
    return (ra && rb ? cmp(ra, rb) : 0) || (order.get(a) ?? 0) - (order.get(b) ?? 0);
  });
}

/**
 * Keep the order the reader is looking at (focus or pointer in the table):
 * rows and stacks that were shown stay where they were, new ones go after
 * them in their computed order, gone ones drop out.
 */
export function keepOrder(next: Section[], prev: readonly string[]): Section[] {
  if (prev.length === 0) return next;
  const pos = new Map(prev.map((k, i) => [k, i]));
  const at = (k: string, i: number) => pos.get(k) ?? prev.length + i;
  const sections = next.map((s, i) => ({
    s: {
      group: s.group,
      rows: s.rows
        .map((r, j) => ({ r, k: at(`r:${r.name}`, j) }))
        .toSorted((a, b) => a.k - b.k)
        .map((x) => x.r),
    },
    k: at(`g:${s.group?.name ?? ""}`, i),
  }));
  return sections.toSorted((a, b) => a.k - b.k).map((x) => x.s);
}

/** The order keys of `sections`, for `keepOrder`. */
export function orderOf(sections: readonly Section[]): string[] {
  const out: string[] = [];
  for (const s of sections) {
    out.push(`g:${s.group?.name ?? ""}`);
    for (const r of s.rows) out.push(`r:${r.name}`);
  }
  return out;
}

export interface Counts {
  all: number;
  running: number;
  stopped: number;
  problem: number;
}

export function counts(groups: readonly GroupRow[]): Counts {
  const c: Counts = { all: 0, running: 0, stopped: 0, problem: 0 };
  for (const g of groups) {
    for (const r of g.containers) {
      c.all++;
      if (isRunning(r)) c.running++;
      else c.stopped++;
      if (isProblem(r)) c.problem++;
    }
  }
  return c;
}

/** A stack is folded when the reader folded it (else its config says so); never while filtering by text or stack. */
export function isClosed(g: GroupRow, v: View, folded: Record<string, boolean>): boolean {
  if (v.q || v.stack) return false;
  return g.name in folded ? folded[g.name] === true : g.collapsed;
}

/** Why nothing is shown: good news for a state filter on its own, else no match (OV-24). */
export function noneWhy(v: View): "stopped" | "problem" | "filter" {
  if (!v.q && !v.stack && (v.state === "stopped" || v.state === "problem")) return v.state;
  return "filter";
}

/**
 * "Nice" ceiling for a shared sparkline scale (1, 2, 4, 5, 8 × 10^k), so
 * the scale moves rarely while numbers wander (chart.rs `nice_ceil`).
 */
export function niceCeil(x: number): number {
  if (!(Number.isFinite(x) && x > 0)) return 1;
  const e = 10 ** Math.floor(Math.log10(x));
  const m = x / e;
  const step = [1, 2, 4, 5, 8, 10].find((s) => m <= s + 1e-9) ?? 10;
  return step * e;
}
