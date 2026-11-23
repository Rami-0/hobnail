import { test } from "node:test";
import assert from "node:assert/strict";
import { collides, compact, firstFree, fit, moveTo, nudge, readingOrder, resizeTo, rows, shift, stretch, type Rect } from "./grid.ts";

const r = (x: number, y: number, w: number, h: number): Rect => ({ x, y, w, h });
const overlapFree = (rects: Rect[]) => rects.every((a, i) => rects.every((b, j) => i === j || !collides(a, b)));

test("compaction matches the server: gravity in reading order, full-width floors", () => {
  // Same case as src/board_grid.rs compaction_closes_gaps_and_refuses_overlap_and_bounds.
  assert.deepEqual(compact([r(0, 3, 4, 2), r(4, 9, 4, 1), r(0, 6, 12, 1), r(4, 4, 8, 1)]),
    [r(0, 0, 4, 2), r(4, 3, 4, 1), r(0, 2, 12, 1), r(4, 0, 8, 1)]);
  // Overlap from a merge is pushed below, never kept.
  const settled = compact([r(0, 0, 4, 2), r(2, 1, 4, 2)]);
  assert.ok(overlapFree(settled)); assert.deepEqual(settled[1], r(2, 2, 4, 2));
});

test("fit keeps rectangles on the canvas and above their minimum", () => {
  assert.deepEqual(fit(r(11, -3, 4, 0)), r(8, 0, 4, 1));
  assert.deepEqual(fit(r(0, 0, 1, 1), { w: 3, h: 2 }), r(0, 0, 3, 2));
  assert.deepEqual(fit(r(NaN, 2.6, 40, 99)), r(0, 3, 12, 16));
});

test("dragging onto a tile pushes it out of the way; past it swaps", () => {
  const start = [r(0, 0, 4, 2), r(4, 0, 4, 2), r(0, 2, 4, 2)];
  // Drop the first tile onto the third: the third hops above.
  const swapped = moveTo(start, 0, 0, 2);
  assert.deepEqual(swapped, [r(0, 2, 4, 2), r(4, 0, 4, 2), r(0, 0, 4, 2)]);
  // Drop the third onto the second's column: the second is pushed down.
  const pushed = moveTo(start, 2, 4, 0);
  assert.deepEqual(pushed[2], r(4, 0, 4, 2)); assert.deepEqual(pushed[1], r(4, 2, 4, 2));
  assert.ok(overlapFree(pushed));
  // Never leaves a hole above the dropped tile.
  assert.deepEqual(moveTo(start, 1, 8, 9)[1], r(8, 0, 4, 2));
});

test("resizing respects the minimum and the right edge, pushing tiles below", () => {
  const start = [r(0, 0, 4, 2), r(0, 2, 4, 2)];
  const taller = resizeTo(start, 0, 4, 4);
  assert.deepEqual(taller, [r(0, 0, 4, 4), r(0, 4, 4, 2)]);
  assert.deepEqual(resizeTo(start, 0, 1, 1, { w: 3, h: 2 })[0], r(0, 0, 3, 2));
  assert.deepEqual(resizeTo([r(8, 0, 4, 1)], 0, 6, 1)[0], r(6, 0, 6, 1));
});

test("keyboard steps always change something when they can", () => {
  const start = [r(0, 0, 4, 2), r(0, 2, 4, 2)];
  // Down from the top: a one-row move would compact back, so it swaps.
  const down = nudge(start, 0, "ArrowDown");
  assert.deepEqual(down, [r(0, 2, 4, 2), r(0, 0, 4, 2)]);
  assert.deepEqual(nudge(start, 0, "ArrowRight")[0], r(1, 0, 4, 2));
  assert.deepEqual(nudge(start, 0, "ArrowLeft"), start);
  assert.deepEqual(nudge([r(0, 0, 4, 2)], 0, "ArrowDown"), [r(0, 0, 4, 2)]);
  assert.deepEqual(stretch(start, 0, "ArrowRight")[0], r(0, 0, 5, 2));
  assert.deepEqual(stretch(start, 0, "ArrowUp", { w: 1, h: 2 })[0], r(0, 0, 4, 2));
});

test("first free slot, reading order, canvas rows and phone reorder", () => {
  const placed = [r(0, 0, 4, 2), r(4, 0, 8, 1)];
  assert.deepEqual(firstFree(placed, 4, 1), r(4, 1, 4, 1));
  assert.deepEqual(firstFree(placed, 12, 1), r(0, 2, 12, 1));
  assert.deepEqual(firstFree([], 3, 1), r(0, 0, 3, 1));
  assert.deepEqual(readingOrder([r(4, 0, 1, 1), r(0, 1, 1, 1), r(0, 0, 1, 1)]), [2, 0, 1]);
  assert.equal(rows(placed), 2); assert.equal(rows([]), 0);
  const line = [r(0, 0, 4, 2), r(4, 0, 4, 2), r(8, 0, 4, 2)];
  assert.deepEqual(readingOrder(shift(line, 0, 1)), [1, 0, 2]);
  assert.deepEqual(readingOrder(shift(line, 2, -1)), [0, 2, 1]);
  assert.deepEqual(shift(line, 0, -1), line);
  const mixed = [r(0, 0, 12, 1), r(0, 1, 4, 3)];
  const moved = shift(mixed, 1, -1);
  assert.ok(overlapFree(moved)); assert.deepEqual(readingOrder(moved), [1, 0]);
});
