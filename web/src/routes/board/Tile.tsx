import type { CSSProperties, ReactNode } from "react";
import { ArrowUpRightIcon, BoxIcon, ClockIcon, CpuIcon, InfoIcon, MemoryStickIcon, PlugIcon } from "lucide-react";
import type { BoardItem } from "../../api/gen/BoardItem.ts";
import type { ContainerRow } from "../../api/gen/ContainerRow.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import type { ResolvedItem } from "../../api/gen/ResolvedItem.ts";
import { containerHref } from "../../app/router.ts";
import { ACCENTS } from "./look.ts";
import { brandColor, useBrand } from "../../ui/brand.ts";
import { bytes, duration, imageLabel, pct, rowLink, viewerHost } from "../../format/index.ts";
import { useBootstrap, useContainers, useNow, useStat, useStrips } from "../../live/index.ts";
import { RelativeTime } from "../../ui/RelativeTime.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { HealthStrip, Meter } from "../../ui/Sparkline.tsx";
import { StatusPill, type StatusTone } from "../../ui/StatusPill.tsx";
import { WidgetContent, widgetStatus } from "../../ui/WidgetContent.tsx";

const containerTone = (dot: string): StatusTone => dot === "ok" ? "ok" : dot === "warn" ? "warn" : dot === "bad" ? "bad" : "off";
const capitalize = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

function hostOf(url: string | null | undefined): string {
  if (!url) return "";
  try { const u = new URL(url, window.location.href); return u.port ? `${u.hostname}:${u.port}` : u.hostname; } catch { return url; }
}

function integrationStatus(integration?: IntegrationView | null): { tone: StatusTone; label: string } {
  if (!integration) return { tone: "off", label: "Unknown" };
  if (!integration.enabled) return { tone: "off", label: "Disabled" };
  const check = integration.check;
  if (!check) return { tone: "off", label: "Not tested" };
  if (check.verdict.ok) return { tone: "ok", label: "Connected" };
  return check.last_good_at !== null ? { tone: "warn", label: "Failing" } : { tone: "bad", label: "Unreachable" };
}

/** CPU and memory now, the uptime and the last 24 hours, from the live stores. */
function ContainerStats({ row }: { row: ContainerRow }) {
  const stat = useStat(row.name);
  const now = useNow();
  const strips = useStrips();
  const cpu = stat.cpu_pct ?? row.cpu_pct;
  const mem = stat.mem_bytes ?? row.mem_bytes;
  const up = row.started_at && row.state === "running" ? Math.max(0, now - row.started_at) : null;
  const strip = strips?.containers[row.name];
  const running = row.state === "running";
  return <div className="tile-stats">
    {running ? <dl className="tile-metrics">
      <div><dt><CpuIcon aria-hidden="true" />CPU</dt><dd>{pct(cpu)}{cpu !== null && <Meter value={Math.min(cpu, 100)} max={100} label={`${row.title} CPU`} />}</dd></div>
      <div><dt><MemoryStickIcon aria-hidden="true" />Memory</dt><dd>{mem === null ? "—" : bytes(mem)}</dd></div>
      <div><dt><ClockIcon aria-hidden="true" />Up</dt><dd>{up === null ? "—" : duration(up)}</dd></div>
    </dl> : <p className="tile-note">{row.exit_code ? `Exited with code ${row.exit_code}` : "Not running"}</p>}
    {strip && strips && <HealthStrip className="tile-strip" codes={strip} step={strips.step} label={`${row.title}, last 24 hours`} />}
  </div>;
}

/** `inert` while editing: the tile is a picture of itself, its links and
 * controls out of the tab order and the accessibility tree. */
