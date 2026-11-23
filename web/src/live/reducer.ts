// Pure data rules of the live layer (reducer.test.ts): merging `stats`
// events into the containers view, choosing between a fetched view and a
// streamed one, the problem count of the tab title, and the short ring
// buffers behind the vitals sparklines.
import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { StatsEvent } from "../api/gen/StatsEvent.ts";
import type { Strips } from "../api/gen/Strips.ts";
import type { SystemView } from "../api/gen/SystemView.ts";

/**
 * The view with each container's cpu/mem replaced by the event's numbers.
 * Rows (and groups) the event does not change keep their identity, so a
 * memoised row does not re-render.
 */
export function mergeStats(view: ContainersView, ev: StatsEvent): ContainersView {
  let changed = false;
  const groups = view.groups.map((g) => {
    let gChanged = false;
    const containers = g.containers.map((c) => {
      const s = ev.containers[c.name];
      if (!s || (s.cpu_pct === c.cpu_pct && s.mem_bytes === c.mem_bytes)) return c;
      gChanged = true;
      return { ...c, cpu_pct: s.cpu_pct, mem_bytes: s.mem_bytes };
    });
    if (!gChanged) return g;
    changed = true;
    return { ...g, containers };
  });
  return changed ? { ...view, groups } : view;
}

/**
 * `started_at` of the list is read from Docker's "Up 3 hours", so it moves
 * a little with every read: the same start when it moved by less than two
 * minutes or a tenth of the uptime.
 */
function sameStart(a: number | null, b: number | null, now: number): boolean {
  if (a === null || b === null) return a === b;
  return Math.abs(a - b) <= Math.max(120, Math.abs(now - a) / 10);
}

function samePorts(a: ContainerRow["ports"], b: ContainerRow["ports"]): boolean {
  return a.length === b.length && a.every((port, i) => {
    const next = b[i];
    return next && port.host_ip === next.host_ip && port.host_port === next.host_port
      && port.container_port === next.container_port && port.proto === next.proto;
  });
}

function sameRow(a: ContainerRow, b: ContainerRow, now: number): boolean {
  return (
    a.name === b.name &&
    a.title === b.title &&
    a.image === b.image &&
    a.stack === b.stack &&
    a.icon === b.icon &&
    a.link === b.link &&
    a.dot === b.dot &&
    a.label === b.label &&
    a.state === b.state &&
    a.health === b.health &&
    a.exit_code === b.exit_code &&
    a.restarts === b.restarts &&
    a.created === b.created &&
    sameStart(a.started_at, b.started_at, now) &&
    samePorts(a.ports, b.ports) &&
    a.attention?.severity === b.attention?.severity &&
    a.attention?.reason === b.attention?.reason
  );
}

/**
 * Structural sharing: `next` with every row and group that shows the same
 * as in `cur` replaced by the `cur` object (live numbers are the stats
 * store's, not the view's, so they do not count), and `cur` itself when
 * nothing shown changed. The server sends a `containers` event whenever a
 * Docker status text changes ("Up 3 minutes" → "Up 4 minutes"), which with
 * many containers is most ticks: the pages then re-render nothing.
 */
export function shareView(cur: ContainersView | null, next: ContainersView): ContainersView {
  if (!cur) return next;
  const now = next.now;
  const oldRows = new Map<string, ContainerRow>();
  for (const g of cur.groups) for (const c of g.containers) oldRows.set(c.name, c);
  const oldGroups = new Map(cur.groups.map((g) => [g.name, g]));
  let allSame = cur.groups.length === next.groups.length && cur.docker_error === next.docker_error;
  const groups = next.groups.map((g, gi) => {
    const containers = g.containers.map((c) => {
      const o = oldRows.get(c.name);
      return o && sameRow(o, c, now) ? o : c;
    });
    const og = oldGroups.get(g.name);
    const same =
      !!og &&
      og.title === g.title &&
      og.icon === g.icon &&
      og.collapsed === g.collapsed &&
      og.running === g.running &&
      og.stopped === g.stopped &&
      og.containers.length === containers.length &&
      containers.every((c, i) => c === og.containers[i]);
    if (!same || cur.groups[gi] !== og) allSame = false;
    return same && og ? og : { ...g, containers };
  });
  return allSame ? cur : { ...next, groups };
}

