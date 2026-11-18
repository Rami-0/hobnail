// The container page's CPU and memory charts (Recharts 3). The only module
// that imports Recharts; the container page reaches it through a lazy
// import, so Recharts is its own chunk and stays out of the first route.
//
// Recharts draws the lines, axes, grid, tooltip and crosshair. The max
// band uses its axis scales. Recharts also provides the
// keyboard tooltip (accessibilityLayer), and the linked crosshair across
// both charts (`syncId`, index-synced: both charts share one row array, see
// series.ts `chartRows`). Lines break at the empty rows chartRows puts into
// gaps (`connectNulls` off), and the tooltip there says "no data".
// Custom code: drag to zoom (pointer events on the plot, so mouse, touch and
// pen; a tap under 8 px is no zoom; Esc cancels; the selection is an HTML
// overlay, so a drag does not re-render the chart), event markers (dashed
// lines from the axis scale hook inside the SVG, and HTML buttons above the
// plot, so they are focusable and named), local-time
// ticks (series.ts `ticks`), lone points as dots, and the legend toggles
// (buttons, remembered in localStorage under the old UI's key).
import { startTransition, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  ComposedChart,
  Line,
  Tooltip,
  usePlotArea,
  useXAxisScale,
  useYAxisScale,
  XAxis,
  YAxis,
  type TooltipContentProps,
} from "recharts";

import type { HistoryMetric } from "../api/gen/HistoryMetric.ts";
import type { SeriesSet } from "../api/gen/SeriesSet.ts";
import { bytes, pct } from "../format/index.ts";
import { chartMarkers, chartRows, lonePoints, tickLabel, ticks, yTop, zoomTo, type ChartRow, type Marker } from "./series.ts";
import "./charts.css";

export interface UsageChartsProps {
  series: SeriesSet;
  /** A drag selected this window. */
  onZoom: (from: number, to: number) => void;
  /** A marker was activated: open its timeline row (`id`), at `at`. */
  onMarker: (id: number, at: number) => void;
  /** History is recording at all (else the empty charts say so). */
  recording?: boolean;
  metrics?: HistoryMetric[];
}

type Kind = "cpu" | "mem";
type Layer = "avg" | "max" | "cmp";
type Unit = "pct" | "bytes" | "rate" | "scalar" | "temp";
const HISTORY: Record<string, [string, Unit]> = {
  cpu: ["CPU", "pct"], load1: ["Load (1 min)", "scalar"], mem: ["Memory", "bytes"],
  swap: ["Swap", "bytes"], disk: ["Disk used", "pct"], temp: ["Temperature", "temp"],
  net_rx: ["Network in", "rate"], net_tx: ["Network out", "rate"],
  throttled: ["Throttled periods", "pct"], cache: ["Page cache", "bytes"],
  anon: ["Anonymous memory", "bytes"], memory_limit: ["Memory limit", "bytes"],
  oom: ["OOM kills", "scalar"], block_read: ["Block read", "rate"],
  block_write: ["Block write", "rate"], pids: ["PIDs", "scalar"],
};

function metricLabel(key: string): [string, Unit] {
  const known = HISTORY[key];
  if (known) return known;
  const prefix = key.split(".")[0];
  if (prefix === "cpu" || prefix === "psi" || key.endsWith(".util")) return [key, "pct"];
  if (prefix === "temp") return [key.slice(5), "temp"];
  if (prefix === "mount" || prefix === "mem") return [key, "bytes"];
  if (prefix === "net" || prefix === "disk") return [key, "rate"];
  return [key, "scalar"];
}

/** The old UI's key: layers switched off on every chart ("avg", "max", "cmp"). */
const OFF_KEY = "hb.chartOff";
const LAYERS = new Set(["avg", "max", "cmp"]);

function loadOff(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(OFF_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((s): s is string => typeof s === "string" && LAYERS.has(s)) : [];
  } catch {
    return [];
  }
}

function axisTrim(v: number): string {
  return String(Math.round(v * 100) / 100);
}

function format(unit: Unit, v: number): string {
  switch (unit) {
    case "pct": return pct(v);
    case "bytes": return bytes(v);
    case "rate": return `${bytes(v)}/s`;
    case "temp": return `${axisTrim(v)} °C`;
    case "scalar": return axisTrim(v);
  }
}

// Intl's date formatting is slow the first time (locale data): warm it in a
// task of its own, before the first chart asks for tick labels.
window.setTimeout(() => tickLabel(Math.floor(Date.now() / 1000), 3600), 0);

