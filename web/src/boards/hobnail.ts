// Hobnail's own board widgets: pure builders from the data the client
// already has (host sample, vitals rings, container list, history, log
// text) to the structured bodies the shared renderer draws. The hooks live
// in HobnailWidget.tsx.
import type { BodyRow } from "../api/gen/BodyRow.ts";
import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import type { EventRow } from "../api/gen/EventRow.ts";
import type { GroupRow } from "../api/gen/GroupRow.ts";
import type { Health } from "../api/gen/Health.ts";
import type { MeterRow } from "../api/gen/MeterRow.ts";
import type { StatTile } from "../api/gen/StatTile.ts";
import type { SystemView } from "../api/gen/SystemView.ts";
import { ago, bytes, duration, pct } from "../format/index.ts";

const MAX_SERIES = 48;
const newest = (values: readonly number[]) => values.slice(-MAX_SERIES);
const round = (n: number) => Math.max(0, Math.min(100, Math.round(n)));

/** Bytes per second as "1.2 MiB/s". */
export const rate = (bps: number | null) => bps === null ? "—" : `${bytes(bps)}/s`;

export interface Trends { cpu: readonly number[]; mem: readonly number[]; net: readonly number[] }

/** Host overview: CPU, memory, network and load tiles, then the fullest disks. */
export function hostBody(system: SystemView, trends: Trends): { tiles: StatTile[]; meters: MeterRow[] } {
  const memPct = system.mem_total ? (system.mem_used / system.mem_total) * 100 : 0;
  const tiles: StatTile[] = [
    { label: "CPU", value: pct(system.cpu_pct), glyph: "cpu", delta: `${system.cores} cores`, series: newest(trends.cpu) },
    { label: "Memory", value: pct(memPct), glyph: "memory", delta: `${bytes(system.mem_used)} of ${bytes(system.mem_total)}`, series: newest(trends.mem) },
    { label: "Network", value: `↓ ${rate(system.net_rx_bps)}`, glyph: "activity", delta: `↑ ${rate(system.net_tx_bps)}`, series: newest(trends.net) },
    { label: "Load", value: system.load.map((n) => n.toFixed(2)).join(" · "), glyph: "server", delta: `up ${duration(system.uptime_secs)}` },
  ];
  if (system.temp_c !== null) tiles.push({ label: "Temperature", value: `${Math.round(system.temp_c)} °C`, delta: "CPU package" });
  const meters = system.disks
    .filter((disk) => disk.total > 0)
    .map((disk) => ({ disk, used: (disk.used / disk.total) * 100 }))
    .toSorted((a, b) => b.used - a.used)
    .slice(0, 3)
    .map(({ disk, used }) => ({ label: disk.label, percent: round(used), detail: `${bytes(disk.used)} of ${bytes(disk.total)}` }));
  return { tiles, meters };
}

const DOT: Record<string, Health> = { ok: "up", warn: "warn", bad: "down" };
export const rowHealth = (row: Pick<ContainerRow, "dot" | "state">): Health =>
  DOT[row.dot] ?? (row.state === "running" ? "up" : "down");

export interface Live { cpu: number | null; mem: number | null }

/** The busiest running containers by CPU or memory. */
export function topContainers(rows: readonly ContainerRow[], live: (row: ContainerRow) => Live, sort: string, count: number): BodyRow[] {
  const byMemory = sort === "memory";
  return rows
    .filter((row) => row.state === "running")
    .map((row) => { const stat = live(row); return { row, cpu: stat.cpu, mem: stat.mem }; })
    .toSorted((a, b) => (byMemory ? (b.mem ?? -1) - (a.mem ?? -1) : (b.cpu ?? -1) - (a.cpu ?? -1)) || a.row.name.localeCompare(b.row.name))
    .slice(0, count)
    .map(({ row, cpu, mem }) => {
      const out: BodyRow = {
        title: row.title,
        secondary: [row.stack, mem === null ? null : bytes(mem)].filter(Boolean).join(" · "),
        value: byMemory ? (mem === null ? "—" : bytes(mem)) : pct(cpu),
        status: rowHealth(row),
      };
      if (!byMemory) out.progress = cpu === null ? 0 : round(cpu);
      return out;
    });
}

