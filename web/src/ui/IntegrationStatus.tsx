import type { IntegrationView } from "../api/gen/IntegrationView.ts";
import { RelativeTime } from "./RelativeTime.tsx";
import { StaleBadge } from "./StaleBadge.tsx";

export function IntegrationStatus({ integration }: { integration?: IntegrationView | null }) {
  if (integration?.enabled === false) return <p>Disabled</p>;
  const check = integration?.check;
  if (!check) return <p>No connection test recorded</p>;
  const { verdict, last_good_at, last_good_latency_ms } = check;
  const stale = !verdict.ok && last_good_at !== null;
  return <div data-testid="integration-verdict">
    {stale ? <StaleBadge at={last_good_at} error={verdict.message} /> : <p>{verdict.message}</p>}
    {last_good_at !== null && <small className="muted">Last success <RelativeTime at={last_good_at} /> · {last_good_latency_ms} ms</small>}
  </div>;
}