export default function UsageCharts({ series, onZoom, onMarker, recording = true, metrics }: UsageChartsProps) {
  const rows = useMemo(() => chartRows(series), [series]);
  const markers = useMemo(() => chartMarkers(series.markers, series.from, series.to), [series]);
  const xt = useMemo(() => ticks(series.from, series.to), [series.from, series.to]);
  const [off, setOff] = useState<string[]>(loadOff);
  const toggle = useCallback((l: Layer) => {
    setOff((o) => {
      const n = o.includes(l) ? o.filter((x) => x !== l) : [...o, l];
      try {
        localStorage.setItem(OFF_KEY, JSON.stringify(n));
      } catch {
        /* not remembered */
      }
      return n;
    });
  }, []);
  // Another tab (or the old UI) changed the layers.
  useEffect(() => {
    const on = (e: StorageEvent) => {
      if (e.key === OFF_KEY) setOff(loadOff());
    };
    window.addEventListener("storage", on);
    return () => window.removeEventListener("storage", on);
  }, []);
  // One chart per task: the first after the page's head has painted, the
  // second in the next task (a first Recharts render is the costly part).
  const [stage, setStage] = useState(0);
  useEffect(() => {
    if (stage >= (metrics?.length ?? 2)) return;
    const t = window.setTimeout(() => startTransition(() => setStage((n) => n + 1)), 0);
    return () => window.clearTimeout(t);
  }, [stage, metrics?.length]);
  const common = { rows, series, markers, xt, off, toggle, onZoom, onMarker, recording };
  if (metrics) return <div className="charts two">
    {metrics.map((metric, i) => {
      const [title, unit] = metricLabel(metric.key);
      return i < stage
        ? <HistoryChart key={metric.key} metric={metric} title={title} unit={unit} {...common} />
        : <div key={metric.key} className="chart ch-wait" aria-busy="true" />;
    })}
  </div>;
  return (
    <div className="charts two">
      {stage >= 1 ? <OneChart kind="cpu" title="CPU" {...common} /> : <div className="chart ch-wait" aria-busy="true" />}
      {stage >= 2 ? <OneChart kind="mem" title="Memory" {...common} /> : <div className="chart ch-wait" aria-busy="true" />}
    </div>
  );
}

function HistoryChart({ metric, ...props }: Omit<OneProps, "kind"> & { metric: HistoryMetric }) {
  const rows = useMemo(() => chartRows({
    step: metric.step, cpu: metric.points, cpu_max: metric.max, mem: metric.yesterday ?? [], mem_max: null,
  }), [metric]);
  return <OneChart {...props} kind="cpu" rows={rows} testKey={metric.key} compare={metric.yesterday !== null} sync="history" />;
}

interface OneProps {
  kind: Kind;
  title: string;
  unit?: Unit;
  testKey?: string;
  compare?: boolean;
  sync?: string;
  rows: ChartRow[];
  series: SeriesSet;
  markers: Marker[];
  xt: number[];
  off: string[];
  toggle: (l: Layer) => void;
  onZoom: (from: number, to: number) => void;
  onMarker: (id: number, at: number) => void;
  recording: boolean;
}

interface Plot {
  x: number;
  width: number;
}