/** Each stack, running of total: partly running first, then healthy, then stopped. */
const stackRank = (g: GroupRow) => g.running === 0 ? 2 : g.stopped > 0 ? 0 : 1;

export function stackRows(groups: readonly GroupRow[]): BodyRow[] {
  return groups
    .filter((group) => group.containers.length > 0)
    .toSorted((a, b) => stackRank(a) - stackRank(b) || a.title.localeCompare(b.title))
    .map((group) => {
      const total = group.running + group.stopped;
      const health: Health = group.running === 0 ? "down" : group.stopped > 0 ? "warn" : "up";
      return { title: group.title, secondary: `${group.running} of ${total} running`, status: health, progress: total ? round((group.running / total) * 100) : 0 };
    });
}

const LEVEL: Record<EventRow["level"], Health> = { info: "up", warn: "warn", error: "down" };

/** Container events, newest first. */
export function eventRows(events: readonly EventRow[], now: number, limit = 8): BodyRow[] {
  return events
    .filter((event) => event.kind.startsWith("container."))
    .toSorted((a, b) => b.at - a.at)
    .slice(0, limit)
    .map((event) => ({ title: event.subject, secondary: event.message, value: ago(event.at, now), status: LEVEL[event.level] }));
}

/** The busiest running container, for a log tail without a chosen one. */
export function busiest(rows: readonly ContainerRow[], live: (row: ContainerRow) => Live): string | null {
  return rows.filter((row) => row.state === "running")
    .map((row) => ({ name: row.name, cpu: live(row).cpu ?? -1 }))
    .toSorted((a, b) => b.cpu - a.cpu || a.name.localeCompare(b.name))[0]?.name ?? null;
}

/** The last `count` lines of a log download, with ISO stamps cut to the time. */
export function logLines(text: string, count: number): string[] {
  return text.split("\n").filter((line) => line.trim() !== "").slice(-count)
    .map((line) => line.replace(/^\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})(\.\d+)?Z?\s/, "$1 ").slice(0, 400));
}

export const METRIC_LABEL: Record<string, string> = {
  cpu: "CPU", mem: "Memory", disk: "Disk", load1: "Load", net_rx: "Network in", net_tx: "Network out", temp: "Temperature", swap: "Swap",
};

/** A history value as text, by series. */
export function metricValue(metric: string, value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  if (metric === "net_rx" || metric === "net_tx") return rate(value);
  if (metric === "load1") return value.toFixed(2);
  if (metric === "temp") return `${Math.round(value)} °C`;
  return pct(value);
}

/** Percent series have a fixed 0–100 scale; the rest scale to their peak. */
export const percentMetric = (metric: string) => ["cpu", "mem", "disk", "swap"].includes(metric);

/** Points thinned to at most `max` values (every n-th, keeping the newest). */
export function thin(points: readonly (readonly [number, number])[], max = 120): number[] {
  const step = Math.max(1, Math.ceil(points.length / max));
  const out: number[] = [];
  for (let i = points.length - 1; i >= 0; i -= step) out.push(points[i]?.[1] ?? 0);
  return out.toReversed();
}

export interface Counts { running: number; stopped: number; problems: number; firing: number; cleanStops: number }

/** Needs attention: container counts from the live list when it is there
 * (else the overview read), and the alerts firing now. */
export function attentionCounts(groups: readonly GroupRow[] | null, view: { running: number; stopped: number; problems: number; firing: number }): Counts {
  if (!groups) return { ...view, cleanStops: 0 };
  const rows = groups.flatMap((group) => group.containers);
  return {
    running: groups.reduce((n, group) => n + group.running, 0),
    stopped: groups.reduce((n, group) => n + group.stopped, 0),
    problems: rows.filter((row) => row.attention !== null).length,
    firing: view.firing,
    cleanStops: rows.filter((row) => row.state !== "running" && row.attention === null).length,
  };
}