export function Tile({ item, resolved, inert }: { item: BoardItem; resolved?: ResolvedItem; inert?: boolean }) {
  const b = useBootstrap();
  const live = useContainers();
  const p = item.payload;
  const look = item.look;
  const matched = resolved?.container;
  const container = matched ? live.data ? live.data.groups.flatMap((g) => g.containers).find((c) => c.name === matched.name) ?? null : matched : null;
  const targetMissing = resolved?.missing || (p.kind === "container" && !!matched && !!live.data && !container);
  const integration = resolved?.integration;
  const widget = resolved?.widget;
  const title = p.kind === "link" ? p.title : p.kind === "container" ? p.title ?? container?.title ?? (p.target.by === "name" ? p.target.name : `${p.target.project}/${p.target.service}`) : p.kind === "widget" ? p.title ?? widget?.title ?? p.widget_id : p.title ?? integration?.name ?? "Integration";
  const icon = look?.icon ?? (p.kind === "link" ? p.icon : p.kind === "container" ? p.icon ?? container?.icon : p.kind === "widget" ? widget?.icon : integration?.admin?.icon);
  const service = container ? rowLink(container, viewerHost(window.location.hostname)) : null;
  const href = look?.link ?? (p.kind === "link" ? p.url : p.kind === "container" ? p.link ?? service ?? (container ? containerHref(container.name) : null) : p.kind === "widget" ? widget?.link : (integration?.admin?.base_url ?? integration?.origin));
  const details = p.kind === "container" && container && href !== containerHref(container.name) ? containerHref(container.name) : null;

  let subtitle: ReactNode = null;
  let pill: ReactNode = null;
  let body: ReactNode = null;
  if (targetMissing) {
    pill = <StatusPill tone="bad">Missing</StatusPill>;
    body = <p className="tile-note">This target is no longer available. Edit the board to remove or replace it.</p>;
  } else if (p.kind === "container") {
    subtitle = container ? imageLabel(container.image) : "Container";
    pill = container ? <StatusPill tone={containerTone(container.dot)}>{capitalize(container.label)}</StatusPill> : <StatusPill tone="off">Unknown</StatusPill>;
    body = container ? <ContainerStats row={container} /> : <p className="tile-note">Status unavailable</p>;
  } else if (p.kind === "link") {
    // A saved link is never health evidence: no status, only where it goes.
    subtitle = hostOf(p.url);
    pill = <span className="tile-kind"><ArrowUpRightIcon aria-hidden="true" /><span className="sr">Link</span></span>;
  } else if (p.kind === "integration") {
    subtitle = hostOf(integration?.admin?.base_url ?? integration?.origin);
    const status = integrationStatus(integration);
    pill = <StatusPill tone={status.tone} title={integration?.check?.verdict.message}>{status.label}</StatusPill>;
    const check = integration?.check;
    body = <div className="tile-stats" data-testid="integration-verdict">
      {check && !check.verdict.ok && <p className="tile-note" role="status">{check.verdict.message}</p>}
      {check && check.last_good_at !== null ? <p className="tile-foot"><PlugIcon aria-hidden="true" />Last success <RelativeTime at={check.last_good_at} /> · {check.last_good_latency_ms} ms</p>
        : <p className="tile-foot"><InfoIcon aria-hidden="true" />{integration?.enabled === false ? "Disabled" : "No connection test recorded"}</p>}
    </div>;
  } else {
    subtitle = widget?.link ? hostOf(widget.link) : "Widget";
    if (widget) {
      const status = widgetStatus(widget.status);
      pill = <StatusPill tone={status.tone} title={widget.error ?? undefined}>{status.label}</StatusPill>;
      body = <WidgetContent widget={widget} remoteIcons={b?.remote_icons} />;
    } else {
      pill = <StatusPill tone="off">Waiting</StatusPill>;
      body = <p className="tile-note">Waiting for widget data…</p>;
    }
  }

  if (look?.hide_status && !targetMissing && p.kind !== "link") pill = null;
  if (look?.hide_metrics && !targetMissing && (p.kind === "container" || p.kind === "integration")) body = null;
  // An explicit accent wins; otherwise the service's brand colour, softer.
  const brand = brandColor(useBrand(icon)?.color);
  const accent = look?.accent ? { "--tile-accent": ACCENTS[look.accent].color } as CSSProperties : brand ? { "--tile-accent": brand } as CSSProperties : undefined;
  return <article className="board-tile" data-testid="board-tile" data-kind={p.kind} data-missing={targetMissing || undefined} data-linked={href ? "" : undefined} data-accent={look?.accent ?? (brand ? "brand" : undefined)} style={accent} inert={inert}>
    <header className="board-tile-head">
      <ServiceIcon className="board-avatar" tint name={icon} title={title} enabled={b?.remote_icons ?? false} />
      <div className="board-tile-id">
        <h3>{href ? <a className="tile-link" href={href}>{title}</a> : title}</h3>
        {subtitle && <p className="tile-sub">{subtitle}</p>}
      </div>
      {pill}
    </header>
    {body}
    {details && <a className="tile-details" href={details} aria-label={`${title}: container details`} title="Container details"><BoxIcon aria-hidden="true" /></a>}
  </article>;
}
