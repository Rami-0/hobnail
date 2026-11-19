import { eventLabel } from "../container/events.ts";
import { lazy, memo, Suspense, useEffect } from "react";
import type { HistoryView } from "../../api/gen/HistoryView.ts";
import { useRead } from "../../live/read.ts";
import { containerHref, logsHref } from "../../app/paths.ts";
import { navigate, setSearch, useRoute } from "../../app/router.ts";
import { useBootstrap, useContainers } from "../../live/index.ts";
import { Button } from "../../ui/Button.tsx";
import { ChartPlaceholder } from "../../ui/Sparkline.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { historyWindow, RANGES, windowSteps } from "./window.ts";
import "./history.css";

const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));

export default function History() {
  const { search, hash } = useRoute();
  const { query, level, zoomed, range } = historyWindow(search);
  const bootstrap = useBootstrap();
  const sessionScoped = bootstrap?.scoped;
  const { data: containers } = useContainers();
  const { data, error } = useRead<HistoryView>(sessionScoped === false ? `/api/v1/history?${query}` : null);

  const selected = hash.startsWith("#ev-") ? Number(hash.slice(4)) : null;
  const extra = data?.markers.find((e) => e.id === selected && !data.events.some((row) => row.id === e.id));
  useEffect(() => {
    if (!data) return;
    const target = selected === null ? (hash === "#events" ? "events" : "") : `ev-${selected}`;
    if (target) document.getElementById(target)?.scrollIntoView({ block: "nearest" });
  }, [selected, hash, data]);
  if (sessionScoped) return <p className="page">History is unavailable with board-scoped access.</p>;

  const names = new Set(containers?.groups.flatMap((g) => g.containers.map((c) => c.name)));
  const steps = data ? windowSteps(data.from, data.to, data.now) : null;
  return <div className="page" data-testid="history">
    <h1>History</h1>
    <div className="toolbar history-controls">
      {RANGES.map((r) => <Button key={r} aria-pressed={!zoomed && r === range} onClick={() => setSearch({ range: r, from: null, to: null, before: null })}>{r}</Button>)}
      {steps && (["earlier", "in", "out", "later"] as const).map((key, index) => <Button key={key} disabled={!steps[key]} onClick={() => { if (steps[key]) setSearch(steps[key]); }}>{["Earlier", "Zoom in", "Zoom out", "Later"][index]}</Button>)}
      <Button disabled={!!data && data.to - data.from > 86400} aria-pressed={search.get("cmp") === "1"} onClick={() => setSearch({ cmp: search.get("cmp") === "1" ? null : "1" })}>Compare with yesterday</Button>
      {zoomed && <Button onClick={() => setSearch({ from: null, to: null, before: null })}>Reset zoom</Button>}
    </div>
    {error && <p role="alert">{error}</p>}
    {!data ? !error && <ChartPlaceholder count={8} /> : <>
      <HistoryCharts data={data} />
      <section className="panel history-block">
        <h2>Container starts per day</h2>
        {Object.keys(data.starts_per_day).length === 0 ? <p>No container starts recorded in the last seven days.</p> :
          <ul>{Object.entries(data.starts_per_day).map(([name, days]) => <li key={name}>
            {names.has(name) ? <a href={containerHref(name)}>{name}</a> : name}
            {" "}<span aria-label="Starts per day, newest first">{days.join(" · ")}</span>
          </li>)}</ul>}
      </section>
      <section className="panel history-block" id="events" aria-label="Incident timeline">
        <h2>Incident timeline</h2>
        <div className="toolbar history-controls">
          {["incidents", "all", "info", "warn", "error"].map((item) => <Button key={item} aria-pressed={level === item} onClick={() => setSearch({ level: item, before: null })}>{item === "all" ? "All events" : item}</Button>)}
        </div>
        {data.events.length === 0 && !extra && <p>No matching events in this window.</p>}
        <ul className="history-events rows">
          {[...(extra ? [extra] : []), ...data.events].map((event) => <li key={event.id} id={`ev-${event.id}`} data-selected={event.id === selected || undefined}>
            <RelativeTime at={event.at} /> <strong>{eventLabel(event.kind)}</strong>{" "}
            {names.has(event.subject) ? <>
              <a href={containerHref(event.subject)}>{event.subject}</a>{" "}
              <a href={logsHref(event.subject, event.at)}>Logs</a>
            </> : event.subject}
            {event.message && <div className="muted">{event.message}</div>}
          </li>)}
        </ul>
        {search.has("before") && <Button onClick={() => setSearch({ before: null })}>Back to newest</Button>}
        {data.older !== null && <Button onClick={() => setSearch({ before: String(data.older) })}>Older events</Button>}
      </section>
    </>}
  </div>;
}

const HistoryCharts = memo(function HistoryCharts({ data }: { data: HistoryView }) {
  return <Suspense fallback={<ChartPlaceholder count={data.metrics.length} />}>
        <Charts
          series={{ from: data.from, to: data.to, step: 0, cpu: [], mem: [], cpu_max: null, mem_max: null, markers: data.markers }}
          metrics={data.metrics}
          recording={data.recording}
          onZoom={(from, to) => setSearch({ from: String(from), to: String(to), before: null })}
          onMarker={(id) => navigate(`/app/history?${window.location.search.slice(1)}#ev-${id}`)}
        />
      </Suspense>;
});
