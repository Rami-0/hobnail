import assert from "node:assert/strict";
import { test } from "node:test";

import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { SystemView } from "../api/gen/SystemView.ts";
import { badgedTitle, EMPTY_RING, mergeStats, newerView, problemCount, pushRing, RING_MAX, seedRing, sumSeries, vitalsOf } from "./reducer.ts";

const row = (name: string, over: Partial<ContainerRow> = {}): ContainerRow =>
  ({
    name,
    title: name,
    image: "img",
    stack: "s",
    icon: null,
    link: null,
    dot: "ok",
    label: "running",
    state: "running",
    health: null,
    exit_code: null,
    restarts: 0,
    started_at: 1,
    created: 1,
    ports: [],
    cpu_pct: 1,
    mem_bytes: 100,
    attention: null,
    ...over,
  }) as ContainerRow;

const view = (fetched: number, rows: ContainerRow[][]): ContainersView =>
  ({
    groups: rows.map((cs, i) => ({
      name: `g${i}`,
      title: `g${i}`,
      icon: null,
      collapsed: false,
      running: cs.length,
      stopped: 0,
      containers: cs,
    })),
    fetched_at: fetched,
    docker_error: null,
    now: fetched,
  }) as ContainersView;

test("stats merge in place; untouched rows and groups keep their identity", () => {
  const v = view(1, [[row("a"), row("b")], [row("c")]]);
  const out = mergeStats(v, { at: 2, containers: { a: { cpu_pct: 50, mem_bytes: 200 }, c: { cpu_pct: 1, mem_bytes: 100 } } });
  assert.notEqual(out, v);
  assert.equal(out.groups[0]?.containers[0]?.cpu_pct, 50);
  assert.equal(out.groups[0]?.containers[0]?.mem_bytes, 200);
  assert.equal(out.groups[0]?.containers[1], v.groups[0]?.containers[1]);
  assert.equal(out.groups[1], v.groups[1], "c did not change");
  const same = mergeStats(v, { at: 3, containers: { zz: { cpu_pct: 9, mem_bytes: 9 } } });
  assert.equal(same, v);
  const gone = mergeStats(v, { at: 3, containers: { b: { cpu_pct: null, mem_bytes: null } } });
  assert.equal(gone.groups[0]?.containers[1]?.cpu_pct, null);
});

test("the newer Docker read wins the start-up race", () => {
  const old = view(10, []);
  const fresh = view(20, []);
  assert.equal(newerView(fresh, old), fresh);
  assert.equal(newerView(old, fresh), fresh);
  assert.equal(newerView(null, old), old);
  const tie = view(20, []);
  assert.equal(newerView(fresh, tie), tie);
});

test("problem count: unhealthy, restarting, dead", () => {
  const v = view(1, [
    [row("a", { label: "unhealthy" }), row("b", { label: "restarting" })],
    [row("c", { label: "dead" }), row("d", { label: "stopped" }), row("e", { label: "starting" })],
  ]);
  assert.equal(problemCount(v), 3);
  assert.equal(problemCount(null), 0);
  assert.equal(badgedTitle("Containers · Hobnail", 3), "(3) Containers · Hobnail");
  assert.equal(badgedTitle("Containers · Hobnail", 0), "Containers · Hobnail");
});

test("rings: a burst within 1.5 s is one point; 40 kept; gaps skipped", () => {
  let r = pushRing(EMPTY_RING, 1, 10000);
  r = pushRing(r, 2, 10500);
  assert.deepEqual(r.v, [2]);
  r = pushRing(r, 3, 12000);
  assert.deepEqual(r.v, [2, 3]);
  assert.equal(pushRing(r, null, 20000), r);
  assert.equal(pushRing(r, Number.NaN, 20000), r);
  for (let i = 0; i < 50; i++) r = pushRing(r, i, 20000 + i * 2000);
  assert.equal(r.v.length, RING_MAX);
  assert.equal(r.v[RING_MAX - 1], 49);
});

test("seeding: last 40 finite points, mapped; series summed on equal times", () => {
  const pts: [number, number][] = Array.from({ length: 50 }, (_, i) => [i, i]);
  const r = seedRing(pts, (v) => v * 2);
  assert.equal(r.v.length, 40);
  assert.equal(r.v[0], 20);
  assert.deepEqual(
    sumSeries(
      [
        [1, 1],
        [2, 2],
        [3, 3],
      ],
      [
        [2, 10],
        [3, 20],
      ],
    ),
    [
      [2, 12],
      [3, 23],
    ],
  );
});

test("vitals: memory and first disk in percent (df-style), net is rx + tx", () => {
  const s = {
    at: 1,
    cpu_pct: 12.5,
    cores: 4,
    load: [0, 0, 0],
    mem_total: 1000,
    mem_used: 250,
    swap_total: 0,
    swap_used: 0,
    disks: [{ label: "/", total: 100, used: 30, avail: 50 }],
    net_rx_bps: 100,
    net_tx_bps: null,
    temp_c: null,
    uptime_secs: 5,
  } as unknown as SystemView;
  assert.deepEqual(vitalsOf(s), { cpu: 12.5, mem: 25, disk: 37.5, net: 100 });
  assert.deepEqual(vitalsOf(null), { cpu: null, mem: null, disk: null, net: null });
  const none = { ...s, mem_total: 0, disks: [], net_rx_bps: null } as unknown as SystemView;
  assert.deepEqual(vitalsOf(none), { cpu: 12.5, mem: null, disk: null, net: null });
});
