import { ActivityIcon, CpuIcon, MemoryStickIcon, ThermometerIcon } from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";
import type { HostValue } from "../../api/gen/HostValue.ts";
import type { HostView } from "../../api/gen/HostView.ts";
import type { HostHistory } from "../../api/gen/HostHistory.ts";
import { containerHref, navigate, useRoute } from "../../app/router.ts";
import { bytes, duration, pct } from "../../format/index.ts";
import { useBootstrap, useNow, watchReadTopic } from "../../live/index.ts";
import { useRead } from "../../live/read.ts";
import { Button } from "../../ui/Button.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { Meter } from "../../ui/Sparkline.tsx";
import { RANGES, windowSteps } from "../history/window.ts";
import { apiQuery, parseWindow } from "../container/range.ts";
import { historyHref } from "../container/events.ts";
import "../history/history.css";
import "./host.css";

const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));
const HISTORY = [["cpu", "CPU"], ["psi", "Pressure"], ["mem swap", "Memory"], ["disk mount", "Disks"], ["net", "Network"], ["temp", "Temperatures"]];

function format(value: number | null, unit: string): string {
  if (value === null) return "—";
  if (unit === "pct") return pct(value);
  if (unit === "bytes") return bytes(value);
  if (unit === "rate") return `${bytes(value)}/s`;
  if (unit === "temp") return `${value.toFixed(1)} °C`;
  if (unit === "mbps") return `${value} Mbit/s`;
  if (unit === "rpm") return `${Math.round(value)} rpm`;
  if (unit === "per_sec") return `${value.toFixed(1)}/s`;
  return String(value);
}