function OneChart({ kind, title, rows, series, markers, xt, off, toggle, onZoom, onMarker, recording, unit = kind === "cpu" ? "pct" : "bytes", testKey = kind, compare = false, sync = "ctr-usage" }: OneProps) {
  const bandKey = kind === "cpu" ? "cpuBand" : "memBand";
  const hasBand = rows.some((r) => r[bandKey] !== null);
  const showAvg = !off.includes("avg");
  const showMax = hasBand && !off.includes("max");
  const showCompare = compare && !off.includes("cmp");
  const top = useMemo(() => Math.max(yTop(rows, kind, showMax), showCompare ? yTop(rows, "mem", false) : 0), [rows, kind, showMax, showCompare]);
  const bottom = unit === "temp" ? Math.min(0, ...rows.map((row) => row[kind] ?? 0), ...rows.map((row) => showCompare ? row.mem ?? 0 : 0)) : 0;
  const lone = useMemo(() => lonePoints(rows, kind), [rows, kind]);
  const values = rows.flatMap((r) => (r[kind] === null ? [] : [r[kind] as number]));
  const last = values.length ? (values[values.length - 1] as number) : null;
  const span = series.to - series.from;
  const plot = useRef<Plot | null>(null);
  // The plot's place, for the marker buttons above it (and the drag, via the ref).
  const [layout, setLayout] = useState<Plot | null>(null);
  const onPlot = useCallback((p: Plot) => {
    plot.current = p;
    setLayout((l) => (l && l.x === p.x && l.width === p.width ? l : p));
  }, []);
  const wrap = useRef<HTMLDivElement>(null);
  const [sel, setSel] = useState<{ left: number; width: number } | null>(null);
  const drag = useRef<{ x0: number; id: number; cleanup: () => void } | null>(null);

  useEffect(() => () => drag.current?.cleanup(), []);

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === "mouse" && e.button !== 0) return;
    if (e.target instanceof Element && e.target.closest(".mk")) return;
    const box = wrap.current;
    const p = plot.current;
    if (!box || !p || drag.current) return;
    if (e.pointerType === "mouse") e.preventDefault();
    const r0 = box.getBoundingClientRect();
    const left = r0.left + p.x;
    const x0 = e.clientX - left;
    const end = (commitX: number | null) => {
      const d = drag.current;
      if (!d) return;
      d.cleanup();
      drag.current = null;
      setSel(null);
      if (commitX === null) return;
      const z = zoomTo(d.x0, commitX, p.width, series.from, series.to);
      if (z) onZoom(z.from, z.to);
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== e.pointerId) return;
      const x1 = ev.clientX - left;
      const a = Math.max(0, Math.min(x0, x1));
      const b = Math.min(p.width, Math.max(x0, x1));
      setSel(b - a >= 3 ? { left: p.x + a, width: b - a } : null);
    };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId === e.pointerId) end(ev.clientX - left);
    };
    const cancel = (ev: PointerEvent) => {
      if (ev.pointerId === e.pointerId) end(null);
    };
    const key = (ev: globalThis.KeyboardEvent) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        ev.stopPropagation();
        end(null);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
    drag.current = {
      x0,
      id: e.pointerId,
      cleanup: () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", cancel);
        window.removeEventListener("keydown", key, true);
      },
    };
  };

  const summary =
    values.length === 0
      ? `${title}: no data in this window`
      : `${title}: last ${format(unit, last ?? 0)}, highest ${format(unit, Math.max(...values))}, lowest ${format(unit, Math.min(...values))}`;

  return (
    <figure className={`chart ch-${kind}`} data-testid={`chart-${testKey}`} aria-label={summary}>
      <figcaption className="ch-h">
        <span className="ch-t">{title}</span>
        <span className="ch-v num">{last === null ? "—" : format(unit, last)}</span>
        <fieldset className="ch-lg">
          <legend className="sr">{`${title} layers`}</legend>
          <button type="button" className="lg avg" aria-pressed={showAvg} onClick={() => toggle("avg")}>
            Average
          </button>
          {hasBand && (
            <button type="button" className="lg max" aria-pressed={!off.includes("max")} onClick={() => toggle("max")}>
              Max
            </button>
          )}
          {compare && <button type="button" className="lg" aria-pressed={showCompare} onClick={() => toggle("cmp")}>Yesterday</button>}
        </fieldset>
      </figcaption>
      {values.length === 0 && !rows.some((r) => showCompare && r.mem !== null) ? (
        <p className="ch-empty muted">
          {recording ? "No data in this window." : "History is off: nothing is recorded, so there is nothing to draw."}
        </p>
      ) : (
        <div className="ch-canvas" ref={wrap} onPointerDown={onPointerDown}>
          <ComposedChart
            data={rows}
            syncId={sync}
            syncMethod={sync === "history" ? "value" : "index"}
            responsive
            width="100%"
            height={180}
            margin={{ top: 10, right: 10, bottom: 0, left: 0 }}
          >
            <PlotGrid values={[bottom, (top + bottom) / 2, top]} />
            <XAxis
              dataKey="t"
              type="number"
              domain={[series.from, series.to]}
              allowDataOverflow
              ticks={xt}
              tickFormatter={(t: number) => tickLabel(t, span)}
              tickLine={false}
              // Our ticks are spaced already: no collision check (it measures
              // every label in the DOM), and no "nice" tick search.
              interval={0}
              niceTicks="none"
            />
            <YAxis
              domain={[bottom, top]}
              allowDataOverflow
              ticks={[bottom, (top + bottom) / 2, top]}
              tickFormatter={(v: number) => v === 0 ? "0" : format(unit, v)}
              interval={0}
              niceTicks="none"
              width={56}
              tickLine={false}
              axisLine={false}
            />
            <Tooltip
              isAnimationActive={false}
              cursor={{ className: "ch-cursor" }}
              content={<Tip kind={kind} unit={unit} compare={showCompare} span={span} markers={markers} />}
            />
            {showMax && <Band rows={rows} bandKey={bandKey} />}
            {showAvg && (
              <Line
                dataKey={kind}
                className="ch-line"
                stroke="currentColor"
                strokeWidth={1.6}
                dot={false}
                activeDot={{ r: 3 }}
                isAnimationActive={false}
                connectNulls={false}
                name={title}
              />
            )}
            {showCompare && <Line dataKey="mem" stroke="currentColor" strokeOpacity={0.55} strokeDasharray="4 3" name="Yesterday" dot={false} isAnimationActive={false} />}
            {showAvg && <LoneDots rows={rows} kind={kind} indices={lone} />}
            <MarkerLines markers={markers} />
            <PlotProbe onPlot={onPlot} />
          </ComposedChart>
          {sel && <div className="ch-sel" style={{ left: sel.left, width: sel.width }} aria-hidden="true" />}
          {layout && (
            <div className="ch-marks" style={{ left: layout.x, width: layout.width }}>
              {markers.map((m) => {
                const left = ((m.at - series.from) / Math.max(1, span)) * 100;
                if (!(left >= 0 && left <= 100)) return null;
                return (
                  <button
                    key={m.id}
                    type="button"
                    className={`mk ${m.level}`}
                    style={{ left: `${left}%` }}
                    title={m.label}
                    aria-label={m.label}
                    onClick={() => onMarker(m.id, m.at)}
                  />
                );
              })}
            </div>
          )}
        </div>
      )}
    </figure>
  );
}

