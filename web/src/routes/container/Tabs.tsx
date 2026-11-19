import { lazy, Suspense, useState } from "react";
import type { ContainerDetail } from "../../api/gen/ContainerDetail.ts";
import type { ContainerInspect } from "../../api/gen/ContainerInspect.ts";
import type { ContainerEnvironment } from "../../api/gen/ContainerEnvironment.ts";
import type { ContainerProcesses } from "../../api/gen/ContainerProcesses.ts";
import type { ContainerEvents } from "../../api/gen/ContainerEvents.ts";
import type { ContainerMetrics } from "../../api/gen/ContainerMetrics.ts";
import type { HistoryMetric } from "../../api/gen/HistoryMetric.ts";
import { useRoute } from "../../app/router.ts";
import { bytes, duration } from "../../format/index.ts";
import { useRead } from "../../live/read.ts";
import { Button } from "../../ui/Button.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { eventLabel } from "./events.ts";
import { apiQuery, logsAt, parseWindow, type Tab } from "./range.ts";

const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));
const KINDS = [["all", "All"], ["lifecycle", "Lifecycle"], ["alerts", "Alerts"], ["die", "Exits"], ["health", "Health"], ["oom", "OOM"], ["start", "Starts"]];

interface Props {
  detail: ContainerDetail;
  tab: Tab["id"];
  admin: boolean;
  scoped: boolean;
  recording: boolean;
  onZoom: (from: number, to: number) => void;
  onMarker: (id: number, at: number) => void;
}

export default function ExtraTab(props: Props) {
  const root = `/api/v1/containers/${encodeURIComponent(props.detail.row.name)}`;
  switch (props.tab) {
    case "metrics": return <Metrics root={root} {...props} />;
    case "inspect": return <Inspect root={root} admin={props.admin} />;
    case "events": return <Events root={root} name={props.detail.row.name} scoped={props.scoped} />;
    case "processes": return props.admin ? <Processes root={root} /> : null;
    default: return null;
  }
}

function Metrics({ root, detail, recording, onZoom, onMarker }: Props & { root: string }) {
  const { query } = useRoute();
  const win = apiQuery(parseWindow(query));
  const { data, error } = useRead<ContainerMetrics>(`${root}/metrics?${win}`);
  const s = detail.series;
  const { data: levels } = useRead<{ step: number; points: [number, number[]][] }>(
    `/api/logs/levels?c=${encodeURIComponent(detail.row.name)}&range=${s.range}`);
  const limit = data?.memory_limit_bytes;
  const quiet = new Set(["throttled", "oom", "block_read", "block_write"]);
  const zeroed = data?.series?.filter((m) => quiet.has(m.key) && m.points.length && m.points.every(([, v]) => v === 0)) ?? [];
  const metrics: HistoryMetric[] = [
    { key: "cpu", step: s.step, points: s.cpu, max: s.cpu_max, yesterday: null },
    { key: "mem", step: s.step, points: s.mem, max: s.mem_max, yesterday: null },
    ...(limit ? [{ key: "mem_pct", step: s.step, points: s.mem.map(([t, value]): [number, number] => [t, value / limit * 100]), max: null, yesterday: null }] : []),
    ...data?.series?.filter((m) => !zeroed.includes(m) && (data.network === "container" || !m.key.startsWith("net_"))) ?? [],
  ];
  return <>
    {error && <p role="alert">{error}</p>}
    {data?.series && <section className="panel history-block">
      <h3>Current limits and activity</h3>
      <Pairs rows={[
        ["Policy restarts", data.policy_restarts ?? "—"],
        ["Starts", `${data.starts_24h} in 24 h · ${data.starts_7d} in 7 days`],
        ["OOM kills", data.oom_killed ? "Last exit was an OOM kill" : data.oom_kills ?? "—"],
        ["PIDs", `${data.pids ?? "—"} / ${data.pid_limit ?? "unlimited"}`],
        ["Memory limit", data.memory_limit_bytes ? bytes(data.memory_limit_bytes) : "Unlimited"],
        ["CPU throttling", data.throttled_periods === null ? "—" : `${data.throttled_periods} of ${data.cpu_periods ?? "—"} periods · ${duration((data.throttled_usec ?? 0) / 1e6)} throttled`],
      ]} />
      {data.network === "host" && <p>Traffic is counted on the <a href="/app/host">host</a>.</p>}
      {data.network === "shared" && <p>Traffic is counted in the shared network namespace.</p>}
    </section>}
    <Suspense fallback={<p role="status">Loading charts…</p>}>
      <Charts series={s} metrics={metrics} recording={recording} onZoom={onZoom} onMarker={onMarker} />
    </Suspense>
    <h3>Log levels</h3>
    {levels?.points.length ? <Suspense fallback={<p role="status">Loading log-level charts…</p>}>
      <Charts series={s} metrics={["error", "warn"].map((level, i) => ({
        key: `log_${level}`, step: levels.step, points: levels.points.filter(([t]) => t >= s.from && t <= s.to).map(([t, counts]): [number, number] => [t, counts[i] ?? 0]), max: null, yesterday: null,
      }))} recording={recording} onZoom={onZoom} onMarker={onMarker} />
    </Suspense> : <p>No log lines counted in this range yet. <a href="/app/alerts">Add a log alert</a> to count errors and warnings for this container.</p>}
    {!!zeroed.length && <p className="muted">Zero over this range · {zeroed.map((item) => item.key.replaceAll("_", " ")).join(", ")}</p>}
  </>;
}