export default function Host() {
  const boot = useBootstrap();
  const allowed = boot?.scoped === false;
  const { data: initial, error } = useRead<HostView>(allowed ? "/api/v1/host" : null);
  const [live, setLive] = useState<HostView>();
  useEffect(() => {
    if (!allowed) return;
    let release: (() => void) | undefined;
    const sync = () => {
      release?.();
      release = document.hidden ? undefined : watchReadTopic("host", (value) => setLive(value as HostView));
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => { document.removeEventListener("visibilitychange", sync); release?.(); };
  }, [allowed]);
  if (boot?.scoped) return <p className="page">Host is unavailable with board-scoped access.</p>;
  const host = live ?? initial;
  const facts = new Map(host?.groups.map((group) => [group.name, group.values]));
  const memory = new Map(facts.get("Memory")?.map((entry) => [entry.label, entry.value]));
  const used = memory.get("Used");
  const total = memory.get("Total");
  return <div className="page hx" data-testid="host">
    <p className="page-sub">Everything the kernel reports about this machine. Live values refresh while this tab is visible; rates are averages since the previous reading.</p>
    {error && <p role="alert">{error}</p>}
    {host ? <>
      <div className="hx-grid">
      <section className="panel hx-p host-summary" data-testid="host-sample" data-sampled-at={host.at}>
        <div className="panel-h"><h2><CpuIcon aria-hidden="true" /> CPU</h2><span className="hx-h muted">{host.cpu_cores} cores</span></div>
        <div className="hx-big"><b>{pct(host.cpu_pct)}</b><span className="muted">busy</span>
          <span className="hx-load">load <b>{host.load.map((n) => n.toFixed(2)).join(" · ")}</b></span>
        </div>
        <Meter value={host.cpu_pct ?? 0} max={100} label="Total CPU usage" />
        <div className="hx-legend">{facts.get("CPU time")?.map((entry) => <span key={entry.label}>{entry.label} <b>{format(entry.value, entry.unit)}</b></span>)}</div>
        {!!facts.get("CPU cores")?.length && <ul className="hx-cores">{facts.get("CPU cores")?.map((entry) => <li key={entry.label}>
          <span className="hx-l">{entry.label}</span><Meter value={entry.value ?? 0} max={100} label={entry.label} />
          <span className="hx-v">{format(entry.value, entry.unit)}</span>
        </li>)}</ul>}
      </section>
      <section className="panel hx-p">
        <div className="panel-h"><h2><ActivityIcon aria-hidden="true" /> Pressure stall</h2></div>
        <Pressure values={facts.get("Pressure")} />
      </section>
      <section className="panel hx-p">
        <div className="panel-h"><h2><MemoryStickIcon aria-hidden="true" /> Memory</h2><span className="hx-h muted">{format(total ?? null, "bytes")}</span></div>
        <div className="hx-big"><b>{format(used ?? null, "bytes")}</b><span className="muted">in use · {format(memory.get("Available") ?? null, "bytes")} available</span></div>
        <Meter value={used ?? 0} max={total || 1} label="Memory in use" />
        <Facts values={facts.get("Memory")?.filter((entry) => !["Used", "Total", "Available"].includes(entry.label))} />
        <details className="hx-more" data-testid="host-disclosure"><summary>Paging activity</summary><Facts values={facts.get("Paging activity")} /></details>
      </section>
      <section className="panel hx-p">
        <div className="panel-h"><h2><ThermometerIcon aria-hidden="true" /> Sensors</h2></div>
        {facts.has("Sensors") ? <Facts values={facts.get("Sensors")} /> :
          <p className="muted">No hwmon temperatures or fans are exposed under /sys/class/hwmon.</p>}
      </section>
      {host.processes && boot?.role !== "viewer" && <details className="panel hx-p hx-wide" open>
        <summary>Top processes</summary>
        {host.processes.length ? <div className="host-table"><table className="tbl"><thead><tr>
          <th scope="col">PID</th><th scope="col">Command</th><th scope="col">Container</th>
          <th scope="col">UID</th><th scope="col">CPU</th><th scope="col">RSS</th><th scope="col">Threads</th>
        </tr></thead><tbody>{host.processes.map((p) => <tr key={p.pid}>
          <td>{p.pid}</td><td>{p.command}</td><td>{p.container === null ? "Host" : <a href={containerHref(p.container)}>{p.container}</a>}</td>
          <td>{p.uid ?? "—"}</td><td>{pct(p.cpu_pct)}</td><td>{bytes(p.rss_bytes)}</td><td>{p.threads}</td>
        </tr>)}</tbody></table></div> : <p>No processes visible.</p>}
      </details>}
      {!!host.process_groups?.length && boot?.role !== "viewer" && <details className="panel hx-p hx-wide">
        <summary>Process usage by container</summary>
        {host.process_lifetime && <p className="muted">CPU averages since the processes started.</p>}
        <ul className="history-events">{host.process_groups.map((group) => <li key={group.container ?? "host"}>
          {group.container ? <a href={containerHref(group.container)}>{group.container}</a> : "Host"}
          {" · "}{group.count} processes · {pct(group.cpu_pct)} CPU · {bytes(group.rss_bytes)}
        </li>)}</ul>
      </details>}
      {["Disks", "Mounts", "Network", "GPU"].filter((name) => facts.has(name)).map((name) =>
        <section className="panel hx-p hx-wide" key={name}><div className="panel-h"><h2>{name}</h2></div><Facts values={facts.get(name)} /></section>
      )}
      </div>
      <p className="foot-line">Updated <RelativeTime at={host.at} /> · up {duration(host.uptime_secs)}</p>
      <History recording={boot?.features.history ?? false} />
    </> : !error && <p role="status">Loading host facts…</p>}
  </div>;
}

function Facts({ values }: { values: HostValue[] | undefined }) {
  return <dl className="hx-kv">{values?.map((entry) => <div key={entry.label}>
    <dt>{entry.label}</dt><dd>{format(entry.value, entry.unit)}</dd>
  </div>)}</dl>;
}

const pressureWindows = ["some now", "some 10 s", "some 1 min", "some 5 min", "full 10 s"];

function Pressure({ values }: { values: HostValue[] | undefined }) {
  if (!values?.length) return <p className="muted">Pressure information is unavailable.</p>;
  const byLabel = new Map(values.map((entry) => [entry.label, entry.value]));
  return <>
    <table className="tbl hx-t"><thead><tr><th scope="col" aria-label="Resource" />
      {["Now", "10 s", "1 min", "5 min", "Full 10 s"].map((label) => <th scope="col" className="num" key={label}>{label}</th>)}
    </tr></thead><tbody>{[["cpu", "CPU"], ["memory", "Memory"], ["io", "I/O"]].map(([key, label]) => <tr key={key}>
      <th scope="row">{label}</th>
      {pressureWindows.map((window) => <td className="num" key={window}>{format(byLabel.get(key + " " + window) ?? null, "pct")}</td>)}
    </tr>)}</tbody></table>
    <p className="hx-note">Share of time tasks were stalled waiting for this resource. <strong>Some</strong>: at least one task waited. <strong>Full</strong>: all non-idle tasks waited at once.</p>
  </>;
}

function History({ recording }: { recording: boolean }) {
  const { search, setSearch } = useRoute();
  const now = useNow();
  const window = parseWindow(search.size ? `?${search}` : "?range=1h");
  const defaulted = !search.has("range") && !search.has("from") && !search.has("to") && window.kind === "range";
  const range = defaulted ? "1h" : window.kind === "range" ? window.range : "1h";
  const query = window.kind === "zoom" ? apiQuery(window) : `range=${range}`;
  const { data, error } = useRead<HostHistory>(`/api/v1/host/history?${query}`);
  const group = search.get("group") ?? "cpu";
  const metrics = data?.metrics.filter((m) => group.split(" ").some((prefix) => m.key.startsWith(`${prefix}.`))) ?? [];
  const steps = data ? windowSteps(data.from, data.to, Math.floor(now)) : null;
  return <section className="history-block" id="history" aria-label="Host history">
    <h2>History</h2>
    <div className="toolbar history-controls">
      {RANGES.map((r) => <Button key={r} aria-pressed={r === range && window.kind !== "zoom"}
        onClick={() => setSearch({ range: r, from: null, to: null })}>{r}</Button>)}
      {steps && (["earlier", "in", "out", "later"] as const).map((k, i) => <Button key={k} disabled={!steps[k]}
        onClick={() => { if (steps[k]) setSearch(steps[k]); }}>{["Earlier", "Zoom in", "Zoom out", "Later"][i]}</Button>)}
      {window.kind === "zoom" && <Button onClick={() => setSearch({ from: null, to: null })}>Reset zoom</Button>}
    </div>
    <div className="toolbar history-controls">
      {HISTORY.map(([key, label]) => <Button key={key} aria-pressed={key === group} onClick={() => setSearch({ group: key ?? null })}>{label}</Button>)}
    </div>
    {error && <p role="alert">{error}</p>}
    {!metrics.length && <p>{data ? "No samples for this group." : "Loading host history…"}</p>}
    {data && !!metrics.length && <Suspense fallback={null}><Charts
      series={{ from: data.from, to: data.to, range, step: 0, cpu: [], mem: [], cpu_max: null, mem_max: null, markers: data.markers }}
      metrics={metrics}
      recording={recording}
      onZoom={(from, to) => setSearch({ from: String(from), to: String(to) })}
      onMarker={(id, at) => navigate(historyHref(id, at, data.markers.find((e) => e.id === id)?.level ?? "warn"))}
    /></Suspense>}
  </section>;
}
