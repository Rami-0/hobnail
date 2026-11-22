import { PageHeader } from "../../ui/PageHeader.tsx";
import { Checkbox } from "../../ui/Checkbox.tsx";
import { eventLabel } from "../container/events.ts";
import { memo, useCallback, useEffect, useMemo, useRef } from "react";
import { NearCharts } from "../container/NearCharts.tsx";
import type { HistoryView } from "../../api/gen/HistoryView.ts";
import { useRead } from "../../live/read.ts";
import { containerHref, logsHref } from "../../app/paths.ts";
import { navigate, setSearch, useRoute } from "../../app/router.ts";
import { useBootstrap, useContainers } from "../../live/index.ts";
import { Button } from "../../ui/Button.tsx";
import { ChartPlaceholder } from "../../ui/Sparkline.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { historyWindow, RANGES, windowSteps } from "./window.ts";
import { MAX_DEFER_MS, QUIET_MS, quietDelay } from "./quiet.ts";
import "./history.css";

/** Apply background History reads once the current gesture has settled. */
function useQuietCommit() {
  const input = useRef({ pressed: new Set<number | string>(), until: 0 });
  useEffect(() => {
    const current = input.current;
    const track = (event: PointerEvent | KeyboardEvent) => {
      const id = "pointerId" in event ? event.pointerId : event.code;
      if (event.type.endsWith("down")) current.pressed.add(id);
      else current.pressed.delete(id);
      current.until = performance.now() + QUIET_MS;
    };
    const clear = () => { current.pressed.clear(); current.until = 0; };
    const lifetime = new AbortController();
    for (const event of ["pointerdown", "pointerup", "pointercancel", "lostpointercapture", "keydown", "keyup"] as const) {
      window.addEventListener(event, track, { capture: true, signal: lifetime.signal });
    }
    window.addEventListener("blur", clear, { signal: lifetime.signal });
    document.addEventListener("visibilitychange", clear, { signal: lifetime.signal });
    return () => lifetime.abort();
  }, []);

  return useCallback((commit: () => void) => {
    const current = input.current;
    const deadline = performance.now() + MAX_DEFER_MS;
    const apply = () => {
      const delay = quietDelay(current.pressed.size > 0, current.until, deadline, performance.now());
      if (delay > 0) timer = window.setTimeout(apply, delay);
      else commit();
    };
    // Give already queued input a chance to arrive before applying the reply.
    let timer = window.setTimeout(apply, 0);
    return () => window.clearTimeout(timer);
  }, []);
}

export default function History() {
  const { search, hash } = useRoute();
  const { query, level, zoomed, range } = historyWindow(search);
  const bootstrap = useBootstrap();
  const sessionScoped = bootstrap?.scoped;
  const { data: containers } = useContainers();
  const quietCommit = useQuietCommit();
  const { data, error } = useRead<HistoryView>(sessionScoped === false ? `/api/v1/history?${query}` : null, undefined, undefined, quietCommit);

  const selected = hash.startsWith("#ev-") ? Number(hash.slice(4)) : null;
  const extra = data?.markers.find((e) => e.id === selected && !data.events.some((row) => row.id === e.id));
  useEffect(() => {
    if (!data) return;
    const target = selected === null ? (hash === "#events" ? "events" : "") : `ev-${selected}`;
    if (target) document.getElementById(target)?.scrollIntoView({ block: "nearest" });
  }, [selected, hash, data]);
  const names = useMemo(() => new Set(containers?.groups.flatMap((g) => g.containers.map((c) => c.name))), [containers]);
  if (sessionScoped) return <p className="page">History is unavailable with board-scoped access.</p>;

  const steps = data ? windowSteps(data.from, data.to, data.now) : null;
  return <div className="page" data-testid="history">
    <PageHeader title="History" description="Host metrics and incidents from the local history database." />
    <div className="toolbar hist-bar">
      <nav className="seg" aria-label="Range">
        {RANGES.map((r) => {
          const rangeQuery = new URLSearchParams(search);
          rangeQuery.set("range", r);
          for (const key of ["from", "to", "before"]) rangeQuery.delete(key);
          return <a key={r} href={"/app/history?" + rangeQuery.toString()} aria-current={!zoomed && r === range ? "true" : undefined}>{r}</a>;
        })}
      </nav>
      {steps && <nav className="zoom-nav" aria-label="Move or zoom the window">
        {(["earlier", "in", "out", "later"] as const).map((key, index) => <Button variant="ghost" size="sm" key={key}
          disabled={!steps[key]} onClick={() => { if (steps[key]) setSearch(steps[key]); }}>{["‹ Earlier", "Zoom in", "Zoom out", "Later ›"][index]}</Button>)}
      </nav>}
      <Checkbox label="Compare with yesterday" disabled={!!data && data.to - data.from > 86400}
        checked={search.get("cmp") === "1"} onChange={(on) => setSearch({ cmp: on ? "1" : null })} />
      {zoomed ? <Button variant="ghost" size="sm" onClick={() => setSearch({ from: null, to: null, before: null })}>Reset zoom</Button> :
        <span className="hint muted">Drag across a chart to zoom in</span>}
    </div>
    {error && <p role="alert">{error}</p>}
    {!data ? !error && <ChartPlaceholder count={8} /> : <>
      <HistoryCharts data={data} />
      <section className="panel history-block">
        <div className="panel-h"><h2>Container starts per day</h2></div>
        {Object.keys(data.starts_per_day).length === 0 ? <p>No container starts recorded in the last seven days.</p> :
          <ul>{Object.entries(data.starts_per_day).map(([name, days]) => <li key={name}>
            {names.has(name) ? <a href={containerHref(name)}>{name}</a> : name}
            {" "}<span aria-label="Starts per day, newest first">{days.join(" · ")}</span>
          </li>)}</ul>}
      </section>
      <section className="panel history-block" id="events" aria-label="Incident timeline">
        <div className="panel-h"><h2>Incident timeline</h2></div>
        <div className="history-controls seg" role="group" aria-label="Incident severity">
          {["incidents", "all", "info", "warn", "error"].map((item) => <Button key={item} aria-pressed={level === item} onClick={() => { if (item !== level || search.has("before")) setSearch({ level: item, before: null }); }}>{item === "all" ? "All events" : item}</Button>)}
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
  return <NearCharts
    series={{ from: data.from, to: data.to, step: 0, cpu: [], mem: [], cpu_max: null, mem_max: null, markers: data.markers }}
    metrics={data.metrics}
    recording={data.recording}
    onZoom={(from, to) => setSearch({ from: String(from), to: String(to), before: null })}
    onMarker={(id) => navigate(`/app/history?${historyWindow(new URLSearchParams(window.location.search)).query}#ev-${id}`)}
  />;
});
