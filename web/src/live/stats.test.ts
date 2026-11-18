import assert from "node:assert/strict";
import { test } from "node:test";

import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { StatsEvent } from "../api/gen/StatsEvent.ts";
import { shareStrips, shareView } from "./reducer.ts";
import { NO_STAT, StatsHub } from "./stats.ts";

const view = (rows: [string, number | null][]): ContainersView =>
  ({
    groups: [{ name: "g", title: "g", icon: null, collapsed: false, running: 0, stopped: 0, containers: rows.map(([name, cpu]) => ({ name, cpu_pct: cpu, mem_bytes: 1 })) }],
    fetched_at: 1,
    docker_error: null,
    now: 1,
  }) as unknown as ContainersView;

test("stats: only the names that changed are told; snapshots are new maps", () => {
  const h = new StatsHub();
  let a = 0;
  const batches: string[][] = [];
  h.subscribe("a", () => a++);
  h.subscribeAll((c) => batches.push([...c]));
  h.seed(view([["a", 1], ["b", 2]]));
  assert.equal(a, 1);
  const s1 = h.snapshot();
  h.applyEvent({ at: 2, containers: { a: { cpu_pct: 1, mem_bytes: 1 }, b: { cpu_pct: 3, mem_bytes: 1 } } } as unknown as StatsEvent);
  assert.equal(a, 1, "a did not change");
  assert.deepEqual(batches, [["a", "b"], ["b"]]);
  assert.notEqual(h.snapshot(), s1);
  assert.equal(h.get("b").cpu_pct, 3);
  const v = h.version();
  h.applyEvent({ at: 3, containers: { b: { cpu_pct: 3, mem_bytes: 1 } } } as unknown as StatsEvent);
  assert.equal(h.version(), v, "no change, no new version");
  h.seed(view([["a", 1]]));
  assert.equal(h.get("b"), NO_STAT, "gone from the list");
  assert.equal(batches.length, 3);
});

const row = (name: string, o: Record<string, unknown> = {}) => ({
  name, title: name, image: "i", stack: "g", icon: null, link: null, dot: "ok", label: "running", state: "running",
  health: null, exit_code: null, restarts: 0, started_at: 1000, created: 1, ports: [], cpu_pct: 1, mem_bytes: 1, attention: null, ...o,
});
const v = (rows: ReturnType<typeof row>[], now = 5000) =>
  ({ groups: [{ name: "g", title: "g", icon: null, collapsed: false, running: rows.length, stopped: 0, containers: rows }], fetched_at: now, docker_error: null, now }) as unknown as ContainersView;

test("shareView: a view that shows the same keeps its identity", () => {
  const a = v([row("x"), row("y")]);
  assert.equal(shareView(a, v([row("x", { cpu_pct: 9, started_at: 1030 }), row("y")], 5030)), a, "numbers and a jittering start do not count");
  const b = shareView(a, v([row("x", { label: "unhealthy" }), row("y")]));
  assert.notEqual(b, a);
  assert.equal(b.groups[0]?.containers[1], a.groups[0]?.containers[1], "the unchanged row keeps its object");
  assert.notEqual(shareView(a, v([row("x", { started_at: 4900 }), row("y")])), a, "a restart is a change");
  assert.equal(shareView(null, a), a);
});

test("shareStrips: unchanged strips keep their arrays and object", () => {
  const a = { now: 1, step: 900, buckets: 2, containers: { x: [2, 2], y: [1, 2] }, stacks: { g: [2, 2] } };
  assert.equal(shareStrips(a, { ...a, now: 2, containers: { x: [2, 2], y: [1, 2] }, stacks: { g: [2, 2] } }), a);
  const b = shareStrips(a, { ...a, now: 3, containers: { x: [2, 2], y: [2, 2] }, stacks: { g: [2, 2] } });
  assert.notEqual(b, a);
  assert.equal(b.containers.x, a.containers.x);
  assert.equal(b.stacks.g, a.stacks.g);
});
