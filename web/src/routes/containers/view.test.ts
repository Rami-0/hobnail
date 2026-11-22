import assert from "node:assert/strict";
import { test } from "node:test";

import type { ContainerRow } from "../../api/gen/ContainerRow.ts";
import type { GroupRow } from "../../api/gen/GroupRow.ts";
import { LiveCells, niceCeilBytes } from "./liveCells.ts";
import {
  counts,
  flipDir,
  isClosed,
  isDesc,
  keepOrder,
  matches,
  noneWhy,
  orderForPin,
  parseView,
  remember,
  select,
  sortBy,
  viewSearch,
  DEFAULT_VIEW,
  type View,
} from "./view.ts";

const NOW = 1_800_000_000;

function row(name: string, stack: string, o: Partial<ContainerRow> = {}): ContainerRow {
  const running = (o.state ?? "running") === "running";
  return {
    name,
    title: name,
    image: `img/${name}`,
    stack,
    icon: null,
    link: null,
    dot: running ? "ok" : "off",
    label: running ? "running" : "stopped",
    state: "running",
    health: null,
    exit_code: null,
    restarts: 0,
    started_at: running ? NOW - 3600 : null,
    created: NOW - 7200,
    ports: [],
    cpu_pct: null,
    mem_bytes: null,
    attention: null,
    ...o,
  } as ContainerRow;
}

function group(name: string, containers: ContainerRow[], o: Partial<GroupRow> = {}): GroupRow {
  return {
    name,
    title: name,
    icon: null,
    collapsed: false,
    running: containers.filter((c) => c.state === "running").length,
    stopped: containers.filter((c) => c.state !== "running").length,
    containers,
    ...o,
  } as GroupRow;
}

// containers.rs tests: zeta (b, 50 %), alpha (a, 5 %), mid (a, stopped exit 1).
const GROUPS: GroupRow[] = [
  group("a", [
    row("alpha", "a", { cpu_pct: 5, mem_bytes: 900, started_at: NOW - 100 }),
    row("mid", "a", {
      state: "exited",
      label: "stopped",
      dot: "off",
      exit_code: 1,
      attention: { severity: "warn", reason: "stopped, exit 1" },
    } as Partial<ContainerRow>),
  ]),
  group("b", [row("zeta", "b", { cpu_pct: 50, mem_bytes: 100, started_at: NOW - 5000 })]),
];

const names = (v: View) => select(GROUPS, v, NOW).flatMap((s) => s.rows.map((r) => r.name));
const q = (s: string) => parseView(s);

test("query parsing and round trip", () => {
  const v = q("q=my%20app&state=problem&stack=a%26b&sort=cpu&dir=asc&group=flat");
  assert.deepEqual(v, { q: "my app", state: "problem", stack: "a&b", sort: "cpu", dir: "asc", group: "flat" });
  assert.deepEqual(q(viewSearch(v)), v);
  assert.equal(viewSearch(DEFAULT_VIEW), "");
  const junk = q("state=zzz&sort=zzz&dir=zzz&group=7");
  assert.deepEqual(junk, DEFAULT_VIEW);
  assert.ok(isDesc(q("sort=mem")));
  assert.ok(!isDesc(q("sort=uptime")));
  assert.equal(viewSearch(q("sort=mem&dir=desc")), "sort=mem");
  assert.equal(viewSearch(q("sort=mem&dir=asc")), "sort=mem&dir=asc");
  assert.equal(q(`q=${"x".repeat(500)}`).q.length, 100);
});

test("remembered sort and grouping apply only without view keys in the URL", () => {
  const mem = { sort: "cpu", group: "flat" };
  assert.equal(parseView("", mem).sort, "cpu");
  assert.equal(parseView("", mem).group, "flat");
  assert.equal(parseView("?q=x", mem).sort, "name", "a URL with a view key wins entirely");
  assert.deepEqual(remember(q("sort=cpu&dir=asc&group=flat&q=x&state=running")), { sort: "cpu", dir: "asc", group: "flat" });
  assert.deepEqual(remember(DEFAULT_VIEW), {});
});

test("header clicks flip the same column, start others in their natural direction", () => {
  let v = sortBy(DEFAULT_VIEW, "name");
  assert.equal(viewSearch(v), "dir=desc");
  v = sortBy(v, "cpu");
  assert.equal(viewSearch(v), "sort=cpu");
  assert.ok(isDesc(v));
  v = sortBy(v, "cpu");
  assert.equal(viewSearch(v), "sort=cpu&dir=asc");
  assert.equal(viewSearch(flipDir(v)), "sort=cpu");
});

const flat = (s: string) => names(q(`group=flat&${s}`));

