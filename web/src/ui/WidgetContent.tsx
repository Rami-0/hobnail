import type { WidgetView } from "../api/gen/WidgetView.ts";
import { RelativeTime } from "./RelativeTime.tsx";

import { ServiceIcon } from "./ServiceIcon.tsx";
import { StaleBadge } from "./StaleBadge.tsx";
import { StructuredWidget } from "./StructuredWidget.ts";

const statusText: Record<string, string> = {
  disabled: "Disabled", waiting: "Waiting for the first answer…", ok: "OK", bad: "Not OK", error: "Error",
};

export function WidgetContent({ widget, remoteIcons = false }: { widget: WidgetView; remoteIcons?: boolean }) {
  const { body, error, status } = widget;
  return <>
    {status === "stale" && widget.last_good_at !== null && <StaleBadge at={widget.last_good_at} error={error ?? "Refresh failed"} />}
    {statusText[status] && <p className="muted">{statusText[status]}{error ? ` · ${error}` : ""}</p>}
    {widget.structured && <StructuredWidget body={widget.structured} renderIcon={(icon, title) =>
      <ServiceIcon className="service-icon widget-item-icon" name={icon} title={title} enabled={remoteIcons} />} />}
    {body?.headline && <p className="tile-value">{body.headline}</p>}
    {!!body?.rows.length && <dl>{body.rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>}
    {body?.text && <pre className="wrap">{body.text}</pre>}
    {status !== "stale" && !!widget.fetched_at && <small className="muted"><RelativeTime at={widget.fetched_at} />{!!widget.duration_ms && ` · ${widget.duration_ms} ms`}</small>}
  </>;
}
