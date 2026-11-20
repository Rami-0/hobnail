import assert from "node:assert/strict";
import { test } from "node:test";
import { treemap } from "./treemap.ts";

test("the treemap fills the area and folds small tiles", () => {
  const { boxes, shown } = treemap([95, 2, 1, 1, 1], 360, 180, [58, 24]);
  assert.equal(boxes.length, shown + 1);
  assert.ok(shown < 5);
  assert.ok(Math.abs(boxes.reduce((a, box) => a + box[2] * box[3], 0) - 64800) < 0.001);
  assert.deepEqual(treemap([], 360, 180, [58, 24]), { boxes: [], shown: 0 });
});
