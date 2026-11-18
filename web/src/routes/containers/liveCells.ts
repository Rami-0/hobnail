// Live CPU and memory per row, outside React state: the table's rows do not
// re-render on a stats tick, only the two cells of a row whose numbers
// changed (each subscribes to its own name), and every sparkline when the
// shared column scale moves (rarely: the scale is a "nice" ceiling).
// A ring of the last 40 readings per row feeds the sparklines (the old
// UI's `sparks`, MAXP = 40): one point per snapshot that changed the row.
// Pure apart from the listeners: view.test.ts.
import { niceCeil } from "./view.ts";

export const SPARK_POINTS = 40;

export interface Cell {
  cpu: number | null;
  mem: number | null;
  cpuHist: (number | null)[];
  memHist: (number | null)[];
}

export interface Scale {
  cpu: number;
  mem: number;
}

export interface Reading {
  name: string;
  cpu: number | null;
  mem: number | null;
}

export interface Update {
  changed: string[];
  scaleMoved: boolean;
}

const EMPTY: Cell = { cpu: null, mem: null, cpuHist: [], memHist: [] };

function push(h: (number | null)[], v: number | null): (number | null)[] {
  if (v === null) return h;
  const out = h.length >= SPARK_POINTS ? h.slice(h.length - SPARK_POINTS + 1) : h.slice();
  out.push(v);
  return out;
}

export class LiveCells {
  private cells = new Map<string, Cell>();
  private byName = new Map<string, Set<() => void>>();
  private scaleListeners = new Set<() => void>();
  private scale: Scale = { cpu: 1, mem: 1 };
  private lastToken: unknown = undefined;
  private lastUpdate: Update = { changed: [], scaleMoved: false };

  get(name: string): Cell {
    return this.cells.get(name) ?? EMPTY;
  }

  getScale = (): Scale => this.scale;

  subscribe(name: string, fn: () => void): () => void {
    const s = this.byName.get(name) ?? new Set<() => void>();
    this.byName.set(name, s);
    s.add(fn);
    return () => {
      s.delete(fn);
      if (s.size === 0) this.byName.delete(name);
    };
  }

  subscribeScale = (fn: () => void): (() => void) => {
    this.scaleListeners.add(fn);
    return () => this.scaleListeners.delete(fn);
  };

  /**
   * One snapshot of every row (`token` names it: the same token twice is
   * one update, so a render that runs twice does not push twice). A row
   * whose numbers changed gets a new point; rows that went away are
   * dropped. Listeners are not told: call `notify` with the result once
   * the render that fed the store is committed.
   */
  update(readings: readonly Reading[], token: unknown): Update {
    if (token !== undefined && token === this.lastToken) return this.lastUpdate;
    this.lastToken = token;
    const seen = new Set<string>();
    const changed: string[] = [];
    for (const r of readings) {
      seen.add(r.name);
      const old = this.cells.get(r.name);
      if (old && old.cpu === r.cpu && old.mem === r.mem) continue;
      this.cells.set(r.name, {
        cpu: r.cpu,
        mem: r.mem,
        cpuHist: push(old?.cpuHist ?? [], r.cpu),
        memHist: push(old?.memHist ?? [], r.mem),
      });
      changed.push(r.name);
    }
    for (const k of this.cells.keys()) if (!seen.has(k)) this.cells.delete(k);
    // One scale per column: the nice ceiling of every value shown.
    let cpu = 0;
    let mem = 0;
    for (const c of this.cells.values()) {
      for (const v of c.cpuHist) if (v !== null && v > cpu) cpu = v;
      for (const v of c.memHist) if (v !== null && v > mem) mem = v;
    }
    const next: Scale = { cpu: Math.max(5, niceCeil(cpu * 1.1)), mem: niceCeilBytes(mem * 1.1) };
    const scaleMoved = next.cpu !== this.scale.cpu || next.mem !== this.scale.mem;
    if (scaleMoved) this.scale = next;
    this.lastUpdate = { changed, scaleMoved };
    return this.lastUpdate;
  }

  /** Tell the cells of `u.changed` (and, when the scale moved, every sparkline). */
  notify(u: Update): void {
    for (const n of u.changed) {
      const s = this.byName.get(n);
      if (s) for (const fn of Array.from(s)) fn();
    }
    if (u.scaleMoved) for (const fn of Array.from(this.scaleListeners)) fn();
  }
}

/** A nice multiple of a power of 1024 (chart.rs `nice_ceil_bytes`). */
export function niceCeilBytes(x: number): number {
  if (!(Number.isFinite(x) && x > 0)) return 1;
  const k = Math.max(0, Math.floor(Math.log2(x) / 10));
  const unit = 1024 ** k;
  return niceCeil(x / unit) * unit;
}
