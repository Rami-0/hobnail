import assert from "node:assert/strict";
import { test } from "node:test";
import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import type { GroupRow } from "../api/gen/GroupRow.ts";
import type { SystemView } from "../api/gen/SystemView.ts";
import { busiest, eventRows, hostBody, logLines, metricValue, stackRows, thin, topContainers } from "./hobnail.ts";

const GiB = 1024 ** 3;
const row = (name: string, cpu: number | null, mem: number | null, state = "running"): ContainerRow => ({
  name, title: name, stack: "lab", state, dot: state === "running" ? "ok" : "off", cpu_pct: cpu, mem_bytes: mem,
} as ContainerRow);

const live = (r: ContainerRow) => ({ cpu: r.cpu_pct, mem: r.mem_bytes });
const g = (title: string, running: number, stopped: number) => ({ name: title, title, running, stopped, containers: [row(title, 0, 0)] } as GroupRow);

test("host overview: tiles with trends and the fullest disks first", () => {
  const system = { cpu_pct: 12.5, cores: 4, load: [0.5, 0.75, 1], mem_total: 16 * GiB, mem_used: 4 * GiB, swap_total: 0, swap_used: 0,
    disks: [{ label: "/", total: 100, used: 50, avail: 50 }, { label: "/srv", total: 100, used: 90, avail: 10 }, { label: "/empty", total: 0, used: 0, avail: 0 }],
    net_rx_bps: 2048, net_tx_bps: null, temp_c: null, uptime_secs: 90_000, at: 0 } as SystemView;
  const { tiles, meters } = hostBody(system, { cpu: [1, 2, 3], mem: [], net: [] });
  assert.deepEqual(tiles.map((t) => [t.label, t.value]), [["CPU", "12%"], ["Memory", "25%"], ["Network", "↓ 2.0 KiB/s"], ["Load", "0.50 · 0.75 · 1.00"]]);
  assert.deepEqual(tiles[0]?.series, [1, 2, 3]);
  assert.equal(tiles[3]?.delta, "up 1d 1h");
  assert.deepEqual(meters.map((m) => [m.label, m.percent]), [["/srv", 90], ["/", 50]]);
});

test("top containers by CPU or memory, running only", () => {
  const rows = [row("a", 5, 10 * 1024 ** 2), row("b", 40, 1024 ** 2), row("c", null, null), row("off", 99, 99, "exited")];
  assert.deepEqual(topContainers(rows, live, "cpu", 2).map((r) => [r.title, r.value, r.progress]), [["b", "40%", 40], ["a", "5.0%", 5]]);
  assert.deepEqual(topContainers(rows, live, "memory", 3).map((r) => r.title), ["a", "b", "c"]);
  assert.equal(busiest(rows, live), "b");
});

test("stacks: partly running first, stopped last", () => {
  assert.deepEqual(stackRows([g("idle", 0, 3), g("ok", 2, 0), g("half", 1, 1)]).map((r) => [r.title, r.status, r.secondary]),
    [["half", "warn", "1 of 2 running"], ["ok", "up", "2 of 2 running"], ["idle", "down", "0 of 3 running"]]);
});

test("events, logs and history values", () => {
  const events = [
    { id: 1, at: 100, kind: "container.start", level: "info" as const, subject: "a", message: "Started" },
    { id: 2, at: 200, kind: "container.die", level: "error" as const, subject: "b", message: "Exited with code 1" },
    { id: 3, at: 300, kind: "alert.fired", level: "warn" as const, subject: "x", message: "" },
  ];
  assert.deepEqual(eventRows(events, 1000).map((r) => [r.title, r.status]), [["b", "down"], ["a", "up"]]);
  assert.deepEqual(logLines("2026-11-23T12:04:01.123Z GET /\n\nplain line\n2026-11-23T12:04:02Z done\n", 2), ["plain line", "12:04:02 done"]);
  assert.equal(metricValue("net_rx", 1024), "1.0 KiB/s");
  assert.equal(metricValue("temp", 41.6), "42 °C");
  assert.equal(metricValue("cpu", null), "—");
  assert.deepEqual(thin([[1, 1], [2, 2], [3, 3], [4, 4], [5, 5]], 3), [1, 3, 5]);
  assert.deepEqual(thin([[1, 1], [2, 2], [3, 3], [4, 4], [5, 5]], 2), [2, 5]);
});
