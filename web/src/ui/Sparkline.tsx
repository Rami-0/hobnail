// Plain-SVG data marks (no Recharts; the charts chunk stays lazy):
// Sparkline (vitals chips, table cells, tiles), HealthStrip (24 h status
// bar), Meter (a used-of-total bar). Stretched to their box; CSS sets the
// size (static/app.css `.spark`, `.hstrip`, `.meter`). The drawing rules
// are in svg.ts.
import type { CSSProperties } from "react";

import { sparkPaths, stripClass, stripRuns, stripSummary } from "./svg.ts";

export interface SparklineProps {
  values: readonly (number | null)[];
  /** Top of the scale (100 for a percentage); default: the highest value. */
  max?: number;
  /** What it shows, for screen readers; "" hides it from them (a chip that says it in words). */
  label: string;
  width?: number;
  height?: number;
  /** The series colour token, e.g. "--ch-cpu" (default: the accent). */
  color?: string;
  className?: string;
}

export function Sparkline({ values, max, label, width, height, color, className }: SparklineProps) {
  const p = sparkPaths(values, max);
  return (
    <svg
      className={["spark", className ?? ""].filter(Boolean).join(" ")}
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      width={width}
      height={height}
      style={color ? ({ "--s": `var(${color})` } as CSSProperties) : undefined}
      role={label ? "img" : undefined}
      aria-label={label || undefined}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <path className="base" d="M0 23.5H100" />
      {p && <path className="area" d={p.area} />}
      {p && <path className="line" d={p.line} />}
    </svg>
  );
}

export interface HealthStripProps {
  /** One code per bucket, oldest first (GET /api/v1/strips; svg.ts STRIP). */
  codes: readonly number[];
  /** The subject, for the accessible summary ("web: running 98% of the last 24 h"). */
  label: string;
  /** Seconds per bucket (default 900, the strips' 15 min). */
  step?: number;
  /** One rect per bucket with hairline gaps (wide strips) instead of merged runs. */
  cells?: boolean;
  className?: string;
}

const WORD: Record<string, string> = { u: "no data", f: "not running", o: "running", w: "restarted", b: "problem" };

export function HealthStrip({ codes, label, step = 900, cells, className }: HealthStripProps) {
  const buckets = codes.length ? codes : Array<number>(Math.max(1, Math.ceil(86400 / step))).fill(0);
  const n = buckets.length;
  const ago = (i: number) => {
    const s = (n - i) * step;
    const h = Math.round(s / 360) / 10;
    return s <= 0 ? "now" : h >= 1 ? `${h} h ago` : `${Math.round(s / 60)} min ago`;
  };
  const rects = cells ? buckets.map((c, i) => [i, i, c] as [number, number, number]) : stripRuns(buckets);
  return (
    <svg
      className={["hstrip", className ?? ""].filter(Boolean).join(" ")}
      viewBox={`0 0 ${n} 8`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`${label}: ${stripSummary(buckets, step)}`}
    >
      {rects.map(([a, b, c]) => {
        const cls = stripClass(c);
        return (
          <rect key={a} className={cls} x={a} y={0} width={cells ? 0.82 : b + 1 - a} height={8}>
            <title>{`${ago(a)} – ${b === n - 1 ? "now" : ago(b + 1)}: ${WORD[cls]}`}</title>
          </rect>
        );
      })}
    </svg>
  );
}

export interface MeterProps {
  value: number;
  max: number;
  label: string;
  className?: string;
}

/** A used-of-total bar; amber from 80 %, red from 90 % (the vitals thresholds). */
export function Meter({ value, max, label, className }: MeterProps) {
  const pct = max > 0 && Number.isFinite(value) ? Math.max(0, Math.min(100, (value / max) * 100)) : 0;
  const lv = pct >= 90 ? "bad" : pct >= 80 ? "warn" : "";
  return (
    <svg
      viewBox="0 0 100 4" preserveAspectRatio="none"
      className={["meter", lv, className ?? ""].filter(Boolean).join(" ")}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={`${Math.round(pct)}%`}
    >
      <rect width={pct} height="4" />
    </svg>
  );
}