function Pairs({ rows }: { rows: readonly (readonly [string, React.ReactNode])[] }) {
  return <dl className="kv">{rows.map(([label, value]) => <div className="kv-r" key={label}>
    <dt>{label}</dt><dd>{value}</dd>
  </div>)}</dl>;
}

function Environment({ root }: { root: string }) {
  const { data, error } = useRead<ContainerEnvironment>(`${root}/env`);
  return error ? <p role="alert">{error}</p> : data ? <Pairs rows={data.env} /> : <p role="status">Loading environment…</p>;
}

function Inspect({ root, admin }: { root: string; admin: boolean }) {
  const { data, error } = useRead<ContainerInspect>(`${root}/inspect`);
  const [revealed, setRevealed] = useState(false);
  if (!data) return <p role={error ? "alert" : "status"}>{error || "Loading inspection…"}</p>;
  return <section className="panel history-block" data-testid="container-inspect">
    {error && <p role="alert">{error}</p>}
    <h2>Inspect</h2>
    <Pairs rows={[
      ["Image ID", data.image_id || "—"], ["Image digests", data.image_digests.join(", ") || "—"],
      ["User", data.user || "Default"], ["Working directory", data.work_dir || "Default"],
      ["Started", data.started_at === null ? "—" : <RelativeTime key="started" at={data.started_at} />],
      ["Finished", data.finished_at === null ? "—" : <RelativeTime key="finished" at={data.finished_at} />],
      ["Restart policy", data.restart_policy || "None"], ["Policy restarts", data.restart_count],
      ["Last exit OOM killed", data.oom_killed ? "Yes" : "No"],
      ["Memory limit", data.memory_limit_bytes > 0 ? bytes(data.memory_limit_bytes) : "Unlimited"],
      ["PID limit", data.pid_limit && data.pid_limit > 0 ? data.pid_limit : "Unlimited"],
      ["Privileged", data.privileged ? "Yes" : "No"], ["Read-only root", data.read_only ? "Yes" : "No"],
      ["Network mode", data.network_mode || "Default"], ["Networks", data.networks.join(", ") || "—"],
      ...(admin ? [
        ["Command", data.command?.join(" ") || "—"], ["Entrypoint", data.entrypoint?.join(" ") || "—"],
        ["Health test", data.health_test?.join(" ") || "—"],
      ] as const : []),
    ]} />
    <h3>Network addresses</h3>
    {data.network_addresses.length ? data.network_addresses.map(([network, ipv4, ipv6, gateway, mac]) => <section key={network}>
      <h4>{network}</h4><Pairs rows={[["IPv4", ipv4 || "—"], ["IPv6", ipv6 || "—"], ["Gateway", gateway || "—"], ["MAC", mac || "—"]]} />
    </section>) : <p>No attached networks.</p>}
    <h3>Health-check schedule</h3>
    {data.health_schedule ? <Pairs rows={[
      ["Interval", data.health_schedule[0] > 0 ? duration(data.health_schedule[0] / 1e9) : "Docker default (30s)"],
      ["Timeout", data.health_schedule[1] > 0 ? duration(data.health_schedule[1] / 1e9) : "Docker default (30s)"],
      ["Retries", data.health_schedule[2] > 0 ? data.health_schedule[2] : 3],
      ["Start period", duration(data.health_schedule[3] / 1e9)],
    ]} /> : <p>No health check configured.</p>}
    <h3>Mounts</h3>
    {data.mounts.length ? <ul>{data.mounts.map((m) => <li key={m.destination}>
      {m.source && <>{m.source} → </>}{m.destination} · {m.kind} · {m.writable ? "read/write" : "read-only"}
    </li>)}</ul> : <p>No mounts.</p>}
    <h3>Environment</h3>
    {data.env_keys.length ? <>
      {admin && <Button data-testid="reveal-env" aria-expanded={revealed} onClick={() => setRevealed(!revealed)}>{revealed ? "Hide values" : "Reveal values"}</Button>}
      {revealed && admin ? <Environment root={root} /> : <ul>{data.env_keys.map((key) => <li key={key}><code>{key}</code></li>)}</ul>}
    </> : <p>No environment variables.</p>}
  </section>;
}

