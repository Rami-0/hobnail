import { lazy, Suspense } from "react";
import { eventLabel } from "../container/events.ts";
import { logsHref } from "../../app/paths.ts";
import type { AlertsView } from "../../api/gen/AlertsView.ts";
import { containerHref, useRoute } from "../../app/router.ts";
import { useBootstrap, useContainers, useNow } from "../../live/index.ts";
import { useRead } from "../../live/read.ts";
import { Button } from "../../ui/Button.tsx";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import "../history/history.css";


const Notifications = lazy(() => import("../settings/Notifications.tsx"));

export default function Alerts() {
  const boot = useBootstrap();
  const { query, search, setSearch } = useRoute();
  const admin = boot && !boot.scoped && (boot.role === "admin" || boot.role === "owner");
  const filters = new URLSearchParams(query);
  filters.delete("add");
  filters.delete("edit");
  const now = useNow();
  const { data: containers } = useContainers();
  const { data, error } = useRead<AlertsView>(boot?.scoped === false ? `/api/v1/alerts?${filters}` : null, "alerts");
  const names = new Set(containers?.groups.flatMap((g) => g.containers.map((c) => c.name)));
  if (boot?.scoped) return <p className="page">Alerts are unavailable with board-scoped access.</p>;
  return <div className="page" data-testid="alerts">
    <h1>Alerts</h1>
    {admin && (search.has("add") || search.has("edit")) && <Suspense fallback={<p role="status">Loading editor…</p>}><Notifications editorOnly /></Suspense>}
    {error && <p role="alert">{error}</p>}
    {!data ? !error && <p role="status">Loading alerts…</p> : <>
      <section className="panel history-block" aria-label="Firing now">
        <h2>Firing now</h2>
        {data.firing.length ? <ul className="history-events">{data.firing.map((incident) => <li key={`${incident.rule}:${incident.subject}`} data-testid="row">
          <strong><a href={incident.href}>{incident.subject}</a></strong>{" "}
          {incident.state} · {incident.rule} · <RelativeTime at={incident.since} />
          {incident.container && names.has(incident.subject) && <> · <a href={logsHref(incident.subject, incident.since)}>Logs</a></>}
          {!incident.notified && <span className="muted"> · Notification pending or suppressed</span>}
        </li>)}</ul> : <p>{boot?.docker_error ? "Container rules cannot run while Docker is unreachable." : "No alerts firing."}</p>}
      </section>
      <section className="panel history-block">
        <h2>Rules</h2>
        {admin && <Button onClick={() => setSearch({ add: "rule", edit: null })}>Add rule</Button>}
        {data.rules.length ? <ul className="history-events">{data.rules.map((rule) => <li key={rule.name}>
          <strong>{rule.name}</strong> · {rule.kind} · {rule.summary}{!!rule.channels.length && <> · {rule.channels.join(", ")}</>}
          {admin && <Button className="sm" aria-label={`Edit ${rule.name}`} onClick={() => setSearch({ add: null, edit: `rule:${rule.name}` })}>Edit</Button>}
        </li>)}</ul> : <p>No alert rules configured.</p>}
      </section>
      <section className="panel history-block">
        <h2>Notification channels</h2>
        {admin && <Button onClick={() => setSearch({ add: "channel", edit: null })}>Add channel</Button>}
        {data.channels.length ? <ul className="history-events">{data.channels.map((channel) => <li key={channel.name}>
          <strong>{channel.name}</strong> · {channel.kind} · {channel.status}
          {channel.last_success !== null && <> · Last sent <RelativeTime at={channel.last_success} /></>}
          {channel.last_failure !== null && <> · Last failure <RelativeTime at={channel.last_failure} /></>}
          {channel.pending > 0 && <> · {channel.pending} pending</>}
          {admin && <Button className="sm" aria-label={`Edit ${channel.name}`} onClick={() => setSearch({ add: null, edit: `channel:${channel.name}` })}>Edit</Button>}
        </li>)}</ul> : <p>No channels configured.</p>}
      </section>
      <section className="panel history-block">
        <h2>Alert log</h2>
        <div className="toolbar history-controls">
          <Button aria-pressed={!search.has("from")} onClick={() => setSearch({ from: null, to: null, before: null })}>All time</Button>
          {[[86400, "24 hours"], [604800, "7 days"]].map(([span, label]) => <Button key={span}
            aria-pressed={Number(search.get("to")) - Number(search.get("from")) === span}
            onClick={() => setSearch({ from: String(Math.floor(now) - Number(span)), to: String(Math.floor(now)), before: null })}>{label}</Button>)}
        </div>
        {!data.log.length && <p>No alert events found.</p>}
        <ul className="history-events">{data.log.map((event) => <li key={event.id}>
          <RelativeTime at={event.at} /> · <strong>{eventLabel(event.kind)}</strong>{" "}
          {names.has(event.subject) ? <>
            <a href={containerHref(event.subject)}>{event.subject}</a>{" · "}
            <a href={logsHref(event.subject, event.at)}>Logs</a>
          </> : event.subject}
          {event.message && <div className="muted">{event.message}</div>}
        </li>)}</ul>
        {search.has("before") && <Button onClick={() => setSearch({ before: null })}>Back to newest</Button>}
        {data.older !== null && <Button onClick={() => setSearch({ before: String(data.older) })}>Older events</Button>}
      </section>
    </>}
  </div>;
}
