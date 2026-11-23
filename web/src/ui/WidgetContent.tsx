import type { CSSProperties } from "react";
import type { WidgetView } from "../api/gen/WidgetView.ts";
import { brandColor, useBrand } from "./brand.ts";
import { RelativeTime } from "./RelativeTime.tsx";

import { ServiceIcon } from "./ServiceIcon.tsx";
import { StaleBadge } from "./StaleBadge.tsx";
import { StructuredWidget } from "./StructuredWidget.ts";
import type { StatusTone } from "./StatusPill.tsx";

const statusText: Record<string, string> = {
  disabled: "Disabled", waiting: "Waiting for the first answer…", bad: "The service answered with a problem", error: "Could not read this widget",
};

/** The pill a widget tile shows for its status. */
export function widgetStatus(status: string): { tone: StatusTone; label: string } {
  switch (status) {
    case "ok": case "ready": return { tone: "ok", label: "Live" };
    case "stale": return { tone: "warn", label: "Stale" };
    case "waiting": return { tone: "off", label: "Waiting" };
    case "disabled": return { tone: "off", label: "Disabled" };
    default: return { tone: "bad", label: status === "bad" ? "Problem" : "Error" };
  }
}

export function WidgetContent({ widget, remoteIcons = false }: { widget: WidgetView; remoteIcons?: boolean }) {
  const { body, error, status } = widget;
  const brand = brandColor(useBrand(widget.icon)?.color);
  return <div className="widget-content" data-brand={brand ? "" : undefined} style={brand ? { "--brand": brand } as CSSProperties : undefined}>
    {status === "stale" && widget.last_good_at !== null && <StaleBadge at={widget.last_good_at} error={error ?? "Refresh failed"} />}
    {statusText[status] && <p className="widget-note">{statusText[status]}{error ? `: ${error}` : ""}</p>}
    {widget.structured && <StructuredWidget body={widget.structured} renderIcon={(icon, title) =>
      <ServiceIcon className="service-icon widget-item-icon" name={icon} title={title} enabled={remoteIcons} />} />}
    {body?.headline && <p className="sw-stat wrap">{body.headline}</p>}
    {!!body?.rows.length && <dl className="sw-grid">{body.rows.map(([label, value]) => <div key={label} className="sw-cell"><dt>{label}</dt><dd>{value}</dd></div>)}</dl>}
    {body?.text && <pre className="wrap">{body.text}</pre>}
    {status !== "stale" && !!widget.fetched_at && <small className="widget-foot">Updated <RelativeTime at={widget.fetched_at} />{!!widget.duration_ms && ` · ${widget.duration_ms} ms`}</small>}
  </div>;
}
