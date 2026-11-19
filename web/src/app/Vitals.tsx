// The top bar's host vitals (src/ui/shell.rs vitals): CPU, memory and the
// first disk in percent, and network throughput, each with a sparkline of
// the last points (seeded from the last hour of history, then live from the
// `system` topic). They link to the Host page of the current interface.
// Amber from 80 %, red from 90 %, said in words too.
import { useEffect, type ReactNode } from "react";

import { useBootstrap, useSystem, useVitalsRings, vitalsOf } from "../live/index.ts";
import { seedVitals } from "../live/runtime.ts";
import { level } from "../ui/svg.ts";
import { Sparkline } from "../ui/Sparkline.tsx";
import { bytes, pct0 } from "./fmt.ts";

function Chip({
  label,
  value,
  lv,
  tip,
  color,
  points,
  max,
}: {
  label: string;
  value: ReactNode;
  lv: "" | "warn" | "bad";
  tip: string;
  color: string;
  points: number[];
  max?: number;
}) {
  return (
    <a className={lv ? `chip ${lv}` : "chip"} href="/app/host" title={tip}>
      <span className="chip-l">{label}</span>
      <b className="num">{value}</b>
      {lv && <span className="sr"> (high)</span>}
      <Sparkline values={points} max={max} label="" color={color} />
    </a>
  );
}

export function Vitals() {
  const b = useBootstrap();
  const s = useSystem();
  const r = useVitalsRings();
  useEffect(() => {
    if (b) void seedVitals(b);
  }, [b]);
  if (b && !b.features.system) return null;
  const v = vitalsOf(s);
  const disk = s?.disks[0];
  const cpuTip = s ? `CPU · load ${s.load.map((x) => x.toFixed(2)).join(" ")}` : "CPU";
  const memTip = s && s.mem_total > 0 ? `Memory: ${bytes(s.mem_used)} of ${bytes(s.mem_total)}` : "Memory";
  const diskTip = disk && disk.total > 0 ? `Disk ${disk.label}: ${bytes(disk.used)} of ${bytes(disk.total)}` : "Disk";
  const netTip =
    s && (s.net_rx_bps !== null || s.net_tx_bps !== null)
      ? `Network: ${bytes(s.net_rx_bps ?? 0)}/s in, ${bytes(s.net_tx_bps ?? 0)}/s out`
      : "Network";
  return (
    <div className="vitals" role="group" aria-label="Host vitals" data-testid="vitals">
      <Chip label="CPU" value={pct0(v.cpu)} lv={level(v.cpu)} tip={cpuTip} color="--ch-cpu" points={r.cpu.v} />
      <Chip label="MEM" value={pct0(v.mem)} lv={level(v.mem)} tip={memTip} color="--ch-mem" points={r.mem.v} />
      <Chip label="DISK" value={pct0(v.disk)} lv={level(v.disk)} tip={diskTip} color="--ch-disk" points={r.disk.v} />
      <Chip
        label="NET"
        value={v.net === null ? "—" : `${bytes(v.net)}/s`}
        lv=""
        tip={netTip}
        color="--ch-rx"
        points={r.net.v}
      />
    </div>
  );
}
