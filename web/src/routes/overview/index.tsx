import { ActivityIcon, ArrowDownIcon, ArrowRightIcon, ArrowUpIcon, CheckIcon, ClockIcon, CpuIcon, HardDriveIcon, MemoryStickIcon, ScrollTextIcon, ThermometerIcon } from "lucide-react";
import { eventLabel } from "../container/events.ts";
import { logsHref } from "../../app/paths.ts";
import { lazy, Suspense, memo, useEffect, useState } from "react";
import type { OverviewView } from "../../api/gen/OverviewView.ts";
import type { WidgetView } from "../../api/gen/WidgetView.ts";
import type { BookmarkView } from "../../api/gen/BookmarkView.ts";
import { containerHref, navigate, useRoute } from "../../app/router.ts";
import { local } from "../../app/prefs.ts";
import { useBootstrap, useContainers, useNow, useStatsMap, useStrips, useSystem, onConfigChange, watchReadTopic } from "../../live/index.ts";
import { useRead } from "../../live/read.ts";
import { bytes, duration, pct } from "../../format/index.ts";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { ChartPlaceholder, HealthStrip, Meter } from "../../ui/Sparkline.tsx";
import { WidgetContent } from "../../ui/WidgetContent.tsx";
import { SEEN, overviewPath, readSeen } from "./request.ts";
import { treemap } from "./treemap.ts";
import "../history/history.css";
import "./overview.css";

const EditBookmarks = lazy(() => import("../settings/Bookmarks.tsx"));
const EditWidgets = lazy(() => import("../settings/Widgets.tsx"));
const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));
const HostCharts = memo(function HostCharts({ overview, recording }: { overview: OverviewView; recording: boolean }) {
  return <Suspense fallback={<ChartPlaceholder count={4} />}><Charts
    series={{ from: overview.now - 3600, to: overview.now, range: "1h", step: 0, cpu: [], mem: [], cpu_max: null, mem_max: null, markers: overview.markers }}
    metrics={overview.hour} recording={recording}
    onZoom={(from, to) => navigate(`/app/history?from=${from}&to=${to}`)}
    onMarker={(id, at) => navigate(`/app/history?from=${at-1800}&to=${at+1800}#ev-${id}`)} /></Suspense>;
});

