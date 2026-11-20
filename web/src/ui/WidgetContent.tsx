import type { WidgetView } from "../api/gen/WidgetView.ts";
import { RelativeTime } from "./RelativeTime.tsx";

const statusText: Record<string, string> = {
  disabled: "Disabled", waiting: "Waiting for the first answer…", ok: "OK", bad: "Not OK", error: "Error",
};

export function WidgetContent({ widget }: { widget: WidgetView }) {
  const { body, error, status } = widget;
  return <>
    {statusText[status] && <p className="muted">{statusText[status]}{error ? ` · ${error}` : ""}</p>}
    {body?.headline && <p className="tile-value">{body.headline}</p>}
    {!!body?.rows.length && <dl>{body.rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>}
    {body?.text && <pre className="wrap">{body.text}</pre>}
    {!!widget.fetched_at && <small className="muted"><RelativeTime at={widget.fetched_at} />{!!widget.duration_ms && ` · ${widget.duration_ms} ms`}</small>}
  </>;
}
