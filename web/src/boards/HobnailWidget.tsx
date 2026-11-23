// A Hobnail widget on a board: the view the server named, drawn from the
// live host sample, the container list and stats, the history API and the
// log download. Builders are in hobnail.ts; drawing is the shared renderer.
import { useEffect, useState } from "react";
import type { HobnailOptions } from "../api/gen/HobnailOptions.ts";
import type { HistoryView } from "../api/gen/HistoryView.ts";
import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import { useContainers, useNow, useStatsMap, useSystem, useVitalsRings } from "../live/index.ts";
import { useRead } from "../live/read.ts";
import { StructuredWidget } from "../ui/StructuredWidget.ts";
import { Sparkline } from "../ui/Sparkline.tsx";
import { busiest, eventRows, hostBody, logLines, METRIC_LABEL, metricValue, percentMetric, stackRows, thin, topContainers, type Live } from "./hobnail.ts";

const LOG_POLL = 20_000;
const CHART_COLOR: Record<string, string> = { cpu: "cpu", mem: "mem", disk: "disk", load1: "load", net_rx: "rx", net_tx: "tx", temp: "temp", swap: "swap" };
const HISTORY_POLL = 60_000;

const waiting = (text: string) => <p className="tile-note">{text}</p>;

function useLive(): (row: ContainerRow) => Live {
  const map = useStatsMap(true);
  return (row) => {
    const stat = map.get(row.name);
    return { cpu: stat?.cpu_pct ?? row.cpu_pct, mem: stat?.mem_bytes ?? row.mem_bytes };
  };
}

function Host() {
  const system = useSystem();
  const rings = useVitalsRings();
  if (!system) return waiting("Waiting for the first host sample…");
  const { tiles, meters } = hostBody(system, { cpu: rings.cpu.v, mem: rings.mem.v, net: rings.net.v });
  return <div className="hw-stack">
    <StructuredWidget body={{ kind: "tiles", tiles }} />
    {meters.length > 0 && <StructuredWidget body={{ kind: "meters", rows: meters }} />}
  </div>;
}

function useRows() {
  const { data, error } = useContainers();
  return { groups: data?.groups ?? null, rows: data?.groups.flatMap((group) => group.containers) ?? null, error };
}

function Containers({ options }: { options: HobnailOptions }) {
  const { rows, error } = useRows();
  const live = useLive();
  if (!rows) return waiting(error ?? "Loading containers…");
  const list = topContainers(rows, live, options.sort ?? "cpu", options.count ?? 6);
  return list.length ? <StructuredWidget body={{ kind: "rows", rows: list }} /> : waiting("No container is running.");
}

function Stacks() {
  const { groups, error } = useRows();
  if (!groups) return waiting(error ?? "Loading stacks…");
  return <StructuredWidget body={{ kind: "rows", rows: stackRows(groups) }} />;
}

function Events() {
  const now = useNow();
  const { data, error } = useRead<HistoryView>("/api/v1/history?range=24h&metric=none&events=all", undefined, HISTORY_POLL);
  if (!data) return waiting(error || "Loading events…");
  const rows = eventRows(data.events, now);
  return rows.length ? <StructuredWidget body={{ kind: "rows", rows }} /> : waiting("No container events in the last 24 hours.");
}

function Logs({ options }: { options: HobnailOptions }) {
  const { rows } = useRows();
  const live = useLive();
  const name = options.container || (rows ? busiest(rows, live) : null);
  const count = options.count ?? 12;
  const [state, setState] = useState<{ name: string | null; lines: string[] | null; error: string }>({ name: null, lines: null, error: "" });
  useEffect(() => {
    if (!name) return;
    const controller = new AbortController();
    const load = async () => {
      if (document.hidden) return;
      try {
        const response = await fetch(`/api/logs/download?c=${encodeURIComponent(name)}&tail=${count}`, { credentials: "same-origin", signal: controller.signal });
        const text = await response.text();
        if (controller.signal.aborted) return;
        setState(response.ok ? { name, lines: logLines(text, count), error: "" } : { name, lines: null, error: text.trim() || `HTTP ${response.status}` });
      } catch {
        if (!controller.signal.aborted) setState((previous) => ({ ...previous, error: "Could not read the log." }));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), LOG_POLL);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [name, count]);
  if (!name) return waiting(rows ? "No container is running." : "Loading containers…");
  const lines = state.name === name ? state.lines : null;
  return <div className="hw-logs" data-testid="hobnail-logs">
    <p className="hw-caption">{name}{!options.container && " · busiest now"}</p>
    {lines ? (lines.length ? <pre className="hw-log">{lines.join("\n")}</pre> : waiting("No log lines yet."))
      : waiting(state.error || "Reading the log…")}
  </div>;
}

function History({ options }: { options: HobnailOptions }) {
  const metric = options.metric ?? "cpu";
  const range = options.range ?? "6h";
  const { data, error } = useRead<HistoryView>(`/api/v1/history?range=${encodeURIComponent(range)}&metric=${encodeURIComponent(metric)}`, undefined, HISTORY_POLL);
  if (!data) return waiting(error || "Loading history…");
  const points = data.metrics[0]?.points ?? [];
  if (!points.length) return waiting(data.recording ? "No samples in this range yet." : "History recording is off.");
  const values = thin(points);
  const peak = Math.max(...points.map(([, v]) => v));
  const label = `${METRIC_LABEL[metric] ?? metric}, last ${range}`;
  return <div className="hw-history" data-testid="hobnail-history">
    <div className="hw-history-head">
      <span className="hw-caption">{label}</span>
      <strong>{metricValue(metric, points.at(-1)?.[1])}</strong>
      <span className="hw-caption">peak {metricValue(metric, peak)}</span>
    </div>
    <Sparkline className="hw-chart" values={values} max={percentMetric(metric) ? 100 : undefined} label={label} color={`--ch-${CHART_COLOR[metric] ?? "cpu"}`} />
  </div>;
}

export function HobnailWidget({ view, options }: { view: string; options: HobnailOptions }) {
  return <div className="widget-content hobnail-widget" data-view={view}>{
    view === "host" ? <Host />
      : view === "containers" ? <Containers options={options} />
        : view === "stacks" ? <Stacks />
          : view === "events" ? <Events />
            : view === "logs" ? <Logs options={options} />
              : view === "history" ? <History options={options} />
                : waiting("This Hobnail widget needs a newer page. Reload to update.")
  }</div>;
}
