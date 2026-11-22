// Live CPU and memory per container, apart from the containers view: a
// `stats` event (every few seconds) changes only this store, so the view's
// identity (the sidebar tree, the table's sections, the container page's
// head and charts) stays as it is and nothing structural re-renders. The
// cells that show the numbers subscribe here, per name or to the whole
// batch. A `containers` view (fetch or event) seeds it with the numbers it
// carries. No DOM (stats.test.ts).
import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { StatsEvent } from "../api/gen/StatsEvent.ts";

export interface Stat {
  cpu_pct: number | null;
  mem_bytes: number | null;
}

export const NO_STAT: Stat = { cpu_pct: null, mem_bytes: null };

export class StatsHub {
  private map = new Map<string, Stat>();
  private byName = new Map<string, Set<() => void>>();
  private batch = new Set<(changed: readonly string[]) => void>();
  private ver = 0;
  private snap: ReadonlyMap<string, Stat> | null = null;

  get = (name: string): Stat => this.map.get(name) ?? NO_STAT;

  /** Bumped on every change: a snapshot key for "any number moved". */
  version = (): number => this.ver;

  /** Every container's numbers; a new map after each change (an immutable snapshot). */
  snapshot = (): ReadonlyMap<string, Stat> => (this.snap ??= new Map(this.map));

  names(): IterableIterator<string> {
    return this.map.keys();
  }

  /** Per name: called when that container's numbers change. */
  subscribe(name: string, fn: () => void): () => void {
    const s = this.byName.get(name) ?? new Set<() => void>();
    this.byName.set(name, s);
    s.add(fn);
    return () => {
      s.delete(fn);
      if (s.size === 0) this.byName.delete(name);
    };
  }

  /** Every change, with the names that changed (empty: only removals). */
  subscribeAll = (fn: (changed: readonly string[]) => void): (() => void) => {
    this.batch.add(fn);
    return () => this.batch.delete(fn);
  };

  /** A `stats` event: its numbers replace those of the names it carries. */
  applyEvent(ev: StatsEvent): string[] {
    const changed: string[] = [];
    for (const [name, s] of Object.entries(ev.containers)) {
      if (!s) continue;
      const old = this.map.get(name);
      if (old && old.cpu_pct === s.cpu_pct && old.mem_bytes === s.mem_bytes) continue;
      this.map.set(name, { cpu_pct: s.cpu_pct, mem_bytes: s.mem_bytes });
      changed.push(name);
    }
    this.emit(changed, false);
    return changed;
  }

  /** A containers view: the listed names get its numbers, names no longer listed go. */
  seed(view: ContainersView): string[] {
    const seen = new Set<string>();
    const changed: string[] = [];
    for (const g of view.groups) {
      for (const c of g.containers) {
        seen.add(c.name);
        const old = this.map.get(c.name);
        if (old && old.cpu_pct === c.cpu_pct && old.mem_bytes === c.mem_bytes) continue;
        this.map.set(c.name, { cpu_pct: c.cpu_pct, mem_bytes: c.mem_bytes });
        changed.push(c.name);
      }
    }
    let removed = false;
    for (const k of Array.from(this.map.keys())) {
      if (!seen.has(k)) {
        this.map.delete(k);
        removed = true;
      }
    }
    this.emit(changed, removed);
    return changed;
  }

  private emit(changed: string[], removed: boolean): void {
    if (!changed.length && !removed) return;
    this.ver++;
    this.snap = null;
    for (const n of changed) {
      const s = this.byName.get(n);
      if (s) for (const fn of Array.from(s)) fn();
    }
    for (const fn of Array.from(this.batch)) fn(changed);
  }
}
