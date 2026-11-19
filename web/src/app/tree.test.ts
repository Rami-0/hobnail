import assert from "node:assert/strict";
import { test } from "node:test";

import type { ContainersView } from "../api/gen/ContainersView.ts";
import { filterTree, treeOf, treeTotals } from "./tree.ts";
import { readFolded, readRail, readTreeQuery, settingsHref, writeFolded, writeRail, writeTreeQuery, type KV } from "./prefs.ts";

const ctr = (name: string, stack: string, state = "running", label = "running") => ({
  name,
  title: name,
  image: `img/${name}`,
  stack,
  icon: null,
  link: null,
  dot: "ok",
  label,
  state,
  health: null,
  exit_code: null,
  restarts: 0,
  started_at: 1,
  created: 1,
  ports: [],
  cpu_pct: null,
  mem_bytes: null,
  attention: null,
});

const view = {
  groups: [
    { name: "shop", title: "Shop", icon: null, collapsed: false, running: 1, stopped: 1, containers: [ctr("web", "shop"), ctr("db", "shop", "exited", "stopped")] },
    { name: "tools", title: "Tools", icon: null, collapsed: false, running: 1, stopped: 0, containers: [ctr("proxy", "", "running", "unhealthy")] },
    { name: "empty", title: "Empty", icon: null, collapsed: false, running: 0, stopped: 0, containers: [] },
  ],
  fetched_at: 1,
  docker_error: null,
  now: 1,
} as unknown as ContainersView;

test("tree: stacks with counts, trouble and project flags; empty groups left out", () => {
  const t = treeOf(view);
  assert.deepEqual(
    t.map((s) => [s.name, s.running, s.total, s.trouble, s.project]),
    [
      ["shop", 1, 2, false, true],
      ["tools", 1, 1, true, false],
    ],
  );
  assert.deepEqual(treeTotals(t), { running: 2, total: 3 });
  assert.equal(t[0]?.ctrs[0]?.search, "web shop img/web");
  assert.deepEqual(treeOf(null), []);
});

test("filter: a stack name keeps all its containers; otherwise only matches; empty stacks go", () => {
  const t = treeOf(view);
  assert.deepEqual(
    filterTree(t, "SHOP").map((f) => [f.stack.name, f.ctrs.map((c) => c.name)]),
    [["shop", ["web", "db"]]],
  );
  assert.deepEqual(
    filterTree(t, " img/pro ").map((f) => [f.stack.name, f.ctrs.map((c) => c.name)]),
    [["tools", ["proxy"]]],
  );
  assert.equal(filterTree(t, "nothing").length, 0);
  assert.equal(filterTree(t, "").length, 2);
});

function kv(init: Record<string, string> = {}): KV & { m: Map<string, string> } {
  const m = new Map(Object.entries(init));
  return { m, getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
}

test("prefs: storage keys and formats", () => {
  const s = kv();
  writeRail(s, true);
  assert.equal(s.m.get("rh-side"), "rail");
  assert.ok(readRail(s));
  writeRail(s, false);
  assert.equal(s.m.get("rh-side"), "open");
  assert.ok(!readRail(s));
  writeFolded(s, "shop", true);
  writeFolded(s, "tools", true);
  writeFolded(s, "shop", false);
  assert.equal(s.m.get("rh-tree"), '{"tools":true}');
  assert.deepEqual(readFolded(kv({ "rh-tree": "[1]" })), {});
  assert.deepEqual(readFolded(kv({ "rh-tree": "{bad" })), {});
  assert.deepEqual(readFolded(kv({ "rh-tree": '{"a":true,"b":1}' })), { a: true });
  writeTreeQuery(s, "we");
  assert.equal(s.m.get("rh-tree-q"), "we");
  assert.equal(readTreeQuery(s), "we");
  assert.equal(settingsHref(kv({ "rh-set-tab": "widgets" })), "/settings/widgets");
  assert.equal(settingsHref(kv({ "rh-set-tab": "../x" })), "/settings");
  assert.equal(settingsHref(null), "/settings");
  const broken: KV = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
  };
  assert.ok(!readRail(broken));
  writeRail(broken, true);
  assert.deepEqual(readFolded(broken), {});
});