function PlotGrid({ values }: { values: number[] }) {
  const plot = usePlotArea();
  const y = useYAxisScale();
  if (!plot || !y) return null;
  return <g className="ch-grid" aria-hidden="true">{values.map((value) => {
    const py = y(value);
    return <line key={value} x1={plot.x} x2={plot.x + plot.width} y1={py} y2={py} />;
  })}</g>;
}

// A range band and isolated samples use the same Recharts scales as the line.
function Band({ rows, bandKey }: { rows: ChartRow[]; bandKey: "cpuBand" | "memBand" }) {
  const x = useXAxisScale();
  const y = useYAxisScale();
  if (!x || !y) return null;
  let high: string[] = [];
  let low: string[] = [];
  let path = "";
  const close = () => {
    if (high.length) path += `M${high.join("L")}L${low.toReversed().join("L")}Z`;
    high = [];
    low = [];
  };
  for (const row of rows) {
    const band = row[bandKey];
    const left = x(row.t);
    if (!band || left === undefined) { close(); continue; }
    high.push(`${left},${y(band[1])}`);
    low.push(`${left},${y(band[0])}`);
  }
  close();
  return <path d={path} fill="currentColor" fillOpacity={.16} aria-hidden="true" />;
}

function LoneDots({ rows, kind, indices }: { rows: ChartRow[]; kind: Kind; indices: number[] }) {
  const x = useXAxisScale();
  const y = useYAxisScale();
  if (!x || !y) return null;
  return <g aria-hidden="true">{indices.map((index) => {
    const row = rows[index];
    return row && <circle key={row.t} cx={x(row.t)} cy={y(row[kind])} r={2.5} fill="currentColor" />;
  })}</g>;
}

/** Tells the chart where its plot is (for the drag), from Recharts' layout. */
function PlotProbe({ onPlot }: { onPlot: (p: Plot) => void }) {
  const a = usePlotArea();
  const x = a?.x;
  const w = a?.width;
  useLayoutEffect(() => {
    if (x !== undefined && w !== undefined) onPlot({ x, width: w });
  }, [x, w, onPlot]);
  return null;
}

/** The tooltip body; Recharts passes the hover state (active, label, payload) into the element. */
function Tip({ kind, unit, compare, span, markers, ...p }: Partial<TooltipContentProps> & { kind: Kind; unit: Unit; compare: boolean; span: number; markers: Marker[] }) {
  if (!p.active || p.label === undefined) return null;
  const t = Number(p.label);
  // The hovered row itself (a hidden layer has no payload entry of its own).
  const row = p.payload?.[0]?.payload as ChartRow | undefined;
  const v = row?.[kind];
  const max = (kind === "cpu" ? row?.cpuBand : row?.memBand)?.[1];
  // A marker within 1 % of the window from the crosshair says what happened (OV-17).
  const mk = markers.find((m) => Math.abs(m.at - t) <= span / 100);
  const d = new Date(t * 1000);
  const when =
    span <= 86400
      ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })
      : d.toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  return (
    <div className="ch-tip">
      <span className="ch-tip-t">{when}</span>
      <span className="ch-tip-v">{typeof v === "number" ? format(unit, v) : "no data"}</span>
      {typeof max === "number" && <span className="ch-tip-m">max {format(unit, max)}</span>}
      {compare && typeof row?.mem === "number" && <span className="ch-tip-m">yesterday {format(unit, row.mem)}</span>}
      {mk && <span className="ch-tip-mk">{mk.label}</span>}
    </div>
  );
}

/** The markers' dashed lines across the plot (the flags are buttons above it). */
function MarkerLines({ markers }: { markers: Marker[] }) {
  const x = useXAxisScale();
  const plot = usePlotArea();
  if (!x || !plot) return null;
  return (
    <g className="ch-mlines" aria-hidden="true">
      {markers.map((m) => {
        const px = x(m.at);
        if (px === undefined || !Number.isFinite(px)) return null;
        return <line key={m.id} className={`mk-l ${m.level}`} x1={px} x2={px} y1={plot.y} y2={plot.y + plot.height} />;
      })}
    </g>
  );
}
