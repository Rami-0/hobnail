import { Badge, statusBadge } from "../../ui/Badge.tsx";
import { PageHeader } from "../../ui/PageHeader.tsx";
// Container detail routes share the time window and subscribe to live stats.
import {
  ActivityIcon,
  BoxIcon,
  CircleAlertIcon,
  CpuIcon,
  ExternalLinkIcon,
  MemoryStickIcon,
  PencilIcon,
  RefreshCwIcon,
  ScrollTextIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { lazy, memo, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { ContainerDetail } from "../../api/gen/ContainerDetail.ts";
import type { ContainerRow } from "../../api/gen/ContainerRow.ts";
import type { Why } from "../../api/gen/Why.ts";
import { api, useSession } from "../../api/session.ts";
import { navigate, useRoute } from "../../app/router.ts";
import { bindingLink, bytes, duration, localTime, pct, rowLink, viewerHost } from "../../format/index.ts";
import { onConfigChange, useBootstrap, useContainers, useNow, useStat } from "../../live/index.ts";
import { EmptyState } from "../../ui/Note.tsx";
import { Sparkline } from "../../ui/Sparkline.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { TabNav } from "../../ui/TabNav.tsx";
import { ServiceIcon, StatusDot } from "../containers/parts.tsx";
import { SPARK_POINTS } from "../containers/liveCells.ts";
import { EditDialog } from "./EditDialog.tsx";
import { eventLabel, historyHref, levelClass } from "./events.ts";
import {
  apiQuery,
  historyNote,
  logsAt,
  parseWindow,
  rangeSearch,
  RANGES,
  resetSearch,
  tabs,
  withParams,
  zoomSearch,
} from "./range.ts";
import "../containers/containers.css";
import "./container.css";
import "../history/history.css";

// Memoised: the charts re-render only when their series (or handlers) change.
// The chunk is asked for when this page's module loads, so it downloads
// while the detail is fetched.
const ExtraTab = lazy(() => import("./Tabs.tsx"));
const chartsChunk = import("../../charts/RechartsUsage.tsx");
const UsageCharts = memo(lazy(() => chartsChunk));

type Load =
  | { kind: "loading" }
  | { kind: "ok"; data: ContainerDetail; key: string }
  | { kind: "missing"; message: string }
  | { kind: "error"; message: string };

/** Labels Docker Compose and image builds add on their own (OV-44). */
const STATUS = "status";

function isBookkeeping(k: string): boolean {
  return ["com.docker.compose.", "org.opencontainers.image.", "com.docker.desktop."].some((p) => k.startsWith(p));
}

export default function ContainerPage() {
  const loc = useRoute();
  const name = loc.params.name ?? "";
  const win = parseWindow(loc.query);
  const q = apiQuery(win);
  const session = useSession();
  const role = session.session?.user?.role;
  const bootstrap = useBootstrap();
  const canEdit = !!bootstrap && !bootstrap.scoped && (role === "admin" || role === "owner");
  const live = useContainers();
  const now = useNow();
  const host = viewerHost(window.location.hostname);

  // ---- the detail, once per window (and again after a config change or a save) ----
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const key = `${name}?${q}`;
  /** One GET of the detail: what the page shows next (null: aborted). */
  const fetchDetail = useCallback(
    async (signal?: AbortSignal): Promise<Load | null> => {
      const r = await api.get<ContainerDetail>(`/api/v1/containers/${encodeURIComponent(name)}?${q}`, signal ? { signal } : {});
      if (signal?.aborted) return null;
      if (r.kind === "ok") return { kind: "ok", data: r.data, key };
      if (r.kind === "error" && r.status === 404) return { kind: "missing", message: r.message };
      return { kind: "error", message: r.message };
    },
    [name, q, key],
  );
  // A failed refresh keeps what is shown.
  const apply = useCallback(
    (next: Load | null) => {
      if (next) setLoad((l) => (next.kind === "error" && l.kind === "ok" && l.data.row.name === name ? l : next));
    },
    [name],
  );
  const refetch = useCallback(() => void fetchDetail().then(apply), [fetchDetail, apply]);
  useEffect(() => {
    if (!name) return;
    const ac = new AbortController();
    void fetchDetail(ac.signal).then(apply);
    return () => ac.abort();
  }, [name, fetchDetail, apply]);
  useEffect(() => onConfigChange(refetch), [refetch]);

  // The live row (header tiles), and a fresh detail when its state changes.
  const liveRow = useMemo<ContainerRow | null>(() => {
    for (const g of live.data?.groups ?? []) for (const r of g.containers) if (r.name === name) return r;
    return null;
  }, [live.data, name]);
  const stateKey = liveRow ? `${liveRow.state}|${liveRow.label}|${liveRow.started_at ?? ""}` : "";
  const lastState = useRef(stateKey);
  useEffect(() => {
    const was = lastState.current;
    lastState.current = stateKey;
    if (was && stateKey && stateKey !== was) refetch();
  }, [stateKey, refetch]);


  // Tile sparklines: the newest history points, then the live readings.
  // Another container's detail (a link from one page to the next) is not shown under this name.
  const detail = load.kind === "ok" && load.data.row.name === name ? load.data : null;
  const seedCpu = useMemo(() => detail?.series.cpu.slice(-SPARK_POINTS).map((p) => p[1]) ?? [], [detail]);
  const seedMem = useMemo(() => detail?.series.mem.slice(-SPARK_POINTS).map((p) => p[1]) ?? [], [detail]);

  const onZoom = useCallback((from: number, to: number) => navigate(loc.pathname + zoomSearch(loc.query, from, to), { keepScroll: true }), [loc.pathname, loc.query]);
  const onMarker = useCallback((id: number, at: number) => {
    const el = document.getElementById(`ev-${id}`);
    if (el) {
      el.scrollIntoView({ block: "center", behavior: "smooth" });
      el.classList.remove("flash");
      void el.offsetWidth;
      el.classList.add("flash");
      el.focus({ preventScroll: true });
      return;
    }
    if (bootstrap?.scoped) return;
    // Open the matching row in History when it is outside the overview timeline.
    const ev = detail?.series.markers.find((m) => m.id === id);
    navigate(historyHref(id, at, ev?.level ?? "warn"));
  }, [bootstrap?.scoped, detail]);

  if (load.kind === "missing" && !detail) {
    return (
      <div className="page cd-page">
        <EmptyState
          icon={<BoxIcon aria-hidden="true" />}
          title="No such container"
          testId="ctr-missing"
          actions={<a className="btn sm" href="/app/containers">All containers</a>}
        >
          Nothing called <code>{name}</code> is listed. It may have been removed or renamed, or it is hidden.
        </EmptyState>
      </div>
    );
  }
  if (!detail) {
    return (
      <div className="page cd-page">
        {load.kind === "error" ? (
          <EmptyState icon={<CircleAlertIcon aria-hidden="true" />} title="Could not load this container" tone="bad">
            {load.message}
          </EmptyState>
        ) : (
          <output className="muted">Loading {name}…</output>
        )}
      </div>
    );
  }

  // The live row wins for what changes by the second; the detail has the exact start time.
  const row: ContainerRow = liveRow ? { ...liveRow, started_at: detail.row.started_at ?? liveRow.started_at } : detail.row;
  const s = detail.series;
  const range = s.range;
  const visibleTabs = tabs(row.name, loc.query).filter((item) =>
    (item.id !== "processes" || canEdit));
  const tab = visibleTabs.find((item) => item.id !== "logs" && item.id === loc.search.get("tab"))?.id ?? "overview";
  const charts = tab === "overview" || tab === "metrics";
  const zoomed = win.kind === "zoom";
  const open = rowLink(row, host);
  const loading = load.kind === "ok" && load.key !== key;
  // `?edit=override` first, the rest of the query (the range) kept.
  const editHref = `?${new URLSearchParams([["edit", "override"], ...[...loc.search].filter(([k]) => k !== "edit")]).toString()}`;
  const editOpen = loc.search.get("edit") === "override" && canEdit;
  const note = historyNote(s.cpu[0]?.[0] ?? null, range, now);
  const health = row.health && row.health !== "none" ? row.health : null;
  const stateWord = health && row.state === "running" ? "running" : row.label;
  const stateDot = health && row.state === "running" ? "ok" : row.dot;
  const dockerError = live.data?.docker_error ?? null;
  const own = detail.labels.filter(([k]) => !isBookkeeping(k));
  const book = detail.labels.filter(([k]) => isBookkeeping(k));
  const up = row.state === "running" && row.started_at ? duration(now - row.started_at) : null;

  return (
    <div className="page cd-page cd">
      <PageHeader className="ctr-head" testId="ctr-head"
        icon={<ServiceIcon name={row.icon} label={row.title} />}
        title={<><StatusDot dot={row.dot} /> <span className="ctr-t">{row.title}</span></>}
        description={<p className="mono muted">
            {row.title !== row.name && <>{row.name} · </>}
            {row.image}
            {row.stack !== row.name && (
              <>
                {" · stack "}
                <a href={`/app/containers?stack=${encodeURIComponent(row.stack)}`}>{row.stack}</a>
              </>
            )}
            {" · id "}
            <span title={detail.id}>{detail.id.slice(0, 12)}</span>
          </p>}
        actions={open || canEdit ? <>{open && (
            <a className="btn btn-link" href={open} target="_blank" rel="noopener noreferrer" data-testid="open">
              <ExternalLinkIcon aria-hidden="true" />
              Open
            </a>
          )}
          {canEdit && (
            <a className="btn btn-ghost" href={editHref} data-testid="edit" title="Display name, group, icon, link">
              <PencilIcon aria-hidden="true" />
              Edit
            </a>
          )}</> : undefined}
        tabs={<TabNav layout="container" label="Container sections" items={visibleTabs} current={tab} testId="ctr-tabs" />} />
      {dockerError && (
        <output className="cd-why warn">
          <TriangleAlertIcon aria-hidden="true" />
          <span>Docker is not reachable ({dockerError}). Showing the last known state.</span>
        </output>
      )}
      {detail.why && <WhyNote why={detail.why} name={row.name} admin={canEdit} />}

      <section className="strip facts" aria-label="Status" data-testid="tiles">
        <div className="cell">
          <div className="cell-h">
            <span className="label">State</span>
          </div>
          <div className="big">
            <Badge variant={statusBadge(stateDot)}>{stateWord}</Badge>
          </div>
          <div className="sub" title={row.started_at ? new Date(row.started_at * 1000).toLocaleString() : undefined}>{up ? `up ${up}` : row.state === "exited" ? "stopped" : row.state}</div>
        </div>
        <StatTile key={`cpu:${row.name}`} name={row.name} kind="cpu" seed={seedCpu} />
        <StatTile key={`mem:${row.name}`} name={row.name} kind="mem" seed={seedMem} />
        <div className="cell">
          <div className="cell-h">
            <RefreshCwIcon aria-hidden="true" />
            <span className="label">Restarts (1 h)</span>
          </div>
          <div className="big num">{row.restarts}</div>
          <div className="sub">
            {detail.restarts_7d > 0
              ? `${detail.restarts_7d} ${detail.restarts_7d === 1 ? "start" : "starts"} in 7 days`
              : row.exit_code !== null
                ? `last exit code ${row.exit_code}`
                : "exits in the last hour"}
          </div>
        </div>
        {health && (
          <div className="cell">
            <div className="cell-h">
              <ActivityIcon aria-hidden="true" />
              <span className="label">Health check</span>
            </div>
            <div className="big">
              <Badge variant={statusBadge(row.dot)}>{health}</Badge>
            </div>
            <div className="sub">Docker health check</div>
          </div>
        )}
      </section>

      {charts && <section className="sec" aria-labelledby="cd-usage">
        <div className="panel-h">
          <h2 id="cd-usage">{tab === "metrics" ? "Metrics" : "Usage"}</h2>
          <nav className="seg" aria-label="Range" data-testid="range">
            {RANGES.map((r) => (
              <a key={r} href={loc.pathname + rangeSearch(loc.query, r)} aria-current={!zoomed && r === range ? "true" : undefined}>
                {r}
              </a>
            ))}
          </nav>
          {zoomed && (
            <a className="btn sm ghost" href={loc.pathname + resetSearch(loc.query, range)} data-testid="reset-zoom">
              Reset zoom
            </a>
          )}
          {loading && (
            <output className="muted small">Loading…</output>
          )}
        </div>
        {zoomed && (
          <p className="muted small cd-hnote">
            {localTime(s.from)} – {localTime(s.to)}
          </p>
        )}
        {note && !zoomed && (
          <p className="muted small cd-hnote">
            History for this container starts {duration(note.have)} ago.
            {note.fit && (
              <>
                {" "}
                <a href={loc.pathname + rangeSearch(loc.query, note.fit)}>Show {note.fit}</a>
              </>
            )}
          </p>
        )}
        {tab === "overview" && <Suspense
          fallback={
            <div className="charts two" aria-busy="true">
              <div className="chart ch-wait" />
              <div className="chart ch-wait" />
            </div>
          }
        >
          <UsageCharts series={s} onZoom={onZoom} onMarker={onMarker} />
        </Suspense>}
      </section>}
      {tab !== "overview" && <Suspense fallback={<p role="status">Loading tab…</p>}>
        <ExtraTab key={tab} tab={tab} detail={detail} admin={canEdit} scoped={!!bootstrap?.scoped} recording={!!bootstrap?.features.history} onZoom={onZoom} onMarker={onMarker} />
      </Suspense>}
      {tab === "overview" && <>

      <div className="cols">
        <section className="panel" aria-labelledby="cd-tl" data-testid="timeline">
          <div className="panel-h">
            <h2 id="cd-tl">Timeline</h2>
            <a className="panel-a" href={visibleTabs.find((item) => item.id === "events")?.href}>
              All events
            </a>
          </div>
          {detail.events.length === 0 ? (
            <p className="muted">No events recorded for this container yet.</p>
          ) : (
            <ul className="events">
              {detail.events.map((e) => (
                <li key={e.id} id={`ev-${e.id}`} className="ev" tabIndex={-1}>
                  <span className="when"><RelativeTime at={e.at} /></span>
                  <span className={`pill ${levelClass(e.level)}`}>{eventLabel(e.kind)}</span>
                  {e.message && <span className="muted detail">{e.message}</span>}
                  {e.kind.startsWith("container.") && (
                    <a className="ia logs" href={logsAt(row.name, e.at)} title="Logs at that moment" aria-label={`Logs of ${row.name} at that moment`}>
                      <ScrollTextIcon aria-hidden="true" />
                    </a>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
        <section className="panel" aria-labelledby="cd-ports">
          <div className="panel-h">
            <h2 id="cd-ports">Ports and links</h2>
          </div>
          {row.ports.length === 0 ? (
            <p className="muted">No published ports.</p>
          ) : (
            <table className="tbl small cd-stack">
              <thead>
                <tr>
                  <th scope="col">Host</th>
                  <th scope="col">Container</th>
                  <th scope="col">Link</th>
                </tr>
              </thead>
              <tbody>
                {row.ports.map((p) => {
                  const l = bindingLink(p, host);
                  return (
                    <tr key={`${p.host_ip}:${p.host_port}:${p.container_port}/${p.proto}`}>
                      <td className="mono" data-l="Host">
                        {p.host_ip === "" ? "*" : p.host_ip}:{p.host_port}
                      </td>
                      <td className="mono" data-l="Container">
                        {p.container_port}/{p.proto}
                      </td>
                      <td data-l="Link">
                        {"url" in l ? (
                          <a className="mono" href={l.url} target="_blank" rel="noopener noreferrer">
                            {l.url}
                          </a>
                        ) : (
                          <span className="muted">{l.none}</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </section>
      </div>
      <div className="cols">
        <section className="panel" aria-labelledby="cd-rules">
          <div className="panel-h">
            <h2 id="cd-rules">Alert rules</h2>
            {!bootstrap?.scoped && <a className="panel-a" href="/app/alerts">
              All alerts
            </a>}
          </div>
          {detail.rules.length === 0 ? (
            <p className="muted">No alert rule covers this container.</p>
          ) : (
            <ul className="rows cd-rules">
              {detail.rules.map((r) => (
                <li key={r.name}>
                  <span className={r.firing ? "dot bad" : "dot ok"} aria-hidden="true" />
                  <span className="sr">{r.firing ? "firing: " : "ok: "}</span>
                  <span className="nm">{r.name}</span>
                  <span className="why">{r.summary}</span>
                  <span className="when">{r.channels.length ? r.channels.join(", ") : "page only"}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
        <section className="panel" aria-labelledby="cd-labels">
          <div className="panel-h">
            <h2 id="cd-labels">Labels</h2>
          </div>
          {detail.labels.length === 0 ? (
            <p className="muted">No labels.</p>
          ) : (
            <>
              {own.length > 0 && (
                <dl className="kv">
                  {own.map(([k, v]) => (
                    <div key={k} className="kv-r">
                      <dt className="mono">{k}</dt>
                      <dd className="mono">{v}</dd>
                    </div>
                  ))}
                </dl>
              )}
              {book.length > 0 && (
                <details className="cd-labels" open={own.length === 0 && book.length <= 3}>
                  <summary>
                    {book.length} Compose and image {book.length === 1 ? "label" : "labels"}
                  </summary>
                  <dl className="kv">
                    {book.map(([k, v]) => (
                      <div key={k} className="kv-r">
                        <dt className="mono">{k}</dt>
                        <dd className="mono">{v}</dd>
                      </div>
                    ))}
                  </dl>
                </details>
              )}
            </>
          )}
        </section>
      </div>
      </>}
      <EditDialog
        open={editOpen}
        name={row.name}
        title={row.title}
        onClosed={() => navigate(loc.pathname + withParams(loc.query, { edit: null }), { replace: true, keepScroll: true })}
        onSaved={refetch}
      />
    </div>
  );
}

/**
 * "Why is this red?" (NAV-11): what is wrong, since when, a logs link at
 * that moment; signed in, the probe's first line and the last checks
 * (folded on a phone); signed out, a sign-in link and no probe output.
 */
function WhyNote({ why, name, admin }: { why: Why; name: string; admin: boolean }) {
  const phone = typeof window !== "undefined" && window.matchMedia?.("(max-width: 640px)").matches;
  return (
    // A status region of blocks (an <output> holds phrasing only).
    <div className={`cd-why ${why.severity}`} role={STATUS} data-testid="why">
      <CircleAlertIcon aria-hidden="true" />
      <div className="cd-why-b">
        <p>
          <strong>Why is this red? </strong>
          {why.title}
          {why.since !== null && (
            <>
              {" since "}
              <RelativeTime at={why.since} />
            </>
          )}
          {why.probe ? (
            <>
              {": "}
              <code className="probe">{why.probe}</code>
            </>
          ) : null}
          {why.detail && <> · {why.detail}</>}{" "}
          {<a className="cd-why-logs" href={logsAt(name, why.logs_at)}>
            {why.logs_at !== null ? `Logs around ${localTime(why.logs_at)}` : "Logs"}
          </a>}
        </p>
        {admin && why.checks.length > 0 && (
          <details className="cd-probes" open={!phone}>
            <summary>Last {why.checks.length} checks</summary>
            <ol className="cd-hlog">
              {why.checks.map((c) => (
                <li key={`${c.at ?? "?"}|${c.exit_code}|${c.output.length}|${c.output.slice(0, 32)}`}>
                  <div className="cd-hl-h">
                    <span className={`pill ${c.exit_code === 0 ? "ok" : "bad"}`}>exit {c.exit_code}</span>
                    {c.at !== null && <> {localTime(c.at)}</>}
                  </div>
                  {c.output.trim() === "" ? <p className="muted small">(no output)</p> : <pre className="cd-out">{c.output.trim()}</pre>}
                </li>
              ))}
            </ol>
          </details>
        )}
      </div>
    </div>
  );
}

/**
 * The CPU or memory tile: the live number (the stats store, so a stats
 * tick re-renders this tile only) and a sparkline seeded with the newest
 * history points, then the live readings.
 */
function StatTile({ name, kind, seed }: { name: string; kind: "cpu" | "mem"; seed: number[] }) {
  const st = useStat(name);
  const v = kind === "cpu" ? st.cpu_pct : st.mem_bytes;
  const [tail, setTail] = useState<{ last: number | null; pts: number[] }>({ last: null, pts: [] });
  if (v !== null && v !== tail.last) setTail({ last: v, pts: [...tail.pts, v].slice(-SPARK_POINTS) });
  const values = useMemo(() => [...seed, ...tail.pts].slice(-SPARK_POINTS), [seed, tail.pts]);
  return (
    <div className="cell">
      <div className="cell-h">
        {kind === "cpu" ? <CpuIcon aria-hidden="true" /> : <MemoryStickIcon aria-hidden="true" />}
        <span className="label">{kind === "cpu" ? "CPU" : "Memory"}</span>
      </div>
      <div className="big num">{v === null ? "—" : kind === "cpu" ? pct(v) : bytes(v)}</div>
      <div className="sub">{kind === "cpu" ? "of one core" : "resident"}</div>
      <Sparkline values={values} label="" color={kind === "cpu" ? "--ch-cpu" : "--ch-mem"} className="tile-spark" />
    </div>
  );
}