export default function Overview() {
  const boot = useBootstrap();
  const allowed = boot?.scoped === false;
  const { search } = useRoute();
  const admin = allowed && boot.role !== "viewer";
  const now = useNow();
  const [seen] = useState(readSeen);
  const { data: overview, error } = useRead<OverviewView>(allowed ? overviewPath(seen, admin) : null, "alerts", 15000);
  const { data: widgets } = useRead<WidgetView[]>(allowed ? "/api/v1/widgets" : null);
  const [widgetUpdates, setWidgetUpdates] = useState<Record<string, WidgetView>>({});
  useEffect(() => {
    if (!allowed) return;
    const reset = onConfigChange(() => setWidgetUpdates({}));
    const stop = watchReadTopic("widget", (data) => {
      const widget = data as WidgetView;
      setWidgetUpdates((previous) => ({ ...previous, [widget.id]: widget }));
    });
    return () => { reset(); stop(); };
  }, [allowed]);
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
      mark();
      window.removeEventListener("pagehide", mark);
      document.removeEventListener("visibilitychange", onHidden);
    };
  }, [allowed, now]);
  if (boot?.scoped) return <p className="page">Overview is unavailable with board-scoped access.</p>;
  if (!overview || !containers) return <p className="page" role={error ? "alert" : "status"}>{error || "Loading Overview…"}</p>;
  const bookmarkGroups = new Map<string, BookmarkView[]>();
  for (const bookmark of bookmarks ?? []) {
    const group = bookmark.group || "Bookmarks";
    bookmarkGroups.set(group, [...bookmarkGroups.get(group) ?? [], bookmark]);
  }
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
  const running = groups.reduce((n, g) => n + g.running, 0);
  const stopped = groups.reduce((n, g) => n + g.stopped, 0);
  const problems = rows.filter((r) => r.attention !== null).length;
  const health = [
    { value: running, label: "running", href: "/app/containers?state=running", hot: false },
    { value: stopped, label: "stopped", href: "/app/containers?state=stopped", hot: false },
    { value: problems, label: "problems", href: "/app/containers?state=problem", hot: problems > 0 },
    { value: overview.firing, label: "alerts firing", href: "/app/alerts", hot: overview.firing > 0 },
  ];
  const total = running + stopped;
  const localTime = new Date(now * 1000);
  const clock = localTime.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" }) + "  " +
    localTime.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const hasCoverage = strips && Object.values(strips.stacks).some((codes) => codes.some((code) => code > 0));
  const extraProblems = Math.max(0, problems - overview.attention.length);
  const cleanStops = rows.filter((row) => row.state !== "running" && row.attention === null).length;
  return <div className="page ov" data-testid="overview">
    <div className="ov-head">
      <div className="ov-hi">
        <h1 className="display">{overview.greeting ?? "Hello"}</h1>
        {seen > 0 && now - seen >= 300 && <p className="since">Since you last looked (<RelativeTime at={seen} />) · {overview.starts_since} starts · {overview.crashes_since} crashes · {overview.alerts_since} alerts</p>}
      </div>
      <div className="ov-meta"><span className="ov-clock num">{clock}</span><span className="upd">updated <RelativeTime at={overview.now} /></span></div>
    </div>
    {error && <p role="alert">{error}</p>}
    <section className="panel primary pulse" aria-label="Status">
      <section className="hs" aria-label="Container health">
        {health.map(({ value, label, href, hot }) => (
          <a key={label} href={href} className={["hs-cell", hot && "hot", value === 0 && "zero"].filter(Boolean).join(" ")}>
            <span className="hs-n">{value}</span><span className="hs-l">{label}</span>
          </a>
        ))}
        {total > 0 && <div className="hs-bar" role="img" aria-label={running + " of " + total + " running"}>
          <span className="r" style={{ width: running / total * 100 + "%" }} />
          {problems > 0 && <span className="u" style={{ width: problems / total * 100 + "%" }} />}
        </div>}
      </section>
      <section className={overview.attention.length ? "attn" : "attn quiet"} aria-label="Needs attention">
        {overview.attention.length ? <>
          <div className="attn-h">
            <h2 className="label">Needs attention</h2>
            <span className="count-pill bad">{overview.attention.length + extraProblems}</span>
          </div>
          <ul className="rows">{overview.attention.map((item) => <li key={[item.name, item.reason, item.detail].join(":")} data-name={item.container ? item.name : undefined}>
            <span className={"dot " + item.severity} aria-hidden="true" />
            <a className="nm" href={item.href}>{item.title}</a>
            <span className="why">{item.detail}{item.probe && <small className="overview-probe"> · {item.probe}</small>}</span>
            {item.since !== null && <span className="when"><RelativeTime at={item.since} /></span>}
            {item.container && <a className="ia" aria-label={"Logs of " + item.title} title="Logs" href={logsHref(item.name, item.since)}><ScrollTextIcon aria-hidden="true" /></a>}
          </li>)}</ul>
          {extraProblems > 0 && <p className="foot-line"><a href="/app/containers?state=problem">{extraProblems} more on the Containers page</a></p>}
        </> : <p className="ok-line"><CheckIcon aria-hidden="true" /> {total ? <>All clear. <span className="muted">{total} containers, nothing needs you.</span></> : "No containers yet."}</p>}
        {cleanStops > 0 && <p className="foot-line"><a href="/app/containers?state=stopped">{cleanStops} {cleanStops === 1 ? "container is" : "containers are"} stopped cleanly</a></p>}
      </section>
      {groups.length > 0 && <div className="strips">
        <div className="strips-h">
          <h2 className="label">Last 24h</h2>
          <span className="strips-key" aria-hidden="true"><i className="o" />running<i className="w" />restarted<i className="b" />problem<i className="f" />stopped</span>
        </div>
        {strips && hasCoverage ? <ul className="strip-list">{groups.map((g) => {
          const codes = strips.stacks[g.name];
          return codes ? <li key={g.name}>
            <a className="sl-n" href={"/app/containers?stack=" + encodeURIComponent(g.name)}>{g.title}</a>
            <span className="sl-c num">{g.running}/{g.running + g.stopped}</span>
            <HealthStrip codes={codes} step={strips.step} label={g.title} />
          </li> : null;
        })}</ul> : <p className="muted">Health strips fill in as history records (one cell per 15 minutes).</p>}
      </div>}
    </section>
    {boot?.features.system && <section className="sec">
      <div className="sec-h"><h2 className="label">Host</h2><a className="more" href="/app/host">Host details <ArrowRightIcon aria-hidden="true" /></a></div>
      <HostStrip system={system} />
      {!!overview.hour.length && <HostCharts overview={overview} recording={boot.features.history} />}
    </section>}
    {boot?.features.stats && <section className="overview-consumers" aria-label="Where resources go">
        <div className="panel"><div className="panel-h"><h2>Memory by container</h2><a className="more" href="/app/containers?sort=mem">All <ArrowRightIcon aria-hidden="true" /></a></div>{!map.boxes.length ? <p>No memory samples yet.</p> :
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
        <div className="panel res-cpu"><div className="panel-h"><h2>Top CPU</h2><a className="more" href="/app/containers?sort=cpu">All <ArrowRightIcon aria-hidden="true" /></a></div>{!top.length && <p>No CPU samples yet.</p>}<ul className="history-events">{top.map((r) => <li key={r.name}>
          <a href={containerHref(r.name)}>{r.title}</a> · {pct(r.cpu)}
          <Meter value={r.cpu ?? 0} max={Math.max(100, top[0]?.cpu ?? 0)} label={`${r.title} CPU`} />
        </li>)}</ul></div>
    </section>}
    {(admin || !!widgets?.length) && <section className="panel history-block"><h2>Widgets</h2>
      {admin && <a href="/app/overview?add=widget">Add widget</a>}
      <div className="overview-widget-grid">{widgets?.map((widget) => widgetUpdates[widget.id] ?? widget).map((w) => <article key={w.id} id={`w-${w.id}`} className="panel history-block">
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
    {admin && <Suspense fallback={<p role="status">Loading editor…</p>}>
      {search.get("add") === "bookmark" && <EditBookmarks editorOnly />}
      {search.get("add") === "widget" && <EditWidgets editorOnly />}
    </Suspense>}
    {(admin || !!bookmarks?.length) && <section className="panel history-block"><h2>Bookmarks</h2>
      {admin && <a href="/app/overview?add=bookmark">Add bookmark</a>}
      {[...bookmarkGroups].map(([group, items]) => <div key={group}>
        {(bookmarkGroups.size > 1 || group !== "Bookmarks") && <h3>{group}</h3>}
        <ul className="history-events">{items.map((b) => <li key={`${b.name}:${b.url}`}>
          <a href={b.url} target="_blank" rel="noopener noreferrer">{b.name}</a>{b.description && <> · {b.description}</>}
        </li>)}</ul>
      </div>)}
    </section>}
  </div>;
}

function rate(value: number | null | undefined) {
  return typeof value === "number" ? bytes(value) + "/s" : "—";
}

function HostStrip({ system }: { system: ReturnType<typeof useSystem> }) {
  const disk = system?.disks.find((volume) => volume.total > 0);
  const temperature = system?.temp_c;
  const widgets = [
    { Icon: CpuIcon, label: "CPU", value: pct(system?.cpu_pct ?? null), note: system && "load " + system.load.map((v) => v.toFixed(2)).join(" ") },
    { Icon: MemoryStickIcon, label: "Memory", value: system?.mem_total ? bytes(system.mem_used) : "—",
      note: system?.mem_total ? "of " + bytes(system.mem_total) + (system.swap_total > 0 ? " · swap " + bytes(system.swap_used) : "") : "—",
      gauge: system?.mem_total ? system.mem_used / system.mem_total * 100 : undefined },
    { Icon: HardDriveIcon, label: "Disk", value: disk ? pct(disk.used / disk.total * 100) : "—",
      note: disk && bytes(disk.avail) + " free on " + disk.label, gauge: disk && disk.used / disk.total * 100 },
    { Icon: ActivityIcon, label: "Network", value: <><span title="received"><ArrowDownIcon aria-hidden="true" />{rate(system?.net_rx_bps)}</span><span title="sent"><ArrowUpIcon aria-hidden="true" />{rate(system?.net_tx_bps)}</span></> },
    { Icon: ThermometerIcon, label: "Temperature", value: typeof temperature === "number" ? Math.round(temperature) + " °C" : "—", note: "CPU package" },
    { Icon: ClockIcon, label: "Uptime", value: system?.uptime_secs ? duration(system.uptime_secs) : "—", note: "since last boot" },
  ];
  return <div className="strip" aria-label="Host vitals">{widgets.map(({ Icon, label, value, note, gauge }) => (
    <div className="cell" key={label}>
      <div className="cell-h"><Icon aria-hidden="true" /><span className="label">{label}</span></div>
      <div className={label === "Network" ? "big net" : "big"}>{value}</div>
      <div className="sub">{note}</div>
      {gauge !== undefined && <Meter value={gauge} max={100} label={label + " usage"} />}
    </div>
  ))}</div>;
}
