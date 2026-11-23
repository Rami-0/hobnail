/** The board canvas: 12 columns of integer cells per section, items as
 * rectangles. Every operation returns new rectangles in the same order as
 * its input and ends with the same vertical compaction the server applies
 * (src/board_grid.rs), so a saved layout comes back unchanged. */

export const COLS = 12;
export const MAX_H = 16;

export type Rect = { x: number; y: number; w: number; h: number };
export type Size = { w: number; h: number };

export const collides = (a: Rect, b: Rect): boolean =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n) || 0));

/** Keep a rectangle inside the canvas and at least `min`. */
export function fit(rect: Rect, min: Size = { w: 1, h: 1 }): Rect {
  const w = clamp(rect.w, Math.min(min.w, COLS), COLS);
  const h = clamp(rect.h, Math.min(min.h, MAX_H), MAX_H);
  return { x: clamp(rect.x, 0, COLS - w), y: Math.max(0, Math.round(rect.y) || 0), w, h };
}

/** Indices in reading order: top to bottom, then left to right. */
export function readingOrder(rects: readonly Rect[]): number[] {
  const key = rects.map((r, i) => ({ i, y: r.y, x: r.x }));
  return key.toSorted((a, b) => a.y - b.y || a.x - b.x || a.i - b.i).map((k) => k.i);
}

/** Move every item up as far as it goes, in reading order: each lands
 * directly below the lowest placed item sharing a column with it (the
 * server's rule, src/board_grid.rs). Overlapping input, from a merge, is
 * resolved the same way: the later item ends up below. */
export function compact(rects: readonly Rect[]): Rect[] {
  const out = rects.map((r) => ({ ...r }));
  const placed: Rect[] = [];
  for (const i of readingOrder(out)) {
    const r = out[i]; if (!r) continue;
    r.y = placed.reduce((y, p) => p.x < r.x + r.w && r.x < p.x + p.w ? Math.max(y, p.y + p.h) : y, 0);
    placed.push(r);
  }
  return out;
}

/** Push whatever item `i` now covers out of the way. The item the user
 * moves lets a collider hop above it when there is room (so dragging past
 * a tile swaps them); everything else is pushed down, cascading. */
function resolve(out: Rect[], i: number, hop: boolean, depth = 0): void {
  if (depth > out.length) return;
  const m = out[i]; if (!m) return;
  const hits = out.filter((r, j) => j !== i && collides(r, m)).toSorted((a, b) => a.y - b.y);
  for (const r of hits) {
    if (!collides(r, m)) continue;
    const above = m.y - r.h;
    if (hop && above >= 0 && !out.some((o) => o !== r && collides(o, { ...r, y: above }))) { r.y = above; continue; }
    r.y = m.y + m.h;
    resolve(out, out.indexOf(r), false, depth + 1);
  }
}

/** Put item `i` at cell (x, y), pushing others away, then compact. */
export function moveTo(rects: readonly Rect[], i: number, x: number, y: number): Rect[] {
  const out = rects.map((r) => ({ ...r }));
  const m = out[i]; if (!m) return out;
  Object.assign(m, fit({ ...m, x, y }));
  resolve(out, i, true);
  return compact(out);
}

/** Give item `i` a new size (at least `min`, at most the canvas; a tile
 * at the right edge that grows wider moves left). */
export function resizeTo(rects: readonly Rect[], i: number, w: number, h: number, min?: Size): Rect[] {
  const out = rects.map((r) => ({ ...r }));
  const m = out[i]; if (!m) return out;
  m.w = clamp(w, Math.min(min?.w ?? 1, COLS), COLS);
  m.h = clamp(h, Math.min(min?.h ?? 1, MAX_H), MAX_H);
  m.x = Math.min(m.x, COLS - m.w);
  resolve(out, i, false);
  return compact(out);
}

/** The top-most, then left-most, free slot of a size. */
export function firstFree(rects: readonly Rect[], w: number, h: number): Rect {
  const size = fit({ x: 0, y: 0, w, h });
  const ys = [...new Set([0, ...rects.map((r) => r.y + r.h)])].toSorted((a, b) => a - b);
  const xs = [...new Set([0, ...rects.map((r) => r.x + r.w)])].filter((x) => x + size.w <= COLS).toSorted((a, b) => a - b);
  for (const y of ys) for (const x of xs) {
    const candidate = { ...size, x, y };
    if (!rects.some((r) => collides(r, candidate))) return candidate;
  }
  return { ...size, x: 0, y: ys[ys.length - 1] ?? 0 };
}

/** Rows a section's canvas needs. */
export const rows = (rects: readonly Rect[]): number => rects.reduce((n, r) => Math.max(n, r.y + r.h), 0);

const same = (a: readonly Rect[], b: readonly Rect[]) => a.every((r, i) => { const o = b[i]; return !!o && r.x === o.x && r.y === o.y && r.w === o.w && r.h === o.h; });

export type Step = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown";
/** One keyboard step for item `i`: the nearest target cell in that
 * direction that actually changes the layout (compaction can undo a
 * one-row move, so keep looking a little further). */
export function nudge(rects: readonly Rect[], i: number, key: Step): Rect[] {
  const m = rects[i]; if (!m) return [...rects];
  const [dx, dy] = key === "ArrowLeft" ? [-1, 0] : key === "ArrowRight" ? [1, 0] : key === "ArrowUp" ? [0, -1] : [0, 1];
  for (let step = 1; step <= MAX_H + 1; step++) {
    const x = m.x + dx * step; const y = m.y + dy * step;
    if (x < 0 || x + m.w > COLS || y < 0) break;
    const next = moveTo(rects, i, x, y);
    if (!same(next, rects)) return next;
    if (dx) break;
  }
  return rects.map((r) => ({ ...r }));
}

/** Grow or shrink item `i` by one cell. */
export function stretch(rects: readonly Rect[], i: number, key: Step, min?: Size): Rect[] {
  const m = rects[i]; if (!m) return [...rects];
  const w = m.w + (key === "ArrowRight" ? 1 : key === "ArrowLeft" ? -1 : 0);
  const h = m.h + (key === "ArrowDown" ? 1 : key === "ArrowUp" ? -1 : 0);
  return resizeTo(rects, i, w, h, min);
}

/** Phone reorder: swap item `i` with its neighbour in reading order
 * (their corners trade places; compaction settles any overlap). */
export function shift(rects: readonly Rect[], i: number, by: -1 | 1): Rect[] {
  const order = readingOrder(rects);
  const other = order[order.indexOf(i) + by];
  const out = rects.map((r) => ({ ...r }));
  const a = out[i]; const b = other === undefined ? undefined : out[other];
  if (!a || !b) return out;
  const [ax, ay] = [a.x, a.y];
  Object.assign(a, fit({ ...a, x: b.x, y: b.y }));
  Object.assign(b, fit({ ...b, x: ax, y: ay }));
  return compact(out);
}