/** A fetched view and a streamed one race at start-up: the newer Docker read wins (ties go to `next`). */
export function newerView(cur: ContainersView | null, next: ContainersView): ContainersView {
  if (cur && cur.fetched_at > next.fetched_at) return cur;
  return next;
}

/** Status words that count as a problem. */
const PROBLEM = new Set(["unhealthy", "restarting", "dead"]);

export function problemCount(view: ContainersView | null): number {
  if (!view) return 0;
  let n = 0;
  for (const g of view.groups) for (const c of g.containers) if (PROBLEM.has(c.label)) n++;
  return n;
}

/** The tab title with the problem count first: "(2) Containers · Hobnail". */
export function badgedTitle(base: string, problems: number): string {
  return problems > 0 ? `(${problems}) ${base}` : base;
}

// ---- vitals rings ----

/** Points kept per sparkline. */
export const RING_MAX = 40;

export interface Ring {
  v: number[];
  /** When the last value was pushed (ms). */
  t: number;
}

export const EMPTY_RING: Ring = { v: [], t: 0 };

/**
 * Push a live value: one that comes within 1.5 s of the last replaces it
 * (a burst of events is one point), else it is appended; RING_MAX kept.
 */
export function pushRing(r: Ring, value: number | null | undefined, now: number): Ring {
  if (value === null || value === undefined || !Number.isFinite(value)) return r;
  if (now - r.t < 1500 && r.v.length) return { v: [...r.v.slice(0, -1), value], t: r.t };
  const v = [...r.v, value];
  return { v: v.length > RING_MAX ? v.slice(v.length - RING_MAX) : v, t: now };
}

/** A ring from history points ([unix, value], oldest first): the last RING_MAX finite values. */
export function seedRing(points: readonly (readonly [number, number])[], map: (v: number) => number = (v) => v): Ring {
  const v = points.map((p) => map(p[1])).filter((x) => Number.isFinite(x));
  return { v: v.slice(Math.max(0, v.length - RING_MAX)), t: 0 };
}

/** Two series added point by point where their times match (net rx + tx). */
export function sumSeries(
  a: readonly (readonly [number, number])[],
  b: readonly (readonly [number, number])[],
): [number, number][] {
  const m = new Map<number, number>();
  for (const [t, v] of b) m.set(t, v);
  const out: [number, number][] = [];
  for (const [t, v] of a) {
    const w = m.get(t);
    if (w !== undefined) out.push([t, v + w]);
  }
  return out;
}

export interface Vitals {
  cpu: number | null;
  mem: number | null;
  disk: number | null;
  /** Bytes per second, received + sent. */
  net: number | null;
}

/** The chip numbers of a system sample (ui::shell vitals: memory and first-disk percent, df-style). */
export function vitalsOf(s: SystemView | null): Vitals {
  if (!s) return { cpu: null, mem: null, disk: null, net: null };
  const d = s.disks[0];
  const denom = d ? d.used + d.avail : 0;
  const net = s.net_rx_bps === null && s.net_tx_bps === null ? null : (s.net_rx_bps ?? 0) + (s.net_tx_bps ?? 0);
  return {
    cpu: s.cpu_pct,
    mem: s.mem_total > 0 ? (s.mem_used * 100) / s.mem_total : null,
    disk: d && denom > 0 ? (d.used * 100) / denom : null,
    net,
  };
}

const sameCodes = (a: readonly number[] | undefined, b: readonly number[]): boolean =>
  !!a && a.length === b.length && a.every((x, i) => x === b[i]);

function shareMap(cur: Record<string, number[]>, next: Record<string, number[]>): { map: Record<string, number[]>; same: boolean } {
  let same = Object.keys(cur).length === Object.keys(next).length;
  const map: Record<string, number[]> = {};
  for (const [k, v] of Object.entries(next)) {
    const o = cur[k];
    if (o && sameCodes(o, v)) map[k] = o;
    else {
      map[k] = v;
      same = false;
    }
  }
  return { map, same };
}

/**
 * Strips refetched every minute: every strip that is the same keeps its
 * array (rows and stacks compare by identity and do not re-render), and the
 * old object when nothing changed.
 */
export function shareStrips(cur: Strips | null, next: Strips): Strips {
  if (!cur || cur.step !== next.step || cur.buckets !== next.buckets) return next;
  const c = shareMap(cur.containers, next.containers);
  const s = shareMap(cur.stacks, next.stacks);
  if (c.same && s.same) return cur;
  return { ...next, containers: c.map, stacks: s.map };
}
