// The sidebar's stack tree as data (src/ui/shell.rs tree_inner,
// static/app.js treeFilter), pure so tree.test.ts can check the filter.
import type { ContainersView } from "../api/gen/ContainersView.ts";

export interface TreeCtr {
  name: string;
  title: string;
  dot: string;
  label: string;
  running: boolean;
  /** Lower-cased "title name stack image": what the filter searches. */
  search: string;
}

export interface TreeStack {
  name: string;
  title: string;
  running: number;
  total: number;
  /** A container in it is unhealthy, restarting or dead. */
  trouble: boolean;
  /** A compose project of this name exists: its merged logs page does too (LG-09). */
  project: boolean;
  ctrs: TreeCtr[];
}

const TROUBLE = new Set(["unhealthy", "restarting", "dead"]);

export function treeOf(view: ContainersView | null): TreeStack[] {
  if (!view) return [];
  return view.groups
    .filter((g) => g.containers.length > 0)
    .map((g) => {
      const ctrs = g.containers.map((c) => {
        const terms: string[] = [];
        for (const t of [c.title, c.name, c.stack ?? "", c.image]) if (t && !terms.includes(t)) terms.push(t);
        return {
          name: c.name,
          title: c.title,
          dot: c.dot,
          label: c.label,
          running: c.state === "running",
          search: terms.join(" ").toLowerCase(),
        };
      });
      return {
        name: g.name,
        title: g.title,
        running: ctrs.filter((c) => c.running).length,
        total: ctrs.length,
        trouble: g.containers.some((c) => TROUBLE.has(c.label)),
        project: g.containers.some((c) => c.stack === g.name),
        ctrs,
      };
    });
}

export interface FilteredStack {
  stack: TreeStack;
  ctrs: TreeCtr[];
}

/**
 * The stacks and containers a filter text leaves (static/app.js
 * treeFilter): a stack whose name matches keeps all its containers, else
 * only the matching ones; a stack with none goes.
 */
export function filterTree(stacks: TreeStack[], query: string): FilteredStack[] {
  const q = query.trim().toLowerCase();
  if (!q) return stacks.map((stack) => ({ stack, ctrs: stack.ctrs }));
  const out: FilteredStack[] = [];
  for (const stack of stacks) {
    const hit = stack.name.toLowerCase().includes(q);
    const ctrs = hit ? stack.ctrs : stack.ctrs.filter((c) => c.search.includes(q));
    if (ctrs.length) out.push({ stack, ctrs });
  }
  return out;
}

/** Running and total containers over the whole tree (the "12/14" in the tree head). */
export function treeTotals(stacks: TreeStack[]): { running: number; total: number } {
  let running = 0;
  let total = 0;
  for (const s of stacks) {
    running += s.running;
    total += s.total;
  }
  return { running, total };
}
