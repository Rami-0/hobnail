// The container page's CPU and memory charts (Recharts 3). The only module
// that imports Recharts; the container page reaches it through a lazy
// import, so Recharts is its own chunk and stays out of the first route.
//
// Recharts draws the lines, axes, grid, tooltip and crosshair. The max
// band uses its axis scales. Recharts also provides the
// keyboard tooltip (accessibilityLayer), and the linked crosshair across
// both charts (synced by timestamp; both charts share a source row array, see
// series.ts `chartRows`). Lines break at the empty rows chartRows puts into
// gaps (`connectNulls` off), and the tooltip there says "no data".
// Custom code: drag to zoom (pointer events on the plot, so mouse, touch and
// pen; a tap under 8 px is no zoom; Esc cancels; the selection is an HTML
// overlay, so a drag does not re-render the chart), event markers (dashed
// lines from the axis scale hook inside the SVG, and HTML buttons above the
// plot, so they are focusable and named), local-time
// ticks (series.ts `ticks`), lone points as dots, and the legend toggles
// (buttons, remembered in localStorage ).
import { memo, startTransition, useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
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
import { Button } from "../ui/Button.tsx";
import { bytes, pct } from "../format/index.ts";
import { chartMarkers, chartRows, lonePoints, rowsForWidth, tickLabel, ticks, yTop, zoomTo, type ChartRow, type Marker } from "./series.ts";
import "./charts.css";

export interface UsageChartsProps {
  series: Omit<SeriesSet, "range"> & Partial<Pick<SeriesSet, "range">>;
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
  mem_pct: ["Memory of limit", "pct"], log_error: ["Log errors", "scalar"], log_warn: ["Log warnings", "scalar"],
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

/** Layers switched off on every chart ("avg", "max", "cmp"). */
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

const Y_AXIS_WIDTH = 56;
const MARGIN = { top: 10, right: 10, bottom: 0, left: 0 };

// Mount and refresh one plot per frame. Recharts registers its axes and
// lines synchronously, so batching plots can block the browser.
const plotWork = new Set<() => void>();
let mountFrame = 0;
function schedulePlotWork() {
  if (mountFrame || plotWork.size === 0) return;
  mountFrame = requestAnimationFrame(() => {
    window.setTimeout(() => {
      mountFrame = 0;
      const next = plotWork.values().next().value;
      if (next) {
        plotWork.delete(next);
        next();
      }
      schedulePlotWork();
    }, 0);
  });
}

export default function UsageCharts({ series, onZoom, onMarker, recording = true, metrics }: UsageChartsProps) {
  const rows = useMemo(() => chartRows(series), [series]);
  const markers = useMemo(() => chartMarkers(series.markers, series.from, series.to), [series]);
  const { from, to } = series;
  const axis = useMemo(() => {
    const domain: [number, number] = [from, to];
    const marks = ticks(from, to);
    const labels = new Map(marks.map((t) => [t, tickLabel(t, to - from)]));
    return { domain, marks, label: (t: number) => labels.get(t) ?? tickLabel(t, to - from) };
  }, [from, to]);
  const [off, setOff] = useState<string[]>(loadOff);
  const toggle = useCallback((l: Layer) => {
    startTransition(() => setOff((o) => {
      const n = o.includes(l) ? o.filter((x) => x !== l) : [...o, l];
      try {
        localStorage.setItem(OFF_KEY, JSON.stringify(n));
      } catch {
        /* not remembered */
      }
      return n;
    }));
  }, []);
  // Another tab changed the layers.
  useEffect(() => {
    const on = (e: StorageEvent) => {
      if (e.key === OFF_KEY) setOff(loadOff());
    };
    window.addEventListener("storage", on);
    return () => window.removeEventListener("storage", on);
  }, []);
  const common = { rows, series, markers, axis, off, toggle, onZoom, onMarker, recording };
  return <div className="charts two">
    {metrics ? metrics.map((metric) => {
      const [title, unit] = metricLabel(metric.key);
      return <HistoryChart key={metric.key} metric={metric} title={title} unit={unit} {...common} />;
    }) : <>
      <OneChart kind="cpu" title="CPU" {...common} />
      <OneChart kind="mem" title="Memory" {...common} />
    </>}
  </div>;
}

function HistoryChart({ metric, series, axis, markers, ...props }: Omit<OneProps, "kind"> & { metric: HistoryMetric }) {
  const [current, setCurrent] = useState({ metric, series, axis, markers });
  useEffect(() => {
    if (current.metric === metric && current.series === series) return;
    const update = () => startTransition(() => setCurrent({ metric, series, axis, markers }));
    plotWork.add(update);
    schedulePlotWork();
    return () => { plotWork.delete(update); };
  }, [metric, series, axis, markers, current]);

  const rows = useMemo(() => chartRows({
    step: current.metric.step, cpu: current.metric.points, cpu_max: current.metric.max,
    mem: current.metric.yesterday ?? [], mem_max: null,
  }), [current.metric]);
  return <OneChart {...props} kind="cpu" rows={rows} series={current.series} axis={current.axis}
    markers={current.markers} testKey={current.metric.key} compare={current.metric.yesterday !== null} sync="history" />;
}

interface OneProps {
  kind: Kind;
  title: string;
  unit?: Unit;
  testKey?: string;
  compare?: boolean;
  sync?: string;
  rows: ChartRow[];
  series: Omit<SeriesSet, "range"> & Partial<Pick<SeriesSet, "range">>;
  markers: Marker[];
  axis: { domain: [number, number]; marks: number[]; label: (t: number) => string };
  off: string[];
  toggle: (l: Layer) => void;
  onZoom: (from: number, to: number) => void;
  onMarker: (id: number, at: number) => void;
  recording: boolean;
}

const OneChart = memo(function OneChart({ kind, title, rows, series, markers, axis, off, toggle, onZoom, onMarker, recording, unit = kind === "cpu" ? "pct" : "bytes", testKey = kind, compare = false, sync = "ctr-usage" }: OneProps) {
  const bandKey = kind === "cpu" ? "cpuBand" : "memBand";
  const values = useMemo(() => rows.flatMap((row) => row[kind] ?? []), [rows, kind]);
  const last = values.at(-1) ?? null;
  const hasBand = rows.some((row) => row[bandKey] !== null);
  const showAvg = !off.includes("avg");
  const showMax = hasBand && !off.includes("max");
  const showCompare = compare && !off.includes("cmp");
  const top = useMemo(() => Math.max(yTop(rows, kind, showMax), showCompare ? yTop(rows, "mem", false) : 0), [rows, kind, showMax, showCompare]);
  const bottom = unit === "temp" ? Math.min(0, ...values, ...rows.map((row) => showCompare ? row.mem ?? 0 : 0)) : 0;
  const yAxis = useMemo(() => [bottom, (top + bottom) / 2, top], [bottom, top]);
  const yDomain = useMemo(() => [bottom, top], [bottom, top]);
  const span = series.to - series.from;
  const wrap = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [decorated, setDecorated] = useState(false);
  const plotWidth = width - Y_AXIS_WIDTH - MARGIN.right;
  const plotRows = useMemo(
    () => plotWidth > 0 ? rowsForWidth(rows, series.from, series.to, plotWidth) : [],
    [rows, series.from, series.to, plotWidth],
  );
  const lone = useMemo(() => lonePoints(plotRows, kind), [plotRows, kind]);
  const hasValues = last !== null || (showCompare && rows.some((row) => row.mem !== null));
  const plotted = width > 0 && hasValues;
  useEffect(() => {
    if (!plotted || decorated) return;
    let timer = 0;
    const frame = requestAnimationFrame(() => {
      timer = window.setTimeout(() => startTransition(() => setDecorated(true)), 0);
    });
    return () => { cancelAnimationFrame(frame); window.clearTimeout(timer); };
  }, [plotted, decorated]);
  useEffect(() => {
    const canvas = wrap.current;
    if (!hasValues || !canvas) return;
    let measured = 0;
    let mounted = false;
    const mount = () => {
      if (measured <= 0) return;
      mounted = true;
      startTransition(() => setWidth(measured));
    };
    const size = new ResizeObserver(([entry]) => {
      if (!entry) return;
      measured = Math.floor(entry.contentRect.width);
      if (mounted) startTransition(() => setWidth(measured));
      else { plotWork.add(mount); schedulePlotWork(); }
    });
    // Mount just before the canvas enters view, including below a wrapped phone header.
    const near = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) size.observe(canvas);
      else {
        size.unobserve(canvas);
        plotWork.delete(mount);
      }
    }, { rootMargin: "160px 0px" });
    near.observe(canvas);
    return () => { near.disconnect(); size.disconnect(); plotWork.delete(mount); };
  }, [hasValues]);
  const selection = useRef<HTMLDivElement>(null);
  const drag = useRef<(() => void) | null>(null);

  useEffect(() => () => drag.current?.(), []);

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === "mouse" && e.button !== 0) return;
    if (e.target instanceof Element && e.target.closest(".mk")) return;
    const box = wrap.current;
    const sel = selection.current;
    if (!box || plotWidth <= 0 || !sel || drag.current) return;
    if (e.pointerType === "mouse") e.preventDefault();
    const r0 = box.getBoundingClientRect();
    const left = r0.left + Y_AXIS_WIDTH;
    const x0 = e.clientX - left;
    const end = (commitX: number | null) => {
      const d = drag.current;
      if (!d) return;
      d();
      drag.current = null;
      sel.hidden = true;
      if (commitX === null) return;
      const z = zoomTo(x0, commitX, plotWidth, series.from, series.to);
      if (z) onZoom(z.from, z.to);
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== e.pointerId) return;
      const x1 = ev.clientX - left;
      const a = Math.max(0, Math.min(x0, x1));
      const b = Math.min(plotWidth, Math.max(x0, x1));
      sel.hidden = b - a < 3;
      if (!sel.hidden) {
        sel.style.left = `${Y_AXIS_WIDTH + a}px`;
        sel.style.width = `${b - a}px`;
      }
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
    drag.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
    };
  };

  const summary =
    last === null
      ? `${title}: no data in this window`
      : `${title}: last ${format(unit, last)}, highest ${format(unit, Math.max(...values))}, lowest ${format(unit, Math.min(...values))}`;

  return (
    <figure className={`chart ch-${kind}`} data-testid={`chart-${testKey}`} aria-label={summary}>
      <figcaption className="ch-h">
        <span className="ch-t">{title}</span>
        <span className="ch-v num">{last === null ? "—" : format(unit, last)}</span>
        <fieldset className="ch-lg">
          <legend className="sr">{`${title} layers`}</legend>
          <Button variant="ghost" size="sm" className="lg avg" aria-pressed={showAvg} onClick={() => toggle("avg")}>
            Average
          </Button>
          {hasBand && (
            <Button variant="ghost" size="sm" className="lg max" aria-pressed={!off.includes("max")} onClick={() => toggle("max")}>
              Max
            </Button>
          )}
          {compare && <Button variant="ghost" size="sm" className="lg" aria-pressed={showCompare} onClick={() => toggle("cmp")}>Yesterday</Button>}
        </fieldset>
      </figcaption>
      {!hasValues ? (
        <p className="ch-empty muted">
          {recording ? "No data in this window." : "History is off: nothing is recorded, so there is nothing to draw."}
        </p>
      ) : (
        <div className="ch-canvas" ref={wrap} onPointerDown={onPointerDown}>
          {width > 0 && <ComposedChart
            data={plotRows}
            syncId={sync}
            syncMethod="value"
            throttleDelay={48}
            width={width}
            height={180}
            margin={MARGIN}
          >
            <PlotGrid values={yAxis} />
            <XAxis
              dataKey="t"
              type="number"
              domain={axis.domain}
              allowDataOverflow
              ticks={axis.marks}
              tickFormatter={axis.label}
              tickLine={false}
              // Our ticks are spaced already: no collision check (it measures
              // every label in the DOM), and no "nice" tick search.
              interval={0}
              niceTicks="none"
            />
            <YAxis
              domain={yDomain}
              allowDataOverflow
              ticks={yAxis}
              tickFormatter={(v: number) => v === 0 ? "0" : format(unit, v)}
              interval={0}
              niceTicks="none"
              width={Y_AXIS_WIDTH}
              tickLine={false}
              axisLine={false}
            />
            {decorated && <Tooltip
              isAnimationActive={false}
              cursor={{ className: "ch-cursor" }}
              content={<Tip kind={kind} unit={unit} compare={showCompare} span={span} markers={markers} />}
            />}
            {showMax && <Band rows={plotRows} bandKey={bandKey} />}
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
            {showAvg && <LoneDots rows={plotRows} kind={kind} indices={lone} />}
            {decorated && <MarkerLines markers={markers} />}
          </ComposedChart>}
          <div className="ch-sel" ref={selection} hidden aria-hidden="true" />
          {decorated && plotted && markers.length > 0 && (
            <div className="ch-marks" style={{ left: Y_AXIS_WIDTH, width: plotWidth }}>
              {markers.map((m) => {
                const left = ((m.at - series.from) / Math.max(1, span)) * 100;
                if (!(left >= 0 && left <= 100)) return null;
                return (
                  <Button
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
});

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

/** The tooltip body; Recharts passes the hover state (active, label, payload) into the element. */
let tipDate: Intl.DateTimeFormat | undefined;

function Tip({ kind, unit, compare, span, markers, ...p }: Partial<TooltipContentProps> & { kind: Kind; unit: Unit; compare: boolean; span: number; markers: Marker[] }) {
  if (!p.active || p.label === undefined) return null;
  const t = Number(p.label);
  // The hovered row itself (a hidden layer has no payload entry of its own).
  const row = p.payload?.[0]?.payload as ChartRow | undefined;
  const v = row?.[kind];
  const max = (kind === "cpu" ? row?.cpuBand : row?.memBand)?.[1];
  // A marker within 1 % of the window from the crosshair says what happened (OV-17).
  const mk = markers.find((m) => Math.abs(m.at - t) <= span / 100);
  const when = span <= 86400 ? tickLabel(t, span)
    : (tipDate ??= new Intl.DateTimeFormat([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23" })).format(t * 1000);
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