function Processes({ root }: { root: string }) {
  const { search, setSearch } = useRoute();
  const sort = search.get("sort") ?? "";
  const dir = search.get("dir") === "asc" ? "asc" : "desc";
  const { data, error } = useRead<ContainerProcesses>(`${root}/top?${new URLSearchParams({ sort, dir })}`, undefined, 3000);
  return <section className="panel history-block" data-testid="container-processes">
    <h2>{data ? `${data.processes.length} ${data.processes.length === 1 ? "process" : "processes"}` : "Processes"}</h2>
    {data && error && <p role="alert">{error}</p>}
    {!data ? <p role={error ? "alert" : "status"}>{error || "Loading processes…"}</p> :
      <div className="detail-table"><table className="tbl">
        <thead><tr>{data.titles.map((title, i) => <th key={title} scope="col" aria-sort={String(i) === sort ? (dir === "asc" ? "ascending" : "descending") : undefined}>
          <Button variant="ghost" onClick={() => setSearch({ sort: String(i), dir: String(i) === sort && dir === "desc" ? "asc" : "desc" })}>{title}</Button>
        </th>)}</tr></thead>
        <tbody>{data.processes.map((row) => <tr key={row.join("\0")}>{row.map((value, i) => <td key={data.titles[i] ?? i}>{value}</td>)}</tr>)}</tbody>
      </table></div>}
  </section>;
}

function Events({ name, root, scoped }: { name: string; root: string; scoped: boolean }) {
  const { search, query, setSearch } = useRoute();
  const kind = KINDS.some(([k]) => k === search.get("kind")) ? search.get("kind") ?? "all" : "all";
  const params = new URLSearchParams({ kind });
  const w = parseWindow(query);
  if (w.kind === "zoom") { params.set("from", String(w.from)); params.set("to", String(w.to)); }
  if (search.has("before")) params.set("before", search.get("before") ?? "");
  const { data, error } = useRead<ContainerEvents>(`${root}/events?${params}`, undefined, 15000);
  return <section className="panel history-block" data-testid="container-events">
    <h2>Events</h2>
    <div className="toolbar history-controls">{KINDS.map(([key, label]) => <Button key={key} aria-pressed={kind === key}
      onClick={() => setSearch({ kind: key ?? "all", before: null })}>{label}</Button>)}</div>
    {error && <p role="alert">{error}</p>}
    {data && !data.events.length && <p>No matching events.</p>}
    <ul className="history-events">{data?.events.map((event) => <li key={event.id} id={`ev-${event.id}`} tabIndex={-1}>
      <RelativeTime at={event.at} /> · {eventLabel(event.kind)} · {event.message}
      {!scoped && event.kind.startsWith("container.") && <> · <a href={logsAt(name, event.at)}>Logs</a></>}
    </li>)}</ul>
    {params.has("before") && <Button onClick={() => setSearch({ before: null })}>Newest</Button>}
    {data && data.older !== null && <Button onClick={() => setSearch({ before: String(data.older) })}>Older</Button>}
  </section>;
}
