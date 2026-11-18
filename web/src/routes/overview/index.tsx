import { eventLabel } from "../container/events.ts";
import { logsHref } from "../../app/paths.ts";
import { lazy, Suspense, useEffect, useState } from "react";
import type { OverviewView } from "../../api/gen/OverviewView.ts";
import type { WidgetView } from "../../api/gen/WidgetView.ts";
import type { BookmarkView } from "../../api/gen/BookmarkView.ts";
import { containerHref, navigate } from "../../app/router.ts";
import { local } from "../../app/prefs.ts";
import { useBootstrap, useContainers, useNow, useStatsMap, useStrips, useSystem } from "../../live/index.ts";
import { useRead } from "../../live/read.ts";
import { bytes, duration, pct } from "../../format/index.ts";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { HealthStrip, Meter } from "../../ui/Sparkline.tsx";
import { WidgetContent } from "../../ui/WidgetContent.tsx";
import { treemap } from "./treemap.ts";
import "../history/history.css";
import "./overview.css";

const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));
const SEEN = "rh-seen";

function readSeen(): number {
  try {
    const value = Number(local()?.getItem(SEEN));
    return Number.isSafeInteger(value) && value > 0 ? value : 0;
  } catch {
    return 0;
  }
}

export default function Overview() {
  const boot = useBootstrap();
  const allowed = boot?.scoped === false;
  const admin = allowed && boot.role !== "viewer";
  const now = useNow();
  const [seen] = useState(readSeen);
  const { data: overview, error } = useRead<OverviewView>(allowed ? `/api/v1/overview?since=${seen}${admin ? "&probes=1" : ""}` : null, "alerts", 15000);
  const { data: widgets } = useRead<WidgetView[]>(allowed ? "/api/v1/widgets" : null, "widget");
  const { data: bookmarks } = useRead<BookmarkView[]>(allowed ? "/api/v1/bookmarks" : null);
  const { data: containers } = useContainers();
  const stats = useStatsMap(allowed && !!boot?.features.stats);
  const strips = useStrips();
  const system = useSystem();
  useEffect(() => {
    if (!allowed) return;
    const mark = () => {
      try { local()?.setItem(SEEN, String(Math.floor(now))); } catch { /* storage disabled */ }
    };
    const onHidden = () => { if (document.hidden) mark(); };
    window.addEventListener("pagehide", mark);
    document.addEventListener("visibilitychange", onHidden);
    return () => {
      window.removeEventListener("pagehide", mark);
      document.removeEventListener("visibilitychange", onHidden);
    };
  }, [allowed, now]);
  if (boot?.scoped) return <p className="page">Overview is unavailable with board-scoped access.</p>;
  const groups = containers?.groups ?? [];
  const rows = groups.flatMap((g) => g.containers);
  const names = new Set(rows.map((r) => r.name));
  const consumers = (rows.length ? rows : overview?.consumers ?? []).map((r) => {
    const s = stats.get(r.name);
    return { name: r.name, title: r.title, cpu: s?.cpu_pct ?? r.cpu_pct, mem: s?.mem_bytes ?? r.mem_bytes };
  });
  const top = consumers.filter((r) => r.cpu !== null).toSorted((a, b) => (b.cpu ?? 0) - (a.cpu ?? 0)).slice(0, 8);
  const memory = consumers.filter((r) => (r.mem ?? 0) > 0).toSorted((a, b) => (b.mem ?? 0) - (a.mem ?? 0));
  const map = treemap(memory.map((r) => r.mem ?? 0));
  const running = containers ? groups.reduce((n, g) => n + g.running, 0) : overview?.running;
  const stopped = containers ? groups.reduce((n, g) => n + g.stopped, 0) : overview?.stopped;
  const problems = containers ? rows.filter((r) => r.attention !== null).length : overview?.problems;
  return <div className="page overview" data-testid="overview">
    <h1>{overview?.greeting ?? "Overview"}</h1>
    {error && <p role="alert">{error}</p>}
    {seen > 0 && now - seen >= 300 && overview && <p>Since you last looked (<RelativeTime at={seen} />) · {overview.starts_since} starts · {overview.crashes_since} crashes · {overview.alerts_since} alerts</p>}
    <section className="panel history-block">
      <h2>Container health</h2>
      <div className="overview-counts">
        <a href="/app/containers?state=running"><strong>{running ?? "—"}</strong> running</a>
        <a href="/app/containers?state=stopped"><strong>{stopped ?? "—"}</strong> stopped</a>
        <a href="/app/containers?state=problem"><strong>{problems ?? "—"}</strong> problems</a>
        <a href="/app/alerts"><strong>{overview?.firing ?? "—"}</strong> alerts firing</a>
      </div>
      {running !== undefined && stopped !== undefined && running + stopped > 0 && <Meter
        value={running} max={running + stopped} label="Containers running" className="overview-health-meter" />}
      {!!overview?.attention.length && <><h3>Needs attention</h3><ul className="history-events rows">{overview.attention.map((item) => <li key={`${item.name}:${item.reason}:${item.detail}`} data-name={item.container ? item.name : undefined}>
        <span className={`dot ${item.severity}`} /> <a href={item.href}>{item.title}</a> · {item.detail}
        {item.since !== null && <> · <RelativeTime at={item.since} /></>}
        {item.container && <>{" · "}<a href={logsHref(item.name, item.since)}>Logs</a></>}
        {item.probe && <small className="overview-probe">{item.probe}</small>}
      </li>)}</ul></>}
      <div className="overview-strips">{groups.map((g) => {
        const codes = strips?.stacks[g.name];
        return codes ? <a key={g.name} href={`/app/containers?stack=${encodeURIComponent(g.name)}`}>
          <span>{g.title}</span><HealthStrip codes={codes} step={strips?.step} label={g.title} />
        </a> : null;
      })}</div>
    </section>
    {boot?.features.system && <section className="history-block">
      <h2><a href="/app/host">Host</a></h2>
      {system && <div className="overview-counts">
        <span>CPU {pct(system.cpu_pct)}</span><span>Memory {bytes(system.mem_used)} / {bytes(system.mem_total)}</span>
        <span>Load {system.load[0].toFixed(2)}</span><span>Uptime {duration(system.uptime_secs)}</span>
      </div>}
      {!!overview?.hour.length && <Suspense fallback={null}><Charts
        series={{ from: overview.now - 3600, to: overview.now, range: "1h", step: 0, cpu: [], mem: [], cpu_max: null, mem_max: null, markers: overview.markers }}
        metrics={overview.hour} recording={boot.features.history}
        onZoom={(from, to) => navigate(`/app/history?from=${from}&to=${to}`)}
        onMarker={(id, at) => navigate(`/app/history?from=${at-1800}&to=${at+1800}#ev-${id}`)} /></Suspense>}
    </section>}
    {boot?.features.stats && <section className="panel history-block">
      <h2>Top consumers</h2><div className="overview-consumers">
        <div><h3>CPU</h3>{!top.length && <p>No CPU samples yet.</p>}<ul className="history-events">{top.map((r) => <li key={r.name}>
          <a href={containerHref(r.name)}>{r.title}</a> · {pct(r.cpu)}
          <Meter value={r.cpu ?? 0} max={Math.max(100, top[0]?.cpu ?? 0)} label={`${r.title} CPU`} />
        </li>)}</ul></div>
        <div><h3>Memory</h3>{!map.boxes.length ? <p>No memory samples yet.</p> :
          <svg className="overview-map" viewBox="0 0 360 180" aria-label="Memory usage treemap">
            {map.boxes.map(([x, y, width, height], i) => {
              const row = i < map.shown ? memory[i] : null;
              const label = row?.title ?? `+${memory.length - map.shown} others`;
              return <a key={row?.name ?? "others"} href={row ? containerHref(row.name) : "/app/containers?sort=mem"} aria-label={row ? `${label}, ${bytes(row.mem ?? 0)}` : label}>
                <rect x={x} y={y} width={width} height={height} />
                <text x={x + 5} y={y + 16}>{label.slice(0, Math.max(4, Math.floor((width - 8) / 7)))}</text>
                <title>{row ? `${label} · ${bytes(row.mem ?? 0)}` : label}</title>
              </a>;
            })}
          </svg>}</div>
      </div>
    </section>}
    {(admin || !!widgets?.length) && <section className="panel history-block"><h2>Widgets</h2>
      {admin && <a href="/app/settings/widgets?add=widget">Add widget</a>}
      <div className="overview-widget-grid">{widgets?.map((w) => <article key={w.id} id={`w-${w.id}`} className="panel history-block">
        <h3>{w.link ? <a href={w.link}>{w.title}</a> : w.title}</h3><WidgetContent widget={w} />
      </article>)}</div>
    </section>}
    <section className="panel history-block"><h2>Recent events</h2><a href="/app/history#events">Timeline</a>
      {!overview?.events.length && <p>No recent events.</p>}
      <ul className="history-events">{overview?.events.map((e) => <li key={e.id}>
        <RelativeTime at={e.at} /> · {eventLabel(e.kind)} · {names.has(e.subject) ? <>
          <a href={containerHref(e.subject)}>{e.subject}</a> · <a href={logsHref(e.subject, e.at)}>Logs</a>
        </> : e.subject}
      </li>)}</ul>
    </section>
    {(admin || !!bookmarks?.length) && <section className="panel history-block"><h2>Bookmarks</h2>
      {admin && <a href="/app/settings/bookmarks?add=bookmark">Add bookmark</a>}
      <ul className="history-events">{bookmarks?.map((b) => <li key={`${b.name}:${b.url}`}>
        <a href={b.url} target="_blank" rel="noopener noreferrer">{b.name}</a>{b.description && <> · {b.description}</>}
      </li>)}</ul>
    </section>}
  </div>;
}
