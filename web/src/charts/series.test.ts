import assert from "node:assert/strict";
import { test } from "node:test";

import type { EventRow } from "../api/gen/EventRow.ts";
import { chartMarkers, chartRows, lonePoints, markerLevel, ticks, yTop, zoomTo } from "./series.ts";

const S = (cpu: [number, number][], o: Record<string, unknown> = {}) =>
  ({ step: 60, cpu, mem: cpu.map(([t, v]) => [t, v * 1024]), cpu_max: null, mem_max: null, ...o }) as Parameters<typeof chartRows>[0];

test("gaps: more than 1.5 × step apart breaks the line with empty rows", () => {
  const rows = chartRows(S([[0, 1], [60, 2], [300, 3], [360, 4]]));
  assert.deepEqual(rows.map((r) => r.t), [0, 60, 120, 180, 240, 300, 360]);
  assert.deepEqual(rows.map((r) => r.cpu), [1, 2, null, null, null, 3, 4]);
  // 1.5 × step exactly is no gap.
  assert.deepEqual(chartRows(S([[0, 1], [90, 2]])).map((r) => r.t), [0, 90]);
  // Just over: one empty row in between.
  assert.deepEqual(chartRows(S([[0, 1], [100, 2]])).map((r) => r.cpu), [1, null, 2]);
  assert.deepEqual(lonePoints(chartRows(S([[0, 1], [300, 2], [360, 3]])), "cpu"), [0]);
});

test("a huge gap gets a coarse grid of empty rows, not thousands", () => {
  const rows = chartRows(S([[0, 1], [60, 1], [60 * 100000, 2]]));
  assert.ok(rows.length <= 125, `${rows.length} rows`);
  assert.equal(rows[2]?.cpu, null);
  assert.equal(rows[rows.length - 1]?.cpu, 2);
});

test("the band pairs the average with the bucket maximum", () => {
  const rows = chartRows(S([[0, 1], [60, 2]], { cpu_max: [[0, 5], [60, 1]] }));
  assert.deepEqual(rows.map((r) => r.cpuBand), [[1, 5], [1, 1]]);
  assert.equal(rows[0]?.memBand, null);
  assert.equal(yTop(rows, "cpu", true), 8, "nice ceiling over 5 × 1.1");
  assert.equal(yTop(rows, "cpu", false), 5, "at least 5 %");
});

const utc = () => 0;
const ev = (id: number, at: number, kind: string, level = "warn", message = ""): EventRow =>
  ({ id, at, kind, level, subject: "web", message }) as EventRow;

test("local-time ticks on whole units, in a fixed zone", () => {
  assert.deepEqual(ticks(3600 * 10 + 60, 3600 * 16, 6, utc), [39600, 43200, 46800, 50400, 54000, 57600]);
  // UTC+2: ticks sit on local whole hours, the same instants here.
  const t = ticks(0, 6 * 3600, 6, () => 7200);
  assert.ok(t.every((x) => (x + 7200) % 3600 === 0));
  // UTC+5:30: an hour tick is on :30 UTC.
  assert.ok(ticks(0, 6 * 3600, 6, () => 19800).every((x) => (x + 19800) % 3600 === 0));
});

test("markers: only events worth seeing, clustered", () => {
  assert.equal(markerLevel(ev(1, 0, "container.die", "info")), null, "a clean exit is routine");
  assert.equal(markerLevel(ev(1, 0, "container.die", "warn")), "bad");
  assert.equal(markerLevel(ev(1, 0, "container.health", "warn", "unhealthy")), "warn");
  assert.equal(markerLevel(ev(1, 0, "container.health", "info", "healthy")), null);
  assert.equal(markerLevel(ev(1, 0, "container.start", "info")), null);
  const ms = chartMarkers(
    [
      ev(1, 100, "container.health", "warn", "unhealthy"),
      ev(2, 101, "container.die", "bad", "exit code 1"),
      ev(3, 900, "container.oom", "bad"),
      ev(4, 5000, "container.oom", "bad"),
    ],
    0,
    1000,
  );
  assert.equal(ms.length, 2, "outside the window is left out");
  assert.equal(ms[0]?.id, 2, "a cluster opens its worst event");
  assert.deepEqual(ms[0]?.ids, [1, 2]);
  assert.match(ms[0]?.label ?? "", /^2 events: web: health, unhealthy; web: die, exit code 1$/);
});

test("zoom: a tap is no zoom; a drag is a window inside the plot", () => {
  assert.equal(zoomTo(100, 105, 500, 0, 5000), null);
  assert.deepEqual(zoomTo(400, 100, 500, 0, 5000), { from: 1000, to: 4000 });
  assert.deepEqual(zoomTo(-50, 600, 500, 0, 5000), { from: 0, to: 5000 }, "clamped to the plot");
  assert.deepEqual(zoomTo(0, 9, 500, 0, 500), { from: 0, to: 60 }, "at least a minute");
  assert.equal(zoomTo(0, 100, 0, 0, 10), null);
});
