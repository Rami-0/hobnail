import assert from "node:assert/strict";
import { test } from "node:test";

import { humanDuration, level, sparkPaths, stripClass, stripRuns, stripSummary, windowLabel } from "./svg.ts";

test("sparkline: zero based, 1 px padding", () => {
  const p = sparkPaths([0, 10]);
  assert.ok(p);
  assert.equal(p.line, "M0 23L100 1");
  assert.equal(p.area, "M0 23L100 1L100 24L0 24Z");
  // Zero based: a flat series sits at the top, an all-zero one on the floor.
  assert.equal(sparkPaths([5, 5])?.line, "M0 1L100 1");
  assert.equal(sparkPaths([0, 0])?.line, "M0 23L100 23");
});

test("sparkline: max sets the top; values above it are drawn at the top", () => {
  assert.equal(sparkPaths([50, 100], 100)?.line, "M0 12L100 1");
  assert.equal(sparkPaths([50, 150], 100)?.line, "M0 12L100 1");
});

test("sparkline: a gap breaks the line; lone points are not drawn", () => {
  const p = sparkPaths([0, 10, null, 10, 0, null, 5]);
  assert.ok(p);
  assert.equal(p.line, "M0 23L16.7 1M50 1L66.7 23");
  assert.equal((p.area.match(/Z/g) ?? []).length, 2);
  assert.equal(sparkPaths([1]), null);
  assert.equal(sparkPaths([1, null]), null);
  assert.equal(sparkPaths([null, null, null]), null);
});

test("strip: runs merge equal buckets; classes per code", () => {
  assert.deepEqual(stripRuns([2, 2, 3, 2, 2, 2]), [
    [0, 1, 2],
    [2, 2, 3],
    [3, 5, 2],
  ]);
  assert.deepEqual(stripRuns([]), []);
  assert.deepEqual([0, 1, 2, 3, 4, 9].map(stripClass), ["u", "f", "o", "w", "b", "u"]);
});

test("strip summary: the words of ui::view::strip::summary", () => {
  const day = Array.from({ length: 96 }, () => 2);
  assert.equal(stripSummary(day, 900), "running 100% of the last 24 h");
  day[10] = 4;
  day[11] = 4;
  day[50] = 3;
  day[60] = 1;
  assert.equal(stripSummary(day, 900), "running 99% of the last 24 h · 1 problem · 1 restart");
  assert.equal(stripSummary(Array.from({ length: 96 }, () => 0), 900), "no history in the last 24 h");
  // A young instance: the percentage is of the time with data.
  const young = Array.from({ length: 96 }, () => 0);
  for (let i = 92; i < 96; i++) young[i] = 2;
  assert.equal(stripSummary(young, 900), "running 100% of 1h 0m with data");
  assert.equal(windowLabel(7 * 86400), "7 d");
  assert.equal(windowLabel(86400), "24 h");
});

test("durations and levels", () => {
  assert.equal(humanDuration(12), "12s");
  assert.equal(humanDuration(7 * 60 + 5), "7m");
  assert.equal(humanDuration(5 * 3600 + 120), "5h 2m");
  assert.equal(humanDuration(3 * 86400 + 4 * 3600), "3d 4h");
  assert.equal(level(79.9), "");
  assert.equal(level(80), "warn");
  assert.equal(level(90), "bad");
  assert.equal(level(null), "");
});