test("filter and sort (flat)", () => {
  assert.deepEqual(flat(""), ["alpha", "mid", "zeta"]);
  assert.deepEqual(flat("state=running"), ["alpha", "zeta"]);
  assert.deepEqual(flat("state=stopped"), ["mid"]);
  assert.deepEqual(flat("state=problem"), ["mid"]);
  assert.deepEqual(flat("stack=a"), ["alpha", "mid"]);
  assert.deepEqual(flat("q=ZET"), ["zeta"]);
  assert.deepEqual(flat("q=img+mid"), ["mid"], "all terms must match");
  assert.deepEqual(flat("sort=cpu"), ["zeta", "alpha", "mid"], "biggest first, no data sinks");
  assert.deepEqual(flat("sort=cpu&dir=asc"), ["alpha", "zeta", "mid"]);
  assert.deepEqual(flat("sort=name&dir=desc"), ["zeta", "mid", "alpha"]);
  assert.deepEqual(flat("sort=state"), ["alpha", "zeta", "mid"]);
  assert.deepEqual(flat("sort=uptime"), ["alpha", "zeta", "mid"], "shortest up first, stopped sinks");
  assert.deepEqual(flat("sort=uptime&dir=desc"), ["zeta", "alpha", "mid"]);
  assert.deepEqual(flat("sort=stack&dir=desc"), ["zeta", "alpha", "mid"]);
});

test("grouped: stacks keep their order, numeric sorts follow the top row (OV-22)", () => {
  assert.deepEqual(names(q("sort=cpu")), ["zeta", "alpha", "mid"]);
  assert.deepEqual(names(q("sort=cpu&dir=asc")), ["alpha", "mid", "zeta"]);
  assert.deepEqual(names(q("sort=name&dir=desc")), ["mid", "alpha", "zeta"]);
  assert.equal(select(GROUPS, q(""), NOW).length, 2);
});

test("counts, matching and folded stacks", () => {
  assert.deepEqual(counts(GROUPS), { all: 3, running: 2, stopped: 1, problem: 1 });
  assert.ok(matches(q("q=img/alpha"), GROUPS[0]?.containers[0] as ContainerRow));
  const g = GROUPS[0] as GroupRow;
  assert.equal(isClosed(g, DEFAULT_VIEW, {}), false);
  assert.equal(isClosed({ ...g, collapsed: true }, DEFAULT_VIEW, {}), true);
  assert.equal(isClosed({ ...g, collapsed: true }, DEFAULT_VIEW, { a: false }), false);
  assert.equal(isClosed(g, DEFAULT_VIEW, { a: true }), true);
  assert.equal(isClosed(g, q("q=x"), { a: true }), false, "never folded while filtering");
  assert.equal(noneWhy(q("state=stopped")), "stopped");
  assert.equal(noneWhy(q("state=problem")), "problem");
  assert.equal(noneWhy(q("state=problem&q=x")), "filter");
});

test("freezing uses the committed order even when live resources have already changed", () => {
  const before = select(GROUPS, q("group=flat&sort=cpu"), NOW);
  const committed = { key: "cpu", sections: before };
  // zeta's CPU drops: a fresh sort would move it below alpha.
  const g2 = [
    group("a", [row("alpha", "a", { cpu_pct: 5 }), row("new", "a", { cpu_pct: 90 })]),
    group("b", [row("zeta", "b", { cpu_pct: 1 })]),
  ];
  const fresh = select(g2, q("group=flat&sort=cpu"), NOW);
  assert.deepEqual(fresh[0]?.rows.map((r) => r.name), ["new", "alpha", "zeta"]);
  const kept = keepOrder(fresh, orderForPin("cpu", fresh, committed));
  assert.deepEqual(kept[0]?.rows.map((r) => r.name), ["zeta", "alpha", "new"]);
  assert.deepEqual(keepOrder(fresh, orderForPin("mem", fresh, committed)), fresh);
});

test("live cells: only changed rows are told, one scale per column", () => {
  const c = new LiveCells();
  let told = 0;
  let scaled = 0;
  c.subscribe("a", () => told++);
  c.subscribeScale(() => scaled++);
  c.notify(c.update([{ name: "a", cpu: 3, mem: 1000 }, { name: "b", cpu: 40, mem: 5000 }], 1));
  assert.equal(told, 1);
  assert.equal(scaled, 1);
  assert.equal(c.getScale().cpu, 50, "nice ceiling of 40 × 1.1");
  c.notify(c.update([{ name: "a", cpu: 3, mem: 1000 }, { name: "b", cpu: 41, mem: 5000 }], 2));
  assert.equal(told, 1, "a did not change");
  assert.equal(scaled, 1, "the scale did not move");
  assert.deepEqual(c.get("b").cpuHist, [40, 41]);
  const u = c.update([{ name: "b", cpu: 42, mem: 5000 }], 3);
  assert.equal(c.update([{ name: "b", cpu: 42, mem: 5000 }], 3), u, "the same snapshot twice is one update");
  assert.deepEqual(c.get("b").cpuHist, [40, 41, 42]);
  assert.deepEqual(c.get("a").cpuHist, [], "a went away");
  for (let i = 0; i < 50; i++) c.update([{ name: "b", cpu: i, mem: 1 }], 10 + i);
  assert.equal(c.get("b").cpuHist.length, 40);
  assert.deepEqual(c.update([{ name: "b", cpu: 99, mem: 1 }], 100).changed, ["b"]);
  assert.equal(niceCeilBytes(1500), 2048);
  assert.equal(niceCeilBytes(3 * 1024 ** 3), 4 * 1024 ** 3);
});
